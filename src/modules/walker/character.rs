//! Real player ECM/SMD/BON/SKI assembly and direct base-action playback.

use super::physics::Body;
use crate::{
    ecmodel,
    model_asset::ModelAsset,
    modules::designer::{
        assembly::{Character as Assembly, CharacterMeshes, CharacterPose},
        assets::Assets,
        catalog::{ArmorSlot, Class, Gender},
        customize::CharacterDesign,
    },
};
use glam::{Mat4, Vec3};

const HEAD_EYE_OFFSET: f32 = 0.075;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    Stand,
    Run,
    JumpStart,
    JumpLoop,
    Land,
}

struct ActionSet {
    stand: Option<usize>,
    run: Option<usize>,
    jump_start: Option<usize>,
    jump_loop: Option<usize>,
    land: Option<usize>,
}

impl ActionSet {
    fn new(
        combined: &[ecmodel::CombinedAction],
        actions: &[ecmodel::Action],
    ) -> (Self, Vec<String>) {
        fn resolve(
            combined: &[ecmodel::CombinedAction],
            actions: &[ecmodel::Action],
            wanted: &str,
        ) -> Option<usize> {
            if let Some(combined) = combined.iter().find(|action| action.name == wanted) {
                // These locomotion states are single-base native actions. Do
                // not flatten a multi-stage action or substitute a broken
                // binding variant for the ordinary unarmed one.
                let [base] = combined.bases.as_slice() else {
                    return None;
                };
                return actions.iter().position(|action| action.name == base.name);
            }
            actions.iter().position(|action| action.name == wanted)
        }
        fn find(
            combined: &[ecmodel::CombinedAction],
            actions: &[ecmodel::Action],
            names: &[&str],
        ) -> Option<usize> {
            names
                .iter()
                .find_map(|wanted| resolve(combined, actions, wanted))
        }
        let set = Self {
            stand: find(
                combined,
                actions,
                &["站立_通用", "站立_空手", "站立_徒手", "站立_法宝", "站立"],
            ),
            run: find(
                combined,
                actions,
                &[
                    "奔跑_通用",
                    "跑步_通用",
                    "奔跑_空手",
                    "跑步_空手",
                    "跑_空手",
                    "奔跑_法宝",
                    "奔跑",
                    "跑步",
                ],
            ),
            jump_start: find(
                combined,
                actions,
                &["起跳_通用", "起跳_空手", "跳跃开始", "起跳"],
            ),
            jump_loop: find(
                combined,
                actions,
                &[
                    "跳跃空中循环_通用",
                    "跳跃_空手",
                    "空中_空手",
                    "跳跃",
                    "空中",
                ],
            ),
            land: find(
                combined,
                actions,
                &["跳跃落地_通用", "落地_空手", "跳跃结束", "落地"],
            ),
        };
        let mut warnings = Vec::new();
        for (name, present) in [
            ("stand", set.stand),
            ("run", set.run),
            ("jump start", set.jump_start),
            ("jump loop", set.jump_loop),
            ("landing", set.land),
        ] {
            if present.is_none() {
                warnings.push(format!(
                    "No authored unarmed {name} base action was found; that state uses the bind pose."
                ));
            }
        }
        (set, warnings)
    }

    fn get(&self, motion: Motion) -> Option<usize> {
        match motion {
            Motion::Stand => self.stand,
            Motion::Run => self.run,
            Motion::JumpStart => self.jump_start,
            Motion::JumpLoop => self.jump_loop,
            Motion::Land => self.land,
        }
    }
}

/// Direct SMD action clock with the same truncated millisecond endpoints and
/// per-tick speed truncation as the recovered ordinary model playback path.
struct ClipPlayer {
    motion: Motion,
    action: Option<usize>,
    start: u32,
    end: u32,
    current: u32,
    looping: bool,
    speed: f32,
}

