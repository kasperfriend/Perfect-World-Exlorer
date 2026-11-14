//! The presentation choices that are actually implemented. Do not put an SDK
//! name here until its API is called on real frames; a bilinear fallback must
//! never be labelled DLSS, FSR 2/3/4 or XeSS.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Upscaling {
    #[default]
    Bilinear,
    Fsr1,
}

impl Upscaling {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bilinear => "Bilinear (original)",
            Self::Fsr1 => "AMD FSR 1 (EASU + RCAS)",
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::Bilinear => "Bilinear",
            Self::Fsr1 => "FSR1",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "Bilinear" => Some(Self::Bilinear),
            "FSR1" => Some(Self::Fsr1),
            _ => None,
        }
    }
}

/// EASU is specified for up to 4x area upscaling (at most 2x per axis),
/// not downsampling. Outside this range, or if clamping to device limits
/// makes even one input axis larger than the output, keep bilinear instead
/// of feeding EASU an unsupported configuration.
pub fn fsr1_eligible(input: (u32, u32), output: (u32, u32)) -> bool {
    input.0 > 0
        && input.1 > 0
        && output.0 > 0
        && output.1 > 0
        && input.0 <= output.0
        && input.1 <= output.1
        && u64::from(input.0) * 2 >= u64::from(output.0)
        && u64::from(input.1) * 2 >= u64::from(output.1)
        && input != output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_tokens_are_explicit_and_not_sdk_impersonations() {
        assert_eq!(Upscaling::default(), Upscaling::Bilinear);
        for mode in [Upscaling::Bilinear, Upscaling::Fsr1] {
            assert_eq!(Upscaling::parse(mode.token()), Some(mode));
        }
        for missing_sdk in ["DLSS", "FSR3", "FSR4", "XeSS", "NIS", "fsr1"] {
            assert_eq!(Upscaling::parse(missing_sdk), None);
        }
    }

    #[test]
    fn easu_only_runs_within_its_supported_upscale_range() {
        assert!(fsr1_eligible((960, 540), (1920, 1080)));
        assert!(fsr1_eligible((1, 1), (2, 2)));
        assert!(!fsr1_eligible((192, 108), (1920, 1080)));
        assert!(!fsr1_eligible((999, 540), (2000, 1080)));
        assert!(!fsr1_eligible((1920, 1080), (1920, 1080)));
        assert!(!fsr1_eligible((3840, 2160), (1920, 1080)));
        assert!(!fsr1_eligible((2048, 540), (1920, 1080)));
        assert!(!fsr1_eligible((0, 540), (1920, 1080)));
    }
}
