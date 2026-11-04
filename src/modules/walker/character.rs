//! Real player ECM/SMD/BON/SKI assembly and direct base-action playback.

use super::{gfx::CharacterGfx, physics::Body};
use crate::{
    ecmodel,
    model_asset::ModelAsset,
    modules::designer::{
        assembly::{
            Character as Assembly, CharacterMeshes, CharacterPose, TravelAnimation, TravelState,
        },
        assets::Assets,
        catalog::{ArmorSlot, Class, FlightMode, Gender},
        customize::CharacterDesign,
    },
};
use glam::{Mat4, Vec3};

const HEAD_EYE_OFFSET: f32 = 0.075;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    Stand,
    Run,
    Swim,
    WaterIdle,
    JumpStart,
    JumpLoop,
    Land,
    RideStand,
    RideRun,
    RideJumpStart,
    RideJumpLoop,
    RideLand,
    FlyTakeoff,
    FlyHover,
    FlyForward,
    FlyDescendHigh,
    FlyDescendLow,
    FlyLand,
}

#[derive(Default)]
struct FlightActionSet {
    takeoff: Option<usize>,
    hover: Option<usize>,
    forward: Option<usize>,
    descend_high: Option<usize>,
    descend_low: Option<usize>,
    land: Option<usize>,
}

impl FlightActionSet {
    fn body(
        combined: &[ecmodel::CombinedAction],
        actions: &[ecmodel::Action],
        mode: FlightMode,
    ) -> Self {
        let prefix = match mode {
            FlightMode::Wing => "翅膀",
            FlightMode::FlySword | FlightMode::DoubleWheel => "飞剑",
        };
        Self {
            takeoff: find_prefixed(combined, actions, &[prefix], &["起飞_通用", "起飞"]),
            hover: find_prefixed(combined, actions, &[prefix], &["悬停_通用", "悬停"]),
            forward: find_prefixed(combined, actions, &[prefix], &["前进_通用", "前进"]),
            descend_high: find_prefixed(
                combined,
                actions,
                &[prefix],
                &["高空下降_通用", "高空下降"],
            ),
            descend_low: find_prefixed(
                combined,
                actions,
                &[prefix],
                &["低空下降_通用", "低空下降"],
            ),
            land: find_prefixed(combined, actions, &[prefix], &["落地_通用", "落地"]),
        }
    }

    fn child(actions: &[ecmodel::Action], mode: FlightMode, model_path: &str) -> Self {
        let no_combined = &[];
        match mode {
            FlightMode::Wing => {
                let stem = std::path::Path::new(model_path)
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();
                let prefixes = &[stem, ""];
                Self {
                    takeoff: find_prefixed(
                        no_combined,
                        actions,
                        prefixes,
                        &["_起飞", "起飞", "展开"],
                    ),
                    hover: find_prefixed(
                        no_combined,
                        actions,
                        prefixes,
                        &["_飞行悬停", "飞行悬停", "_悬停", "悬停"],
                    ),
                    forward: find_prefixed(
                        no_combined,
                        actions,
                        prefixes,
                        &["_飞行前进", "飞行前进", "_前进", "前进"],
                    ),
                    descend_high: find_prefixed(
                        no_combined,
                        actions,
                        prefixes,
                        &["_高空下降", "高空下降"],
                    ),
                    descend_low: find_prefixed(
                        no_combined,
                        actions,
                        prefixes,
                        &[
                            "_地空下降",
                            "地空下降",
                            "_飞行下降",
                            "飞行下降",
                            "_低空下降",
                            "低空下降",
                        ],
                    ),
                    land: find_prefixed(
                        no_combined,
                        actions,
                        prefixes,
                        &["_飞行落地", "飞行落地", "_落地", "落地"],
                    ),
                }
            }
            FlightMode::FlySword | FlightMode::DoubleWheel => Self {
                takeoff: find_action(no_combined, actions, &["飞剑静止", "飞剑悬停"]),
                hover: find_action(no_combined, actions, &["飞剑悬停"]),
                forward: find_action(no_combined, actions, &["飞剑前进"]),
                descend_high: find_action(no_combined, actions, &["飞剑高空下降"]),
                descend_low: find_action(no_combined, actions, &["飞剑低空下降"]),
                land: find_action(no_combined, actions, &["飞剑静止", "飞剑悬停"]),
            },
        }
    }

