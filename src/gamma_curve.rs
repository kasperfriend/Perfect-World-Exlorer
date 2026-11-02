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

// Captured by tools/capture-native-gamma-x87.c using the client's actual
// MSVCRT _CIpow/_ftol imports at PC24. The CRT's internal precision behavior
// is NOT reproduced by a double-precision pow followed by an f32 store.
// Every word is compared with a fresh independent Windows-x86 capture in CI.
const NATIVE_WORDS: &[u8; 101 * 256 * 2] = include_bytes!("data/display-gamma-pc24.bin");
pub fn entry(index: u8, value: f32) -> u16 {
    let p = (setting(value) as usize * 256 + index as usize) * 2;
    u16::from_le_bytes([NATIVE_WORDS[p], NATIVE_WORDS[p + 1]])
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
/// Uniform-safe packing: 64 aligned vec4s hold the actual 256-entry ramp.
/// Uploading CPU-built entries avoids GPU pow/float-intermediate differences.
pub fn table(value: f32) -> [[f32; 4]; 64] {
    std::array::from_fn(|i| {
        std::array::from_fn(|c| entry((i * 4 + c) as u8, value) as f32 / 65535.)
    })
}
pub const WGSL: &str = r#"
fn client_gamma_entry(index:u32)->f32 {
 return p.ramp[index/4u][index%4u];
}
fn client_gamma_ramp(encoded:vec3<f32>)->vec3<f32> {
 let input=clamp(encoded,vec3<f32>(0.),vec3<f32>(1.))*255.;
 let lo=vec3<u32>(floor(input));let hi=min(lo+vec3<u32>(1u),vec3<u32>(255u));
 let a=vec3<f32>(client_gamma_entry(lo.x),client_gamma_entry(lo.y),client_gamma_entry(lo.z));
 let b=vec3<f32>(client_gamma_entry(hi.x),client_gamma_entry(hi.y),client_gamma_entry(hi.z));
 return mix(a,b,fract(input));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_16bit_words_match_captured_x86_msvcrt() {
        // Reproduce with tools/capture-native-gamma-x87.c, then compare ALL
        // fresh native words with tools/check-native-gamma-words.rs in CI.
        let mut hash = 0xcbf29ce484222325u64;
        for setting in 0..=100 {
            for i in 0..=255 {
                for byte in entry(i, setting as f32).to_le_bytes() {
                    hash = (hash ^ byte as u64).wrapping_mul(0x100000001b3);
                }
            }
        }
        assert_eq!(hash, 0x9f351cddfec7875f);
        // These reject a double/Decimal pow with only surrounding PC24
        // rounding; all eight differ from the native CRT's result.
        for (setting, index, expected) in [
            (2., 239, 53851),
            (26., 128, 25820),
            (29., 194, 46363),
            (61., 151, 44194),
            (70., 204, 56420),
            (75., 42, 20776),
            (82., 59, 27563),
            (87., 45, 24596),
        ] {
            assert_eq!(entry(index, setting), expected);
        }
        assert_eq!(entry(0, 41.), 0);
        assert_eq!(entry(255, 41.), 65535);
    }
    #[test]
    fn uniform_table_contains_every_recovered_word_in_index_order() {
        for setting in 0..=100 {
            let lut = table(setting as f32);
            for i in 0..=255usize {
                assert_eq!(
                    lut[i / 4][i % 4],
                    entry(i as u8, setting as f32) as f32 / 65535.
                );
            }
        }
    }
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
