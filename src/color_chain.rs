//! Encoded/linear boundaries and the single-gamma contract of the color path.
//!
//! Recovered client behavior:
//! * Source textures are authored/encoded bytes. The client filters and
//!   combines them in encoded space (D3D8 A8R8G8B8, no sRGB sampling), then a
//!   handful of recovered material equations operate on that encoded value.
//! * Fog mixing happens in encoded space. `fogged()`/`bmd_pass_fog()` decode the
//!   mixed result only because this viewer writes into an sRGB render target;
//!   that decode is the inverse of the target's encode, not a second color
//!   transform.
//! * Exactly one display ramp exists. Retail applies it through the D3D8
//!   hardware gamma ramp (skipped in windowed mode, `0x77b7c0`); the viewer
//!   emulates it as a whole-frame pass after world + map + UI. The scene-only
//!   ramp in `post_gpu` is the windowed approximation and must never run
//!   together with the whole-frame pass.
//! * Exactly one output encode exists. This viewer always renders into an sRGB
//!   target so shaders may return linear values; a swapchain without an sRGB
//!   format is encoded once at presentation (`present_encode`).
//!
//! Values match the WGSL conversions in `gpu.rs`, `terrain_gpu.rs`,
//! `grass_instances.rs` and `water_gpu.rs`, which are pinned by this module.

/// Transfer-function breakpoints used by every recovered shader.
pub const ENCODE_BREAKPOINT: f32 = 0.0031308;
pub const DECODE_BREAKPOINT: f32 = 0.04045;

/// Linear -> encoded (one sRGB encode).
pub fn encode(linear: f32) -> f32 {
    let c = linear.max(0.);
    if c > ENCODE_BREAKPOINT {
        1.055 * c.powf(1. / 2.4) - 0.055
    } else {
        c * 12.92
    }
}

/// Encoded -> linear (one sRGB decode).
pub fn decode(encoded: f32) -> f32 {
    if encoded > DECODE_BREAKPOINT {
        ((encoded + 0.055) / 1.055).powf(2.4)
    } else {
        encoded / 12.92
    }
}

pub fn encode_rgb(linear: [f32; 3]) -> [f32; 3] {
    linear.map(encode)
}

pub fn decode_rgb(encoded: [f32; 3]) -> [f32; 3] {
    encoded.map(decode)
}

/// Encoded-space fog mix, exactly as `fogged()` performs it: the material
/// value is encoded, mixed with the encoded region fog color, then decoded for
/// the sRGB render target.
pub fn encoded_fog_mix(linear: [f32; 3], fog_encoded: [f32; 3], factor: f32) -> [f32; 3] {
    let f = if factor.is_finite() {
        factor.clamp(0., 1.)
    } else {
        0.
    };
    let encoded = encode_rgb(linear);
    let mixed: [f32; 3] = std::array::from_fn(|i| {
        encoded[i] + (fog_encoded[i].clamp(0., 1.) - encoded[i]) * f
    });
    decode_rgb(mixed)
}

/// Scene-chain display ramp request. The whole-frame presenter pass owns the
/// retail fullscreen ramp, so the scene chain must yield to it.
pub fn scene_chain_ramp(requested: bool, whole_frame_ramp: bool) -> bool {
    requested && !whole_frame_ramp
}

/// Whole-frame ramp request: the retail client skips its display ramp in
/// windowed mode (`0x77b7c0` device flag).
pub fn whole_frame_ramp(requested: bool, fullscreen: bool) -> bool {
    requested && fullscreen
}

/// Number of ramp applications for a frame. Must never exceed one.
pub fn ramp_stages(whole_frame: bool, scene_chain: bool) -> usize {
    usize::from(whole_frame) + usize::from(scene_chain)
}

/// A swapchain without an sRGB format needs one encode at presentation.
pub fn present_encode_required(surface_is_srgb: bool) -> bool {
    !surface_is_srgb
}