    fn get(&self, motion: Motion) -> Option<usize> {
        match motion {
            Motion::FlyTakeoff => self.takeoff,
            Motion::FlyHover => self.hover,
            Motion::FlyForward => self.forward,
            Motion::FlyDescendHigh => self.descend_high,
            Motion::FlyDescendLow => self.descend_low,
            Motion::FlyLand => self.land,
            _ => None,
        }
    }

    fn warnings(&self, label: &str) -> Vec<String> {
        let mut warnings = Vec::new();
        for (name, present) in [
            ("takeoff", self.takeoff),
            ("hover", self.hover),
            ("forward", self.forward),
            ("high descent", self.descend_high),
            ("low descent", self.descend_low),
            ("landing", self.land),
        ] {
            if present.is_none() {
                warnings.push(format!(
                    "No authored {label} {name} action was found; that state uses the bind pose."
                ));
            }
        }
        warnings
    }
}

#[derive(Default)]
struct MountActionSet {
    stand: Option<usize>,
    run: Option<usize>,
    jump_start: Option<usize>,
    jump_loop: Option<usize>,
    land: Option<usize>,
}

impl MountActionSet {
    fn new(combined: &[ecmodel::CombinedAction], actions: &[ecmodel::Action]) -> Self {
        // CECNPC's base run/walk names are resolved by CECModel through the
        // combined-action namespace first. Several Ride pets only have
        // 快速移动 there, with a direct base clip named 奔跑. Keep the existing
        // single-base restriction, then fall back to the literal direct clip
        // for multi-base standing variations rather than dropping animation.
        let resolve = |name| {
            find_action(combined, actions, &[name]).or_else(|| find_action(&[], actions, &[name]))
        };
        Self {
            stand: resolve("站立"),
            run: resolve("快速移动"),
            jump_start: resolve("跳跃起"),
            jump_loop: resolve("跳跃循环"),
            land: resolve("跳跃落"),
        }
    }
}

fn resolve_action(
    combined: &[ecmodel::CombinedAction],
    actions: &[ecmodel::Action],
    wanted: &str,
) -> Option<usize> {
    if let Some(combined) = combined.iter().find(|action| action.name == wanted) {
        // These locomotion states are single-base native actions. Do not
        // flatten a multi-stage action or substitute a broken binding variant.
        let [base] = combined.bases.as_slice() else {
            return None;
        };
        return actions.iter().position(|action| action.name == base.name);
    }
    actions.iter().position(|action| action.name == wanted)
}

fn find_action(
    combined: &[ecmodel::CombinedAction],
    actions: &[ecmodel::Action],
    names: &[&str],
) -> Option<usize> {
    names
        .iter()
        .find_map(|wanted| resolve_action(combined, actions, wanted))
}

fn find_prefixed(
    combined: &[ecmodel::CombinedAction],
    actions: &[ecmodel::Action],
    prefixes: &[&str],
    suffixes: &[&str],
) -> Option<usize> {
    for suffix in suffixes {
        for prefix in prefixes {
            let name = format!("{prefix}{suffix}");
            if let Some(action) = resolve_action(combined, actions, &name) {
                return Some(action);
            }
        }
    }
    None
}

struct ActionSet {
    stand: Option<usize>,
    run: Option<usize>,
    swim: Option<usize>,
    water_idle: Option<usize>,
    jump_start: Option<usize>,
    jump_loop: Option<usize>,
    land: Option<usize>,
    ride_stand: Option<usize>,
    ride_run: Option<usize>,
    ride_jump_start: Option<usize>,
    ride_jump_loop: Option<usize>,
    ride_land: Option<usize>,
    wing: FlightActionSet,
    sword: FlightActionSet,
}

