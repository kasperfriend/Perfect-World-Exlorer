//! Assemble a complete PW player from the same body, face, equipment, and
//! weapon resources used by CECPlayer/CECFace.
//!
//! Geometry is never substituted silently. Required body/face/item resources
//! fail with their actual path; optional face additions retain the base skin
//! and produce an explicit warning when their authentic resource is absent.

use std::collections::HashMap;

use glam::{Mat4, Vec3};

use super::{
    assets::Assets,
    catalog::{ArmorSlot, Catalog, Class, FaceTexture, Gender},
    customize::CharacterDesign,
    face::{self, FaceConfig, FaceResources},
};
use crate::{
    ecmodel,
    model_asset::{self, ModelAsset, PosedMesh, SkinAsset},
    modules::walker::pose::{self, BoneControl, BoneScale, ScaleType},
    skeleton::Skeleton,
    skeleton_pose::Pose,
    skin,
};

#[derive(Debug)]
pub struct EquipmentPart {
    pub target: ArmorSlot,
    pub item_id: u32,
    pub equip_location: i32,
    pub path: String,
    pub visible: bool,
}

#[derive(Debug, Clone)]
pub struct FaceTextureStage {
    pub skin: usize,
    pub mesh: usize,
    pub base: String,
    /// Native stage 1/2 texture. The retained ordinary renderer currently
    /// consumes `base`; keeping the authentic high texture here prevents the
    /// assembly layer from discarding data needed by the face-shader renderer.
    pub high: Option<String>,
}

#[derive(Debug, Default, Clone, Copy)]
pub struct FaceSkinSlots {
    pub face: usize,
    pub hair: Option<usize>,
    pub goatee: Option<usize>,
    pub faling: Option<usize>,
}

pub struct WeaponModel {
    pub asset: ModelAsset,
    pub hook: &'static str,
}

pub struct Character {
    pub design: CharacterDesign,
    pub body: ModelAsset,
    pub face: ModelAsset,
    pub weapons: Vec<WeaponModel>,
    pub equipment: Vec<EquipmentPart>,
    pub face_skins: FaceSkinSlots,
    pub face_texture_stages: Vec<FaceTextureStage>,
    pub warnings: Vec<String>,
    body_scales: Vec<BoneScale>,
    face_controls: Vec<BoneControl>,
    body_head: usize,
    face_neck: usize,
    original_head: Mat4,
    original_neck: Mat4,
}

pub struct CharacterPose {
    pub body: Pose,
    pub face: Pose,
    pub weapons: Vec<Pose>,
}

pub struct CharacterMeshes {
    pub body: Vec<PosedMesh>,
    pub face: Vec<PosedMesh>,
    pub weapons: Vec<Vec<PosedMesh>>,
}

