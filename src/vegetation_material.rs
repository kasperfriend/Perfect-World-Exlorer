//! Explicit vegetation sampler / mip / cutout policy (P4.07).
//!
//! `POLICIES` records native evidence separately from the renderer's current
//! sampler. The approved world path remains generated-mip, 16x-anisotropy, and
//! fixed-threshold, recorded by `legacy_*` and kept default by
//! `ViewSettings::vegetation_authored_mips`.
//! `POLICIES` separates the viewer sampler from native facts. Generic client
//! sampler descriptors are not linked to the vegetation draws, so the native
//! filter/address/anisotropy tuple is explicitly unsupported; `validate` refuses
//! to silently label the viewer sampler as an exact native match.
//!
//! Native evidence (elementclient.exe, PE32 1.4.2):
//! - grass stage 0 U/V = WRAP, stages 1/2 U/V = CLAMP (`0x65127a..0x65129c`);
//!   grass alpha func GREATER (`0x652361`, state 0x19) and per-type ALPHAREF
//!   default 0x54 (`0x652129`, `0x652368..0x652371`, state 0x18).
//! - tree branch/frond and leaf passes write ALPHAREF from SpeedTreeRT object
//!   fields +0x38 / +0x74 (`0x656bf5..0x656c01`, `0x656d3e..0x656d4a`).
//!   Those runtime floats are not retained by the current PWTREE cache, so a
//!   fixed tree cutout reference is explicitly unsupported here.
//! - the generic material apply can write MAXANISOTROPY (`0x796953..0x796966`,
//!   helper `0x77a1d0`, sampler state 0x17), but no vegetation-specific write
//!   was recovered. The opt-in experiment retains the viewer's existing 16x
//!   clamp; that is not asserted to be the native value.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    TreeBark,
    TreeBranch,
    TreeFrond,
    TreeLeaf,
    Grass,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    Point,
    Linear,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Address {
    Wrap,
    Clamp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sampler {
    pub mag: Filter,
    pub min: Filter,
    pub mip: Filter,
    pub u: Address,
    pub v: Address,
    pub anisotropy: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    Never,
    Less,
    Equal,
    LessEqual,
    Greater,
    NotEqual,
    GreaterEqual,
    Always,
}

impl Compare {
    /// D3D alpha-test semantics: the pixel survives when the comparison is true.
    pub fn passes(self, alpha: u8, reference: u8) -> bool {
        match self {
            Self::Never => false,
            Self::Less => alpha < reference,
            Self::Equal => alpha == reference,
            Self::LessEqual => alpha <= reference,
            Self::Greater => alpha > reference,
            Self::NotEqual => alpha != reference,
            Self::GreaterEqual => alpha >= reference,
            Self::Always => true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cutout {
    None,
    Fixed {
        compare: Compare,
        reference: u8,
    },
    /// The client computes the reference at runtime from a source we do not yet
    /// retain. A fixed substitute must be rejected, not silently uploaded.
    Runtime {
        source: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MipRule {
    /// Upload levels 1..7 level-for-level; deeper levels cannot be isolated by
    /// the current atlas packing (each tile reaches one texel at atlas LOD 7).
    PreserveAuthoredThroughLod7,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Policy {
    pub material: Material,
    /// The renderer sampler currently used by this path, not a recovered native tuple.
    pub renderer_sampler: Sampler,
    /// Why this renderer tuple cannot be certified as the client tuple.
    pub native_sampler_gap: &'static str,
    pub cutout: Cutout,
    pub mips: MipRule,
    pub evidence: &'static str,
}

const RENDERER: Sampler = Sampler {
    mag: Filter::Linear,
    min: Filter::Linear,
    mip: Filter::Linear,
    u: Address::Clamp,
    v: Address::Clamp,
    anisotropy: 16,
};

pub const POLICIES: [Policy; 5] = [
    Policy {
        material: Material::TreeBark,
        renderer_sampler: RENDERER,
        native_sampler_gap: concat!(
            "per-material filter/address/anisotropy descriptor is not retained; ",
            "generic apply only (0x796966/0x796e22)"
        ),
        cutout: Cutout::Runtime {
            source: "SpeedTreeRT +0x38 (branch/frond ALPHAREF, 0x656bf5)",
        },
        mips: MipRule::PreserveAuthoredThroughLod7,
        evidence: "trees.pck DDS authored chains; tree pass 0x656b30",
    },
    Policy {
        material: Material::TreeBranch,
        renderer_sampler: RENDERER,
        native_sampler_gap: concat!(
            "per-material filter/address/anisotropy descriptor is not retained; ",
            "generic apply only (0x796966/0x796e22)"
        ),
        cutout: Cutout::Runtime {
            source: "SpeedTreeRT +0x38 (0x656bf5..0x656c01)",
        },
        mips: MipRule::PreserveAuthoredThroughLod7,
        evidence: "ps/treebranch.txt; 0x656b30",
    },
    Policy {
        material: Material::TreeFrond,
        renderer_sampler: RENDERER,
        native_sampler_gap: concat!(
            "per-material filter/address/anisotropy descriptor is not retained; ",
            "generic apply only (0x796966/0x796e22)"
        ),
        cutout: Cutout::Runtime {
            source: "SpeedTreeRT +0x38 (0x656bf5..0x656c01)",
        },
        mips: MipRule::PreserveAuthoredThroughLod7,
        evidence: "ps/treebranch.txt; 0x656b30",
    },
    Policy {
        material: Material::TreeLeaf,
        renderer_sampler: RENDERER,
        native_sampler_gap: concat!(
            "per-material filter/address/anisotropy descriptor is not retained; ",
            "generic apply only (0x796966/0x796e22)"
        ),
        cutout: Cutout::Runtime {
            source: "SpeedTreeRT +0x74 (0x656d3e..0x656d4a)",
        },
        mips: MipRule::PreserveAuthoredThroughLod7,
        evidence: "composite DDS; leaf pass 0x656dd0",
    },
    Policy {
        material: Material::Grass,
        renderer_sampler: RENDERER,
        native_sampler_gap: concat!(
            "filter/anisotropy descriptor unknown; client grass uses stage 0 WRAP ",
            "and stages 1/2 CLAMP, while viewer atlas shares CLAMP world sampler ",
            "(0x65127a..0x65129c)"
        ),
        cutout: Cutout::Fixed {
            compare: Compare::Greater,
            reference: 84,
        },
        mips: MipRule::PreserveAuthoredThroughLod7,
        evidence: concat!(
            "0x65127a..0x65129c, 0x652129, 0x652361..0x652371; ",
            "ps/grassrender.txt"
        ),
    },
];

pub fn policy(material: Material) -> &'static Policy {
    POLICIES
        .iter()
        .find(|p| p.material == material)
        .expect("vegetation material policy")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestedMips {
    Authored,
    Generated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub mips: RequestedMips,
    pub anisotropy: u16,
    /// `None` means the caller has no fixed reference to offer. A runtime cutout
    /// still rejects the request because the native source is not retained.
    pub cutout_reference: Option<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    NativeSamplerUnknown {
        requested_anisotropy: u16,
        reason: &'static str,
    },
    RuntimeCutout {
        source: &'static str,
    },
    FixedCutoutMismatch {
        requested: u8,
        native: u8,
    },
    GeneratedMips,
}

/// Fail closed on unsupported sampler/threshold pairs. The caller may keep the
/// approved legacy path, but it must not label a rejected request as native.
pub fn validate(material: Material, request: Request) -> Result<(), Rejection> {
    let p = policy(material);
    if request.mips != RequestedMips::Authored || p.mips != MipRule::PreserveAuthoredThroughLod7 {
        return Err(Rejection::GeneratedMips);
    }
    Err(Rejection::NativeSamplerUnknown {
        requested_anisotropy: request.anisotropy,
        reason: p.native_sampler_gap,
    })
}

/// Validate only the cutout component, independently from the sampler gap.
pub fn validate_cutout(material: Material, reference: Option<u8>) -> Result<(), Rejection> {
    match policy(material).cutout {
        Cutout::None => Ok(()),
        Cutout::Runtime { source } => Err(Rejection::RuntimeCutout { source }),
        Cutout::Fixed {
            reference: native, ..
        } => match reference {
            Some(r) if r == native => Ok(()),
            Some(r) => Err(Rejection::FixedCutoutMismatch {
                requested: r,
                native,
            }),
            None => Err(Rejection::FixedCutoutMismatch {
                requested: 0,
                native,
            }),
        },
    }
}

/// The shipped, approved tree/grass viewer path. P4.08 keeps this default until
/// the replacement material path is complete; it is intentionally separate from
/// the native-evidence policy table above.
pub const LEGACY_ANISOTROPY: u16 = 16;
pub const LEGACY_TREE_ALPHA_REF: u8 = 128;
pub const LEGACY_GRASS_ALPHA_REF: u8 = 84;

pub fn legacy_cutout(material: Material) -> Option<u8> {
    match material {
        Material::Grass => Some(LEGACY_GRASS_ALPHA_REF),
        _ => Some(LEGACY_TREE_ALPHA_REF),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approved_legacy_path_is_unchanged_and_native_sampler_gap_is_explicit() {
        assert_eq!(LEGACY_ANISOTROPY, 16);
        assert_eq!(LEGACY_TREE_ALPHA_REF, 128);
        assert_eq!(LEGACY_GRASS_ALPHA_REF, 84);
        for p in POLICIES {
            assert_eq!(p.renderer_sampler.anisotropy, 16);
            assert!(!p.native_sampler_gap.is_empty());
            assert_eq!(p.mips, MipRule::PreserveAuthoredThroughLod7);
        }
        let request = Request {
            mips: RequestedMips::Authored,
            anisotropy: 16,
            cutout_reference: Some(84),
        };
        assert!(matches!(
            validate(Material::Grass, request),
            Err(Rejection::NativeSamplerUnknown { .. })
        ));
    }

    #[test]
    fn grass_greater_alpha_test_matches_native_boundary() {
        let Cutout::Fixed { compare, reference } = policy(Material::Grass).cutout else {
            panic!("grass cutout must be fixed");
        };
        assert_eq!(reference, 84);
        assert!(!compare.passes(83, reference));
        assert!(!compare.passes(84, reference));
        assert!(compare.passes(85, reference));
        assert!(compare.passes(255, reference));
        assert_eq!(validate_cutout(Material::Grass, Some(84)), Ok(()));
        assert_eq!(
            validate_cutout(Material::Grass, Some(128)),
            Err(Rejection::FixedCutoutMismatch {
                requested: 128,
                native: 84,
            })
        );
    }

    #[test]
    fn tree_alpha_thresholds_remain_explicitly_unsupported() {
        for material in [
            Material::TreeBark,
            Material::TreeBranch,
            Material::TreeFrond,
            Material::TreeLeaf,
        ] {
            assert!(matches!(
                validate_cutout(material, Some(LEGACY_TREE_ALPHA_REF)),
                Err(Rejection::RuntimeCutout { .. })
            ));
        }
    }

    #[test]
    fn generated_mips_are_not_mislabeled_authored() {
        let request = Request {
            mips: RequestedMips::Generated,
            anisotropy: 16,
            cutout_reference: Some(84),
        };
        assert_eq!(
            validate(Material::Grass, request),
            Err(Rejection::GeneratedMips)
        );
    }
}
