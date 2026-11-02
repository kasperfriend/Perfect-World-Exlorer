//! Receiving-class matrix for the 1.4.2 scene-light consumers.
//!
//! This module is deliberately dependency-free decision data: it states which
//! object classes receive runtime scene lights at all, which mesh kind maps to
//! which vertex-shader variant, and which constants that variant consumes.
//! It performs no drawing. The arithmetic itself lives in `model_material`,
//! `model_light` and `model_receiver_gpu`.
//!
//! Evidence summary (see Findings/Phase-1-Agent-1-report.md for the table):
//! * Ordinary CECModel skins/rigid meshes are the receivers: 0x7a6eff selects
//!   the receiver direction/point/attenuation constants, 0x7a6110 binds
//!   material constants, 0x7a62db builds directional specular.
//! * 0x6cfa64 pushes the specular selector through 0x7924b0/0x7924c1, so a
//!   normally loaded ECM uses the specular vertex variants by default.
//! * Baked BMD scenery is lit by its baked day/night vertex colors
//!   (0x7b9bc0), terrain/grass by T2LM maps, and the approved tree fallback
//!   by its own shader. None of those may be point-lit a second time.

#![allow(dead_code)]

/// Object class that a render pass could submit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recipient {
    /// CECModel skin/rigid children drawn by the recovered receiver shader.
    OrdinaryModel,
    /// Baked BMD static scenery with per-vertex day/night colors.
    BakedModel,
    /// Terrain layers with T2LM day/night maps.
    Terrain,
    /// Instanced grass with area-centered lightmaps.
    Grass,
    /// Approved preserved tree/foliage shader.
    Tree,
}

impl Recipient {
    /// Only ordinary receiving models consume runtime scene lights. The other
    /// classes are already lit by the paths recovered for Agents 2 and 4 and
    /// must not be relit here.
    pub fn receives_runtime_scene_lights(self) -> bool {
        matches!(self, Recipient::OrdinaryModel)
    }

    /// Why the class is excluded, or what makes the ordinary class eligible.
    pub fn light_source(self) -> &'static str {
        match self {
            Recipient::OrdinaryModel => {
                "material ambient/diffuse/emissive + regional ambient/diffuse/specular + scene query"
            }
            Recipient::BakedModel => "baked per-vertex day/night blend, 0x7b9bc0",
            Recipient::Terrain => "T2LM terrain lightmaps",
            Recipient::Grass => "T2LM area lightmaps",
            Recipient::Tree => "approved tree shader default lighting",
        }
    }

    /// Render passes in which the class is actually submitted by the viewer.
    pub fn passes(self) -> &'static [&'static str] {
        match self {
            Recipient::OrdinaryModel => &["main", "reflection", "clipped refraction"],
            Recipient::BakedModel => &["main", "reflection", "clipped refraction"],
            Recipient::Terrain => &["main", "clipped water"],
            Recipient::Grass => &["main", "clipped water"],
            Recipient::Tree => &["main", "reflection"],
        }
    }
}

/// Serialized mesh kind. `Morph` and `Supple` are named so their rejection is
/// explicit; the production SKI reader already fails closed on those records.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshKind {
    /// Weighted skin mesh (three/four-bone palette deformation).
    Weighted,
    /// Rigid skin mesh (single direct skeleton bone).
    Rigid,
    /// Morph mesh: a distinct shader family, not implemented here.
    Morph,
    /// Supple/physique mesh: distinct shader family, not implemented here.
    Supple,
}

/// Vertex-shader variant actually selected for a receiver mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variant {
    NonSpecular,
    NonSpecularPoint,
    Specular,
    SpecularPoint,
}

impl Variant {
    /// Shader helper the variant corresponds to. `skin_mesh_m4_s_dl` shares the
    /// specular instruction sequence for the inspected corpus meshes.
    pub fn shader(self) -> &'static str {
        match self {
            Variant::NonSpecular => "rigid_mesh_ns",
            Variant::NonSpecularPoint => "rigid_mesh_ns_dl",
            Variant::Specular => "rigid_mesh_s",
            Variant::SpecularPoint => "rigid_mesh_s_dl",
        }
    }

    pub fn point_enabled(self) -> bool {
        matches!(self, Variant::NonSpecularPoint | Variant::SpecularPoint)
    }

    /// Specular variants clamp the attenuation denominator up to one
    /// (`max r9.x, r9.x, c0.x`) before the reciprocal; the non-specular
    /// variants do not.
    pub fn attenuation_floor_one(self) -> bool {
        matches!(self, Variant::Specular | Variant::SpecularPoint)
    }

    /// `_ns_dl` multiplies every c8 channel by the directional Lambert; the
    /// non-point variant copies c8.a unchanged. `_s*` always preserves c8.a.
    pub fn alpha_uses_directional_lambert(self) -> bool {
        matches!(self, Variant::NonSpecularPoint)
    }
}