impl Character {
    pub fn assemble(design: CharacterDesign, assets: &mut Assets) -> Result<Self, String> {
        if !design.class.allows(design.gender) {
            return Err(format!(
                "{} does not support {} characters",
                design.class.name(),
                design.gender.name()
            ));
        }
        if !assets
            .body_variants(design.class, design.gender)
            .contains(&design.customize.body_id)
        {
            return Err(format!(
                "body ID {} does not have a complete authentic body/head/face set",
                design.customize.body_id
            ));
        }
        let mut warnings = Vec::new();
        let mut effective_design = design.clone();
        apply_head_hair_override(&mut effective_design, &design, &assets.catalog)?;

        let equipment = plan_equipment(&design, &assets.catalog)?;
        let body_path = design.class.body_model(design.gender);
        let mut body = ModelAsset::load_with(&body_path, |path| assets.read(path))?;

        // SMD owns the skeleton/actions; CECPlayer replaces its skin slots.
        // Keep those native slots isolated rather than modifying shared skin
        // data or inventing a combined equipment skeleton.
        let body_skin_path = design
            .class
            .body_skin(design.gender, design.customize.body_id);
        let head_skin_path = design
            .class
            .head_skin(design.gender, design.customize.body_id);
        let head_skin = load_skin(assets, &head_skin_path)?;
        let head_mesh_count = head_skin.data.meshes.len();
        let mut body_skins = vec![load_skin(assets, &body_skin_path)?, head_skin];
        let mut body_skin_paths = vec![body_skin_path, head_skin_path];
        for part in &equipment {
            match load_skin(assets, &part.path) {
                Ok(skin) if part.visible => {
                    body_skin_paths.push(part.path.clone());
                    body_skins.push(skin);
                }
                Ok(_) => {}
                Err(error) if part.item_id != 0 => {
                    return Err(format!("equipment {}: {error}", part.item_id));
                }
                Err(error) => warnings.push(format!(
                    "default {} skin {} could not be loaded: {error}; leaving the underlying body visible",
                    part.target.name(),
                    part.path
                )),
            }
        }
        body.skins = body_skins;
        body.model.skins = body_skin_paths;
        body.texture_overrides = HashMap::new();
        body.mesh_tints = HashMap::new();
        body.hidden_meshes = (0..head_mesh_count).map(|mesh| (1, mesh)).collect();
        let body_color = if design.class == Class::Barbarian {
            // CECPlayer::SetBodyColor explicitly disables Orc skin tinting.
            u32::MAX
        } else {
            design.customize.color_body
        };
        if let Some(base) = body.skins.first() {
            for (mesh, data) in base.data.meshes.iter().enumerate() {
                if data.texture == 0 {
                    body.mesh_tints.insert((0, mesh), body_color);
                }
            }
        }

        let face_bon = design
            .class
            .face_asset(design.gender, design.customize.body_id, "bon");
        let face_ski = design
            .class
            .face_asset(design.gender, design.customize.body_id, "ski");
        let mut face_model = ModelAsset::load_static_skins_with(
            &face_bon,
            std::slice::from_ref(&face_ski),
            |path| assets.read(path),
        )?;
        let mut face_skins = FaceSkinSlots::default();
        let customized = &effective_design.customize.face;

        if customized.id_hair_model != 0 {
            match assets
                .catalog
                .hair_item(customized.id_hair_model as u32)
                .cloned()
            {
                Some(item) => {
                    if let Some(skin) = load_optional_face_skin(
                        assets,
                        &item.skin,
                        &format!("hair model {}", item.id),
                        &mut warnings,
                    ) {
                        let index = face_model.skins.len();
                        face_model.model.skins.push(item.skin.clone());
                        face_model.skins.push(skin);
                        face_skins.hair = Some(index);
                    }
                }
                None => warnings.push(format!(
                    "hair model ID {} is absent from elements.data; no hair skin was added",
                    customized.id_hair_model
                )),
            }
        }
        if design.gender == Gender::Male && customized.id_moustache_skin != 0 {
            match assets
                .catalog
                .moustache_item(customized.id_moustache_skin as u32)
                .cloned()
            {
                Some(item) => {
                    if let Some(skin) = load_optional_face_skin(
                        assets,
                        &item.path,
                        &format!("goatee skin {}", item.id),
                        &mut warnings,
                    ) {
                        let index = face_model.skins.len();
                        face_model.model.skins.push(item.path.clone());
                        face_model.skins.push(skin);
                        face_skins.goatee = Some(index);
                    }
                }
                None => warnings.push(format!(
                    "goatee skin ID {} is absent from elements.data; no goatee skin was added",
                    customized.id_moustache_skin
                )),
            }
        }
        if design.gender == Gender::Male
            && design.class != Class::Barbarian
            && customized.id_faling_skin != 0
        {
            match assets
                .catalog
                .faling_item(customized.id_faling_skin as u32)
                .cloned()
            {
                Some(item) => {
                    if let Some(skin) = load_optional_face_skin(
                        assets,
                        &item.path,
                        &format!("faling skin {}", item.id),
                        &mut warnings,
                    ) {
                        let index = face_model.skins.len();
                        face_model.model.skins.push(item.path.clone());
                        face_model.skins.push(skin);
                        face_skins.faling = Some(index);
                    }
                }
                None => warnings.push(format!(
                    "faling skin ID {} is absent from elements.data; no faling skin was added",
                    customized.id_faling_skin
                )),
            }
        }

        let mut texture_stages = Vec::new();
        apply_face_textures_and_colors(
            &effective_design,
            &face_skins,
            &mut face_model,
            assets,
            &mut texture_stages,
            &mut warnings,
        )?;

        let face_config = FaceConfig::load(assets)?;
        let face_resources = FaceResources::load(&effective_design, assets, &mut warnings);
        let rig = face::evaluate_face(
            &face_model.skeleton,
            face_config,
            &effective_design,
            &face_resources,
        )?;
        let face_controls = rig
            .controlled_bones()
            .map(|(bone, matrix)| BoneControl { bone, matrix })
            .collect::<Vec<_>>();

        let body_head = bone_index(&body.skeleton, "Bip01 Head")
            .ok_or("body skeleton has no Bip01 Head attachment bone")?;
        let face_neck = bone_index(&face_model.skeleton, "脖子对齐")
            .ok_or("face skeleton has no 脖子对齐 attachment bone")?;
        let original_head = Mat4::from_cols_array(&body.skeleton.bones[body_head].relative);
        let original_neck = Mat4::from_cols_array(&face_model.skeleton.bones[face_neck].relative);
        if !original_head.inverse().is_finite() || !original_neck.is_finite() {
            return Err("non-invertible/non-finite native head attachment matrix".into());
        }
        let body_scales = body_scales(&body.skeleton, &design);

        let mut weapons = Vec::new();
        if design.equipment.weapon != 0 {
            let weapon = assets
                .catalog
                .weapon(design.equipment.weapon)
                .cloned()
                .ok_or_else(|| {
                    format!(
                        "weapon ID {} is absent from PW 1.4.2 elements.data",
                        design.equipment.weapon
                    )
                })?;
            for (path, hook) in [
                (weapon.right_model, "HH_righthandweapon"),
                (weapon.left_model, "HH_lefthandweapon"),
            ] {
                if path.is_empty() {
                    continue;
                }
                let asset = ModelAsset::load_with(&path, |path| assets.read(path))
                    .map_err(|error| format!("weapon {} ({path}): {error}", weapon.id))?;
                weapons.push(WeaponModel { asset, hook });
            }
        }
        if customized.id_third_eye != 0 {
            warnings.push(format!(
                "third-eye GFX ID {} is retained in the design but is not part of the static character assembly",
                customized.id_third_eye
            ));
        }

        Ok(Self {
            design,
            body,
            face: face_model,
            weapons,
            equipment,
            face_skins,
            face_texture_stages: texture_stages,
            warnings,
            body_scales,
            face_controls,
            body_head,
            face_neck,
            original_head,
            original_neck,
        })
    }

