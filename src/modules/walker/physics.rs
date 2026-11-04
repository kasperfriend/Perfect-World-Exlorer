//! Offline ground locomotion using the recovered client rules.
//!
//! PWClient-152 `EC_HostMove.h/.cpp`, `EC_HostMsg.cpp`, and `EC_CDR.cpp` give
//! 5.0 m/s as the client default run speed, a 10 m/s ordinary jump impulse,
//! 9.8 m/s² gravity, a support-normal threshold of 0.5, a 0.3 m down trace,
//! and at most two jumps. Gravity is applied before integration, so apex is
//! timestep dependent; it is not a separate retail movement field.

use crate::navigation::MapSpace;
use glam::Vec3;

pub const NATIVE_RUN_SPEED: f32 = 5.0;
pub const NATIVE_JUMP_IMPULSE: f32 = 10.0;
pub const GRAVITY: f32 = 9.8;
pub const MAX_JUMPS: u8 = 2;
pub const STEP_SECONDS: f32 = 1.0 / 60.0;
pub const CONTINUOUS_JUMP_HEIGHT: f32 = NATIVE_JUMP_IMPULSE * NATIVE_JUMP_IMPULSE / (2.0 * GRAVITY);
const GROUND_NORMAL_MIN: f32 = 0.5;
const DOWN_TRACE: f32 = 0.3;
const STEP_HEIGHT: f32 = 0.8;

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
    let a = map.heights[iy * map.width + ix];
    let b = map.heights[iy * map.width + ix + 1];
    let c = map.heights[(iy + 1) * map.width + ix];
    let d = map.heights[(iy + 1) * map.width + ix + 1];
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
            jump_generation: 0,
            landed_generation: 0,
            accumulator: 0.,
            jump_queued: false,
        }
    }

    pub fn advance(&mut self, delta: f32, mechanics: &Mechanics, input: Input, map: &MapSpace) {
        if !delta.is_finite() || delta <= 0. {
            return;
        }
        self.jump_queued |= input.jump;
        self.accumulator = (self.accumulator + delta.min(0.25)).min(0.25);
        while self.accumulator + f32::EPSILON >= STEP_SECONDS {
            self.accumulator -= STEP_SECONDS;
            if self.accumulator < 0. {
                self.accumulator = 0.;
            }
            self.step(mechanics, input.direction, map);
        }
    }

    pub fn begin_flight(&mut self) {
        self.grounded = false;
        self.vertical_velocity = 0.0;
        self.jump_count = 0;
        self.jump_queued = false;
        self.accumulator = 0.0;
    }

    /// Air movement uses the equipped item's speed bonus over the class fly
    /// speed. The complete 3D wish velocity is clamped, matching AirWaterMove;
    /// gravity and ground jump rules are not applied while flying.
    pub fn advance_flight(&mut self, delta: f32, input: FlightInput, map: &MapSpace) {
        if !delta.is_finite() || delta <= 0.0 || !input.speed.is_finite() {
            return;
        }
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
            self.position += velocity * STEP_SECONDS;
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

    fn step(&mut self, mechanics: &Mechanics, direction: Vec3, map: &MapSpace) {
        let direction = Vec3::new(direction.x, 0., direction.z).normalize_or_zero();
        self.moving = direction.length_squared() > 0.5;
        let jump = std::mem::take(&mut self.jump_queued);
        if jump
            && ((self.grounded && self.jump_count == 0)
                || (!self.grounded && self.jump_count > 0 && self.jump_count < MAX_JUMPS))
        {
            // OnMsgHstJump replaces, rather than adds to, vertical velocity.
            self.vertical_velocity = mechanics.launch_speed;
            self.grounded = false;
            self.jump_count += 1;
            self.jump_generation = self.jump_generation.wrapping_add(1);
        }

        if self.grounded {
            let support = ground(map, self.position.x, self.position.z);
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
            let next = ground(map, self.position.x, self.position.z);
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

        let horizontal = direction * mechanics.move_speed;
        self.vertical_velocity -= GRAVITY * STEP_SECONDS;
        let velocity = horizontal + Vec3::Y * self.vertical_velocity;
        self.position += velocity * STEP_SECONDS;
        let support = ground(map, self.position.x, self.position.z);
        if self.position.y <= support.height {
            self.position.y = support.height;
            if support.normal.y >= GROUND_NORMAL_MIN && self.vertical_velocity <= 0. {
                self.vertical_velocity = 0.;
                self.grounded = true;
                self.jump_count = 0;
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
    use std::sync::Arc;

    fn flat_map() -> MapSpace {
        MapSpace {
            width: 8,
            height: 8,
            spacing: 2.,
            heights: Arc::from(vec![10.; 64]),
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
                &map,
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
            &map,
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
            &map,
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
            &map,
        );
        assert_eq!(body.jump_count, 2, "third jump must be rejected");
        for _ in 0..300 {
            body.advance(STEP_SECONDS, &mechanics, Input::default(), &map);
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
            &map,
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
            );
        }
        assert!(body.grounded);
        assert_eq!(body.position.y, 10.0);
        assert_eq!(body.vertical_velocity, 0.0);
    }

    #[test]
    fn terrain_triangle_height_and_normal_agree() {
        let map = MapSpace {
            width: 3,
            height: 3,
            spacing: 2.,
            heights: Arc::from(vec![0., 2., 4., 0., 2., 4., 0., 2., 4.]),
        };
        let sample = ground(&map, -1., 1.);
        assert!((sample.height - map.ground(-1., 1.)).abs() < 0.000_001);
        assert!(sample.normal.is_finite());
        assert!(sample.normal.y > 0.);
        assert!(sample.normal.x < 0.);
    }
}
