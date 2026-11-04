//! Real player ECM/SMD/BON/SKI assembly and direct base-action playback.

use super::{archive::Archive, physics::Body, pose, race::Race};
use crate::{
    ecmodel,
    model_asset::{ModelAsset, PosedMesh, SkinAsset},
    skeleton_pose::Pose,
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
    fn new(actions: &[ecmodel::Action]) -> (Self, Vec<String>) {
        fn find(actions: &[ecmodel::Action], names: &[&str]) -> Option<usize> {
            names
                .iter()
                .find_map(|wanted| actions.iter().position(|action| action.name == *wanted))
        }
        let set = Self {
            stand: find(actions, &["站立_空手", "站立_徒手", "站立"]),
            run: find(
                actions,
                &["奔跑_空手", "跑步_空手", "跑_空手", "奔跑", "跑步"],
            ),
            jump_start: find(actions, &["起跳_空手", "跳跃开始", "起跳"]),
            jump_loop: find(actions, &["跳跃_空手", "空中_空手", "跳跃", "空中"]),
            land: find(actions, &["落地_空手", "跳跃结束", "落地"]),
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
    pub pose: Pose,
    pub meshes: Vec<PosedMesh>,
}

pub struct Character {
    pub asset: ModelAsset,
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
    pub fn load(archive: &mut Archive, race: Race, world: Mat4) -> Result<Self, String> {
        let mut asset = ModelAsset::load_with(race.model_path(), |path| archive.read(path))?;
        // CECPlayer loads this ECM with LSF_NOSKIN and then installs body slot 0
        // and head slot 1. Do not draw any SMD default skin in addition to them.
        asset.skins.clear();
        for (slot, path) in [race.body_skin_path(), race.head_skin_path()]
            .into_iter()
            .enumerate()
        {
            let normalized = ecmodel::normalize(path);
            let bytes = archive
                .read(&normalized)
                .map_err(|e| format!("{normalized}: {e}"))?;
            let mut data =
                crate::skin::Skin::parse(&bytes).map_err(|e| format!("{normalized}: {e}"))?;
            if slot == 0 {
                // CECPlayer::LoadBodySkin's default body material override.
                for material in &mut data.materials {
                    material.specular = [58. / 255., 58. / 255., 58. / 255., 1.];
                    material.power = 10.;
                }
            }
            asset.skins.push(SkinAsset {
                path: normalized,
                data,
            });
        }
        let (actions, warnings) = ActionSet::new(&asset.model.actions);
        let player = ClipPlayer::new(&asset, Motion::Stand, actions.stand)?;
        let head_bone = asset
            .skeleton
            .bones
            .iter()
            .position(|bone| bone.name.eq_ignore_ascii_case("Bip01 Head"))
            .or_else(|| {
                asset.skeleton.bones.iter().position(|bone| {
                    let name = bone.name.to_ascii_lowercase();
                    name.contains("head") && !name.contains("nub")
                })
            });
        let posed = evaluate(&asset, player.sample(), world)?;
        let eye = eye_position(&posed, head_bone)?;
        Ok(Self {
            asset,
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
                &self.asset,
                Motion::JumpStart,
                self.actions.jump_start,
                true,
            )?;
        } else if landed && self.actions.land.is_some() {
            self.player
                .set(&self.asset, Motion::Land, self.actions.land, true)?;
        }

        if !body.grounded {
            if self.player.motion != Motion::JumpStart || self.player.finished() {
                self.player
                    .set(&self.asset, Motion::JumpLoop, self.actions.jump_loop, false)?;
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
                .set(&self.asset, motion, self.actions.get(motion), false)?;
        }
        self.player.tick((delta.max(0.) * 1000.) as u32);
        let world = character_transform(body.position, facing);
        let posed = evaluate(&self.asset, self.player.sample(), world)?;
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
    asset: &ModelAsset,
    sample: Option<(usize, f32)>,
    world: Mat4,
) -> Result<Posed, String> {
    let pose = if let Some((action, seconds)) = sample {
        let set = asset
            .tracks
            .get(action)
            .ok_or("character action index out of range")?;
        let tracks = match set {
            Some(tracks) => &tracks.tracks,
            None if !asset.skeleton.embedded_tracks.is_empty() => &asset.skeleton.embedded_tracks,
            None => return Err("character action has no external or embedded tracks".into()),
        };
        pose::evaluate(&asset.skeleton, tracks, seconds, world)?
    } else {
        pose::evaluate(&asset.skeleton, &[], 0., world)?
    };
    let meshes = asset.meshes_from_pose(&pose)?;
    Ok(Posed { pose, meshes })
}

/// `Pose.absolute` is the model/world-transformed bone matrix. `Pose.skin`
/// additionally contains inverse bind and is intentionally not used here.
fn eye_position(posed: &Posed, head_bone: Option<usize>) -> Result<Vec3, String> {
    if let Some(index) = head_bone {
        let matrix = posed
            .pose
            .absolute
            .get(index)
            .ok_or("character head bone is outside the pose")?;
        return Ok(matrix.transform_point3(Vec3::ZERO) + Vec3::Y * HEAD_EYE_OFFSET);
    }
    // Unusual rigs without a head bone still use measured geometry, never a
    // race-specific guessed height.
    let top = posed
        .meshes
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
    let origin = posed.pose.model.transform_point3(Vec3::ZERO);
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
        let (set, warnings) = ActionSet::new(&actions);
        assert_eq!(set.stand, Some(0));
        assert_eq!(set.run, Some(1));
        assert_eq!(set.jump_start, Some(2));
        assert_eq!(set.jump_loop, Some(3));
        assert_eq!(set.land, Some(4));
        assert!(warnings.is_empty());
        let (missing, _) = ActionSet::new(&[action("not 站立_空手")]);
        assert_eq!(missing.stand, None);
    }

    #[test]
    fn transform_keeps_feet_at_body_position_and_faces_movement() {
        let matrix = character_transform(Vec3::new(3., 8., -2.), Vec3::X);
        assert_eq!(matrix.transform_point3(Vec3::ZERO), Vec3::new(3., 8., -2.));
        assert_eq!(matrix.transform_vector3(Vec3::Z), Vec3::X);
    }
}
