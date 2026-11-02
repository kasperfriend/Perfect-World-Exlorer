#![allow(dead_code)]
#[path = "../animation.rs"]
mod animation;
#[path = "../ecmodel.rs"]
mod ecmodel;
#[path = "../model_material.rs"]
mod model_material;
#[path = "../pck.rs"]
mod pck;
#[path = "../scene.rs"]
mod scene;
#[path = "../skeleton.rs"]
mod skeleton;
#[path = "../skin.rs"]
mod skin;
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
    let mut versions = BTreeMap::new();
    for m in s.blocks.iter().flat_map(|b| &b.models) {
        if !placed.insert(m.export_id) {
            continue;
        }
        let key = ecmodel::normalize(&m.path);
        if !ecms.insert(key.clone()) {
            continue;
        }
        let entry = match entries.get(&key) {
            Some(e) => e,
            None => {
                missing.push(key);
                continue;
            }
        };
        let ec = ecmodel::parse_ecm(&pck::read_entry(&mut f, entry)?)?;
        let path = ec.skin_model_path;
        if !smds.insert(path.clone()) {
            continue;
        }
        let ent = match entries.get(&path) {
            Some(e) => e,
            None => {
                missing.push(path);
                continue;
            }
        };
        let smd = ecmodel::parse_smd(&pck::read_entry(&mut f, ent)?)?;
        *versions.entry(smd.version).or_insert(0usize) += 1;
        let parent = path.rsplit_once('/').map(|x| x.0).unwrap_or("");
        for refname in std::iter::once(smd.skeleton).chain(smd.skins) {
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
                skins.insert(target);
            }
        }
    }
    let (mut skin_meshes, mut rigid_meshes, mut material_count) = (0, 0, 0);
    let mut skin_versions = BTreeMap::new();
    let mut failures = Vec::new();
    let mut skeleton_versions = BTreeMap::new();
    let (mut bones, mut embedded_tracks) = (0, 0);
    for path in &skeleton_paths {
        let Some(entry) = entries.get(path) else {
            continue;
        };
        match pck::read_entry(&mut f, entry).and_then(|b| skeleton::Skeleton::parse(&b)) {
            Ok(s) => {
                *skeleton_versions.entry(s.version).or_insert(0usize) += 1;
                bones += s.bones.len();
                embedded_tracks += s.embedded_tracks.len();
            }
            Err(e) => failures.push(format!("{path}: {e}")),
        }
    }
    println!(
        "BON versions {skeleton_versions:?}; {bones} bones; {embedded_tracks} embedded tracks"
    );
    for path in &skins {
        let Some(entry) = entries.get(path) else {
            continue;
        };
        match pck::read_entry(&mut f, entry).and_then(|b| skin::Skin::parse(&b)) {
            Ok(s) => {
                *skin_versions.entry(s.version).or_insert(0usize) += 1;
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
            "{} unresolved dependencies; {} skeleton/skin/material failures",
            missing.len(),
            failures.len()
        ));
    }
    println!(
        "Dependency and skin-material checks pass; world rendering is not checked by this survey."
    );
    Ok(())
}
