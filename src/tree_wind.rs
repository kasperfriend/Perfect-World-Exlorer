//! Experimental translation of the client's tree wind-strength noise.
//!
//! The optional viewer preview uses the recovered scalar to modulate the old
//! sampled leaf loop. It is explicitly approximate: this does not reproduce
//! SpeedTree's live leaf rocking or replace the runtime parameters.

#[derive(Clone)]
pub struct ClientWind {
    table: [f32; 256],
    offsets: [u32; 8],
    weights: [f32; 8],
}

impl ClientWind {
    /// Client initialization at elementclient.exe 0x64fcbe and 0x766320.
    pub fn new() -> Self {
        let mut seed = 0x12345_u32;
        let mut raw = [[0.0_f32; 256]; 3];
        for component in &mut raw {
            for value in component {
                let number = next_random(&mut seed) % 2001;
                *value = ((number as i32 - 1000) as f64 * f64::from(0.001_f32)) as f32;
            }
        }
        let mut table = [0.0; 256];
        for i in 0..256 {
            table[i] = ((f64::from(raw[0][(i + 255) % 256]) + f64::from(raw[0][(i + 1) % 256]))
                * 0.25
                + f64::from(raw[0][i]) * 0.5) as f32;
        }
        let persistence = f32::from_bits(0x3f7fbe77);
        let mut weights = [0.0_f32; 8];
        let mut total = 0.0_f32;
        let mut weight = 1.0_f32;
        let mut offsets = [0; 8];
        for i in 0..8 {
            weights[i] = weight;
            total += weight;
            weight *= persistence;
            offsets[i] = next_random(&mut seed) % 1023;
        }
        for weight in &mut weights {
            *weight = (f64::from(*weight) / f64::from(total)) as f32;
        }
        Self {
            table,
            offsets,
            weights,
        }
    }

    /// Evaluate at client-global seconds. `None` denotes inputs for which
    /// x87 integer conversion or the translation's behavior is unverified.
    /// f64 intermediates approximate x87 precision rather than reproduce it.
    pub fn sample(&self, time: f32) -> Option<f32> {
        if !time.is_finite() || time < 0.0 || time > 1_000_000.0 {
            return None;
        }
        let mut result = 0.0_f32;
        for i in 0..8 {
            let period = 256_u32 >> i;
            let coordinate = f64::from(time) / f64::from(period) + f64::from(self.offsets[i]);
            let index = coordinate.trunc() as usize;
            let fraction = (coordinate - index as f64) as f32;
            let a = f64::from(self.table[index % 256]);
            let b = f64::from(self.table[(index + 1) % 256]);
            result = (f64::from(result)
                + (a * (1.0 - f64::from(fraction)) + b * f64::from(fraction)).abs()
                    * f64::from(self.weights[i])) as f32;
        }
        Some(result)
    }
}

/// Experimental translation of SpeedTreeRT's internal wind-state update at
/// 0x1002bc00. This is only one part of the leaf-rocking path; it does not
/// calculate leaf card positions and is not connected to rendering.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RuntimeWindPhase {
    pub strength: f32,
    pub phase: f32,
    pub angular_rate: f32,
    pub amplitude_degrees: f32,
}

impl RuntimeWindPhase {
    /// Mirrors the client's SetWindStrength call with both optional parameters
    /// set to -1 (reuse the previous strength and phase). `time` is the
    /// DLL-global SetTime value. The two object fields at +0x30/+0x34 are
    /// cleared by the original routine; they are not represented here.
    ///
    /// Returns None for nonfinite, negative-strength or overflowed inputs.
    /// f64 arithmetic approximates x87 and may differ at rounding boundaries.
    pub fn update(self, time: f32, strength: f32, amplitude: f32, frequency: f32) -> Option<Self> {
        if ![
            time,
            strength,
            self.strength,
            self.phase,
            amplitude,
            frequency,
        ]
        .into_iter()
        .all(f32::is_finite)
            || strength < 0.0
        {
            return None;
        }
        let old_rate = 8.0 * f64::from(self.strength) * f64::from(frequency);
        let angular_rate = (8.0 * f64::from(strength) * f64::from(frequency)) as f32;
        let amplitude_degrees = (180.0 * f64::from(amplitude) * f64::from(strength)) as f32;
        let phase = (f64::from(time) * old_rate + f64::from(self.phase)
            - f64::from(time) * f64::from(angular_rate)) as f32;
        if ![angular_rate, amplitude_degrees, phase]
            .into_iter()
            .all(f32::is_finite)
        {
            return None;
        }
        Some(Self {
            strength,
            phase,
            angular_rate,
            amplitude_degrees,
        })
    }