/// Complete per-mesh assignment produced before any GPU state is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assignment {
    pub kind: MeshKind,
    pub variant: Variant,
    /// c9 = material.specular * directional specular light color.
    pub directional_specular: bool,
    /// c11.rgb / c12 = material diffuse/ambient * point diffuse/ambient.
    pub point_constants: bool,
    /// c11.a is forced to zero for every point variant (0x7a63af).
    pub zero_point_diffuse_alpha: bool,
}

/// Map a receiver mesh to its lighting variant.
///
/// `specular_selector` is the recovered skin-model specular flag (+0x1a1)
/// that 0x6cfa64 enables for ordinary loaded ECMs. `point_enabled` is true
/// only when the scene query returned a candidate for this placement, which is
/// independent of candidate distance, authored `range` and shader attenuation.
pub fn assign(
    kind: MeshKind,
    specular_selector: bool,
    point_enabled: bool,
) -> Result<Assignment, String> {
    if matches!(kind, MeshKind::Morph | MeshKind::Supple) {
        return Err(format!(
            "{kind:?} receiver mesh has no implemented 1.4.2 shader variant"
        ));
    }
    let variant = match (specular_selector, point_enabled) {
        (false, false) => Variant::NonSpecular,
        (false, true) => Variant::NonSpecularPoint,
        (true, false) => Variant::Specular,
        (true, true) => Variant::SpecularPoint,
    };
    Ok(Assignment {
        kind,
        variant,
        directional_specular: specular_selector,
        point_constants: point_enabled,
        zero_point_diffuse_alpha: point_enabled,
    })
}

/// Shader attenuation has no range cutoff. `SceneLight::range` participates in
/// candidate selection and in the published runtime light's cell bounds only;
/// the recovered vertex shaders evaluate `1/(a0 + a1*d + a2*d*d)` and never
/// compare the distance against a range. Keep both facts separate.
pub fn attenuation_denominator(stored: [f32; 3], distance: f32) -> Option<f32> {
    let denom = stored[0] + stored[1] * distance + stored[2] * distance * distance;
    denom.is_finite().then_some(denom)
}

/// One row of the documented receiving-class matrix. Kept as data so tests can
/// prove that no class/variant silently loses its assignment.
#[derive(Clone, Copy, Debug)]
pub struct MatrixRow {
    pub recipient: Recipient,
    pub kind: Option<MeshKind>,
    pub variant: Option<Variant>,
    pub eligibility: &'static str,
    pub constants: &'static str,
    pub attenuation: &'static str,
    pub passes: &'static [&'static str],
    pub exclusion: &'static str,
}

pub const MATRIX: &[MatrixRow] = &[
    MatrixRow {
        recipient: Recipient::OrdinaryModel,
        kind: Some(MeshKind::Weighted),
        variant: Some(Variant::NonSpecular),
        eligibility: "visible, no point candidate, specular selector off",
        constants: "c7=c7.a unclamped; c8 copied; c11/c12 zero",
        attenuation: "not evaluated",
        passes: &["main", "reflection", "clipped refraction"],
        exclusion: "transparency>=1 or invisible",
    },
    MatrixRow {
        recipient: Recipient::OrdinaryModel,
        kind: Some(MeshKind::Weighted),
        variant: Some(Variant::NonSpecularPoint),
        eligibility: "visible, point candidate, specular selector off",
        constants: "c7, c8 lambert-scaled, c11.a=0, c12",
        attenuation: "1/(a0+a1*d+a2*d*d), no range cut",
        passes: &["main", "reflection", "clipped refraction"],
        exclusion: "unsupported ECM blend/color override",
    },
    MatrixRow {
        recipient: Recipient::OrdinaryModel,
        kind: Some(MeshKind::Rigid),
        variant: Some(Variant::Specular),
        eligibility: "visible, no point candidate, specular selector on",
        constants: "c7, c8.a preserved, c9=directional specular",
        attenuation: "not evaluated",
        passes: &["main", "reflection", "clipped refraction"],
        exclusion: "morph-specular shader (different instructions)",
    },
    MatrixRow {
        recipient: Recipient::OrdinaryModel,
        kind: Some(MeshKind::Rigid),
        variant: Some(Variant::SpecularPoint),
        eligibility: "visible, point candidate, specular selector on",
        constants: "c7, c8.a preserved, c9, c11.a=0, c12",
        attenuation: "denominator floored to 1 before reciprocal",
        passes: &["main", "reflection", "clipped refraction"],
        exclusion: "alpha queue without authored blend mapping",
    },
    MatrixRow {
        recipient: Recipient::BakedModel,
        kind: None,
        variant: None,
        eligibility: "never",
        constants: "baked vertex day/night colors",
        attenuation: "not applicable",
        passes: &["main", "reflection", "clipped refraction"],
        exclusion: "already baked; runtime point lights would double-light it",
    },
    MatrixRow {
        recipient: Recipient::Terrain,
        kind: None,
        variant: None,
        eligibility: "never",
        constants: "T2LM day/night tile colors",
        attenuation: "not applicable",
        passes: &["main", "clipped water"],
        exclusion: "already lightmapped; no runtime point term",
    },
    MatrixRow {
        recipient: Recipient::Grass,
        kind: None,
        variant: None,
        eligibility: "never",
        constants: "T2LM area lightmaps",
        attenuation: "not applicable",
        passes: &["main", "clipped water"],
        exclusion: "already lightmapped; no runtime point term",
    },
    MatrixRow {
        recipient: Recipient::Tree,
        kind: None,
        variant: None,
        eligibility: "never",
        constants: "preserved fallback lighting",
        attenuation: "not applicable",
        passes: &["main", "reflection"],
        exclusion: "approved tree path must keep its current lighting",
    },
];

