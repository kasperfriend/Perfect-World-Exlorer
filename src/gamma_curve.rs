//! Recovered D3D8 display ramp (0x430226, 0x77b7c0).
//! Encoded framebuffer values enter the LUT; this is not a material-space curve.
//! LUT interpolation emulates display hardware, not a verified monitor response.
pub fn enabled(requested: bool, fullscreen: bool) -> bool {
    requested && fullscreen
}

pub fn setting(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0., 100.).floor()
    } else {
        41.
    }
}

pub fn entry(index: u8, value: f32) -> u16 {
    if index == 0 {
        return 0;
    }
    let exponent = 100. / (30. + 1.7 * setting(value)).floor();
    (65535. * (index as f32 / 255.).powf(exponent))
        .floor()
        .max(1.) as u16
}

pub fn sample(encoded: f32, value: f32) -> f32 {
    let pos = encoded.clamp(0., 1.) * 255.;
    let lo = pos.floor() as u8;
    let hi = lo.saturating_add(1);
    let a = entry(lo, value) as f32 / 65535.;
    let b = entry(hi, value) as f32 / 65535.;
    a + (b - a) * pos.fract()
}

pub fn decode_srgb(encoded: f32) -> f32 {
    if encoded <= 0.04045 {
        encoded / 12.92
    } else {
        ((encoded + 0.055) / 1.055).powf(2.4)
    }
}

// Shared by the whole-frame display pass and the standalone post diagnostic.
// Settings are sanitized on the CPU before upload.
pub const WGSL: &str = r#"
fn client_gamma_ramp(encoded:vec3<f32>, setting:f32)->vec3<f32> {
 let input=clamp(encoded,vec3<f32>(0.),vec3<f32>(1.))*255.;
 let lo=floor(input);let hi=min(lo+1.,vec3<f32>(255.));
 let exponent=100./floor(30.+1.7*floor(clamp(setting,0.,100.)));
 let a=select(vec3<f32>(0.),max(floor(pow(lo/255.,vec3<f32>(exponent))*65535.),vec3<f32>(1.))/65535.,lo>vec3<f32>(0.));
 let b=select(vec3<f32>(0.),max(floor(pow(hi/255.,vec3<f32>(exponent))*65535.),vec3<f32>(1.))/65535.,hi>vec3<f32>(0.));
 return mix(a,b,fract(input));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn windowed_never_applies_display_ramp() {
        assert!(!enabled(false, false));
        assert!(!enabled(true, false));
        assert!(!enabled(false, true));
        assert!(enabled(true, true));
    }
    #[test]
    fn every_setting_has_monotonic_ramp_and_exact_endpoints() {
        for value in 0..=100 {
            assert_eq!(entry(0, value as f32), 0);
            assert_eq!(entry(255, value as f32), 65535);
            for i in 1..=255u8 {
                assert!(entry(i, value as f32) >= 1);
                assert!(entry(i, value as f32) >= entry(i - 1, value as f32));
            }
        }
    }
    #[test]
    fn integer_config_truncation_and_invalid_settings() {
        assert_eq!(setting(f32::NAN), 41.);
        assert_eq!(setting(f32::INFINITY), 41.);
        assert_eq!(setting(-1.), 0.);
        assert_eq!(setting(101.), 100.);
        for i in 0..=255u8 {
            assert_eq!(entry(i, 41.99), entry(i, 41.));
        }
        assert!(sample(0.5, 100.) > sample(0.5, 41.));
        assert!(sample(0.5, 41.) > sample(0.5, 0.));
    }
    #[test]
    fn interpolation_and_srgb_boundary() {
        let mid = 127.5 / 255.;
        let expected = (entry(127, 41.) as f32 + entry(128, 41.) as f32) / (2. * 65535.);
        assert!((sample(mid, 41.) - expected).abs() < 1e-6);
        assert_eq!(sample(-1., 41.), 0.);
        assert_eq!(sample(2., 41.), 1.);
        assert!((decode_srgb(0.04045) - 0.0031308).abs() < 1e-7);
        assert!((decode_srgb(0.5) - 0.21404114).abs() < 1e-7);
    }
}
