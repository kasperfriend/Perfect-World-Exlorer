//! Client sky clock, sun/moon state and the day/night texture switch (P5.02).
//!
//! Recovered from `elementclient.exe`:
//!
//! * `CECSunMoon::Tick` 0x4b5f10 accumulates the day fraction at
//!   `ds:0x8db108` = 1/14_400_000 per millisecond and wraps it at
//!   `ds:0x8d6138` = 1.0, so one full sky day takes four real hours.
//! * `CECSunMoon::SetTime` 0x4b6030 stores an authored time and re-runs the
//!   update, so an authored hour and a free-running clock share one curve.
//! * `CECSunMoon::Update` 0x4b60e0 turns that fraction into the sun elevation
//!   `+0x4c`, the moon elevation `+0x50`, the dusk/night flag `+0x74`, the
//!   night strength `+0x70` and the sky-object direction `+0x54/+0x58/+0x5c`.
//! * `A3DSky` 0x4203e0 selects the sky texture set and lighting record from
//!   `[CECSunMoon+0x74]` compared against 0.0 and 1.0 and starts the switch
//!   timer with `CECSunMoon::TransitionLength` 0x4b6250 (0x249f00 = 2_400_000
//!   for the night state, 0x1b7740 = 1_800_000 otherwise).
//! * `A3DSky::Update` 0x79a040 runs the timer down by 0x21 per frame while
//!   `BYTE[+0x5b8]` is set, interpolates the texture transform and the
//!   lighting record through 0x79a0e0 with the same pair of records, and calls
//!   the deferred commit 0x79b180 when the timer reaches zero.
//!
//! The switch is therefore a *linear timed* blend of two texture sets and two
//! lighting records, not a smoothstep: the timer starts at half of the phase
//! duration and runs to zero, so the blend weight is the elapsed fraction of
//! `phase / 2` expressed in sky-clock milliseconds. Because the timer is
//! proportional to the clock, the weight is a pure function of the hour, which
//! is what [`night_weight`] returns.

/// Sky-clock milliseconds for one full day: `1 / ds:0x8db108`.
pub const CLOCK_MS_PER_DAY: f64 = 14_400_000.0;
/// Milliseconds of sky clock per authored hour.
pub const CLOCK_MS_PER_HOUR: f64 = CLOCK_MS_PER_DAY / 24.0;
/// Night switch length, `0x249f00 >> 1` from 0x4b6250.
pub const NIGHT_PHASE_MS: f64 = 1_200_000.0;
/// Day switch length, `0x1b7740 >> 1` from 0x4b6250.
pub const DAY_PHASE_MS: f64 = 900_000.0;
/// Timer step per frame in `A3DSky::Update` 0x79a040 (`+0x21` / `-0x21`).
pub const TRANSITION_STEP: i32 = 0x21;

/// Authored hour (`0..24`) as the client's day fraction, NaN safe.
pub fn day_fraction(hour: f32) -> Option<f64> {
    if !hour.is_finite() {
        return None;
    }
    Some(f64::from(hour.rem_euclid(24.0)) / 24.0)
}

/// `CECSunMoon::Update` 0x4b60e0: elevations, direction and blend factors.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunMoon {
    /// `+0x4c`, sun elevation in radians, `0` at night, up to 70 degrees at noon.
    pub sun_elevation: f32,
    /// `+0x50`, moon elevation in radians, `0` during the day, up to 60 degrees
    /// at midnight.
    pub moon_elevation: f32,
    /// `+0x60`, azimuth in radians, from the region `sun_dir` through 0x4b60b0.
    pub azimuth: f32,
    /// `+0x54/+0x58/+0x5c`, the light-direction curve the client stores. The
    /// disc placement uses `sun_elevation`/`moon_elevation` instead; this stays
    /// for parity with [`crate::client_sun::sky_object_direction`].
    #[allow(dead_code)]
    pub direction: [f32; 3],
    /// `+0x70`, night strength. Above 1 before 03:00, ramps down to 0 at 03:30,
    /// 0 through the day, ramps to 1 between 18:00 and 21:00.
    pub night_strength: f32,
    /// `+0x74`, the dusk/night flag that selects the sky texture set.
    pub night_flag: bool,
}