    /// Evaluate the authored idle/bind pose, including body scales, face-bone
    /// controllers, head attachment, and weapon hooks.
    pub fn bind_pose(&self, world: Mat4) -> Result<CharacterPose, String> {
        let body = pose::evaluate_controlled(
            &self.body.skeleton,
            &[],
            0.0,
            world,
            &[],
            &self.body_scales,
        )?;
        // Native: InverseTM(originalHead) * originalNeck * headAbsolute.
        // All matrices here are transposed native matrices, so reverse the
        // product while preserving the original (not animated) bind offsets.
        let face_world =
            body.absolute[self.body_head] * self.original_neck * self.original_head.inverse();
        if !face_world.is_finite() {
            return Err("non-finite face attachment transform".into());
        }
        let face = pose::evaluate_controlled(
            &self.face.skeleton,
            &[],
            0.0,
            face_world,
            &self.face_controls,
            &[],
        )?;
        let mut weapon_poses = Vec::with_capacity(self.weapons.len());
        for weapon in &self.weapons {
            let hook = model_asset::hook_matrix(&self.body.skeleton, &body, weapon.hook)?;
            weapon_poses.push(pose::evaluate_controlled(
                &weapon.asset.skeleton,
                &[],
                0.0,
                hook,
                &[],
                &[],
            )?);
        }
        Ok(CharacterPose {
            body,
            face,
            weapons: weapon_poses,
        })
    }

    pub fn meshes_from_pose(&self, pose: &CharacterPose) -> Result<CharacterMeshes, String> {
        if pose.weapons.len() != self.weapons.len() {
            return Err("weapon pose count does not match the assembled character".into());
        }
        let body = self.body.meshes_from_pose(&pose.body)?;
        let face = self.face.meshes_from_pose(&pose.face)?;
        let weapons = self
            .weapons
            .iter()
            .zip(&pose.weapons)
            .map(|(weapon, pose)| weapon.asset.meshes_from_pose(pose))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(CharacterMeshes {
            body,
            face,
            weapons,
        })
    }

