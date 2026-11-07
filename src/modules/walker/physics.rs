//! Offline ground locomotion using the recovered client rules.
//!
//! PWClient-152 `EC_HostMove.h/.cpp`, `EC_HostMsg.cpp`, and `EC_CDR.cpp` give
//! 5.0 m/s as the client default run speed, a 10 m/s ordinary jump impulse,
//! 9.8 m/s² gravity, a support-normal threshold of 0.5, a 0.3 m down trace,
//! and at most two jumps. Gravity is applied before integration, so apex is
//! timestep dependent; it is not a separate retail movement field.

use crate::{
    modules::designer::catalog::{Class, Gender},
    navigation::MapSpace,
    water::WaterArea,
};
use glam::Vec3;

pub const NATIVE_RUN_SPEED: f32 = 5.0;
pub const NATIVE_JUMP_IMPULSE: f32 = 10.0;
pub const NATIVE_WATER_JUMP_IMPULSE: f32 = 7.0;
pub const GRAVITY: f32 = 9.8;
pub const MAX_JUMPS: u8 = 2;
pub const MOUNTED_MAX_JUMPS: u8 = 1;
pub const STEP_SECONDS: f32 = 1.0 / 60.0;
pub const CONTINUOUS_JUMP_HEIGHT: f32 = NATIVE_JUMP_IMPULSE * NATIVE_JUMP_IMPULSE / (2.0 * GRAVITY);
const GROUND_NORMAL_MIN: f32 = 0.5;
const DOWN_TRACE: f32 = 0.3;
const STEP_HEIGHT: f32 = 0.8;
pub const NATIVE_MIN_AIR_HEIGHT: f32 = 1.6;

/// Profession/gender values installed by `CECPlayer::CalcPlayerAABB`.
/// Walker positions are feet positions, so the native AABB extends from `y`
/// to `y + 2 * half_height` and its center is `y + half_height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MovementConstants {
    pub half_width: f32,
    pub half_height: f32,
    pub min_water_height: f32,
    pub shore_depth: f32,
    pub water_surface: f32,
}

impl MovementConstants {
    pub fn new(class: Class, gender: Gender) -> Self {
        let male = gender == Gender::Male;
        let barbarian_male = class == Class::Barbarian && male;
        Self {
            half_width: if class == Class::Blademaster && male {
                0.4
            } else if barbarian_male {
                0.5
            } else {
                0.3
            },
            half_height: if barbarian_male {
                1.05
            } else if male {
                0.9
            } else {
                0.85
            },
            min_water_height: 0.3,
            shore_depth: if barbarian_male {
                1.8
            } else if male {
                1.6
            } else {
                1.5
            },
            water_surface: if barbarian_male {
                0.7
            } else if male {
                0.6
            } else {
                0.55
            },
        }
    }

    /// Unscaled `m_aabbServer.Extents` from `CECPlayer::CalcPlayerAABB`.
    pub fn collision_extents(self) -> Vec3 {
        Vec3::new(self.half_width, self.half_height, self.half_width)
    }
}

/// `A3DTerrainWater::GetWaterHeight` selects the first area containing the
/// sampled mask cell. Keep the Option so a genuine zero-height area is not
/// confused with the native no-water return value.
pub fn water_height(areas: &[WaterArea], x: f32, z: f32) -> Option<f32> {
    areas
        .iter()
        .find(|area| area.contains_xz(x, z))
        .map(|area| area.height)
}

/// Native `CheckWaterMoveEnv`, using the unscaled server-AABB half height.
pub fn enters_water(
    position: Vec3,
    ground_height: f32,
    water_height: Option<f32>,
    constants: MovementConstants,
) -> bool {
    let Some(water) = water_height else {
        return false;
    };
    let sample = position.y + constants.half_height;
    let surface = water - constants.water_surface;
    sample < surface - 0.001 || (sample < surface + 0.0001 && position.y - ground_height >= 0.01)
}

pub fn can_take_off_water(
    position: Vec3,
    water_height: Option<f32>,
    constants: MovementConstants,
) -> bool {
    water_height.is_some_and(|water| position.y >= water - constants.shore_depth)
}

/// Gates from `CECHostPlayer::Render`: fully submerged AABBs do not enter the
/// reflection, and AABBs whose minimum is above the surface do not refract.
pub fn water_pass_visibility(min_y: f32, max_y: f32, plane: f32) -> (bool, bool) {
    (max_y >= plane, min_y < plane)
}

#[derive(Clone, Copy, Debug)]
pub struct Ground {
    pub height: f32,
    pub normal: Vec3,
}