impl SunMoon {
    /// Elevation of one of the two discs in radians.
    pub fn elevation(&self, sun: bool) -> f32 {
        if sun {
            self.sun_elevation
        } else {
            self.moon_elevation
        }
    }
}

/// Evaluate 0x4b60e0 for an authored hour and the region's `sun_dir` vector.
///
/// The azimuth comes from `CECSunMoon::SetSunDir` 0x4b60b0, `atan2(-z, -x)`.
/// Without an authored vector the client keeps whatever azimuth it had; `None`
/// is returned for non-finite input rather than inventing one.
pub fn evaluate(hour: f32, sun_dir: [f32; 3]) -> Option<SunMoon> {
    let t = day_fraction(hour)?;
    if !sun_dir.iter().all(|x| x.is_finite()) {
        return None;
    }
    let (x, z) = (f64::from(sun_dir[0]), f64::from(sun_dir[2]));
    let azimuth = (-z).atan2(-x);
    let sun_elevation = if t < 0.145_833_33 {
        0.0
    } else if t < 0.5 {
        ((t - 0.145_833_33) * 3.449_592_1) as f32
    } else if t < 0.875 {
        ((1.0 - (t - 0.5) * 2.666_666_7) * 1.221_730_5) as f32
    } else {
        0.0
    };
    let moon_elevation = if t < 0.145_833_33 {
        ((1.0 - t * 6.857_142_9) * 1.047_197_6) as f32
    } else if t < 0.875 {
        0.0
    } else {
        ((t - 0.875) * 8.377_580_6) as f32
    };
    let night_strength = if t < 0.291_666_66 {
        (1.0 - (t - 0.125) * 6.0) as f32
    } else if t < 0.75 {
        0.0
    } else if t < 0.875 {
        ((t - 0.75) * 8.0) as f32
    } else {
        1.0
    };
    // The stored vector uses the day fraction itself as the angle, exactly as
    // 0x4b6218 does: `[-cos(az)cos(t), -sin(t), -sin(az)cos(t)]`.
    let (sin_t, cos_t) = t.sin_cos();
    let (sin_a, cos_a) = azimuth.sin_cos();
    let direction = [
        (-cos_a * cos_t) as f32,
        (-sin_t) as f32,
        (-sin_a * cos_t) as f32,
    ];
    Some(SunMoon {
        sun_elevation,
        moon_elevation,
        azimuth: azimuth as f32,
        direction,
        night_strength,
        night_flag: t >= 0.75,
    })
}

/// Blend weight (0 = day texture set, 1 = night texture set) at an hour.
///
/// This is the linear timer from `A3DSky::Update` 0x79a040 expressed as a
/// function of the sky clock: the night switch starts at 18:00 and lasts
/// `NIGHT_PHASE_MS`, the day switch starts at midnight and lasts
/// `DAY_PHASE_MS`. Outside those windows the weight is exactly 0 or 1.
pub fn night_weight(hour: f32) -> Option<f32> {
    let t = day_fraction(hour)?;
    let day_ms = CLOCK_MS_PER_DAY;
    let night_start = 0.75;
    let night_span = NIGHT_PHASE_MS / day_ms;
    let day_span = DAY_PHASE_MS / day_ms;
    let w = if t < day_span {
        1.0 - t / day_span
    } else if t < night_start {
        0.0
    } else if t < night_start + night_span {
        (t - night_start) / night_span
    } else {
        1.0
    };
    Some(w.clamp(0.0, 1.0) as f32)
}