    pub fn face_neck_bone(&self) -> usize {
        self.face_neck
    }
}

fn bone_index(skeleton: &Skeleton, name: &str) -> Option<usize> {
    skeleton.bones.iter().position(|bone| bone.name == name)
}

fn load_skin(assets: &mut Assets, path: &str) -> Result<SkinAsset, String> {
    let path = ecmodel::normalize(path);
    let bytes = assets.read(&path)?;
    let data = skin::Skin::parse(&bytes).map_err(|error| format!("{path}: {error}"))?;
    Ok(SkinAsset { path, data })
}

fn load_optional_face_skin(
    assets: &mut Assets,
    path: &str,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<SkinAsset> {
    if path.is_empty() {
        warnings.push(format!("{label} has an empty SKI path; no skin was added"));
        return None;
    }
    match load_skin(assets, path) {
        Ok(skin) => Some(skin),
        Err(error) => {
            warnings.push(format!(
                "{label} could not be loaded: {error}; no skin was added"
            ));
            None
        }
    }
}

fn apply_head_hair_override(
    effective: &mut CharacterDesign,
    original: &CharacterDesign,
    catalog: &Catalog,
) -> Result<(), String> {
    // Orc UpdateHairModel returns before reading helmet hair fields.
    if original.class == Class::Barbarian {
        return Ok(());
    }
    let (id, fashion) = if original.equipment.fashion_mode {
        (original.equipment.fashion_head, true)
    } else {
        (original.equipment.head, false)
    };
    if id == 0 {
        return Ok(());
    }
    if fashion {
        let item = catalog
            .fashion_item(id)
            .ok_or_else(|| format!("fashion head ID {id} is absent from elements.data"))?;
        effective.customize.face.id_hair_model = item.id_hair as u16;
        effective.customize.face.id_hair_tex = item.id_hair_texture as u16;
    } else {
        let item = catalog
            .armor_item(id)
            .ok_or_else(|| format!("armor head ID {id} is absent from elements.data"))?;
        if item.id_hair != 0 {
            effective.customize.face.id_hair_model = item.id_hair as u16;
        }
        if item.id_hair_texture != 0 {
            effective.customize.face.id_hair_tex = item.id_hair_texture as u16;
        }
    }
    Ok(())
}

fn target_index(slot: ArmorSlot) -> usize {
    match slot {
        ArmorSlot::Upper => 0,
        ArmorSlot::Wrist => 1,
        ArmorSlot::Lower => 2,
        ArmorSlot::Foot => 3,
        ArmorSlot::Head => unreachable!(),
    }
}

fn location_target(location: i32) -> Option<ArmorSlot> {
    match location {
        1 | 5 | 6 | 8 => Some(ArmorSlot::Upper),
        2 => Some(ArmorSlot::Wrist),
        3 | 7 => Some(ArmorSlot::Lower),
        4 => Some(ArmorSlot::Foot),
        _ => None,
    }
}

fn plan_equipment(
    design: &CharacterDesign,
    catalog: &Catalog,
) -> Result<Vec<EquipmentPart>, String> {
    let fashion = design.equipment.fashion_mode;
    let mut parts: [Option<EquipmentPart>; 4] = std::array::from_fn(|_| None);
    let mut show_mask = 0_u32;
    for slot in [
        ArmorSlot::Upper,
        ArmorSlot::Wrist,
        ArmorSlot::Lower,
        ArmorSlot::Foot,
    ] {
        let id = if fashion {
            design.equipment.fashion(slot)
        } else {
            design.equipment.armor(slot)
        };
        let basic_location = target_index(slot) as i32 + 1;
        if id == 0 {
            show_mask |= 1 << basic_location;
            let path = if fashion {
                design.class.default_fashion_skin(design.gender, slot)
            } else {
                design.class.default_armor_skin(design.gender, slot)
            };
            if let Some(path) = path {
                parts[target_index(slot)] = Some(EquipmentPart {
                    target: slot,
                    item_id: 0,
                    equip_location: basic_location,
                    path,
                    visible: true,
                });
            }
            continue;
        }

        let (realname, location) = if fashion {
            let item = catalog
                .fashion_item(id)
                .ok_or_else(|| format!("fashion item ID {id} is absent from elements.data"))?;
            (item.realname.clone(), item.equip_location)
        } else {
            let item = catalog
                .armor_item(id)
                .ok_or_else(|| format!("armor item ID {id} is absent from elements.data"))?;
            (item.realname.clone(), item.equip_location)
        };
        let target = location_target(location).ok_or_else(|| {
            format!("item {id} has non-body equip_location {location} in a body slot")
        })?;
        show_mask |= 1 << location;
        parts[target_index(target)] = Some(EquipmentPart {
            target,
            item_id: id,
            equip_location: location,
            // LoadPlayerSkin's close/LOD0 suffix is always 一级. Essence
            // item_level is not an LOD selector.
            path: design.class.equipment_skin(design.gender, &realname, 0),
            visible: true,
        });
    }

    let visible = equipment_visibility(show_mask);
    for (index, part) in parts.iter_mut().enumerate() {
        if let Some(part) = part {
            part.visible = visible[index];
        }
    }
    Ok(parts.into_iter().flatten().collect())
}

/// `_get_skin_show_mask` for one armor or fashion set. Array order is upper,
/// wrist, lower, foot. Composite outfits load their covered default skins but
/// hide those slots, and LowerAndFoot also hides the separately loaded boots.
fn equipment_visibility(show_mask: u32) -> [bool; 4] {
    let mut visible = [true; 4];
    let mut lower_method = 0_i32;
    for location in 1..=8 {
        if show_mask & (1 << location) == 0 {
            continue;
        }
        match location {
            1 => {
                visible[1] = true;
                visible[2] = true;
                visible[3] = lower_method != 7;
            }
            2 => {}
            3 => {
                lower_method = 3;
                visible[3] = true;
            }
            4 => {}
            5 => {
                visible[1] = true;
                visible[2] = false;
                visible[3] = true;
            }
            6 => {
                visible[1] = false;
                visible[2] = true;
                visible[3] = lower_method != 7;
            }
            7 => {
                lower_method = 7;
                visible[3] = false;
            }
            8 => {
                visible[1] = false;
                visible[2] = false;
                visible[3] = true;
            }
            _ => unreachable!(),
        }
    }
    visible
}

fn body_scales(skeleton: &Skeleton, design: &CharacterDesign) -> Vec<BoneScale> {
    fn factor(value: u8) -> f32 {
        1.0 + (value as f32 - 128.0) / 128.0 * 0.99
    }
    let data = &design.customize;
    let head = factor(data.head_scale);
    let up = factor(data.up_scale);
    let waist = factor(data.waist_scale);
    let arm = factor(data.arm_width);
    let leg = factor(data.leg_width);
    let breast = factor(data.breast_scale);
    let mut result = Vec::new();
    let mut add = |name: &str, kind: ScaleType, factor: Vec3| {
        if let Some(bone) = bone_index(skeleton, name) {
            result.push(BoneScale { bone, kind, factor });
        }
    };
    add(
        "Bip01 Neck",
        ScaleType::LocalNoOffset,
        Vec3::new(1.0, head, head),
    );
    add("Bip01 Head", ScaleType::Whole, Vec3::splat(head));
    if design.gender == Gender::Female {
        add("Bone01", ScaleType::Local, Vec3::splat(breast));
    }
    for name in ["Bip01 L Clavicle", "Bip01 R Clavicle"] {
        add(name, ScaleType::Local, Vec3::splat(up));
    }
    for name in ["Bip01 Spine2", "Bip01 Spine1"] {
        add(name, ScaleType::Local, Vec3::new(1.0, up, up));
    }
    add(
        "Bip01 Spine",
        ScaleType::LocalNoOffset,
        Vec3::new(1.0, waist, waist),
    );
    add(
        "Bip01 Pelvis",
        ScaleType::Local,
        Vec3::new(1.0, waist, waist),
    );
    for name in [
        "Bip01 L UpperArm",
        "Bip01 R UpperArm",
        "Bip01 L Forearm",
        "Bip01 R Forearm",
    ] {
        add(name, ScaleType::Local, Vec3::new(1.0, arm, arm));
    }
    for name in [
        "Bip01 L Thigh",
        "Bip01 R Thigh",
        "Bip01 L Calf",
        "Bip01 R Calf",
    ] {
        add(name, ScaleType::Local, Vec3::new(1.0, leg, leg));
    }
    result
}

#[derive(Clone)]
struct TextureChoice {
    base: String,
    high: Option<String>,
}

fn texture_choice(
    assets: &Assets,
    id: u16,
    label: &str,
    warnings: &mut Vec<String>,
) -> Option<FaceTexture> {
    if id == 0 {
        return None;
    }
    match assets.catalog.face_texture(id as u32).cloned() {
        Some(texture) => Some(texture),
        None => {
            warnings.push(format!(
                "{label} texture ID {id} is absent from elements.data; retaining the SKI texture"
            ));
            None
        }
    }
}

fn choice_base(texture: FaceTexture) -> TextureChoice {
    TextureChoice {
        base: texture.base,
        high: None,
    }
}

fn choice_base_high(texture: FaceTexture) -> TextureChoice {
    TextureChoice {
        base: texture.base,
        high: (!texture.high.is_empty()).then_some(texture.high),
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_face_textures_and_colors(
    design: &CharacterDesign,
    slots: &FaceSkinSlots,
    model: &mut ModelAsset,
    assets: &Assets,
    stages: &mut Vec<FaceTextureStage>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let data = &design.customize.face;
    let face_choice = texture_choice(assets, data.id_face_tex, "face", warnings).map(choice_base);
    let eye_base = texture_choice(assets, data.id_eye_base_tex, "eye base", warnings);
    let eye_high = texture_choice(assets, data.id_eye_high_tex, "eye highlight", warnings);
    let eye_choice = match (eye_base, eye_high) {
        (Some(base), Some(high)) if data.id_eye_base_tex != 0 && data.id_eye_high_tex != 0 => {
            Some(TextureChoice {
                base: base.base,
                high: (!high.high.is_empty()).then_some(high.high),
            })
        }
        _ => None,
    };
    let brow_choice =
        texture_choice(assets, data.id_brow_tex, "brow", warnings).map(choice_base_high);
    let nose_choice = texture_choice(assets, data.id_nose_tex, "nose", warnings).map(choice_base);
    let mouth_choice =
        texture_choice(assets, data.id_mouth_tex, "mouth", warnings).map(choice_base_high);
    let eyeball_choice =
        texture_choice(assets, data.id_eye_ball_tex, "eyeball", warnings).map(choice_base_high);
    let moustache_choice = if design.gender == Gender::Male {
        texture_choice(assets, data.id_moustache_tex, "moustache", warnings).map(choice_base_high)
    } else {
        None
    };
    let moustache_color = if design.gender == Gender::Male {
        data.color_moustache
    } else {
        u32::MAX
    };
    for (part, color, choice) in [
        (0, data.color_face, face_choice),
        (1, data.color_eye, eye_choice),
        (2, data.color_brow, brow_choice),
        (3, data.color_mouth, mouth_choice),
        (4, data.color_face, nose_choice),
        (5, data.color_eye_ball, eyeball_choice),
        (6, moustache_color, moustache_choice),
    ] {
        apply_face_part(
            model, slots.face, part, color, choice, assets, stages, warnings,
        )?;
    }

    let hair_choice =
        texture_choice(assets, data.id_hair_tex, "hair", warnings).map(choice_base_high);
    if let Some(skin) = slots.hair {
        apply_extra_texture(
            model,
            skin,
            data.color_hair,
            hair_choice,
            true,
            assets,
            stages,
            warnings,
        )?;
    }
    if let Some(skin) = slots.goatee {
        let choice =
            texture_choice(assets, data.id_goatee_tex, "goatee", warnings).map(choice_base_high);
        apply_extra_texture(
            model,
            skin,
            data.color_moustache,
            choice,
            true,
            assets,
            stages,
            warnings,
        )?;
    }
    if let Some(skin) = slots.faling {
        let choice =
            texture_choice(assets, data.id_faling_tex, "faling", warnings).map(choice_base);
        apply_extra_texture(
            model,
            skin,
            u32::MAX,
            choice,
            false,
            assets,
            stages,
            warnings,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_face_part(
    model: &mut ModelAsset,
    skin: usize,
    part: usize,
    color: u32,
    choice: Option<TextureChoice>,
    assets: &Assets,
    stages: &mut Vec<FaceTextureStage>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let name = format!("model人脸_{part}");
    let (mesh, texture) = model.skins[skin]
        .data
        .meshes
        .iter()
        .enumerate()
        .find_map(|(index, mesh)| {
            mesh.name
                .eq_ignore_ascii_case(&name)
                .then_some((index, mesh.texture))
        })
        .ok_or_else(|| format!("face SKI is missing native mesh {name}"))?;
    model.mesh_tints.insert((skin, mesh), color);
    if let Some(choice) = choice {
        apply_texture_choice(model, skin, mesh, texture, choice, assets, stages, warnings);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_extra_texture(
    model: &mut ModelAsset,
    skin: usize,
    color: u32,
    choice: Option<TextureChoice>,
    tinted: bool,
    assets: &Assets,
    stages: &mut Vec<FaceTextureStage>,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let meshes = model.skins[skin]
        .data
        .meshes
        .iter()
        .enumerate()
        .filter(|(_, mesh)| mesh.texture == 0)
        .map(|(index, mesh)| (index, mesh.texture))
        .collect::<Vec<_>>();
    if meshes.is_empty() && choice.is_some() {
        warnings.push(format!(
            "{} has no texture-0 mesh for its customized texture",
            model.skins[skin].path
        ));
    }
    for (mesh, texture) in meshes {
        if tinted {
            model.mesh_tints.insert((skin, mesh), color);
        }
        if let Some(choice) = choice.clone() {
            apply_texture_choice(model, skin, mesh, texture, choice, assets, stages, warnings);
        }
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn apply_texture_choice(
    model: &mut ModelAsset,
    skin: usize,
    mesh: usize,
    texture: i32,
    choice: TextureChoice,
    assets: &Assets,
    stages: &mut Vec<FaceTextureStage>,
    warnings: &mut Vec<String>,
) {
    if texture < 0 {
        warnings.push(format!(
            "{} mesh {mesh} is untextured; customized texture was not applied",
            model.skins[skin].path
        ));
        return;
    }
    let base = ecmodel::normalize(&choice.base);
    if base.is_empty() || !assets.contains(&base) {
        warnings.push(format!(
            "missing customized texture {base}; retaining the SKI texture"
        ));
        return;
    }
    let high = choice
        .high
        .map(|path| ecmodel::normalize(&path))
        .filter(|path| !path.is_empty());
    let high = high.and_then(|path| {
        if assets.contains(&path) {
            Some(path)
        } else {
            warnings.push(format!(
                "missing customized highlight texture {path}; highlight stage was not applied"
            ));
            None
        }
    });
    model
        .texture_overrides
        .insert((skin, texture as usize), base.clone());
    stages.push(FaceTextureStage {
        skin,
        mesh,
        base,
        high,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_equipment_matches_native_show_masks() {
        assert_eq!(equipment_visibility(1 << 1), [true, true, true, true]);
        assert_eq!(equipment_visibility(1 << 5), [true, true, false, true]);
        assert_eq!(equipment_visibility(1 << 6), [true, false, true, true]);
        assert_eq!(equipment_visibility(1 << 8), [true, false, false, true]);
        assert_eq!(
            equipment_visibility((1 << 3) | (1 << 7)),
            [true, true, true, false]
        );
        assert_eq!(
            equipment_visibility((1 << 7) | (1 << 1)),
            [true, true, true, false]
        );
    }

    #[test]
    fn native_head_attachment_product_is_reversed_for_glam() {
        let head = Mat4::from_translation(Vec3::new(0.0, 1.0, 0.0));
        let neck = Mat4::from_translation(Vec3::new(0.0, 0.2, 0.1));
        let absolute = Mat4::from_translation(Vec3::new(4.0, 3.0, 2.0)) * head;
        let face = absolute * neck * head.inverse();
        assert_eq!(face.w_axis.truncate(), Vec3::new(4.0, 3.2, 2.1));
    }
}
