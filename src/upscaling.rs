//! Presentation policy and the complete Graphics-menu option lists.
//!
//! Only `Upscaling` below is a runtime selector. The SDK entries in the menu
//! are deliberately disabled until their real API is called on real frames;
//! a bilinear fallback must never be labelled DLSS, FSR 2/3/4.1 or XeSS.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Upscaling {
    #[default]
    Bilinear,
    Fsr1,
}

impl Upscaling {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Bilinear => "Original (bilinear)",
            Self::Fsr1 => "FSR1 (AMD EASU + RCAS)",
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

/// Do not confuse a missing SDK with a GPU that failed a capability test.
/// Only supported, implemented entries can be selected in the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    Unsupported(&'static str),
    NotIntegrated(&'static str),
}

impl Availability {
    pub const fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    pub const fn reason(self) -> Option<&'static str> {
        match self {
            Self::Ready => None,
            Self::Unsupported(reason) | Self::NotIntegrated(reason) => Some(reason),
        }
    }
}

/// Order and spelling in Settings → Graphics → Upscaling. The vendor names are
/// present even on machines where their SDK or hardware is not available.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpscaleChoice {
    Original,
    Dlss,
    Fsr1,
    Fsr2,
    Fsr3,
    Fsr41,
    Xess,
}

impl UpscaleChoice {
    pub const ALL: [Self; 7] = [
        Self::Original,
        Self::Dlss,
        Self::Fsr1,
        Self::Fsr2,
        Self::Fsr3,
        Self::Fsr41,
        Self::Xess,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "Original (bilinear, default)",
            Self::Dlss => "DLSS",
            Self::Fsr1 => "FSR1",
            Self::Fsr2 => "FSR2",
            Self::Fsr3 => "FSR3",
            Self::Fsr41 => "FSR4.1",
            Self::Xess => "XeSS",
        }
    }

    /// `None` is not a request for an automatic fallback. It means this entry
    /// cannot be selected or saved until a real implementation exists.
    pub const fn runtime(self) -> Option<Upscaling> {
        match self {
            Self::Original => Some(Upscaling::Bilinear),
            Self::Fsr1 => Some(Upscaling::Fsr1),
            _ => None,
        }
    }

    /// FSR1 uses the already-supported wgpu sRGB sample/render features. The
    /// other SDKs have no engine integration, so claiming that an AMD, RTX or
    /// Intel device is *supported* here would give a false positive.
    pub fn availability(self, input: (u32, u32), output: (u32, u32)) -> Availability {
        match self {
            Self::Original => Availability::Ready,
            Self::Fsr1 if fsr1_eligible(input, output) => Availability::Ready,
            Self::Fsr1 => Availability::Unsupported(
                "FSR1 EASU supports 50–99% resolution scale. Adjust Resolution scale first.",
            ),
            Self::Dlss => Availability::NotIntegrated(
                "The NVIDIA DLSS/Streamline SDK is not integrated. An RTX GPU alone cannot enable it.",
            ),
            Self::Fsr2 => Availability::NotIntegrated(
                "The AMD FSR2 temporal SDK is not integrated; motion/depth/history inputs are missing.",
            ),
            Self::Fsr3 => Availability::NotIntegrated(
                "The AMD FSR3 temporal SDK is not integrated; motion/depth/history inputs are missing.",
            ),
            Self::Fsr41 => Availability::NotIntegrated(
                "FSR4.1 needs eligible AMD hardware and drivers, plus its SDK; the SDK is not integrated.",
            ),
            Self::Xess => Availability::NotIntegrated(
                "The Intel XeSS SDK is not integrated. GPU support cannot be confirmed by the viewer yet.",
            ),
        }
    }
}

/// Independent from `UpscaleChoice`: turning on a working FG SDK must not
/// force the upscaler to be from the same vendor. Only Off is currently safe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameGenChoice {
    Off,
    Dlss,
    Fsr,
    XeFg,
}

impl FrameGenChoice {
    pub const ALL: [Self; 4] = [Self::Off, Self::Dlss, Self::Fsr, Self::XeFg];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off (default)",
            Self::Dlss => "DLSS",
            Self::Fsr => "FSR",
            Self::XeFg => "XeFG (Intel XeSS)",
        }
    }

    pub const fn availability(self) -> Availability {
        match self {
            Self::Off => Availability::Ready,
            Self::Dlss => Availability::NotIntegrated(
                "DLSS Frame Generation/Streamline is not integrated; no generated frames are presented.",
            ),
            Self::Fsr => Availability::NotIntegrated(
                "AMD FSR Frame Generation and its native swapchain/pacing are not integrated.",
            ),
            Self::XeFg => Availability::NotIntegrated(
                "Intel XeSS Frame Generation and required XeLL/presentation integration are not implemented.",
            ),
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
        for missing_sdk in ["DLSS", "FSR2", "FSR3", "FSR4.1", "XeSS", "NIS", "fsr1"] {
            assert_eq!(Upscaling::parse(missing_sdk), None);
        }
    }

    #[test]
    fn upscaling_menu_has_every_requested_entry_in_order() {
        assert_eq!(
            UpscaleChoice::ALL.map(UpscaleChoice::label),
            [
                "Original (bilinear, default)",
                "DLSS",
                "FSR1",
                "FSR2",
                "FSR3",
                "FSR4.1",
                "XeSS",
            ]
        );
        for choice in UpscaleChoice::ALL {
            let status = choice.availability((960, 540), (1920, 1080));
            assert_eq!(status.is_ready(), choice.runtime().is_some());
            if choice.runtime().is_none() {
                assert!(matches!(status, Availability::NotIntegrated(_)));
                assert!(status.reason().is_some());
            }
        }
        assert!(matches!(
            UpscaleChoice::Fsr1.availability((192, 108), (1920, 1080)),
            Availability::Unsupported(_)
        ));
        assert_eq!(
            UpscaleChoice::Original.availability((192, 108), (1920, 1080)),
            Availability::Ready
        );
    }

    #[test]
    fn frame_generation_menu_is_independent_but_never_fakes_an_sdk() {
        assert_eq!(
            FrameGenChoice::ALL.map(FrameGenChoice::label),
            ["Off (default)", "DLSS", "FSR", "XeFG (Intel XeSS)"]
        );
        assert_eq!(FrameGenChoice::Off.availability(), Availability::Ready);
        for choice in [
            FrameGenChoice::Dlss,
            FrameGenChoice::Fsr,
            FrameGenChoice::XeFg,
        ] {
            assert!(matches!(
                choice.availability(),
                Availability::NotIntegrated(_)
            ));
            assert!(!choice.availability().is_ready());
            assert!(choice.availability().reason().is_some());
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