impl ActionSet {
    fn new(
        combined: &[ecmodel::CombinedAction],
        actions: &[ecmodel::Action],
    ) -> (Self, Vec<String>) {
        let set = Self {
            stand: find_action(
                combined,
                actions,
                &["站立_通用", "站立_空手", "站立_徒手", "站立_法宝", "站立"],
            ),
            run: find_action(
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
            swim: find_action(combined, actions, &["游动_通用"]),
            water_idle: find_action(combined, actions, &["水中漂浮_通用"]),
            jump_start: find_action(
                combined,
                actions,
                &["起跳_通用", "起跳_空手", "跳跃开始", "起跳"],
            ),
            jump_loop: find_action(
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
            land: find_action(
                combined,
                actions,
                &["跳跃落地_通用", "落地_空手", "跳跃结束", "落地"],
            ),
            ride_stand: find_action(combined, actions, &["站立_骑乘_通用"]),
            ride_run: find_action(combined, actions, &["奔跑_骑乘_通用"]),
            ride_jump_start: find_action(combined, actions, &["起跳_骑乘_通用"]),
            ride_jump_loop: find_action(combined, actions, &["跳跃空中循环_骑乘_通用"]),
            ride_land: find_action(combined, actions, &["跳跃落地_骑乘_通用"]),
            wing: FlightActionSet::body(combined, actions, FlightMode::Wing),
            sword: FlightActionSet::body(combined, actions, FlightMode::FlySword),
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
            Motion::Swim => self.swim,
            Motion::WaterIdle => self.water_idle,
            Motion::JumpStart => self.jump_start,
            Motion::JumpLoop => self.jump_loop,
            Motion::Land => self.land,
            Motion::RideStand => self.ride_stand,
            Motion::RideRun => self.ride_run,
            Motion::RideJumpStart => self.ride_jump_start,
            Motion::RideJumpLoop => self.ride_jump_loop,
            Motion::RideLand => self.ride_land,
            _ => None,
        }
    }

    fn flight(&self, mode: FlightMode) -> &FlightActionSet {
        match mode {
            FlightMode::Wing => &self.wing,
            FlightMode::FlySword | FlightMode::DoubleWheel => &self.sword,
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
            looping: matches!(
                motion,
                Motion::Stand
                    | Motion::Run
                    | Motion::Swim
                    | Motion::WaterIdle
                    | Motion::JumpLoop
                    | Motion::RideStand
                    | Motion::RideRun
                    | Motion::RideJumpLoop
                    | Motion::FlyHover
                    | Motion::FlyForward
                    | Motion::FlyDescendHigh
                    | Motion::FlyDescendLow
            ),
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

fn starter_loadout(class: Class) -> (u32, [u32; 4]) {
    use Class::*;
    let weapon = match class {
        Blademaster => 2097,  // Wooden Sword
        Barbarian => 2258,    // Splintered Club
        Archer => 2250,       // Wooden Bow
        Assassin => 26331,    // Bonerazor (Assassin-only record)
        Psychic => 26332,     // Lesser Soulsphere (Psychic-only record)
        Wizard | Venomancer | Cleric => 2251, // Wand
    };
    // Heavy, light, and arcane armor respectively: upper, wrist, lower, foot.
    let armor = match class {
        Blademaster | Barbarian => [8451, 8535, 8456, 8459],
        Archer | Assassin => [8453, 8536, 8457, 8460],
        Wizard | Psychic | Venomancer | Cleric => [8455, 8537, 8458, 8461],
    };
    (weapon, armor)
}

/// Deterministic Walker preview equipment selected from authentic item-level-0
/// weapons and level-1 armor in `elements.data`. This is an Explorer loadout,
/// not a claim to have recovered the server's new-character grant table.
pub(super) fn starter_design(
    assets: &mut Assets,
    class: Class,
    gender: Gender,
) -> Result<CharacterDesign, String> {
    let (weapon, armor) = starter_loadout(class);
    let weapon_record = assets
        .catalog
        .weapon(weapon)
        .ok_or_else(|| format!("starter weapon {weapon} is absent from elements.data"))?;
    if weapon_record.item_level != 0
        || weapon_record.required_level != 1
        || weapon_record.class_mask & class.bit() == 0
    {
        return Err(format!(
            "starter weapon {weapon} is not an allowed level-1, item-level-0 weapon for {}",
            class.name()
        ));
    }

    let mut design = assets.default_design(class, gender)?;
    design.equipment.weapon = weapon;
    design.equipment.fashion_mode = false;
    for (slot, id) in [
        ArmorSlot::Upper,
        ArmorSlot::Wrist,
        ArmorSlot::Lower,
        ArmorSlot::Foot,
    ]
    .into_iter()
    .zip(armor)
    {
        let item = assets
            .catalog
            .armor_item(id)
            .ok_or_else(|| format!("starter armor {id} is absent from elements.data"))?;
        if item.slot != slot
            || item.required_level != 1
            || item.item_level != 0
            || item.class_mask & class.bit() == 0
        {
            return Err(format!(
                "starter armor {id} is not allowed level-1 {} armor for {}",
                slot.name(),
                class.name()
            ));
        }
        design.equipment.set_armor(slot, id);
        design.equipment.set_native_default_armor(slot, false);
    }
    Ok(design)
}

pub struct Character {
    pub assembly: Assembly,
    pub posed: Posed,
    pub eye: Vec3,
    pub warnings: Vec<String>,
    pub gfx: CharacterGfx,
    actions: ActionSet,
    player: ClipPlayer,
    flight_actions: FlightActionSet,
    flight_player: Option<ClipPlayer>,
    mount_actions: MountActionSet,
    mount_player: Option<ClipPlayer>,
    travel: TravelState,
    /// Eye anchor in body-model space, measured from the bind/reference head.
    camera_anchor: Vec3,
    last_jump: u64,
    last_landing: u64,
}

impl Character {
    pub fn load(assets: &mut Assets, design: CharacterDesign, world: Mat4) -> Result<Self, String> {
        let assembly = Assembly::assemble(design, assets)?;
        let gfx = CharacterGfx::load(&assembly, assets);
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
        let mut travel_warnings = Vec::new();
        let (flight_actions, flight_player) = if let Some(model) = &assembly.flight {
            let actions =
                FlightActionSet::child(&model.asset.model.actions, model.mode, &model.model_path);
            travel_warnings.extend(actions.warnings("flight child"));
            let player = ClipPlayer::new(&model.asset, Motion::FlyHover, actions.hover)?;
            (actions, Some(player))
        } else {
            (FlightActionSet::default(), None)
        };
        let (mount_actions, mount_player) = if let Some(model) = &assembly.mount {
            let actions =
                MountActionSet::new(&model.asset.descriptor.actions, &model.asset.model.actions);
            if actions.stand.is_none() || actions.run.is_none() {
                travel_warnings.push(
                    "Ride pet is missing 站立 or 快速移动; the missing state uses its bind pose."
                        .into(),
                );
            }
            let player = ClipPlayer::new(&model.asset, Motion::RideStand, actions.stand)?;
            (actions, Some(player))
        } else {
            (MountActionSet::default(), None)
        };
        // Animation must not feed back into the camera anchor. Keep the
        // reference head's local eye position and transform it by the final
        // body/mount placement, which still follows travel attachment height.
        let reference = evaluate(&assembly, None, Mat4::IDENTITY)?;
        let camera_anchor = eye_position(&reference, head_bone)?;
        let posed = evaluate(&assembly, player.sample(), world)?;
        let eye = posed.pose.body.model.transform_point3(camera_anchor);
        let mut warnings = assembly.warnings.clone();
        warnings.extend(action_warnings);
        warnings.extend(travel_warnings);
        warnings.extend(gfx.warnings.iter().cloned());
        Ok(Self {
            assembly,
            posed,
            eye,
            warnings,
            gfx,
            actions,
            player,
            flight_actions,
            flight_player,
            mount_actions,
            mount_player,
            travel: TravelState::Ground,
            camera_anchor,
            last_jump: 0,
            last_landing: 0,
        })
    }

    pub fn update(
        &mut self,
        delta: f32,
        body: &Body,
        facing: Vec3,
        travel: TravelState,
        rushing: bool,
        vertical: f32,
    ) -> Result<(), String> {
        let previous = self.travel;
        self.travel = travel;
        let jumped = self.last_jump != body.jump_generation;
        let landed = self.last_landing != body.landed_generation;
        self.last_jump = body.jump_generation;
        self.last_landing = body.landed_generation;

        match travel {
            TravelState::Mounted => {
                if jumped {
                    self.player.set(
                        &self.assembly.body,
                        Motion::RideJumpStart,
                        self.actions.ride_jump_start,
                        true,
                    )?;
                } else if landed {
                    self.player.set(
                        &self.assembly.body,
                        Motion::RideLand,
                        self.actions.ride_land,
                        true,
                    )?;
                }
                let motion = if !body.grounded {
                    if self.player.motion == Motion::RideJumpStart && !self.player.finished() {
                        Motion::RideJumpStart
                    } else {
                        Motion::RideJumpLoop
                    }
                } else if self.player.motion == Motion::RideLand && !self.player.finished() {
                    Motion::RideLand
                } else if body.moving {
                    Motion::RideRun
                } else {
                    Motion::RideStand
                };
                self.player
                    .set(&self.assembly.body, motion, self.actions.get(motion), false)?;
                if let Some(model) = &self.assembly.mount {
                    let action = match motion {
                        Motion::RideRun => self.mount_actions.run,
                        Motion::RideJumpStart => self.mount_actions.jump_start,
                        Motion::RideJumpLoop => self.mount_actions.jump_loop,
                        Motion::RideLand => self.mount_actions.land,
                        _ => self.mount_actions.stand,
                    };
                    if let Some(player) = &mut self.mount_player {
                        player.set(&model.asset, motion, action, false)?;
                    }
                }
            }
            TravelState::Flying => {
                let mode = self
                    .assembly
                    .flight
                    .as_ref()
                    .ok_or("flight activated without a flight model")?
                    .mode;
                let entering = previous != TravelState::Flying;
                if entering {
                    let action = self.actions.flight(mode).get(Motion::FlyTakeoff);
                    self.player
                        .set(&self.assembly.body, Motion::FlyTakeoff, action, true)?;
                } else if self.player.motion == Motion::FlyTakeoff && !self.player.finished() {
                    // Let the authored takeoff finish before hovering or moving.
                } else {
                    let motion = if vertical < -0.1 {
                        if rushing {
                            Motion::FlyDescendHigh
                        } else {
                            Motion::FlyDescendLow
                        }
                    } else if body.moving {
                        Motion::FlyForward
                    } else {
                        Motion::FlyHover
                    };
                    let action = self.actions.flight(mode).get(motion);
                    self.player
                        .set(&self.assembly.body, motion, action, false)?;
                }
                if let Some(model) = &self.assembly.flight {
                    let motion = self.player.motion;
                    let action = self.flight_actions.get(motion);
                    if let Some(player) = &mut self.flight_player {
                        player.set(&model.asset, motion, action, entering)?;
                    }
                }
            }
            TravelState::Ground => {
                if previous == TravelState::Flying {
                    if let Some(model) = &self.assembly.flight {
                        let action = self.actions.flight(model.mode).get(Motion::FlyLand);
                        self.player
                            .set(&self.assembly.body, Motion::FlyLand, action, true)?;
                    }
                }
                if self.player.motion == Motion::FlyLand && !self.player.finished() {
                    // Preserve the native wing/fly-sword landing action.
                } else if body.swimming && !body.water_jumping {
                    let motion = if body.moving {
                        Motion::Swim
                    } else {
                        Motion::WaterIdle
                    };
                    self.player.set(
                        &self.assembly.body,
                        motion,
                        self.actions.get(motion),
                        false,
                    )?;
                } else if jumped {
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

                if self.player.motion == Motion::FlyLand && !self.player.finished() {
                    // Still landing from flight.
                } else if body.swimming && !body.water_jumping {
                    // The authored swim/hang action was selected above.
                } else if !body.grounded {
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
                    self.player.set(
                        &self.assembly.body,
                        motion,
                        self.actions.get(motion),
                        false,
                    )?;
                }
            }
        }

        let delta_ms = (delta.max(0.) * 1000.) as u32;
        self.player.tick(delta_ms);
        if travel == TravelState::Mounted {
            if let Some(player) = &mut self.mount_player {
                player.tick(delta_ms);
            }
        }
        if travel == TravelState::Flying {
            if let Some(player) = &mut self.flight_player {
                player.tick(delta_ms);
            }
        }
        let world = character_transform(body.position, facing);
        let animation = TravelAnimation {
            state: travel,
            mount: if travel == TravelState::Mounted {
                self.mount_player.as_ref().and_then(ClipPlayer::sample)
            } else {
                None
            },
            flight: if travel == TravelState::Flying {
                self.flight_player.as_ref().and_then(ClipPlayer::sample)
            } else {
                None
            },
        };
        let posed = evaluate_travel(&self.assembly, self.player.sample(), world, animation)?;
        self.eye = posed.pose.body.model.transform_point3(self.camera_anchor);
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
    evaluate_travel(assembly, sample, world, TravelAnimation::default())
}

fn evaluate_travel(
    assembly: &Assembly,
    sample: Option<(usize, f32)>,
    world: Mat4,
    travel: TravelAnimation,
) -> Result<Posed, String> {
    let pose = assembly.sampled_pose(sample, world, travel)?;
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
    fn native_swim_and_mounted_jump_actions_resolve_exact_names() {
        let actions = vec![
            action("游动_通用"),
            action("水中漂浮_通用"),
            action("起跳_骑乘_通用"),
            action("跳跃空中循环_骑乘_通用"),
            action("跳跃落地_骑乘_通用"),
        ];
        let (set, _) = ActionSet::new(&[], &actions);
        assert_eq!(set.swim, Some(0));
        assert_eq!(set.water_idle, Some(1));
        assert_eq!(set.ride_jump_start, Some(2));
        assert_eq!(set.ride_jump_loop, Some(3));
        assert_eq!(set.ride_land, Some(4));

        let mount = MountActionSet::new(
            &[],
            &[
                action("跳跃起"),
                action("跳跃循环"),
                action("跳跃落"),
            ],
        );
        assert_eq!(mount.jump_start, Some(0));
        assert_eq!(mount.jump_loop, Some(1));
        assert_eq!(mount.land, Some(2));
    }

    #[test]
    fn starter_loadout_is_nonzero_and_uses_class_specific_authentic_records() {
        for class in Class::ALL {
            let (weapon, armor) = starter_loadout(class);
            assert_ne!(weapon, 0, "{} weapon", class.name());
            assert!(armor.iter().all(|id| *id != 0), "{} armor", class.name());
        }
        assert_eq!(starter_loadout(Class::Assassin).0, 26331);
        assert_eq!(starter_loadout(Class::Psychic).0, 26332);
        assert_eq!(starter_loadout(Class::Blademaster).0, 2097);
        assert_eq!(starter_loadout(Class::Archer).0, 2250);
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
                let weapon = assets.catalog.weapon(design.equipment.weapon).unwrap();
                assert_eq!(weapon.item_level, 0);
                assert_eq!(weapon.required_level, 1);
                assert_ne!(weapon.class_mask & class.bit(), 0);
                for slot in [
                    ArmorSlot::Upper,
                    ArmorSlot::Wrist,
                    ArmorSlot::Lower,
                    ArmorSlot::Foot,
                ] {
                    let id = design.equipment.armor(slot);
                    assert_ne!(id, 0);
                    assert!(!design.equipment.native_default_armor(slot));
                    let item = assets.catalog.armor_item(id).unwrap();
                    assert_eq!(item.slot, slot);
                    assert_eq!(item.required_level, 1);
                    assert_eq!(item.item_level, 0);
                    assert_ne!(item.class_mask & class.bit(), 0);
                }
                count += 1;
            }
        }
        assert_eq!(count, 14);
    }

    #[test]
    fn clearing_one_starter_slot_only_removes_that_armor_piece() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let mut design = starter_design(&mut assets, Class::Blademaster, Gender::Male).unwrap();
        design.equipment.set_armor(ArmorSlot::Upper, 0);
        assert!(!design.equipment.native_default_armor(ArmorSlot::Upper));
        for slot in [ArmorSlot::Wrist, ArmorSlot::Lower, ArmorSlot::Foot] {
            assert_ne!(design.equipment.armor(slot), 0);
            assert!(!design.equipment.native_default_armor(slot));
        }
        let character = Assembly::assemble(design, &mut assets).unwrap();
        assert_eq!(character.equipment.len(), 3);
        assert!(character
            .equipment
            .iter()
            .all(|part| part.target != ArmorSlot::Upper));
        assert!(character
            .equipment
            .iter()
            .any(|part| part.target == ArmorSlot::Wrist));
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
            let camera_eye = character.eye;
            character
                .update(
                    0.05,
                    &Body::new(Vec3::ZERO),
                    Vec3::Z,
                    TravelState::Ground,
                    false,
                    0.0,
                )
                .unwrap();
            assert!(
                (character.eye - camera_eye).length() < 1e-6,
                "{} idle head animation moved the first-person camera anchor",
                class.name()
            );
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
    fn authentic_mount_animates_rider_and_pet_with_independent_native_clips() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let mut design = starter_design(&mut assets, Class::Blademaster, Gender::Male).unwrap();
        design.equipment.weapon = 6;
        design.equipment.mount = 9061;
        let mut character = Character::load(&mut assets, design, Mat4::IDENTITY).unwrap();
        let mut body = Body::new(Vec3::ZERO);
        character
            .update(
                1.0 / 60.0,
                &body,
                Vec3::NEG_Z,
                TravelState::Mounted,
                false,
                0.0,
            )
            .unwrap();
        assert_eq!(character.player.motion, Motion::RideStand);
        assert!(!character.posed.meshes.mount.is_empty());
        assert!(character.posed.meshes.weapons.is_empty());

        body.moving = true;
        character
            .update(
                1.0 / 60.0,
                &body,
                Vec3::NEG_Z,
                TravelState::Mounted,
                false,
                0.0,
            )
            .unwrap();
        assert_eq!(character.player.motion, Motion::RideRun);
        assert_eq!(
            character.mount_player.as_ref().unwrap().motion,
            Motion::RideRun
        );
        assert_eq!(
            character.mount_player.as_ref().unwrap().action,
            character.mount_actions.run
        );
        let before = character
            .posed
            .meshes
            .mount
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>();
        for _ in 0..6 {
            character
                .update(
                    1.0 / 60.0,
                    &body,
                    Vec3::NEG_Z,
                    TravelState::Mounted,
                    false,
                    0.0,
                )
                .unwrap();
        }
        let after = character
            .posed
            .meshes
            .mount
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>();
        assert_eq!(before.len(), after.len());
        assert!(
            before
                .iter()
                .zip(&after)
                .any(|(before, after)| before != after),
            "the selected mount run clip did not deform any vertex"
        );
    }

    #[test]
    fn ancient_boa_resolves_combined_run_and_uses_native_pet_shader() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let mut design = starter_design(&mut assets, Class::Blademaster, Gender::Male).unwrap();
        design.equipment.mount = 18304;
        let mut character = Character::load(&mut assets, design, Mat4::IDENTITY).unwrap();
        let mount = character.assembly.mount.as_ref().unwrap();
        assert_eq!(
            mount.asset.skins[0].path,
            "models/npcs/宠物/骑宠/坐骑蛇/坐骑蛇.ski"
        );
        for mesh in 0..mount.asset.skins[0].data.meshes.len() {
            assert!(mount.asset.diffuse_alpha_meshes.contains(&(0, mesh)));
        }
        let run = character
            .mount_actions
            .run
            .expect("Ancient Boa combined run");
        assert_eq!(mount.asset.model.actions[run].name, "奔跑");

        let mut body = Body::new(Vec3::ZERO);
        body.moving = true;
        character
            .update(
                1.0 / 60.0,
                &body,
                Vec3::NEG_Z,
                TravelState::Mounted,
                false,
                0.0,
            )
            .unwrap();
        let before = character
            .posed
            .meshes
            .mount
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>();
        for _ in 0..6 {
            character
                .update(
                    1.0 / 60.0,
                    &body,
                    Vec3::NEG_Z,
                    TravelState::Mounted,
                    false,
                    0.0,
                )
                .unwrap();
        }
        let after = character
            .posed
            .meshes
            .mount
            .iter()
            .flat_map(|mesh| &mesh.vertices)
            .map(|vertex| vertex.position)
            .collect::<Vec<_>>();
        assert!(
            before.iter().zip(&after).any(|(a, b)| a != b),
            "Ancient Boa's combined 快速移动 -> 奔跑 clip did not deform vertices"
        );
    }

    #[test]
    fn every_authentic_ride_model_has_native_skin_and_locomotion() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        let mounts = assets.catalog.mounts.clone();
        let mut seen = std::collections::HashSet::new();
        let mut checked = 0;
        let mut lobster_checked = false;
        let mut failures = Vec::new();
        for item in mounts {
            if !item.has_loadable_model() || !seen.insert(item.model.clone()) {
                continue;
            }
            let mut design = starter_design(&mut assets, Class::Blademaster, Gender::Male).unwrap();
            design.equipment.mount = item.id;
            match Character::load(&mut assets, design, Mat4::IDENTITY) {
                Ok(character) => {
                    let mount = character.assembly.mount.as_ref().unwrap();
                    let expected = ecmodel::normalize(&item.model)
                        .rsplit_once('.')
                        .map(|(stem, _)| format!("{stem}.ski"))
                        .unwrap();
                    if mount.asset.skins.len() != 1 || mount.asset.skins[0].path != expected {
                        failures.push(format!("{}: wrong native pet skin", item.id));
                    }
                    if character.mount_actions.run.is_none() {
                        failures.push(format!("{}: unresolved combined run", item.id));
                    }
                    for (skin_index, skin) in mount.asset.skins.iter().enumerate() {
                        for mesh in 0..skin.data.meshes.len() {
                            if !mount
                                .asset
                                .diffuse_alpha_meshes
                                .contains(&(skin_index, mesh))
                            {
                                failures.push(format!("{}: ordinary alpha on pet mesh", item.id));
                            }
                        }
                    }
                    if item.id == 24767 {
                        // Grand Lobster has three authentic NaN weights on
                        // mesh-0 vertex 738. They must remain visible in the
                        // source record; recovery is triangle submission only.
                        lobster_checked = true;
                        let source = &mount.asset.skins[0].data.meshes[0];
                        let weights = source.vertices[738].weights;
                        if weights[..3].iter().filter(|v| v.is_nan()).count() != 3
                            || !weights[3].is_nan()
                        {
                            failures.push(format!("{}: unexpected Lobster weight record", item.id));
                        }
                        if crate::model_asset::nonfinite_weight_triangle_count(source) == 0 {
                            failures
                                .push(format!("{}: Lobster fault removes no triangle", item.id));
                        }
                        if !character.assembly.warnings.iter().any(|warning| {
                            warning.contains("24767") && warning.contains("source weights")
                        }) {
                            failures.push(format!("{}: missing Lobster recovery warning", item.id));
                        }
                        let posed = mount.asset.pose_bind(Mat4::IDENTITY).unwrap();
                        let posed = &posed[0].vertices[738];
                        if posed.position != [0.0; 3] || posed.normal != [0.0; 3] {
                            failures.push(format!("{}: Lobster tombstone changed", item.id));
                        }
                    }
                    checked += 1;
                }
                Err(error) => failures.push(format!("{} ({}): {error}", item.id, item.name)),
            }
        }
        assert!(checked >= 25, "only {checked} distinct Ride models checked");
        assert!(lobster_checked, "Grand Lobster 24767 was not audited");
        assert!(
            failures.is_empty(),
            "Ride-pet audit:\n{}",
            failures.join("\n")
        );
    }

    #[test]
    fn authentic_flight_animates_body_and_child_after_takeoff() {
        let element = std::path::Path::new("google_drive/pw142/element");
        if !element.join("models.pck").exists() {
            return;
        }
        let mut assets = Assets::open(element).unwrap();
        for (class, gender, item) in [
            (Class::Cleric, Gender::Male, 2092),
            (Class::Cleric, Gender::Male, 2767),
            (Class::Blademaster, Gender::Male, 27210),
        ] {
            let mut design = starter_design(&mut assets, class, gender).unwrap();
            design.equipment.flight = item;
            let mut character = Character::load(&mut assets, design, Mat4::IDENTITY)
                .unwrap_or_else(|error| panic!("flight {item}: {error}"));
            let mut body = Body::new(Vec3::new(0.0, 10.0, 0.0));
            body.grounded = false;
            character
                .update(
                    1.0 / 60.0,
                    &body,
                    Vec3::NEG_Z,
                    TravelState::Flying,
                    false,
                    0.0,
                )
                .unwrap_or_else(|error| panic!("flight {item} animation: {error}"));
            for _ in 0..40 {
                character
                    .update(0.25, &body, Vec3::NEG_Z, TravelState::Flying, false, 0.0)
                    .unwrap_or_else(|error| panic!("flight {item} animation: {error}"));
                if character.player.motion == Motion::FlyHover {
                    break;
                }
            }
            assert_eq!(
                character.player.motion,
                Motion::FlyHover,
                "flight {item}: current {} end {} speed {}",
                character.player.current,
                character.player.end,
                character.player.speed
            );
            assert_eq!(
                character.flight_player.as_ref().unwrap().motion,
                Motion::FlyHover
            );
            assert_eq!(character.posed.pose.travel, TravelState::Flying);
            assert!(!character.posed.meshes.flight.is_empty());
        }
    }

    #[test]
    fn transform_keeps_feet_at_body_position_and_faces_movement() {
        let matrix = character_transform(Vec3::new(3., 8., -2.), Vec3::X);
        assert_eq!(matrix.transform_point3(Vec3::ZERO), Vec3::new(3., 8., -2.));
        assert_eq!(matrix.transform_vector3(Vec3::Z), Vec3::X);
    }
}