/// Frame-driven switch timer, mirroring `A3DSky::Update` 0x79a040.
///
/// The client counts `TRANSITION_STEP` per frame from `phase / 2` to zero and
/// commits the pending set through 0x79b180 at the end. [`Switch`] keeps the
/// same arithmetic so the two can be compared in tests.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Switch {
    remaining: i32,
    total: i32,
}

impl Switch {
    /// Start a switch towards `night`, using the recovered lengths.
    pub fn new(night: bool) -> Self {
        let total = if night {
            NIGHT_PHASE_MS as i32
        } else {
            DAY_PHASE_MS as i32
        };
        Self {
            remaining: total,
            total,
        }
    }
    /// Advance one frame; returns true when the pending set must be committed.
    pub fn advance(&mut self) -> bool {
        self.remaining -= TRANSITION_STEP;
        self.remaining <= 0
    }
    /// Elapsed fraction, 0 at the start of the switch and 1 at the commit.
    pub fn weight(&self) -> f32 {
        if self.total <= 0 {
            return 1.0;
        }
        1.0 - (self.remaining.max(0) as f32 / self.total as f32)
    }
    /// Remaining timer value, the value the client stores at `+0x5b4`.
    pub fn remaining(&self) -> i32 {
        self.remaining.max(0)
    }
    /// Frames a full switch takes at the client's step.
    pub fn frames(&self) -> u32 {
        (self.total / TRANSITION_STEP) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DIRECTION: [f32; 3] = [0.6, -0.4, 0.8];

    fn state(hour: f32) -> SunMoon {
        evaluate(hour, DIRECTION).expect("finite hour")
    }

    #[test]
    fn clock_wraps_at_four_real_hours_per_day() {
        let per_ms = 1.0 / CLOCK_MS_PER_DAY;
        assert!((per_ms - 6.944_444_444_4e-8).abs() < 1e-17);
        assert!((4.0 * 3_600_000.0 - CLOCK_MS_PER_DAY).abs() < 1e-9);
        assert_eq!(day_fraction(24.0).unwrap(), 0.0);
        assert_eq!(day_fraction(-6.0).unwrap(), 0.75);
        assert!(day_fraction(f32::NAN).is_none());
    }

    #[test]
    fn elevations_follow_the_recovered_piecewise_curve() {
        for (hour, sun_deg, moon_deg) in [
            (0.0_f32, 0.0_f32, 60.0_f32),
            (3.5, 0.0, 0.0),
            (12.0, 70.0, 0.0),
            (18.0, 70.0 * 0.333_333_3, 0.0),
            (21.0, 0.0, 0.0),
            (24.0, 0.0, 60.0),
        ] {
            let s = state(hour);
            assert!(
                (s.sun_elevation.to_degrees() - sun_deg).abs() < 0.01,
                "hour {hour} sun {}",
                s.sun_elevation.to_degrees()
            );
            assert!(
                (s.moon_elevation.to_degrees() - moon_deg).abs() < 0.01,
                "hour {hour} moon {}",
                s.moon_elevation.to_degrees()
            );
        }
    }

    #[test]
    fn elevation_ramps_are_continuous_apart_from_the_midnight_strength_step() {
        let mut previous = state(0.0);
        for step in 1..=2400 {
            let hour = step as f32 * 0.01;
            let current = state(hour);
            assert!((current.sun_elevation - previous.sun_elevation).abs() < 0.01);
            assert!((current.moon_elevation - previous.moon_elevation).abs() < 0.06);
            previous = current;
        }
        // `night_strength` is not continuous: it jumps from 1.0 at 23:59 to
        // 1.749 at midnight because 0x4b60e0 uses two different ramps there.
        assert!((state(23.99).night_strength - 1.0).abs() < 1e-5);
        assert!((state(0.01).night_strength - 1.748).abs() < 2e-3);
    }

    #[test]
    fn night_flag_is_the_texture_set_selector() {
        assert!(!state(17.99).night_flag);
        assert!(state(18.0).night_flag);
        assert!(state(23.99).night_flag);
        assert!(!state(0.0).night_flag);
        assert!(!state(12.0).night_flag);
    }

    #[test]
    fn night_weight_is_the_linear_switch_timer() {
        assert_eq!(night_weight(12.0).unwrap(), 0.0);
        assert_eq!(night_weight(17.9).unwrap(), 0.0);
        // Night switch starts at 18:00 and runs 2 400 000 / 2 clock ms.
        let span = (NIGHT_PHASE_MS / CLOCK_MS_PER_HOUR) as f32; // hours
        assert!((span - 2.0).abs() < 1e-6);
        assert!((night_weight(19.0).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(night_weight(20.0).unwrap(), 1.0);
        assert_eq!(night_weight(23.0).unwrap(), 1.0);
        // Day switch starts at midnight and runs 1 800 000 / 2 clock ms.
        let day_span = (DAY_PHASE_MS / CLOCK_MS_PER_HOUR) as f32;
        assert!((day_span - 1.5).abs() < 1e-6);
        assert!((night_weight(0.75).unwrap() - 0.5).abs() < 1e-6);
        assert_eq!(night_weight(1.5).unwrap(), 0.0);
    }

    #[test]
    fn frame_timer_reaches_full_weight_at_the_commit() {
        let mut switch = Switch::new(true);
        assert_eq!(switch.remaining(), 1_200_000);
        assert_eq!(switch.weight(), 0.0);
        assert_eq!(switch.frames(), 36_363);
        let mut frames = 0;
        while !switch.advance() {
            frames += 1;
            assert!(frames <= 36_364);
        }
        assert_eq!(frames + 1, 36_364);
        assert_eq!(switch.weight(), 1.0);
        assert_eq!(switch.remaining(), 0);
    }

    #[test]
    fn frame_timer_weight_matches_the_hour_based_weight() {
        // One frame of the client's timer is 33 clock ms, so the same elapsed
        // span expressed in hours must produce the same weight.
        for (night, start_hour) in [(true, 18.0_f32), (false, 0.0_f32)] {
            let mut switch = Switch::new(night);
            let mut elapsed_ms = 0.0_f64;
            loop {
                if switch.advance() {
                    break;
                }
                elapsed_ms += TRANSITION_STEP as f64;
                let hour = start_hour + (elapsed_ms / CLOCK_MS_PER_HOUR) as f32;
                // The night switch ramps towards the night set; the day switch
                // ramps away from it, so the two count in opposite directions.
                let weight = night_weight(hour).unwrap();
                let expected = if night { weight } else { 1.0 - weight };
                assert!(
                    (switch.weight() - expected).abs() < 0.01,
                    "night {night} hour {hour}: {} vs {expected}",
                    switch.weight()
                );
            }
        }
    }

    #[test]
    fn direction_is_unit_length_and_uses_the_authored_azimuth() {
        for hour in [0.0_f32, 4.0, 9.5, 15.0, 22.5] {
            let s = state(hour);
            let len = (s.direction[0] * s.direction[0]
                + s.direction[1] * s.direction[1]
                + s.direction[2] * s.direction[2])
                .sqrt();
            assert!(len <= 1.0 + 1e-6);
            // 0x4b60b0 stores atan2(-z, -x), which is 180 degrees away from
            // atan2(z, x) for this authored vector.
            assert!((s.azimuth.to_degrees() + 126.87).abs() < 0.01);
            // 0x4b60e0 negates both cosines again, so the stored direction
            // keeps the authored horizontal heading.
            assert!(s.direction[0] > 0.0 && s.direction[2] > 0.0);
        }
        assert!(evaluate(f32::NAN, DIRECTION).is_none());
        assert!(evaluate(12.0, [f32::NAN, 0.0, 0.0]).is_none());
    }
}