    /// The first scalar rotation angle at 0x1002bd00, before the DLL builds
    /// its matrix. `local_phase` is supplied by the leaf-table caller; it is
    /// not a per-instance world position. This is not a complete card transform.
    pub fn rock_angle(
        self,
        time: f32,
        local_phase: f32,
        base_amplitude: f32,
        rocking: bool,
    ) -> Option<f32> {
        if ![
            time,
            local_phase,
            base_amplitude,
            self.phase,
            self.angular_rate,
            self.amplitude_degrees,
        ]
        .into_iter()
        .all(f32::is_finite)
        {
            return None;
        }
        let (phase, extent) = if rocking {
            (
                f64::from(time) * f64::from(self.angular_rate)
                    + f64::from(self.phase)
                    + f64::from(local_phase),
                f64::from(self.amplitude_degrees),
            )
        } else {
            (
                f64::from(local_phase),
                f64::from(base_amplitude) * f64::from(0.15_f32),
            )
        };
        // The x87 FSIN result and libm's sin may diverge in precision; avoid
        // claiming support for huge arguments or nonfinite results.
        if phase.abs() > 1_000_000.0 {
            return None;
        }
        let angle = (-extent + (phase.sin() + 1.0) * 0.5 * (extent * 2.0)) as f32;
        angle.is_finite().then_some(angle)
    }
}

/// Optional viewer preview: apply the recovered client scalar to the old
/// sampled-animation blend. This modulation is not a port of leaf motion.
/// Invalid or unsupported time falls back to the unmodulated legacy blend.
pub fn preview_blend(wind: &ClientWind, time: f32, blend: f32, enabled: bool) -> f32 {
    let blend = if blend.is_finite() {
        blend.clamp(0.0, 1.0)
    } else {
        0.0
    };
    if !enabled {
        return blend;
    }
    match wind.sample(time) {
        Some(strength) => (blend * strength).clamp(0.0, 1.0),
        None => blend,
    }
}

fn next_random(seed: &mut u32) -> u32 {
    *seed = (u64::from(*seed) * 16807 % 2_147_483_647) as u32;
    *seed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_phase_and_angle_fixtures() {
        // Isolated synthetic x87 fixtures, not an end-to-end client capture.
        let initial = RuntimeWindPhase {
            strength: 0.25,
            phase: 1.0,
            angular_rate: 0.25,
            amplitude_degrees: 0.0,
        };
        let state = initial.update(10.0, 0.5, 0.75, 0.125).unwrap();
        assert_eq!(state.phase, -1.5);
        assert_eq!(state.angular_rate, 0.5);
        assert_eq!(state.amplitude_degrees, 67.5);
        assert_eq!(
            state.phase + 10.0 * state.angular_rate,
            initial.phase + 10.0 * initial.angular_rate
        );
        let probe_state = RuntimeWindPhase {
            phase: 1.0,
            ..state
        };
        assert!(
            (probe_state.rock_angle(1.0, 0.2, 0.75, true).unwrap() - 66.9373779).abs() < 0.0001
        );
        assert!(
            (probe_state.rock_angle(1.0, 0.2, 0.75, false).unwrap() - 0.0223503).abs() < 0.000001
        );
        assert!(initial.update(1.0, -1.0, 1.0, 1.0).is_none());
        assert!(initial.update(f32::NAN, 1.0, 1.0, 1.0).is_none());
        assert!(probe_state.rock_angle(1.0e10, 0.0, 1.0, true).is_none());
    }

    #[test]
    fn preview_blend_is_opt_in_bounded_and_uses_time() {
        let wind = ClientWind::new();
        assert_eq!(preview_blend(&wind, 5., 0.6, false), 0.6);
        assert!((preview_blend(&wind, 0., 1., true) - 0.386193573).abs() < 1e-6);
        assert_ne!(
            preview_blend(&wind, 0., 1., true),
            preview_blend(&wind, 16., 1., true)
        );
        assert_eq!(preview_blend(&wind, 0., 0., true), 0.);
        assert_eq!(preview_blend(&wind, f32::NAN, 0.6, true), 0.6);
        assert_eq!(preview_blend(&wind, 1., f32::NAN, true), 0.);
        assert!((0.0..=1.0).contains(&preview_blend(&wind, 1., 5., true)));
    }

    #[test]
    fn emulator_fixtures_and_period() {
        // From isolated Unicorn execution of the identified client EXE.
        let wind = ClientWind::new();
        assert_eq!(wind.offsets, [215, 370, 417, 434, 952, 739, 9, 779]);
        for (time, expected) in [
            (0.0, 0.386193573),
            (0.125, 0.385221481),
            (1.0, 0.378416777),
            (5.0, 0.399008989),
            (16.0, 0.356702805),
            (64.0, 0.353442341),
            (256.0, 0.286081165),
            (1024.0, 0.194191962),
            (65536.0, 0.386193573),
        ] {
            assert!((wind.sample(time).unwrap() - expected).abs() < 0.000001);
        }
        assert_eq!(wind.sample(-1.0), None);
        assert_eq!(wind.sample(f32::NAN), None);
    }
}