/// Recovered 1.4.2 state block at `0x41a2f0`. It is the only state block in
/// the executable that could be mistaken for a third model queue, and it is
/// not one: the block reaches the device only as the state callback argument
/// of the dynamic-geometry submitter `0x416ab0` (`push $0x41a2f0` at
/// `0x41a5b5`), and the only code that makes that call is `0x41a5a0`, whose
/// sole caller is `0x6d17ad` in the effect-record draw path (locals at
/// `+0x2bc..+0x2e0`). No CECModel mesh submitter (`0x7a85d0`, `0x7a9120`,
/// `0x7a9730`, or a `0x42b3xx-0x42c7xx` pass function) passes it, so the block
/// draws alpha-tested, unblended effect sprites only. Receiving models are
/// therefore classified into the opaque and alpha queues and nothing else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpecialState {
    /// Native entry point that issues the state writes.
    pub address: u32,
    pub zenable: bool,
    pub zwrite: bool,
    pub alpha_test: bool,
    pub blend: bool,
    /// `D3DRS_ALPHAFUNC` value exactly as written by `0x77a360`.
    pub alpha_func: u32,
    /// `D3DRS_ALPHAREF` value written by `0x77a380`.
    pub alpha_ref: u32,
    /// Whether any receiving-model submitter passes this block. Recovered
    /// false; a `true` here would mean the matrix is missing a third queue.
    pub receiving_model_submitter: bool,
}

/// The block's recovered field values in write order: ZENABLE 1, ZWRITEENABLE
/// 1, ALPHATESTENABLE 1, ALPHABLENDENABLE 0, ALPHAFUNC 7, ALPHAREF `0x54`,
/// then `0x792fc0(arg, 3, 0)`.
pub const SPECIAL_STATE_BLOCK: SpecialState = SpecialState {
    address: 0x41a2f0,
    zenable: true,
    zwrite: true,
    alpha_test: true,
    blend: false,
    alpha_func: 7,
    alpha_ref: 0x54,
    receiving_model_submitter: false,
};

/// Receiving models never use fixed-function lighting. Every mesh submitter
/// turns `D3DRS_LIGHTING` off (`0x77a3c0(0)` at `0x7a914b` and `0x7a9759`)
/// before its draw and restores `1` in its tail (`0x7a96a3`, `0x7a99f9`);
/// none of the model pass functions in `0x42b3xx-0x42c7xx` calls `0x77a3c0`
/// with `1`. There is consequently no fixed-function receiver variant to
/// implement, and the `D3DRS_LIGHTING` writes in the alpha/overlay brackets
/// (`0x7844d0`, `0x7845f0`) only surround the vertex-shader draw.
pub const RECEIVERS_USE_FIXED_FUNCTION_LIGHTING: bool = false;

