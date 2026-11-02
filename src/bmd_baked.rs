//! Baked BMD draw states for the inspected 1.4.2 client.
//!
//! World BMDs are *baked*: the client lights them into two per-vertex colours
//! (mesh members `+0x284`/`+0x288`) and blends those two arrays at `0x7b9bc0`
//! with the world day/night weight. They never enter the ordinary receiving
//! path (`0x7a714f..0x7a7158`, `0x7926d2..0x79275a`), which would add regional
//! ambient/diffuse/specular a second time.
//!
//! This module pins the draw state the production shader (`gpu.rs`,
//! `bmd_baked_rgb` and `fs`) implements, and the integer blend the client uses.
//! It deliberately covers only the two mesh versions the reader was inspected
//! against: `0x10000004` and `0x10000005`.

/// Mesh versions the reader and this draw state were inspected against.
pub const INSPECTED_MESH_VERSIONS: [u32; 2] = [0x1000_0004, 0x1000_0005];

/// Texture stage combination for one baked mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// No texture: the baked vertex colour is the fragment colour.
    ColorOnly,
    /// MODULATE2X (`0x7a9730`, texture stage 5): `texture.rgb * colour * 2`.
    Modulate2x,
}

/// Alpha test applied to the sampled texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlphaTest {
    /// No alpha test; the mesh is fully opaque apart from the queue/blend state.
    None,
    /// `texture.a < 0.5` (alpha registration `0x7a59c5..0x7a5b6f`).
    TextureHalf,
}

/// Back-face handling for one baked mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cull {
    /// Single-sided: the production shader drops the reversed winding.
    BackFace,
    /// v5 `two_sided`/no-cull material flag (`+0x26c`, culling `0x7a98c5`).
    TwoSided,
}

/// Draw state for one baked mesh, including the flat `kind` float the shared
/// world pipeline carries into the shader.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawState {
    pub stage: Stage,
    pub alpha: AlphaTest,
    pub cull: Cull,
    pub kind: f32,
}

/// Classifies an inspected baked mesh. Unsupported versions are refused rather
/// than silently mapped onto a guessed draw state.
pub fn classify(mesh_version: u32, textured: bool, two_sided: bool) -> Result<DrawState, String> {
    if !INSPECTED_MESH_VERSIONS.contains(&mesh_version) {
        return Err(format!(
            "unsupported BMD mesh version {mesh_version:#x} for baked draw state"
        ));
    }
    Ok(match (textured, two_sided) {
        // The no-cull flag only exists on v5 materials; a v4 mesh with a
        // two-sided flag is a caller bug, not something to paper over.
        (_, true) if mesh_version != 0x1000_0005 => {
            return Err("two-sided flag requires an inspected v5 material".into())
        }
        (false, _) => DrawState {
            stage: Stage::ColorOnly,
            alpha: AlphaTest::None,
            cull: Cull::BackFace,
            kind: 1.0,
        },
        (true, false) => DrawState {
            stage: Stage::Modulate2x,
            alpha: AlphaTest::TextureHalf,
            cull: Cull::BackFace,
            kind: 2.0,
        },
        (true, true) => DrawState {
            stage: Stage::Modulate2x,
            alpha: AlphaTest::TextureHalf,
            cull: Cull::TwoSided,
            kind: 2.1,
        },
    })
}

/// Integer day/night vertex blend, mirroring `bmd_baked_rgb` in `gpu.rs`:
/// `(day * (255 - w) + night * w) >> 8` per channel, `w` in `0..=255`.
pub fn blend(day: [u8; 3], night: [u8; 3], night_factor: u8) -> [u8; 3] {
    let w = u32::from(night_factor);
    let mix = |a: u8, b: u8| -> u8 {
        (((u32::from(a) * (255 - w)) + u32::from(b) * w) >> 8) as u8
    };
    [
        mix(day[0], night[0]),
        mix(day[1], night[1]),
        mix(day[2], night[2]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspected_versions_classify_and_unknown_versions_are_refused() {
        assert_eq!(classify(0x1000_0005, true, false).unwrap().kind, 2.0);
        assert_eq!(
            classify(0x1000_0005, true, true).unwrap().cull,
            Cull::TwoSided
        );
        assert_eq!(classify(0x1000_0004, true, false).unwrap().kind, 2.0);
        assert_eq!(
            classify(0x1000_0004, false, false).unwrap().stage,
            Stage::ColorOnly
        );
        for v in [0x1000_0003, 0x1000_0006, 0] {
            assert!(classify(v, true, false).is_err(), "{v:#x}");
        }
        // v4 has no material block, so a two-sided flag cannot be honoured.
        assert!(classify(0x1000_0004, true, true).is_err());
    }

    #[test]
    fn untextured_meshes_never_take_the_modulate_or_alpha_path() {
        let s = classify(0x1000_0005, false, false).unwrap();
        assert_eq!(s.stage, Stage::ColorOnly);
        assert_eq!(s.alpha, AlphaTest::None);
        assert_eq!(s.kind, 1.0);
    }

    #[test]
    fn integer_blend_matches_the_recovered_arithmetic() {
        // Endpoints are exact, intermediate values keep the client's >>8.
        assert_eq!(blend([10, 20, 30], [200, 210, 220], 0), [10, 20, 30]);
        assert_eq!(blend([10, 20, 30], [200, 210, 220], 255), [9, 19, 29]);
        assert_eq!(blend([0, 0, 0], [255, 255, 255], 128), [127u8, 127, 127]);
        // Full-day is not the identity for a 255 value: 255*255>>8 == 254.
        assert_eq!(blend([255, 255, 255], [0, 0, 0], 0), [254, 254, 254]);
    }

    /// The production shader must keep this module's states. These strings are
    /// the exact WGSL in `gpu.rs`; a change here means the draw state above is
    /// stale.
    #[test]
    fn production_wgsl_implements_the_classified_states() {
        let gpu = include_str!("gpu.rs");
        assert!(gpu.contains(
            "fn bmd_baked_rgb(day:u32, night:u32, night_factor:f32)->vec3<u32>"
        ));
        assert!(
            gpu.contains("((a*(255u-w)+b*w))>>vec3<u32>(8u)")
                || gpu.contains("a*(255u-w)+b*w"),
            "integer day/night blend must stay integer"
        );
        assert!(gpu.contains("1.-camera.direction.w"), "night factor source");
        assert!(gpu.contains("raw_tex.rgb*i.diffuse*2."), "MODULATE2X");
        assert!(gpu.contains("tex.a<select(0.5,"), "texture alpha < 0.5");
        assert!(
            gpu.contains("i.kind>1.5 && i.kind<2.05 && camera.clip.w == 0."),
            "single-sided winding drop"
        );
        // The draw-state kinds must be the ones the world builder emits.
        assert!(gpu.contains("2.1"), "two-sided baked kind");
        assert!(gpu.contains("2.0"), "single-sided baked kind");
        // Baked meshes must not consume the ordinary receiving light.
        let baked = gpu
            .split("fn bmd_baked_rgb")
            .nth(1)
            .expect("baked helper");
        let baked = baked.split("fn ").next().unwrap();
        assert!(!baked.contains("camera.light"));
    }
}