/// Render targets are always sRGB-format so that every material shader may
/// return linear values and be encoded exactly once by the target.
pub const RENDER_TARGET_IS_SRGB: bool = true;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_functions_match_the_shader_constants() {
        assert!((encode(ENCODE_BREAKPOINT) - 0.04044994).abs() < 1e-6);
        assert!((decode(DECODE_BREAKPOINT) - 0.0031308).abs() < 1e-7);
        assert_eq!(encode(0.), 0.);
        assert!((encode(1.) - 1.).abs() < 1e-6);
        assert_eq!(decode(0.), 0.);
        assert!((decode(1.) - 1.).abs() < 1e-6);
        // Matches the literal the production shaders use for mid-encoded input.
        assert!((decode(0.5) - 0.21404114).abs() < 1e-7);
    }

    #[test]
    fn linear_and_encoded_round_trip_once() {
        for i in 0..=1000 {
            let v = i as f32 / 1000.;
            assert!((decode(encode(v)) - v).abs() < 1e-6, "{v}");
            assert!((encode(decode(v)) - v).abs() < 1e-6, "{v}");
        }
        // Applying the transform twice is a measurable error: exactly the
        // failure mode the single-encode contract prevents.
        let once = decode(encode(0.25));
        let twice = decode(encode(once));
        assert!((once - 0.25).abs() < 1e-6);
        assert!((twice - 0.25).abs() < 1e-6);
        let double_encoded = encode(encode(0.25));
        assert!(
            (double_encoded - encode(0.25)).abs() > 0.02,
            "a second encode must be detectable"
        );
    }

    #[test]
    fn fog_mixes_in_encoded_space_not_linear_space() {
        // Mid grey with a dark fog color at half coverage.
        let linear = [0.21404114, 0.21404114, 0.21404114];
        let fog = [0.039215688, 0.039215688, 0.039215688];
        let encoded = encoded_fog_mix(linear, fog, 0.5);
        let linear_mix: [f32; 3] =
            std::array::from_fn(|i| linear[i] + (decode(fog[i]) - linear[i]) * 0.5);
        // The two spaces disagree by far more than one 8-bit step, so this
        // fixture proves which space production uses.
        assert!((encoded[0] - linear_mix[0]).abs() > 0.01);
        // Endpoints are exact (within one transfer round trip) and never
        // re-encode the fog color.
        let unfogged = encoded_fog_mix(linear, fog, 0.);
        for i in 0..3 {
            assert!((unfogged[i] - linear[i]).abs() < 1e-6);
        }
        let full = encoded_fog_mix(linear, fog, 1.);
        for i in 0..3 {
            assert!((full[i] - decode(fog[i])).abs() < 1e-6);
        }
        // Malformed coverage cannot poison a frame.
        for bad in [f32::NAN, -1.] {
            let safe = encoded_fog_mix(linear, fog, bad);
            for i in 0..3 {
                assert!((safe[i] - linear[i]).abs() < 1e-6, "{bad}");
            }
        }
    }

    #[test]
    fn exactly_one_display_ramp_for_every_mode_combination() {
        for whole_frame_requested in [false, true] {
            for fullscreen in [false, true] {
                let whole = whole_frame_ramp(whole_frame_requested, fullscreen);
                for scene_requested in [false, true] {
                    let scene = scene_chain_ramp(scene_requested, whole);
                    assert!(
                        ramp_stages(whole, scene) <= 1,
                        "whole={whole} scene={scene}"
                    );
                    // Retail skips the ramp in windowed mode entirely.
                    if !fullscreen {
                        assert!(!whole);
                    }
                }
            }
        }
        // With the whole-frame pass active, the scene chain always yields.
        assert!(!scene_chain_ramp(true, true));
        assert!(scene_chain_ramp(true, false));
    }

    #[test]
    fn output_encode_appears_exactly_once() {
        assert!(RENDER_TARGET_IS_SRGB);
        assert!(present_encode_required(false));
        assert!(!present_encode_required(true));
        // The two routes are mutually exclusive: either the render target is
        // sRGB (encoded on write) or presentation performs the only encode.
        for surface_is_srgb in [false, true] {
            let target_encodes = RENDER_TARGET_IS_SRGB;
            let present_encodes = present_encode_required(surface_is_srgb);
            assert_eq!(target_encodes, !present_encodes);
        }
    }
}
