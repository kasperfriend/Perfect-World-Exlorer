//! Regional directional colors: elementclient 0x4208d0..0x420db0.
//! Material direction is authored separately; celestial disc rotation is NOT it.
//! PC24 arithmetic, packed sun/moon color round trips, and the asymmetric
//! sunrise expression are intentional native behavior, not an artistic curve.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
}
fn unpack(argb: u32) -> [f32; 3] {
    [16, 8, 0].map(|s| ((argb >> s) & 255) as f32 * f32::from_bits(0x3b808081))
}
/// 0x799540 clamps the SIGNED integer only at 255, not at zero. Preserve
/// shifts/ORs even for malformed negative authored powers, without panicking.
fn pack(rgb: [f32; 3]) -> u32 {
    let [r, g, b] = rgb.map(|v| {
        let v = v * 255.;
        let integer = if !v.is_finite()
            || v as f64 >= 9223372036854775808.
            || (v as f64) < -9223372036854775808.
        {
            i64::MIN
        } else {
            v.trunc() as i64
        };
        (integer as i32).min(255) as u32
    });
    0xff000000 | r.wrapping_shl(16) | g.wrapping_shl(8) | b
}
pub fn weighted(day: [f32; 3], night: [f32; 3], df: f32, nf: f32) -> [f32; 3] {
    std::array::from_fn(|i| day[i] * df + night[i] * nf)
}
pub fn ambient(day: [f32; 3], night: [f32; 3], nf: f32) -> [f32; 3] {
    // SetAmbient(ToRGBA|ff000000) at 0x420e77/0x420e84.
    unpack(pack(weighted(day, night, 1. - nf, nf)))
}
pub fn night_factor(hour: f32) -> f32 {
    let t = hour.rem_euclid(24.) / 24.;
    if t < 0.125 {
        1.
    } else if t < f32::from_bits(0x3e955555) {
        1. - (t - 0.125) * 6.
    } else if (t as f64) < 0.75 {
        0.
    } else if t < 0.875 {
        (t - 0.75) * 8.
    } else {
        1.
    }
}
pub fn evaluate(hour: f32, day: [f32; 3], night: [f32; 3]) -> Colors {
    let hour = if hour.is_finite() { hour } else { 0. };
    let t = hour.rem_euclid(24.) / 24.;
    let nf = night_factor(hour);
    let df = 1. - nf;
    let sun = unpack(pack(weighted(day, [1., 0.5, 0.], df, nf)));
    let moon = unpack(pack(night));
    // 0x4b60e0: +4c is sun pitch, +50 moon pitch. IsSunTime tests
    // sun pitch > 0, NOT visibility flags or a guessed sunrise hour range.
    let (sun_pitch, moon_pitch) = if t < f32::from_bits(0x3e155555) {
        (
            0.,
            (1. - t * f32::from_bits(0x40db6db7)) * f32::from_bits(0x3f860a92),
        )
    } else if t < 0.5 {
        (
            (t - f32::from_bits(0x3e155555)) * f32::from_bits(0x405cc61e),
            0.,
        )
    } else if t < 0.875 {
        (
            (1. - (t - 0.5) * f32::from_bits(0x402aaaab)) * f32::from_bits(0x3f9c61aa),
            0.,
        )
    } else {
        (0., (t - 0.875) * f32::from_bits(0x41060a92))
    };
    let (diffuse, dim) = if sun_pitch > 0. {
        let sunset = t >= 0.75; // +74 night-factor destination, 0x4b61f1.
        if nf < if sunset { 0.5 } else { 0.25 } {
            (sun, 1.)
        } else if sunset {
            let sun_weight = df * 2.;
            (weighted(sun, moon, sun_weight, (nf - 0.5) * 2.), sun_weight)
        } else {
            let sun_weight = df * f32::from_bits(0x3faaaaab);
            // 0x420b35 subtracts stored 1/3, NOT (nf-.25)/.75.
            (
                weighted(sun, moon, sun_weight, nf - f32::from_bits(0x3eaaaaab)),
                sun_weight,
            )
        }
    } else {
        (
            moon,
            (moon_pitch * f32::from_bits(0x3ff47645)).clamp(0., 0.8),
        )
    };
    Colors {
        diffuse,
        specular: diffuse.map(|v| v * dim),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn daylight_and_moonlight_use_packed_color_round_trips() {
        let day = [0.1234, 0.4567, 0.789];
        let night = [0.1, 0.2, 0.3];
        let noon = evaluate(12., day, night);
        assert_eq!(noon.diffuse, unpack(0xff1f74c9));
        assert_eq!(noon.specular, noon.diffuse);
        let midnight = evaluate(0., day, night);
        assert_eq!(midnight.diffuse, unpack(0xff19334c));
        assert_eq!(midnight.specular, midnight.diffuse.map(|v| v * 0.8));
        assert_eq!(evaluate(21., day, night).specular, [0.; 3]);
    }
    #[test]
    fn sunrise_retains_native_asymmetry_instead_of_repaired_lerp() {
        let c = evaluate(4., [1.; 3], [1.; 3]);
        let expected = weighted(
            unpack(0xffff9f3f),
            [1.; 3],
            1. / 3.,
            0.75 - f32::from_bits(0x3eaaaaab),
        );
        for i in 0..3 {
            assert!((c.diffuse[i] - expected[i]).abs() < 1e-6);
        }
        assert!((c.diffuse[0] - 0.75).abs() < 1e-6);
        assert!((c.specular[0] - 0.25).abs() < 1e-6);
        let sunset = evaluate(20.25, [1.; 3], [1.; 3]);
        assert_eq!(
            sunset.diffuse,
            weighted(unpack(0xffff9f3f), [1.; 3], 0.5, 0.5)
        );
        assert_eq!(sunset.specular, sunset.diffuse.map(|v| v * 0.5));
    }
    #[test]
    fn ambient_quantization_and_signed_pack_are_not_saturating_float_lerp() {
        assert_eq!(ambient([0.1234; 3], [0.; 3], 0.), unpack(0xff1f1f1f));
        assert_eq!(pack([-1. / 255., 0., 0.]), 0xffff0000);
        assert_eq!(pack([2.; 3]), 0xffffffff);
        for h in [0., 3., 3.5, 4., 6., 7., 12., 18., 19.5, 20.25, 21., 23.] {
            let a = evaluate(h, [0.5; 3], [0.2; 3]);
            assert_eq!(a.diffuse, evaluate(h + 24., [0.5; 3], [0.2; 3]).diffuse);
            assert!(a.diffuse.iter().chain(&a.specular).all(|v| v.is_finite()));
        }
    }
}
