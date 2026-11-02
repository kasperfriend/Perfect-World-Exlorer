#![allow(dead_code)]
#[path = "../animation.rs"]
mod animation;
#[path = "../ecmodel.rs"]
mod ecmodel;
#[path = "../model_asset.rs"]
mod model_asset;
#[path = "../model_dds.rs"]
mod model_dds;
#[path = "../model_material.rs"]
mod model_material;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene.rs"]
mod scene;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../skeleton_pose.rs"]
mod skeleton_pose;
#[path = "../skin.rs"]
mod skin;
#[path = "../skin_pose.rs"]
mod skin_pose;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
};
fn main() -> Result<(), String> {
    let e = std::path::PathBuf::from(std::env::args().nth(1).ok_or("provide element folder")?);
    let s = scene::Scene::load(&e.join("maps/world/world.ecwld"))?;
    let mut f = File::open(e.join("models.pck")).map_err(|e| e.to_string())?;
    let entries: HashMap<_, _> = pck::directory(&mut f)?
        .into_iter()
        .map(|x| (x.name.to_lowercase(), x))
        .collect();
    let mut placed = HashSet::new();
    let mut ecms = HashSet::new();
    let mut smds = HashSet::new();
    let mut skins = HashSet::new();
    let mut skeleton_paths = HashSet::new();
    let mut missing = Vec::new();
    let mut skin_owners: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut track_owners: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut versions = BTreeMap::new();
    let mut failures = Vec::new();
    let mut texture_references = 0usize;
    let mut texture_mips = 0usize;
    let mut texture_formats = BTreeMap::new();
    let mut asset_pose_cases = 0usize;
    let mut asset_posed_vertices = 0usize;
    let mut combined_actions = 0usize;
    let mut base_actions = 0usize;
    let mut gfx_references = 0usize;
    let mut event_types = HashSet::new();
    let mut action_candidates = [0usize; 2];
    for m in s.blocks.iter().flat_map(|b| &b.models) {
        if !placed.insert(m.export_id) {
            continue;
        }
        let key = ecmodel::normalize(&m.path);
        if !ecms.insert(key.clone()) {
            continue;
        }
        if !entries.contains_key(&key) {
            missing.push(key);
            continue;
        }
        let asset = model_asset::ModelAsset::load_with(&key, |path| {
            let entry = entries
                .get(path)
                .ok_or_else(|| format!("missing dependency {path}"))?;
            pck::read_entry(&mut f, entry)
        })?;
        let ec = &asset.descriptor;
        for skin in &asset.skins {
            for i in 0..skin.data.textures.len() {
                match skin.load_texture_with(i, |path| {
                    let entry = entries
                        .get(path)
                        .ok_or_else(|| format!("missing texture {path}"))?;
                    let bytes = pck::read_entry(&mut f, entry)?;
                    let d = model_dds::Dds::parse(&bytes)?;
                    Ok((format!("{:?}", d.format), d.mips.len()))
                }) {
                    Ok((format, count)) => {
                        texture_references += 1;
                        texture_mips += count;
                        *texture_formats.entry(format).or_insert(0usize) += 1;
                    }
                    Err(e) => failures.push(e),
                }
            }
        }
        let mut checked_embedded = false;
        for (i, set) in asset.tracks.iter().enumerate() {
            let (start, end, fps) = match set {
                Some(t) => (t.start, t.end, t.fps),
                None if !checked_embedded => {
                    checked_embedded = true;
                    (asset.skeleton.start, asset.skeleton.end, asset.skeleton.fps)
                }
                None => continue,
            };
            for frame in [start as f32, (start as f32 + end as f32) * 0.5, end as f32] {
                let world = glam::Mat4::from_rotation_translation(
                    glam::Quat::from_rotation_y(0.37),
                    glam::Vec3::new(10., 4., -12.),
                );
                match asset.pose_action_at_time(i, frame / fps as f32, world) {
                    Ok(meshes) => {
                        for mesh in meshes {
                            let source = &asset.skins[mesh.skin].data.meshes[mesh.mesh];
                            if mesh.vertices.len() != source.vertices.len() {
                                failures
                                    .push(format!("{key}: retained mesh vertex count mismatch"));
                            }
                            asset_posed_vertices += mesh.vertices.len();
                        }
                        asset_pose_cases += 1;
                    }
                    Err(e) => failures.push(format!("{key}: retained asset pose: {e}")),
                }
            }
        }
        combined_actions += ec.actions.len();
        base_actions += ec.actions.iter().map(|a| a.bases.len()).sum::<usize>();
        for event in &ec.events {
            event_types.insert(event.kind);
            if event.gfx_path().is_some() {
                gfx_references += 1;
            }
        }
        let path = ec.skin_model_path.clone();
        if !smds.insert(path.clone()) {
            continue;
        }
        let smd = &asset.model;
        *versions.entry(smd.version).or_insert(0usize) += 1;
        for action in &ec.actions {
            for base in &action.bases {
                if !smd.actions.iter().any(|a| a.name == base.name) {
                    failures.push(format!(
                        "{key}: {} -> missing SMD action {}",
                        action.name, base.name
                    ));
                }
            }
        }
        // The plant's placements use direct base names, whereas e.g. the
        // armillary sphere and right seahorse use different combined names.
        // Count name resolution candidates, not verified playback semantics.
        for placement in s
            .blocks
            .iter()
            .flat_map(|b| &b.models)
            .filter(|m| ecmodel::normalize(&m.path) == key)
        {
            if ec.actions.iter().any(|a| a.name == placement.action) {
                action_candidates[0] += 1;
            } else if smd.actions.iter().any(|a| a.name == placement.action) {
                action_candidates[1] += 1;
            } else {
                failures.push(format!(
                    "{key}: no combined/base candidate for scene action {:?}",
                    placement.action
                ));
            }
        }
        let parent = path.rsplit_once('/').map(|x| x.0).unwrap_or("");
        let skeleton_path = ecmodel::normalize(&format!("{parent}/{}", smd.skeleton));
        for action in &smd.actions {
            if let Some(track_path) = smd.action_track_path(&path, action) {
                track_owners
                    .entry(track_path)
                    .or_default()
                    .insert(skeleton_path.clone());
            }
        }
        for refname in std::iter::once(&smd.skeleton).chain(&smd.skins) {
            if refname.is_empty() {
                continue;
            }
            let target = ecmodel::normalize(&format!("{parent}/{refname}"));
            if !entries.contains_key(&target) {
                missing.push(target.clone())
            }
            if target.ends_with(".bon") {
                skeleton_paths.insert(target.clone());
            }
            if target.ends_with(".ski") {
                skin_owners
                    .entry(target.clone())
                    .or_default()
                    .insert(skeleton_path.clone());
                skins.insert(target);
            }
        }
    }
    let (mut skin_meshes, mut rigid_meshes, mut material_count) = (0, 0, 0);
    let mut skin_versions = BTreeMap::new();
    println!("Model DDS: {texture_references} resolved references, {texture_mips} authored mip levels, formats {texture_formats:?}; payload validation, not pixel/shader validation");
    println!("Retained assets: {asset_pose_cases} sampled poses, {asset_posed_vertices} deformed vertices; not a world draw");
    println!("ECM metadata: {combined_actions} combined actions, {base_actions} base references; scene name candidates {action_candidates:?} (combined/direct); {gfx_references} external GFX references, event types {event_types:?}");
    println!("GFX archive data and ECM schedule/event playback are NOT validated by this models.pck diagnostic");
    let mut skeleton_versions = BTreeMap::new();
    let mut joint_counts = HashMap::new();
    let mut skeleton_data = HashMap::new();
    let mut sampled_poses: HashMap<String, Vec<skeleton_pose::Pose>> = HashMap::new();
    let (mut bones, mut embedded_tracks) = (0, 0);
    for path in &skeleton_paths {
        let Some(entry) = entries.get(path) else {
            continue;
        };
        match pck::read_entry(&mut f, entry).and_then(|b| skeleton::Skeleton::parse(&b)) {
            Ok(s) => {
                *skeleton_versions.entry(s.version).or_insert(0usize) += 1;
                joint_counts.insert(path.clone(), s.joints.len());
                bones += s.bones.len();
                embedded_tracks += s.embedded_tracks.len();
                if !s.embedded_tracks.is_empty() {
                    for frame in [
                        s.start as f32,
                        (s.start as f32 + s.end as f32) * 0.5,
                        s.end as f32,
                    ] {
                        match skeleton_pose::evaluate(
                            &s,
                            &s.embedded_tracks,
                            frame / s.fps as f32,
                            glam::Mat4::IDENTITY,
                        ) {
                            Ok(p) => sampled_poses.entry(path.clone()).or_default().push(p),
                            Err(e) => failures.push(format!("{path}: embedded pose: {e}")),
                        }
                    }
                }
                skeleton_data.insert(path.clone(), s);
            }
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    println!(
        "BON versions {skeleton_versions:?}; {bones} bones; {embedded_tracks} embedded tracks"
    );
    let mut track_versions = BTreeMap::new();
    let mut external_tracks = 0;
    for (path, owners) in &track_owners {
        let Some(entry) = entries.get(path) else {
            missing.push(path.clone());
            continue;
        };
        match pck::read_entry(&mut f, entry).and_then(|b| animation::TrackSet::parse(&b)) {
            Ok(t) => {
                *track_versions.entry(t.version).or_insert(0usize) += 1;
                external_tracks += t.tracks.len();
                for owner in owners {
                    if let Some(&count) = joint_counts.get(owner) {
                        if let Err(e) = t.validate_joint_count(count) {
                            failures.push(format!("{path} -> {owner}: {e}"));
                        }
                        if let Some(s) = skeleton_data.get(owner) {
                            for frame in [
                                t.start as f32,
                                (t.start as f32 + t.end as f32) * 0.5,
                                t.end as f32,
                            ] {
                                match skeleton_pose::evaluate(
                                    s,
                                    &t.tracks,
                                    frame / t.fps as f32,
                                    glam::Mat4::IDENTITY,
                                ) {
                                    Ok(p) => {
                                        sampled_poses.entry(owner.clone()).or_default().push(p)
                                    }
                                    Err(e) => {
                                        failures.push(format!("{path} -> {owner}: pose: {e}"))
                                    }
                                }
                            }
                        }
                    } else {
                        failures.push(format!("{path}: skeleton was not validated: {owner}"));
                    }
                }
                for track in &t.tracks {
                    for frame in [
                        t.start as f32,
                        (t.start as f32 + t.end as f32) * 0.5,
                        t.end as f32,
                    ] {
                        let seconds = frame / t.fps as f32;
                        if (!track.position.keys.is_empty()
                            && !track
                                .position
                                .sample(seconds)
                                .is_some_and(|p| p.iter().all(|v| v.is_finite())))
                            || (!track.rotation.keys.is_empty()
                                && !track
                                    .rotation
                                    .sample(seconds)
                                    .is_some_and(|q| q.iter().all(|v| v.is_finite())))
                        {
                            failures.push(format!(
                                "{path}: invalid sampled transform at frame {frame}, joint {}",
                                track.id
                            ));
                        }
                    }
                }
            }
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    println!(
        "STCK versions {track_versions:?}; {} external files, {external_tracks} joint tracks",
        track_owners.len()
    );
    let mut posed_vertices = 0usize;
    for path in &skins {
        let Some(entry) = entries.get(path) else {
            continue;
        };
        match pck::read_entry(&mut f, entry).and_then(|b| skin::Skin::parse(&b)) {
            Ok(s) => {
                *skin_versions.entry(s.version).or_insert(0usize) += 1;
                if let Some(owners) = skin_owners.get(path) {
                    for owner in owners {
                        if let (Some(skeleton), Some(poses)) =
                            (skeleton_data.get(owner), sampled_poses.get(owner))
                        {
                            for pose in poses {
                                match skeleton_pose::palette(&s, skeleton, pose) {
                                    Ok(palette) => {
                                        for mesh in &s.meshes {
                                            if mesh.rigid_bone.is_some() {
                                                failures.push(format!(
                                                    "{path}: rigid pose is not checked"
                                                ));
                                                continue;
                                            }
                                            for v in &mesh.vertices {
                                                match skin_pose::deform(
                                                    v.position,
                                                    v.normal,
                                                    [v.weights[0], v.weights[1], v.weights[2]],
                                                    v.joints,
                                                    &palette,
                                                ) {
                                                    Ok(v)
                                                        if v.position
                                                            .iter()
                                                            .chain(v.normal.iter())
                                                            .all(|x| x.is_finite()) =>
                                                    {
                                                        posed_vertices += 1
                                                    }
                                                    Ok(_) => failures.push(format!(
                                                        "{path}: nonfinite posed vertex"
                                                    )),
                                                    Err(e) => failures.push(format!("{path}: {e}")),
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => failures.push(format!("{path}: palette: {e}")),
                                }
                            }
                        } else {
                            failures
                                .push(format!("{path}: no validated skeleton/pose for {owner}"));
                        }
                    }
                }
                for mesh in &s.meshes {
                    if mesh.rigid_bone.is_some() {
                        rigid_meshes += 1
                    } else {
                        skin_meshes += 1
                    }
                }
                material_count += s.materials.len();
                for material in &s.materials {
                    let c = material.light_constants(model_material::Lighting {
                        environment_ambient: [0.2, 0.3, 0.4, 0.],
                        additional_ambient: [0.; 4],
                        directional_diffuse: [0.8, 0.7, 0.6, 1.],
                        point_ambient: [0.1, 0.2, 0.3, 0.],
                        point_diffuse: [0.4, 0.3, 0.2, 1.],
                        transparency: 0.,
                        point_enabled: true,
                    });
                    if !c
                        .ambient
                        .iter()
                        .chain(c.directional_diffuse.iter())
                        .chain(c.point_ambient.iter())
                        .chain(c.point_diffuse.iter())
                        .all(|v| v.is_finite())
                        || c.point_diffuse[3] != 0.
                    {
                        failures.push(format!("{path}: invalid material binding"));
                    }
                }
            }
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    println!(
        "{} sampled poses; {posed_vertices} finite vertex deformations",
        sampled_poses.values().map(Vec::len).sum::<usize>()
    );
    println!("SKI versions {skin_versions:?}; {skin_meshes} skinned / {rigid_meshes} rigid meshes; {material_count} materials");
    println!(
        "{} placements, {} ECMs, {} SMDs, {} unique SKIs; SMD versions {:?}",
        placed.len(),
        ecms.len(),
        smds.len(),
        skins.len(),
        versions
    );
    println!("{} unresolved dependencies", missing.len());
    for x in &missing {
        println!("  {x}")
    }
    failures.sort();
    for x in &failures {
        eprintln!("  {x}")
    }
    if !missing.is_empty() || !failures.is_empty() {
        return Err(format!(
            "{} unresolved dependencies; {} skeleton/track/skin/material failures",
            missing.len(),
            failures.len()
        ));
    }
    println!(
        "Dependency, track-sampling and skin-material checks pass; world rendering is not checked by this survey."
    );
    Ok(())
}