/// Sample the same two terrain triangles as `MapSpace::ground`, retaining the
/// exact triangle normal needed by the client's support-plane projection.
pub fn ground(map: &MapSpace, x: f32, z: f32) -> Ground {
    let gx = (x / map.spacing + map.width as f32 * 0.5).clamp(0., (map.width - 1) as f32);
    let gy = (map.height as f32 * 0.5 - z / map.spacing).clamp(0., (map.height - 1) as f32);
    let ix = (gx as usize).min(map.width - 2);
    let iy = (gy as usize).min(map.height - 2);
    let fx = gx - ix as f32;
    let fy = gy - iy as f32;
    let mut heights = map.heights.reader();
    let a = heights.height(iy * map.width + ix);
    let b = heights.height(iy * map.width + ix + 1);
    let c = heights.height((iy + 1) * map.width + ix);
    let d = heights.height((iy + 1) * map.width + ix + 1);
    let (height, dhdx, dhdz) = if fx + fy <= 1. {
        (
            a + (b - a) * fx + (c - a) * fy,
            (b - a) / map.spacing,
            -(c - a) / map.spacing,
        )
    } else {
        (
            d + (c - d) * (1. - fx) + (b - d) * (1. - fy),
            (d - c) / map.spacing,
            -(d - b) / map.spacing,
        )
    };
    Ground {
        height,
        normal: Vec3::new(-dhdx, 1., -dhdz).normalize_or_zero(),
    }
}

/// Upward-facing object hull tops, queried at a feet position. Terrain support
/// is permanent and lives in [`Floor`]; implementations only add BMD/tree
/// surfaces so the fixed-step integrator can see the same floor that wall
/// sliding uses.
pub trait ObjectSupport {
    /// Highest walkable surface within `max_down` of `feet`, or `None`.
    fn object_support(&self, feet: Vec3, extents: Vec3, max_down: f32) -> Option<Ground>;
}

/// The one support plane a physics step is allowed to use. Terrain is always
/// present; an object hull top wins only when it is higher and walkable.
///
/// Keeping this query *inside* the fixed step is essential: a post-step patch
/// that sets `grounded` back to `true` cannot stop the step from first clearing
/// it, projecting movement onto the terrain normal and restarting the landing
/// animation (the object-landing jitter this replaced).
#[derive(Clone, Copy)]
pub struct Floor<'a> {
    pub map: &'a MapSpace,
    pub extents: Vec3,
    pub objects: Option<&'a dyn ObjectSupport>,
}

impl<'a> Floor<'a> {
    /// Terrain-only floor, for tests and callers without object collision.
    pub fn terrain(map: &'a MapSpace) -> Self {
        Self {
            map,
            extents: Vec3::ZERO,
            objects: None,
        }
    }

    /// Native terrain sample, without object hulls (water entry uses it).
    fn terrain_at(&self, x: f32, z: f32) -> Ground {
        ground(self.map, x, z)
    }

    /// Higher of terrain and an object hull top reachable by a down trace from
    /// `feet`. A surface that is not walkable or sits below the terrain is not
    /// a support plane.
    fn at(&self, feet: Vec3, max_down: f32) -> Ground {
        let terrain = self.terrain_at(feet.x, feet.z);
        self.objects
            .and_then(|objects| objects.object_support(feet, self.extents, max_down))
            .filter(|object| {
                object.normal.y >= GROUND_NORMAL_MIN && object.height >= terrain.height
            })
            .unwrap_or(terrain)
    }
}

/// Maximum height of the client's semi-implicit integration at Walker's fixed
/// 60 Hz physics step. This deliberately is not the continuous v²/(2g) value.
pub fn integrated_apex(mut launch_speed: f32) -> f32 {
    let mut y = 0.0f32;
    let mut apex = 0.0f32;
    for _ in 0..4096 {
        launch_speed -= GRAVITY * STEP_SECONDS;
        y += launch_speed * STEP_SECONDS;
        apex = apex.max(y);
        if launch_speed < 0. && y <= 0. {
            break;
        }
    }
    apex
}

pub fn native_jump_height() -> f32 {
    integrated_apex(NATIVE_JUMP_IMPULSE)
}

/// The add-on's jump-height setting is converted back to the launch impulse
/// used by the same integrator. The default therefore remains exactly 10 m/s.
pub fn launch_for_height(height: f32) -> f32 {
    if (height - native_jump_height()).abs() < 0.000_001 {
        return NATIVE_JUMP_IMPULSE;
    }
    let mut low = 0.0f32;
    let mut high = 40.0f32;
    for _ in 0..48 {
        let mid = (low + high) * 0.5;
        if integrated_apex(mid) < height {
            low = mid;
        } else {
            high = mid;
        }
    }
    (low + high) * 0.5
}

#[derive(Clone, Copy, Debug)]
pub struct Mechanics {
    pub move_speed: f32,
    pub jump_height: f32,
    pub launch_speed: f32,
}

impl Default for Mechanics {
    fn default() -> Self {
        Self {
            move_speed: NATIVE_RUN_SPEED,
            jump_height: native_jump_height(),
            launch_speed: NATIVE_JUMP_IMPULSE,
        }
    }
}

impl Mechanics {
    pub fn set_move_speed(&mut self, speed: f32) {
        if speed.is_finite() {
            self.move_speed = speed.clamp(0., 50.);
        }
    }