impl ClipPlayer {
    fn new(asset: &ModelAsset, motion: Motion, action: Option<usize>) -> Result<Self, String> {
        let speed = asset.descriptor.render_settings().default_speed;
        if !speed.is_finite() || speed < 0. {
            return Err("invalid character action speed".into());
        }
        let (start, end) = match action {
            Some(index) => {
                let clip = asset
                    .model
                    .actions
                    .get(index)
                    .ok_or("character action index out of range")?;
                let fps = clip
                    .fps
                    .filter(|fps| *fps > 0)
                    .unwrap_or(asset.skeleton.fps);
                if fps <= 0
                    || !clip.start.is_finite()
                    || !clip.end.is_finite()
                    || clip.start < 0.
                    || clip.end < clip.start
                {
                    return Err("invalid character action range/frame rate".into());
                }
                let start = (clip.start * 1000. / fps as f32).trunc();
                let end = (clip.end * 1000. / fps as f32).trunc();
                if end >= u32::MAX as f32 {
                    return Err("character action millisecond range overflow".into());
                }
                (start as u32, end as u32)
            }
            None => (0, 0),
        };
        Ok(Self {
            motion,
            action,
            start,
            end,
            current: start,
            looping: matches!(motion, Motion::Stand | Motion::Run | Motion::JumpLoop),
            speed,
        })
    }

    fn set(
        &mut self,
        asset: &ModelAsset,
        motion: Motion,
        action: Option<usize>,
        force: bool,
    ) -> Result<(), String> {
        if force || self.motion != motion || self.action != action {
            *self = Self::new(asset, motion, action)?;
        }
        Ok(())
    }

    fn tick(&mut self, delta_ms: u32) {
        let Some(_) = self.action else {
            return;
        };
        let scaled = delta_ms as f32 * self.speed;
        if !scaled.is_finite() || scaled >= u32::MAX as f32 {
            return;
        }
        let advance = scaled as u32;
        let absolute = self.current as u64 + advance as u64;
        if absolute <= self.end as u64 {
            self.current = absolute as u32;
        } else if self.looping {
            let duration = (self.end - self.start) as u64 + 1;
            if duration == 1 {
                self.current = self.end;
            } else {
                let wraps = (absolute - self.start as u64) / duration;
                self.current = (absolute - wraps * duration) as u32;
            }
        } else {
            self.current = self.end;
        }
    }

    fn finished(&self) -> bool {
        self.action
            .map_or(true, |_| !self.looping && self.current >= self.end)
    }

    fn sample(&self) -> Option<(usize, f32)> {
        self.action
            .map(|action| (action, self.current as f32 * 0.001))
    }
}

pub struct Posed {
    pub pose: CharacterPose,
    pub meshes: CharacterMeshes,
}

/// Native zero armor IDs select CECPlayer's profession-specific default SKI
/// set. Explicitly use that set for a new Walker instead of leaving the base
/// body visible or starting in fashion mode.
pub(super) fn starter_design(
    assets: &mut Assets,
    class: Class,
    gender: Gender,
) -> Result<CharacterDesign, String> {
    let mut design = assets.default_design(class, gender)?;
    design.equipment.fashion_mode = false;
    for slot in [
        ArmorSlot::Upper,
        ArmorSlot::Wrist,
        ArmorSlot::Lower,
        ArmorSlot::Foot,
    ] {
        design.equipment.set_armor(slot, 0);
    }
    Ok(design)
}

pub struct Character {
    pub assembly: Assembly,
    pub posed: Posed,
    pub eye: Vec3,
    pub warnings: Vec<String>,
    actions: ActionSet,
    player: ClipPlayer,
    head_bone: Option<usize>,
    last_jump: u64,
    last_landing: u64,
}

impl Character {
    pub fn load(assets: &mut Assets, design: CharacterDesign, world: Mat4) -> Result<Self, String> {
        let assembly = Assembly::assemble(design, assets)?;
        let (actions, action_warnings) = ActionSet::new(
            &assembly.body.descriptor.actions,
            &assembly.body.model.actions,
        );
        let player = ClipPlayer::new(&assembly.body, Motion::Stand, actions.stand)?;
        let head_bone = assembly
            .body
            .skeleton
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case("Bip01 Head"))
            .or_else(|| {
                assembly.body.skeleton.bones.iter().position(|bone| {
                    let name = bone.name.to_ascii_lowercase();
                    name.contains("head") && !name.contains("nub")
                })
            });
        let posed = evaluate(&assembly, player.sample(), world)?;
        let eye = eye_position(&posed, head_bone)?;
        let mut warnings = assembly.warnings.clone();
        warnings.extend(action_warnings);
        Ok(Self {
            assembly,
            posed,
            eye,
            warnings,
            actions,
            player,
            head_bone,
            last_jump: 0,
            last_landing: 0,
        })
    }

    pub fn update(&mut self, delta: f32, body: &Body, facing: Vec3) -> Result<(), String> {
        let jumped = self.last_jump != body.jump_generation;
        let landed = self.last_landing != body.landed_generation;
        self.last_jump = body.jump_generation;
        self.last_landing = body.landed_generation;
        if jumped {
            self.player.set(
                &self.assembly.body,
                Motion::JumpStart,
                self.actions.jump_start,
                true,
            )?;
        } else if landed && self.actions.land.is_some() {
            self.player
                .set(&self.assembly.body, Motion::Land, self.actions.land, true)?;
        }

        if !body.grounded {
            if self.player.motion != Motion::JumpStart || self.player.finished() {
                self.player.set(
                    &self.assembly.body,
                    Motion::JumpLoop,
                    self.actions.jump_loop,
                    false,
                )?;
            }
        } else if self.player.motion == Motion::Land && !self.player.finished() {
            // Let the authored landing action finish before standing/running.
        } else {
            let motion = if body.moving {
                Motion::Run
            } else {
                Motion::Stand
            };
            self.player
                .set(&self.assembly.body, motion, self.actions.get(motion), false)?;
        }
        self.player.tick((delta.max(0.) * 1000.) as u32);
        let world = character_transform(body.position, facing);
        let posed = evaluate(&self.assembly, self.player.sample(), world)?;
        self.eye = eye_position(&posed, self.head_bone)?;
        self.posed = posed;
        Ok(())
    }
}