impl MeshKind {
    /// Whether the inspected retail corpus ships this mesh kind at all. The
    /// census of the private 1.4.2 archive counts 13,820 skin and 1,988 rigid
    /// meshes and **zero** morph, supple or muscle meshes, so the explicit
    /// rejection in `assign` is a fail-closed guard for non-retail data rather
    /// than a behaviour the shipped client exercises.
    pub fn retail_present(self) -> bool {
        matches!(self, MeshKind::Weighted | MeshKind::Rigid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_ordinary_models_receive_runtime_scene_lights() {
        for recipient in [
            Recipient::BakedModel,
            Recipient::Terrain,
            Recipient::Grass,
            Recipient::Tree,
        ] {
            assert!(!recipient.receives_runtime_scene_lights(), "{recipient:?}");
            assert!(!recipient.light_source().is_empty());
            assert!(MATRIX
                .iter()
                .any(|row| row.recipient == recipient && row.variant.is_none()));
        }
        assert!(Recipient::OrdinaryModel.receives_runtime_scene_lights());
        for variant in [
            Variant::NonSpecular,
            Variant::NonSpecularPoint,
            Variant::Specular,
            Variant::SpecularPoint,
        ] {
            let row = MATRIX
                .iter()
                .find(|row| row.variant == Some(variant))
                .unwrap_or_else(|| panic!("matrix omits {variant:?}"));
            // Every variant row names the passes the receiver class uses.
            assert_eq!(row.passes, Recipient::OrdinaryModel.passes());
        }
    }

    #[test]
    fn every_variant_is_distinguishable_by_its_own_flags() {
        let cases = [
            (false, false, Variant::NonSpecular),
            (false, true, Variant::NonSpecularPoint),
            (true, false, Variant::Specular),
            (true, true, Variant::SpecularPoint),
        ];
        let mut seen = Vec::new();
        for (selector, point, expected) in cases {
            let a = assign(MeshKind::Weighted, selector, point).unwrap();
            assert_eq!(a.variant, expected);
            assert_eq!(a.directional_specular, selector);
            assert_eq!(a.point_constants, point);
            assert_eq!(a.zero_point_diffuse_alpha, point);
            assert_eq!(a.variant.point_enabled(), point);
            assert_eq!(a.variant.attenuation_floor_one(), selector);
            assert_eq!(
                a.variant.alpha_uses_directional_lambert(),
                point && !selector
            );
            assert!(!seen.contains(&a.variant.shader()));
            seen.push(a.variant.shader());
        }
        assert_eq!(seen.len(), 4);
        // Rigid and weighted meshes share the variant family; only the
        // deformation path differs.
        assert_eq!(
            assign(MeshKind::Rigid, true, true).unwrap().variant,
            Variant::SpecularPoint
        );
    }

    #[test]
    fn morph_and_supple_receivers_are_rejected_not_approximated() {
        for kind in [MeshKind::Morph, MeshKind::Supple] {
            for selector in [false, true] {
                for point in [false, true] {
                    assert!(assign(kind, selector, point).is_err(), "{kind:?}");
                }
            }
        }
    }

    #[test]
    fn attenuation_has_no_range_cutoff_and_floors_only_for_specular() {
        // Same stored attenuation and distance regardless of authored range.
        let stored = [0., 0.4, 0.];
        assert_eq!(attenuation_denominator(stored, 10.), Some(4.));
        assert_eq!(attenuation_denominator(stored, 0.), Some(0.));
        assert!(attenuation_denominator([1., 0., 0.], 99999.).is_some());
        assert!(attenuation_denominator([f32::NAN, 0., 0.], 1.).is_none());
        assert!(Variant::SpecularPoint.attenuation_floor_one());
        assert!(!Variant::NonSpecularPoint.attenuation_floor_one());
    }
    #[test]
    fn special_state_block_belongs_to_effect_sprites_not_models() {
        assert_eq!(SPECIAL_STATE_BLOCK.address, 0x41a2f0);
        assert!(SPECIAL_STATE_BLOCK.zenable && SPECIAL_STATE_BLOCK.zwrite);
        assert!(SPECIAL_STATE_BLOCK.alpha_test);
        assert!(!SPECIAL_STATE_BLOCK.blend);
        assert_eq!(SPECIAL_STATE_BLOCK.alpha_func, 7);
        assert_eq!(SPECIAL_STATE_BLOCK.alpha_ref, 0x54);
        assert!(!SPECIAL_STATE_BLOCK.receiving_model_submitter);
        // No receiving model is left without an ordinary/alpha assignment.
        for row in MATRIX
            .iter()
            .filter(|r| r.recipient == Recipient::OrdinaryModel)
        {
            assert!(row.variant.is_some(), "{row:?}");
            assert!(!row.exclusion.is_empty(), "{row:?}");
        }
        assert!(!RECEIVERS_USE_FIXED_FUNCTION_LIGHTING);
    }

    #[test]
    fn morph_and_supple_are_named_but_absent_from_the_retail_corpus() {
        assert!(MeshKind::Weighted.retail_present());
        assert!(MeshKind::Rigid.retail_present());
        for kind in [MeshKind::Morph, MeshKind::Supple] {
            assert!(!kind.retail_present(), "{kind:?}");
            assert!(assign(kind, true, true).is_err(), "{kind:?}");
        }
    }
}