    pub fn set_jump_height(&mut self, height: f32) {
        if height.is_finite() {
            self.jump_height = height.clamp(0.05, 20.);
            self.launch_speed = launch_for_height(self.jump_height);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub direction: Vec3,
    pub jump: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct FlightInput {
    pub direction: Vec3,
    pub vertical: f32,
    pub speed: f32,
}

#[derive(Debug)]
pub struct Body {
    pub position: Vec3,
    pub vertical_velocity: f32,
    pub grounded: bool,
    pub jump_count: u8,
    pub moving: bool,
    pub swimming: bool,
    pub water_jumping: bool,
    pub water_height: Option<f32>,
    pub jump_generation: u64,
    pub landed_generation: u64,
    accumulator: f32,
    jump_queued: bool,
}

impl Body {
    pub fn new(position: Vec3) -> Self {
        Self {
            position,
            vertical_velocity: 0.,
            grounded: true,
            jump_count: 0,
            moving: false,
            swimming: false,
            water_jumping: false,
            water_height: None,
            jump_generation: 0,
            landed_generation: 0,
            accumulator: 0.,
            jump_queued: false,
        }
    }

    pub fn advance(&mut self, delta: f32, mechanics: &Mechanics, input: Input, floor: Floor<'_>) {
        self.advance_ground(delta, mechanics, input, floor, MAX_JUMPS);
    }

    /// `CECHostPlayer::CanDo(CANDO_JUMP)` permits the ordinary second jump but
    /// explicitly rejects another jump while jumping and riding a pet.
    pub fn advance_mounted(
        &mut self,
        delta: f32,
        mechanics: &Mechanics,
        input: Input,
        floor: Floor<'_>,
    ) {
        self.advance_ground(delta, mechanics, input, floor, MOUNTED_MAX_JUMPS);
    }

    fn advance_ground(
        &mut self,
        delta: f32,
        mechanics: &Mechanics,
        input: Input,
        floor: Floor<'_>,
        max_jumps: u8,
    ) {
        if !delta.is_finite() || delta <= 0. {
            return;
        }
        self.swimming = false;
        self.water_height = None;
        self.jump_queued |= input.jump;
        self.accumulator = (self.accumulator + delta.min(0.25)).min(0.25);
        while self.accumulator + f32::EPSILON >= STEP_SECONDS {
            self.accumulator -= STEP_SECONDS;
            if self.accumulator < 0. {
                self.accumulator = 0.;
            }
            self.step(mechanics, input.direction, floor, max_jumps);
        }
    }

    /// Ground/air movement with the client's water-environment test and swim
    /// speed. Arrow-style vertical input is separate from the jump edge.
    pub fn advance_with_water(
        &mut self,
        delta: f32,
        mechanics: &Mechanics,
        input: Input,
        vertical: f32,
        swim_speed: f32,
        floor: Floor<'_>,
        water: &[WaterArea],
        constants: MovementConstants,
    ) {
        if !delta.is_finite() || delta <= 0. || !swim_speed.is_finite() {
            return;
        }
        self.jump_queued |= input.jump;
        self.accumulator = (self.accumulator + delta.min(0.25)).min(0.25);
        while self.accumulator + f32::EPSILON >= STEP_SECONDS {
            self.accumulator -= STEP_SECONDS;
            if self.accumulator < 0. {
                self.accumulator = 0.;
            }
            self.step_with_water(
                mechanics,
                input.direction,
                vertical,
                swim_speed.max(0.),
                floor,
                water,
                constants,
            );
        }
    }

    pub fn aabb_y(&self, constants: MovementConstants) -> (f32, f32) {
        (
            self.position.y,
            self.position.y + 2.0 * constants.half_height,
        )
    }

    /// Wall sliding can move x/z onto a higher terrain triangle than the one
    /// the fixed step sampled. Reapply the permanent terrain floor afterwards;
    /// object support is already resolved inside [`Body::step`], so this pass
    /// never touches grounded state for a body standing on a hull.
    pub fn apply_terrain_floor(&mut self, map: &MapSpace) {
        let support = ground(map, self.position.x, self.position.z);
        if self.position.y >= support.height {
            return;
        }
        self.position.y = support.height;
        if support.normal.y < GROUND_NORMAL_MIN || self.vertical_velocity > 0. {
            return;
        }
        let newly_landed = !self.grounded;
        self.vertical_velocity = 0.;
        self.grounded = true;
        self.jump_count = 0;
        self.water_jumping = false;
        if newly_landed {
            self.landed_generation = self.landed_generation.wrapping_add(1);
        }
    }

    pub fn begin_flight(&mut self) {
        self.grounded = false;
        self.swimming = false;
        self.water_jumping = false;
        self.water_height = None;
        self.vertical_velocity = 0.0;
        self.jump_count = 0;
        self.jump_queued = false;
        self.accumulator = 0.0;
    }

    /// Air movement uses the equipped item's speed bonus over the class fly
    /// speed. The complete 3D wish velocity is clamped, matching AirWaterMove;
    /// gravity and ground jump rules are not applied while flying.
    pub fn advance_flight(
        &mut self,
        delta: f32,
        input: FlightInput,
        map: &MapSpace,
        areas: &[WaterArea],
        constants: MovementConstants,
    ) {
        if !delta.is_finite() || delta <= 0.0 || !input.speed.is_finite() {
            return;
        }
        self.swimming = false;
        self.water_jumping = false;
        self.water_height = None;
        self.accumulator = (self.accumulator + delta.min(0.25)).min(0.25);
        while self.accumulator + f32::EPSILON >= STEP_SECONDS {
            self.accumulator -= STEP_SECONDS;
            if self.accumulator < 0.0 {
                self.accumulator = 0.0;
            }
            let direction =
                Vec3::new(input.direction.x, 0.0, input.direction.z).normalize_or_zero();
            self.moving = direction.length_squared() > 0.5;
            let vertical = if input.vertical.is_finite() {
                input.vertical.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            let velocity =
                (direction + Vec3::Y * vertical).normalize_or_zero() * input.speed.max(0.0);
            self.grounded = false;
            self.vertical_velocity = velocity.y;
            let previous_center_y = self.position.y + constants.half_height;
            self.position += velocity * STEP_SECONDS;

            // EC_CDR::AirMove treats water as solid for flight. Its support
            // comparison uses the AABB center and fMinAirHei (1.6 m), and the
            // upward rescue is limited by the actual vertical step or 0.15 m.
            // Convert back to Walker's feet position only after that clamp.
            if let Some(water) = water_height(areas, self.position.x, self.position.z) {
                let center_y = self.position.y + constants.half_height;
                let minimum_center_y = water + NATIVE_MIN_AIR_HEIGHT;
                if center_y < minimum_center_y {
                    let allowance = ((previous_center_y - center_y).abs() + 0.001).max(0.15);
                    self.position.y += (minimum_center_y - center_y).min(allowance);
                }
            }

            let support = ground(map, self.position.x, self.position.z);
            if self.position.y < support.height
                || (self.position.y <= support.height && velocity.y < 0.0)
            {
                self.position.y = support.height;
                self.vertical_velocity = 0.0;
                self.grounded = true;
                self.jump_count = 0;
                self.landed_generation = self.landed_generation.wrapping_add(1);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn step_with_water(
        &mut self,
        mechanics: &Mechanics,
        direction: Vec3,
        vertical: f32,
        swim_speed: f32,
        floor: Floor<'_>,
        areas: &[WaterArea],
        constants: MovementConstants,
    ) {
        let direction = Vec3::new(direction.x, 0., direction.z).normalize_or_zero();
        let terrain = floor.terrain_at(self.position.x, self.position.z);
        let level = water_height(areas, self.position.x, self.position.z);
        self.water_height = level;
        self.swimming = enters_water(self.position, terrain.height, level, constants);
        if !self.swimming {
            let max_jumps = if self.water_jumping {
                MOUNTED_MAX_JUMPS
            } else {
                MAX_JUMPS
            };
            self.step(mechanics, direction, floor, max_jumps);
            return;
        }

        self.grounded = false;
        let jump = std::mem::take(&mut self.jump_queued);
        if self.water_jumping && self.vertical_velocity > 0.0 {
            // A water jump uses GroundMove while its vertical velocity is
            // positive. Further jumps are rejected by IsJumpInWater().
            self.step_water_jump(mechanics, direction, floor);
            return;
        }

        // Fly/swim work resets an earlier ground jump on entry. A new water
        // jump is allowed only near enough to the surface to take off.
        self.jump_count = 0;
        self.water_jumping = false;
        if jump && can_take_off_water(self.position, level, constants) {
            self.vertical_velocity = NATIVE_WATER_JUMP_IMPULSE;
            self.water_jumping = true;
            self.jump_count = 1;
            self.jump_generation = self.jump_generation.wrapping_add(1);
            self.step_water_jump(mechanics, direction, floor);
            return;
        }

        let vertical = if vertical.is_finite() {
            vertical.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        let wish = direction + Vec3::Y * vertical;
        self.moving = wish.length_squared() > 0.5;
        let near_surface = level
            .is_some_and(|water| self.position.y > water - 2.0 && self.position.y > terrain.height);
        let move_speed = if near_surface {
            mechanics.move_speed.min(swim_speed)
        } else {
            swim_speed
        };
        let mut velocity = wish.normalize_or_zero() * move_speed;
        let resting_surface =
            level.map(|water| water - constants.half_height - constants.water_surface);
        let sinking_to_rest =
            wish == Vec3::ZERO && resting_surface.is_some_and(|surface| self.position.y > surface);
        if sinking_to_rest {
            // CECHPWorkStand::Tick_FlySwim sinks a character who entered
            // water too close to the surface for the authored water action.
            velocity.y = -swim_speed;
        }
        self.vertical_velocity = velocity.y;
        self.position += velocity * STEP_SECONDS;
        if sinking_to_rest {
            if let Some(surface) = resting_surface {
                if self.position.y <= surface {
                    self.position.y = surface;
                    self.vertical_velocity = 0.0;
                }
            }
        }
        let water_floor =
            floor.terrain_at(self.position.x, self.position.z).height + constants.min_water_height;
        if self.position.y < water_floor {
            self.position.y = water_floor;
            self.vertical_velocity = 0.0;
        }
    }

    fn step_water_jump(&mut self, mechanics: &Mechanics, direction: Vec3, floor: Floor<'_>) {
        self.moving = direction.length_squared() > 0.5;
        let previous_y = self.position.y;
        self.vertical_velocity -= GRAVITY * STEP_SECONDS;
        self.position +=
            (direction * mechanics.move_speed + Vec3::Y * self.vertical_velocity) * STEP_SECONDS;
        let drop = (previous_y - self.position.y).max(0.);
        let query = Vec3::new(self.position.x, previous_y, self.position.z);
        let support = floor.at(query, DOWN_TRACE + drop);
        if self.position.y <= support.height {
            self.position.y = support.height;
            self.vertical_velocity = 0.0;
            self.grounded = true;
            self.swimming = false;
            self.jump_count = 0;
            self.water_jumping = false;
            self.landed_generation = self.landed_generation.wrapping_add(1);
        } else if self.vertical_velocity <= 0.0 {
            // Once the upward water jump ends, native movement returns to the
            // fly/swim branch and resets its jump state.
            self.jump_count = 0;
            self.water_jumping = false;
        }
    }

    fn step(&mut self, mechanics: &Mechanics, direction: Vec3, floor: Floor<'_>, max_jumps: u8) {
        let direction = Vec3::new(direction.x, 0., direction.z).normalize_or_zero();
        self.moving = direction.length_squared() > 0.5;
        let jump = std::mem::take(&mut self.jump_queued);
        if jump
            && !self.water_jumping
            && ((self.grounded && self.jump_count == 0)
                || (!self.grounded && self.jump_count > 0 && self.jump_count < max_jumps))
        {
            // OnMsgHstJump replaces, rather than adds to, vertical velocity.
            self.vertical_velocity = mechanics.launch_speed;
            self.grounded = false;
            self.jump_count += 1;
            self.jump_generation = self.jump_generation.wrapping_add(1);
        }

        if self.grounded {
            // The combined floor (terrain or hull top) is the plane the fixed
            // step actually stands on. Projecting onto terrain here used to
            // inject a bogus vertical velocity and clear `grounded` every
            // frame while standing on an object.
            let support = floor.at(self.position, DOWN_TRACE);
            let horizontal = direction * mechanics.move_speed;
            let speed = horizontal.length();
            let mut velocity = horizontal;
            if speed > 0. {
                // OnGroundMove projects normalized velocity onto the support
                // plane, including its additional 1% into-plane component.
                let direction = horizontal / speed;
                let dot = direction.dot(support.normal);
                velocity = (direction - support.normal * dot * 1.01) * speed;
            }
            self.position += velocity * STEP_SECONDS;
            let next = floor.at(self.position, DOWN_TRACE);
            let rise = next.height - self.position.y;
            if next.normal.y >= GROUND_NORMAL_MIN && rise <= STEP_HEIGHT && rise >= -DOWN_TRACE {
                self.position.y = next.height;
                self.vertical_velocity = 0.;
            } else {
                // Walking beyond a support surface does not grant a jump.
                self.grounded = false;
                self.vertical_velocity = velocity.y;
            }
            return;
        }

        let previous_y = self.position.y;
        let horizontal = direction * mechanics.move_speed;
        self.vertical_velocity -= GRAVITY * STEP_SECONDS;
        let velocity = horizontal + Vec3::Y * self.vertical_velocity;
        self.position += velocity * STEP_SECONDS;
        // A 60 Hz step can cross a thin hull top in one tick. Trace from the
        // pre-step feet over the whole drop so the highest floor in that range
        // wins; without this the character falls through roofs and never lands.
        let drop = (previous_y - self.position.y).max(0.);
        let query = Vec3::new(self.position.x, previous_y, self.position.z);
        let support = floor.at(query, DOWN_TRACE + drop);
        if self.position.y <= support.height {
            self.position.y = support.height;
            if support.normal.y >= GROUND_NORMAL_MIN && self.vertical_velocity <= 0. {
                self.vertical_velocity = 0.;
                self.grounded = true;
                self.jump_count = 0;
                self.water_jumping = false;
                self.landed_generation = self.landed_generation.wrapping_add(1);
            } else {
                // Steep support does not reset the jump counter. Remove the
                // into-plane component so gravity produces a downhill slide.
                let projected = velocity - support.normal * velocity.dot(support.normal);
                self.vertical_velocity = projected.y;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat_map() -> MapSpace {
        MapSpace {
            width: 8,
            height: 8,
            spacing: 2.,
            heights: vec![10.; 64].into(),
        }
    }

    #[test]
    fn recovered_defaults_preserve_impulse_and_report_real_integrated_height() {
        let mechanics = Mechanics::default();
        assert_eq!(mechanics.move_speed, 5.);
        assert_eq!(mechanics.launch_speed, 10.);
        assert_eq!(GRAVITY, 9.8);
        assert!((CONTINUOUS_JUMP_HEIGHT - 5.102_040_8).abs() < 0.000_001);
        assert!(
            (mechanics.jump_height - 5.018_948).abs() < 0.000_01,
            "{}",
            mechanics.jump_height
        );
        assert!(
            (integrated_apex(mechanics.launch_speed) - mechanics.jump_height).abs() < 0.000_001
        );
    }

    #[test]
    fn custom_jump_height_is_solved_not_mislabelled_as_continuous_height() {
        let mut mechanics = Mechanics::default();
        mechanics.set_jump_height(7.25);
        assert!((integrated_apex(mechanics.launch_speed) - 7.25).abs() < 0.000_01);
        mechanics.set_jump_height(native_jump_height());
        assert_eq!(mechanics.launch_speed, NATIVE_JUMP_IMPULSE);
    }

    #[test]
    fn diagonal_input_does_not_exceed_ground_speed() {
        let map = flat_map();
        let mut body = Body::new(Vec3::new(0., 10., 0.));
        let mechanics = Mechanics::default();
        for _ in 0..60 {
            body.advance(
                STEP_SECONDS,
                &mechanics,
                Input {
                    direction: Vec3::new(1., 0., 1.),
                    jump: false,
                },
                Floor::terrain(&map),
            );
        }
        let displacement = body.position - Vec3::new(0., 10., 0.);
        assert!(
            (displacement.length() - 5.).abs() < 0.000_1,
            "{displacement:?}"
        );
        assert_eq!(body.position.y, 10.);
    }

    #[test]
    fn native_jump_and_double_jump_follow_client_gates() {
        let map = flat_map();
        let mut body = Body::new(Vec3::new(0., 10., 0.));
        let mechanics = Mechanics::default();
        body.advance(
            STEP_SECONDS,
            &mechanics,
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            Floor::terrain(&map),
        );
        assert_eq!(body.jump_count, 1);
        assert!(!body.grounded);
        let first_velocity = body.vertical_velocity;
        body.advance(
            STEP_SECONDS,
            &mechanics,
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            Floor::terrain(&map),
        );
        assert_eq!(body.jump_count, 2);
        // The second jump replaces velocity with 10 m/s. After the same
        // gravity step its velocity equals the first jump's post-step value;
        // retaining the old velocity would make it strictly smaller.
        assert!((body.vertical_velocity - first_velocity).abs() < 0.000_001);
        body.advance(
            STEP_SECONDS,
            &mechanics,
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            Floor::terrain(&map),
        );
        assert_eq!(body.jump_count, 2, "third jump must be rejected");
        for _ in 0..300 {
            body.advance(
                STEP_SECONDS,
                &mechanics,
                Input::default(),
                Floor::terrain(&map),
            );
        }
        assert!(body.grounded);
        assert_eq!(body.jump_count, 0);
        assert_eq!(body.position.y, 10.);
    }

    #[test]
    fn walking_off_support_does_not_grant_a_midair_first_jump() {
        let map = flat_map();
        let mut body = Body::new(Vec3::new(0., 20., 0.));
        body.grounded = false;
        let mechanics = Mechanics::default();
        body.advance(
            STEP_SECONDS,
            &mechanics,
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            Floor::terrain(&map),
        );
        assert_eq!(body.jump_count, 0);
        assert!(body.vertical_velocity < 0.);
    }

    #[test]
    fn flight_clamps_complete_3d_velocity_and_lands_on_descent() {
        let map = flat_map();
        let mut body = Body::new(Vec3::new(0.0, 12.0, 0.0));
        body.begin_flight();
        for _ in 0..60 {
            body.advance_flight(
                STEP_SECONDS,
                FlightInput {
                    direction: Vec3::new(1.0, 0.0, 1.0),
                    vertical: 1.0,
                    speed: 9.0,
                },
                &map,
                &[],
                MovementConstants::new(Class::Blademaster, Gender::Male),
            );
        }
        let displacement = body.position - Vec3::new(0.0, 12.0, 0.0);
        assert!((displacement.length() - 9.0).abs() < 0.000_1);
        assert!(!body.grounded);

        for _ in 0..300 {
            body.advance_flight(
                STEP_SECONDS,
                FlightInput {
                    direction: Vec3::ZERO,
                    vertical: -1.0,
                    speed: 9.0,
                },
                &map,
                &[],
                MovementConstants::new(Class::Blademaster, Gender::Male),
            );
        }
        assert!(body.grounded);
        assert_eq!(body.position.y, 10.0);
        assert_eq!(body.vertical_velocity, 0.0);
    }

    #[test]
    fn flight_water_clearance_uses_native_center_height() {
        let map = flat_map();
        let areas = test_water(12.0);
        let constants = MovementConstants::new(Class::Blademaster, Gender::Male);
        let mut body = Body::new(Vec3::new(0.0, 14.0, 0.0));
        body.begin_flight();
        for _ in 0..300 {
            body.advance_flight(
                STEP_SECONDS,
                FlightInput {
                    direction: Vec3::ZERO,
                    vertical: -1.0,
                    speed: 9.0,
                },
                &map,
                &areas,
                constants,
            );
        }
        assert!(!body.grounded);
        assert!((body.position.y + constants.half_height - 13.6).abs() < 0.000_1);
        assert!((body.position.y - 12.7).abs() < 0.000_1);
    }

    /// A finite upward-facing deck standing in for a BMD hull top: like
    /// `CollisionWorld::support`, the down trace only finds it inside its
    /// footprint, at or below the queried feet, and within `max_down`.
    struct Deck {
        half: f32,
        top: f32,
    }

    impl ObjectSupport for Deck {
        fn object_support(&self, feet: Vec3, _extents: Vec3, max_down: f32) -> Option<Ground> {
            let surface = self.top + 0.031_25; // native DIST_EPSILON clearance
            if feet.x.abs() <= self.half
                && feet.z.abs() <= self.half
                && feet.y >= surface
                && feet.y - surface <= max_down
            {
                Some(Ground {
                    height: surface,
                    normal: Vec3::Y,
                })
            } else {
                None
            }
        }
    }

    fn make_deck(half: f32, top: f32) -> Deck {
        Deck { half, top }
    }

    fn deck_floor<'a>(map: &'a MapSpace, deck: &'a Deck) -> Floor<'a> {
        Floor {
            map,
            extents: Vec3::new(0.3, 0.9, 0.3),
            objects: Some(deck),
        }
    }

    fn sloped_map() -> MapSpace {
        let mut heights = vec![10.0f32; 64];
        for y in 0..8 {
            for x in 0..8 {
                heights[y * 8 + x] = 10. + x as f32 * 0.2;
            }
        }
        MapSpace {
            width: 8,
            height: 8,
            spacing: 2.,
            heights: heights.into(),
        }
    }

    #[test]
    fn landing_on_a_hull_is_one_stable_grounded_state() {
        let map = flat_map();
        let deck = make_deck(3., 12.);
        let floor = deck_floor(&map, &deck);
        let mechanics = Mechanics::default();
        let mut body = Body::new(Vec3::new(0., 14., 0.));
        body.grounded = false;
        for _ in 0..300 {
            body.advance(STEP_SECONDS, &mechanics, Input::default(), floor);
        }
        assert!(body.grounded);
        assert!((body.position.y - deck.top - 0.031_25).abs() < 1e-5);
        assert_eq!(body.landed_generation, 1, "one landing, not one per frame");

        // Standing still must not restart the landing action or drift upward.
        for _ in 0..120 {
            body.advance(STEP_SECONDS, &mechanics, Input::default(), floor);
            assert!(body.grounded);
            assert_eq!(body.vertical_velocity, 0.);
            assert!((body.position.y - deck.top - 0.031_25).abs() < 1e-5);
        }
        assert_eq!(body.landed_generation, 1);
    }

    #[test]
    fn moving_on_a_hull_over_sloped_terrain_stays_grounded() {
        // The terrain normal used to project horizontal movement into a
        // positive vertical velocity, which made the post-step support query
        // skip the hull and left the character in a jittering falling state.
        let map = sloped_map();
        let deck = make_deck(20., 12.);
        let floor = deck_floor(&map, &deck);
        let mechanics = Mechanics::default();
        let mut body = Body::new(Vec3::new(0., deck.top + 0.031_25, 0.));
        let uphill = Input {
            direction: Vec3::X,
            jump: false,
        };
        for _ in 0..60 {
            body.advance(STEP_SECONDS, &mechanics, uphill, floor);
            assert!(body.grounded, "lost hull support at {:?}", body.position);
            assert!(body.vertical_velocity <= 0.);
            assert!((body.position.y - deck.top - 0.031_25).abs() < 1e-5);
        }
        assert_eq!(body.landed_generation, 0);
    }

    #[test]
    fn walking_off_a_hull_falls_to_the_permanent_terrain_floor() {
        let map = flat_map();
        let deck = make_deck(1., 12.);
        let floor = deck_floor(&map, &deck);
        let mechanics = Mechanics::default();
        let mut body = Body::new(Vec3::new(0., deck.top + 0.031_25, 0.));
        let walk = Input {
            direction: Vec3::X,
            jump: false,
        };
        for _ in 0..300 {
            body.advance(STEP_SECONDS, &mechanics, walk, floor);
        }
        assert!(body.grounded);
        assert_eq!(body.position.y, 10.);
        assert_eq!(body.landed_generation, 1);
    }

    #[test]
    fn jumping_from_a_hull_lands_back_on_it_once() {
        let map = flat_map();
        let deck = make_deck(3., 12.);
        let floor = deck_floor(&map, &deck);
        let mechanics = Mechanics::default();
        let mut body = Body::new(Vec3::new(0., deck.top + 0.031_25, 0.));
        body.advance(
            STEP_SECONDS,
            &mechanics,
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            floor,
        );
        assert!(!body.grounded);
        assert_eq!(body.jump_count, 1);
        assert_eq!(body.jump_generation, 1);
        for _ in 0..300 {
            body.advance(STEP_SECONDS, &mechanics, Input::default(), floor);
        }
        assert!(body.grounded);
        assert_eq!(body.position.y, deck.top + 0.031_25);
        assert_eq!(body.landed_generation, 1);
        assert_eq!(body.jump_count, 0);
    }

    #[test]
    fn mounted_jump_is_single_while_ground_double_jump_is_unchanged() {
        let map = flat_map();
        let mut body = Body::new(Vec3::new(0., 10., 0.));
        let mechanics = Mechanics::default();
        for expected in [1, 1, 1] {
            body.advance_mounted(
                STEP_SECONDS,
                &mechanics,
                Input {
                    direction: Vec3::ZERO,
                    jump: true,
                },
                Floor::terrain(&map),
            );
            assert_eq!(body.jump_count, expected);
        }
        assert_eq!(MOUNTED_MAX_JUMPS, 1);
        assert_eq!(MAX_JUMPS, 2);
    }

    fn test_water(height: f32) -> Vec<WaterArea> {
        vec![WaterArea {
            export_id: 1,
            color: 0,
            simple: false,
            height,
            grid_size: 16.0,
            columns: 1,
            rows: 1,
            center: [0.0, height, 0.0],
            mask: vec![1],
            shore_waves: Vec::new(),
        }]
    }

    #[test]
    fn water_height_uses_the_first_authored_matching_area() {
        let areas = [test_water(20.0), test_water(30.0)].concat();
        assert_eq!(water_height(&areas, 0.0, 0.0), Some(20.0));
        assert_eq!(water_height(&areas, 100.0, 0.0), None);
        assert_eq!(water_height(&test_water(0.0), 0.0, 0.0), Some(0.0));
    }

    #[test]
    fn profession_aabbs_and_water_render_gates_match_native_boundaries() {
        let male = MovementConstants::new(Class::Blademaster, Gender::Male);
        assert_eq!(male.half_height, 0.9);
        assert_eq!(male.shore_depth, 1.6);
        assert_eq!(male.water_surface, 0.6);
        let female = MovementConstants::new(Class::Wizard, Gender::Female);
        assert_eq!(female.half_height, 0.85);
        assert_eq!(female.shore_depth, 1.5);
        assert_eq!(female.water_surface, 0.55);
        let orc = MovementConstants::new(Class::Barbarian, Gender::Male);
        assert_eq!(orc.half_height, 1.05);
        assert_eq!(orc.shore_depth, 1.8);
        assert_eq!(orc.water_surface, 0.7);
        assert_eq!(water_pass_visibility(10.0, 12.0, 14.0), (false, true));
        assert_eq!(water_pass_visibility(13.0, 15.0, 14.0), (true, true));
        assert_eq!(water_pass_visibility(14.0, 16.0, 14.0), (true, false));
    }

    #[test]
    fn swimming_uses_native_entry_depth_and_clamps_complete_3d_speed() {
        let map = flat_map();
        let water = test_water(30.0);
        let constants = MovementConstants::new(Class::Blademaster, Gender::Male);
        assert!(enters_water(
            Vec3::new(0.0, 12.0, 0.0),
            10.0,
            Some(14.0),
            constants
        ));
        assert!(!enters_water(
            Vec3::new(0.0, 13.0, 0.0),
            10.0,
            Some(14.0),
            constants
        ));
        let mut body = Body::new(Vec3::new(0.0, 12.0, 0.0));
        for _ in 0..60 {
            body.advance_with_water(
                STEP_SECONDS,
                &Mechanics::default(),
                Input {
                    direction: Vec3::new(1.0, 0.0, 1.0),
                    jump: false,
                },
                1.0,
                3.0,
                Floor::terrain(&map),
                &water,
                constants,
            );
        }
        assert!(body.swimming);
        assert!(!body.grounded);
        assert!(((body.position - Vec3::new(0.0, 12.0, 0.0)).length() - 3.0).abs() < 0.0001);
        assert!(body.moving);
    }

    #[test]
    fn water_takeoff_uses_seven_meter_impulse_and_rejects_a_second_jump() {
        let map = flat_map();
        let water = test_water(14.0);
        let constants = MovementConstants::new(Class::Blademaster, Gender::Male);
        let mut body = Body::new(Vec3::new(0.0, 12.45, 0.0));
        assert!(can_take_off_water(body.position, Some(14.0), constants));
        body.advance_with_water(
            STEP_SECONDS,
            &Mechanics::default(),
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            0.0,
            3.0,
            Floor::terrain(&map),
            &water,
            constants,
        );
        assert_eq!(body.jump_count, 1);
        assert!(body.water_jumping);
        assert!((body.vertical_velocity - (7.0 - GRAVITY * STEP_SECONDS)).abs() < 0.00001);
        body.advance_with_water(
            STEP_SECONDS,
            &Mechanics::default(),
            Input {
                direction: Vec3::ZERO,
                jump: true,
            },
            0.0,
            3.0,
            Floor::terrain(&map),
            &water,
            constants,
        );
        assert_eq!(body.jump_count, 1, "IsJumpInWater rejects a second jump");
    }

    #[test]
    fn terrain_triangle_height_and_normal_agree() {
        let map = MapSpace {
            width: 3,
            height: 3,
            spacing: 2.,
            heights: vec![0., 2., 4., 0., 2., 4., 0., 2., 4.].into(),
        };
        let sample = ground(&map, -1., 1.);
        assert!((sample.height - map.ground(-1., 1.)).abs() < 0.000_001);
        assert!(sample.normal.is_finite());
        assert!(sample.normal.y > 0.);
        assert!(sample.normal.x < 0.);
    }
}