pub fn character_transform(position: Vec3, facing: Vec3) -> Mat4 {
    let direction = Vec3::new(facing.x, 0., facing.z).normalize_or_zero();
    let direction = if direction == Vec3::ZERO {
        Vec3::NEG_Z
    } else {
        direction
    };
    let right = Vec3::Y.cross(direction).normalize_or_zero();
    Mat4::from_cols(
        right.extend(0.),
        Vec3::Y.extend(0.),
        direction.extend(0.),
        position.extend(1.),
    )
}

fn evaluate(
    assembly: &Assembly,
    sample: Option<(usize, f32)>,
    world: Mat4,
) -> Result<Posed, String> {
    let pose = if let Some((action, seconds)) = sample {
        assembly.action_pose(action, seconds, world)?
    } else {
        assembly.bind_pose(world)?
    };
    let meshes = assembly.meshes_from_pose(&pose)?;
    Ok(Posed { pose, meshes })
}

/// `Pose.absolute` is the model/world-transformed bone matrix. `Pose.skin`
/// additionally contains inverse bind and is intentionally not used here.
fn eye_position(posed: &Posed, head_bone: Option<usize>) -> Result<Vec3, String> {
    if let Some(index) = head_bone {
        let matrix = posed
            .pose
            .body
            .absolute
            .get(index)
            .ok_or("character head bone is outside the pose")?;
        return Ok(matrix.transform_point3(Vec3::ZERO) + Vec3::Y * HEAD_EYE_OFFSET);
    }
    // Unusual rigs without a head bone still use measured geometry, never a
    // race-specific guessed height.
    let top = posed
        .meshes
        .body
        .iter()
        .flat_map(|mesh| &mesh.vertices)
        .filter_map(|vertex| {
            let y = vertex.position[1];
            y.is_finite().then_some(y)
        })
        .fold(f32::NEG_INFINITY, f32::max);
    if !top.is_finite() {
        return Err("character has neither a head bone nor measurable geometry".into());
    }
    let origin = posed.pose.body.model.transform_point3(Vec3::ZERO);
    Ok(Vec3::new(origin.x, top - 0.12, origin.z))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn action(name: &str) -> ecmodel::Action {
        ecmodel::Action {
            name: name.into(),
            start: 0.,
            end: 1.,
            fps: Some(30),
            track_file: String::new(),
            joints: vec![],
        }
    }

    #[test]
    fn action_selection_uses_authored_exact_names_not_substrings() {
        let actions = vec![
            action("站立_空手"),
            action("奔跑_空手"),
            action("起跳"),
            action("跳跃"),
            action("落地"),
        ];
        let (set, warnings) = ActionSet::new(&[], &actions);
        assert_eq!(set.stand, Some(0));
        assert_eq!(set.run, Some(1));
        assert_eq!(set.jump_start, Some(2));
        assert_eq!(set.jump_loop, Some(3));
        assert_eq!(set.land, Some(4));
        assert!(warnings.is_empty());
        let (missing, _) = ActionSet::new(&[], &[action("not 站立_空手")]);
        assert_eq!(missing.stand, None);
    }

    #[test]
    fn native_combined_actions_resolve_to_real_direct_clips() {
        fn combined(name: &str, base: &str) -> ecmodel::CombinedAction {
            ecmodel::CombinedAction {
                name: name.into(),
                bases: vec![ecmodel::BaseAction {
                    name: base.into(),
                    ..Default::default()
                }],
                ..Default::default()
            }
        }
        let actions = vec![
            action("妖兽男_站立_通用"),
            action("妖兽男_奔跑_通用"),
            action("跳跃双手短_起"),
            action("跳跃双手短_循环"),
            action("跳跃双手短_落"),
        ];
        let combined = vec![
            combined("站立_通用", "妖兽男_站立_通用"),
            combined("奔跑_通用", "妖兽男_奔跑_通用"),
            combined("起跳_通用", "跳跃双手短_起"),
            combined("跳跃空中循环_通用", "跳跃双手短_循环"),
            combined("跳跃落地_通用", "跳跃双手短_落"),
            // This stale Psychic binding reference must not be needed by
            // ordinary Walker locomotion or make the whole model unloadable.
            combined("跳跃落地_绑定_通用", "落地_绑定"),
        ];
        let (set, warnings) = ActionSet::new(&combined, &actions);
        assert_eq!(set.stand, Some(0));
        assert_eq!(set.run, Some(1));
        assert_eq!(set.jump_start, Some(2));
        assert_eq!(set.jump_loop, Some(3));
        assert_eq!(set.land, Some(4));
        assert!(warnings.is_empty());
    }

    #[test]
    fn authentic_starter_designs_cover_every_native_class_and_gender() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("configs.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let mut count = 0;
        for class in Class::ALL {
            for gender in Gender::ALL {
                if !class.allows(gender) {
                    continue;
                }
                let design = starter_design(&mut assets, class, gender).unwrap();
                assert!(!design.equipment.fashion_mode);
                for slot in [
                    ArmorSlot::Upper,
                    ArmorSlot::Wrist,
                    ArmorSlot::Lower,
                    ArmorSlot::Foot,
                ] {
                    assert_eq!(design.equipment.armor(slot), 0);
                }
                count += 1;
            }
        }
        assert_eq!(count, 14);
    }

    #[test]
    fn authentic_walkers_load_tideborn_with_starter_armor_and_animation() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let mut evaluated = 0;
        for (class, gender) in [
            (Class::Blademaster, Gender::Male),
            (Class::Barbarian, Gender::Male),
            (Class::Cleric, Gender::Male),
            (Class::Psychic, Gender::Male),
        ] {
            let design = starter_design(&mut assets, class, gender).unwrap();
            assert!(!design.equipment.fashion_mode);
            let mut character = Character::load(&mut assets, design, Mat4::IDENTITY)
                .unwrap_or_else(|error| panic!("{} Walker: {error}", class.name()));
            assert_eq!(character.assembly.equipment.len(), 4);
            assert!(
                character.assembly.equipment.iter().all(|part| part.visible),
                "{} starter armor is not visible",
                class.name()
            );
            assert!(character.assembly.body.skins.len() > 2);
            assert!(!character.posed.meshes.body.is_empty());
            assert!(!character.posed.meshes.face.is_empty());
            assert!(
                [
                    character.actions.stand,
                    character.actions.run,
                    character.actions.jump_start,
                    character.actions.jump_loop,
                    character.actions.land,
                ]
                .iter()
                .all(Option::is_some),
                "{} is missing native locomotion actions: {:?}",
                class.name(),
                character.warnings
            );
            character.player.tick(34);
            let posed = evaluate(
                &character.assembly,
                character.player.sample(),
                Mat4::IDENTITY,
            )
            .unwrap_or_else(|error| panic!("{} animated pose: {error}", class.name()));
            for vertex in posed
                .meshes
                .body
                .iter()
                .chain(&posed.meshes.face)
                .flat_map(|mesh| &mesh.vertices)
            {
                assert!(vertex.position.iter().all(|value| value.is_finite()));
            }
            evaluated += 1;
        }
        assert_eq!(evaluated, 4);
    }

    #[test]
    fn transform_keeps_feet_at_body_position_and_faces_movement() {
        let matrix = character_transform(Vec3::new(3., 8., -2.), Vec3::X);
        assert_eq!(matrix.transform_point3(Vec3::ZERO), Vec3::new(3., 8., -2.));
        assert_eq!(matrix.transform_vector3(Vec3::Z), Vec3::X);
    }
}
