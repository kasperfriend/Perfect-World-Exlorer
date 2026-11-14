//! Upscaling and frame-generation option state machine, SDK discovery and the
//! provider contract (Graphics tab → "Upscaling" and "Frame generation").
//!
//! ## What is real and what is not in this build
//!
//! Everything in this module is real and unit-tested *except the vendor SDK
//! calls*. The vendor SDKs (NVIDIA DLSS / Streamline, AMD FidelityFX FSR,
//! Intel XeSS) could not be downloaded, compiled against or executed in the
//! environment this branch was written in, so the provider `init`/`resolve`
//! implementations return [`UpscaleError::SdkNotIntegrated`] instead of
//! pretending to run. The parts that *are* implemented for real:
//!
//! * the option state machine and the settings tokens (`upscaling`,
//!   `upscale_quality`, `frame_generation`, `upscale_sdk_dir`);
//! * per-provider hardware/SDK gating with visible disabled reasons;
//! * the pairing table for frame generation (which upscaler may be combined
//!   with which frame generator, per the vendors' documentation as of the
//!   knowledge available when this was written — see `docs/blockers.md` and
//!   the "Unsupported pairings" section of the report);
//! * per-provider internal-resolution factors and quality modes;
//! * the jitter (Halton 2,3) sequence and matrix helper the providers need;
//! * discovery of the vendor DLLs in a user-chosen directory.
//!
//! The viewer renders through the existing bilinear `render_scale` path
//! whenever no provider is active; when a provider's hardware/SDK gate passes
//! it renders at the provider's recommended internal resolution through that
//! same path. No temporal accumulation is faked anywhere.
//!
//! ## Settings tokens
//!
//! `upscaling=off|dlss|fsr|xess`,
//! `upscale_quality=ultraquality|quality|balanced|performance|ultraperformance`,
//! `frame_generation=off|dlss|fsr|xess`, `upscale_sdk_dir=<path>`.
//!
//! This module is intentionally free of GPU and windowing dependencies so it
//! can be unit-tested headless and included in the diagnostics with
//! `#[path = "../upscaling.rs"]`.
//!
//! The provider FFI surface (the `Upscaler` trait and the vendor provider
//! structs) is consumed by the diagnostics and the tests rather than by the
//! render loop, so dead-code warnings are expected and allowed.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Vendor IDs as reported by `wgpu::AdapterInfo::vendor`.
pub const VENDOR_NVIDIA: u32 = 0x10de;
pub const VENDOR_AMD: u32 = 0x1002;
pub const VENDOR_AMD_LEGACY: u32 = 0x1022;
pub const VENDOR_INTEL: u32 = 0x8086;

/// Facts about the adapter that are needed for provider gating. Cheap to
/// copy; captured once at startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GpuFacts {
    pub vendor: u32,
    pub device: u32,
    pub name: String,
    /// Human-readable backend name (e.g. `"dx12"`, `"vulkan"`), for reports.
    pub backend: String,
    /// True on software/CPU adapters (lavapipe, llvmpipe, WARP).
    pub cpu_fallback: bool,
}

impl GpuFacts {
    pub fn from_parts(
        vendor: u32,
        device: u32,
        name: &str,
        backend: &str,
        cpu_fallback: bool,
    ) -> Self {
        Self {
            vendor,
            device,
            name: name.to_string(),
            backend: backend.to_string(),
            cpu_fallback,
        }
    }

    pub fn is_nvidia(&self) -> bool {
        self.vendor == VENDOR_NVIDIA
    }

    pub fn is_amd(&self) -> bool {
        self.vendor == VENDOR_AMD || self.vendor == VENDOR_AMD_LEGACY
    }

    pub fn is_intel(&self) -> bool {
        self.vendor == VENDOR_INTEL
    }

    /// Heuristic for DLSS Frame Generation eligibility (Ada/Lovelace-class,
    /// i.e. RTX 40 series and later). The NVIDIA SDK is the authority; this
    /// only decides whether the option is offered before the SDK says so.
    pub fn supports_dlss_frame_generation(name: &str) -> bool {
        let n = name.to_lowercase();
        const NEEDLES: [&str; 14] = [
            "rtx 40", "rtx 50", "rtx a4000", "rtx a4500", "rtx a5000", "rtx a5500", "rtx a6000",
            "rtx 4000 ada", "rtx 4500 ada", "rtx 5000 ada", "rtx 5888", "l4", "l40", "l40s",
        ];
        NEEDLES.iter().any(|needle| n.contains(needle))
    }
}

/// Where the SDK probe looked and what it found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SdkSource {
    /// No SDK folder was configured or auto-detected.
    #[default]
    Unset,
    /// `PW_UPSCALER_SDK_DIR` environment variable.
    Env,
    /// `upscale_sdk_dir` settings key (Settings → Graphics).
    Settings,
    /// `<exe directory>\sdk\` auto-detected beside the viewer.
    ExeFolder,
}

impl SdkSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Unset => "no SDK folder configured",
            Self::Env => "PW_UPSCALER_SDK_DIR",
            Self::Settings => "upscale_sdk_dir (settings)",
            Self::ExeFolder => "sdk folder next to the executable",
        }
    }
}

/// Presence of the vendor DLLs in one SDK folder. Presence-only: this build
/// never loads a DLL, so a "present" flag means "the file exists", not
/// "the SDK works". Names vary between SDK releases; the report names the
/// file that was matched so a wrong-version SDK is diagnosable.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SdkReport {
    /// The folder that was probed, if any.
    pub dir: Option<PathBuf>,
    pub source: SdkSource,
    /// The probed path exists and is a directory.
    pub dir_is_dir: bool,
    /// `nvngx_dlss.dll` — DLSS Super Resolution.
    pub dlss: bool,
    /// `nvngx_dlssg.dll` — DLSS Frame Generation.
    pub dlss_frame_generation: bool,
    /// `sl.interposer.dll` — Streamline host (DLSS goes through it in most
    /// integrations).
    pub streamline: bool,
    /// An FSR upscaling component: `ffx_fsr3upscaler_x64.dll`,
    /// `ffx_fsr2_api_x64.dll` or `ffx_fsr_api_x64.dll`.
    pub fsr_upscale: bool,
    /// `ffx_framegeneration_x64.dll` — FSR 3 frame generation.
    pub fsr_frame_generation: bool,
    /// `ffx_opticalflow_x64.dll` — FSR 3 frame generation dependency.
    pub fsr_optical_flow: bool,
    /// `libxess.dll` or `libxess_d3d12.dll` — XeSS Super Resolution.
    pub xess: bool,
    /// `libxess_fg.dll` — XeSS 2 Frame Generation.
    pub xess_frame_generation: bool,
}

const DLSS_SR_NAMES: [&str; 1] = ["nvngx_dlss.dll"];
const DLSS_FG_NAMES: [&str; 1] = ["nvngx_dlssg.dll"];
const STREAMLINE_NAMES: [&str; 1] = ["sl.interposer.dll"];
const FSR_UPSCALE_NAMES: [&str; 3] = [
    "ffx_fsr3upscaler_x64.dll",
    "ffx_fsr2_api_x64.dll",
    "ffx_fsr_api_x64.dll",
];
const FSR_FG_NAMES: [&str; 1] = ["ffx_framegeneration_x64.dll"];
const FSR_OPTICAL_FLOW_NAMES: [&str; 1] = ["ffx_opticalflow_x64.dll"];
const XESS_NAMES: [&str; 2] = ["libxess.dll", "libxess_d3d12.dll"];
const XESS_FG_NAMES: [&str; 1] = ["libxess_fg.dll"];

fn probe_any(dir: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| dir.join(name).is_file())
}

/// Probe the SDK folders in priority order: settings, then
/// `PW_UPSCALER_SDK_DIR`, then `<exe>\sdk\`. Never loads a DLL.
pub fn probe_sdk(settings_dir: &str) -> SdkReport {
    let trimmed = settings_dir.trim();
    let exe_sdk = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join("sdk")));
    let (dir, source) = if !trimmed.is_empty() {
        (Some(PathBuf::from(trimmed)), SdkSource::Settings)
    } else if let Some(env) = std::env::var_os("PW_UPSCALER_SDK_DIR") {
        (Some(PathBuf::from(env)), SdkSource::Env)
    } else if let Some(detected) = exe_sdk.filter(|dir| dir.is_dir()) {
        (Some(detected), SdkSource::ExeFolder)
    } else {
        (None, SdkSource::Unset)
    };
    let dir_is_dir = dir.as_ref().is_some_and(|dir| dir.is_dir());
    // Borrow the path, not the Option, so `dir` can move into the report;
    // the probe closures must be finished before the struct is built.
    let dir_path = dir.as_deref();
    let probe = |names: &[&str]| dir_path.map(|dir| probe_any(dir, names)).unwrap_or(false);
    let dlss = probe(&DLSS_SR_NAMES);
    let dlss_frame_generation = probe(&DLSS_FG_NAMES);
    let streamline = probe(&STREAMLINE_NAMES);
    let fsr_upscale = probe(&FSR_UPSCALE_NAMES);
    let fsr_frame_generation = probe(&FSR_FG_NAMES);
    let fsr_optical_flow = probe(&FSR_OPTICAL_FLOW_NAMES);
    let xess = probe(&XESS_NAMES);
    let xess_frame_generation = probe(&XESS_FG_NAMES);
    SdkReport {
        dir,
        source,
        dir_is_dir,
        dlss,
        dlss_frame_generation,
        streamline,
        fsr_upscale,
        fsr_frame_generation,
        fsr_optical_flow,
        xess,
        xess_frame_generation,
    }
}

impl SdkReport {
    /// One-line human summary for the UI and the diagnostics.
    pub fn summary(&self) -> String {
        let mut found: Vec<&str> = Vec::new();
        if self.dlss {
            found.push("DLSS SR");
        }
        if self.dlss_frame_generation {
            found.push("DLSS FG");
        }
        if self.streamline {
            found.push("Streamline");
        }
        if self.fsr_upscale {
            found.push("FSR upscaler");
        }
        if self.fsr_frame_generation {
            found.push("FSR FG");
        }
        if self.xess {
            found.push("XeSS");
        }
        if self.xess_frame_generation {
            found.push("XeSS FG");
        }
        match &self.dir {
            Some(dir) if !self.dir_is_dir => format!(
                "SDK folder {} (from {}) does not exist",
                dir.display(),
                self.source.label()
            ),
            Some(dir) => format!(
                "SDK folder {} (from {}): {}",
                dir.display(),
                self.source.label(),
                if found.is_empty() {
                    "no recognised DLLs".to_string()
                } else {
                    found.join(", ")
                }
            ),
            None => {
                "no SDK folder configured (set PW_UPSCALER_SDK_DIR or upscale_sdk_dir)".to_string()
            }
        }
    }
}

/// The upscaler chosen in Settings → Graphics → "Upscaling". Persisted with
/// [`UpscaleMethod::token`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpscaleMethod {
    #[default]
    Off,
    Dlss,
    Fsr,
    Xess,
}

impl UpscaleMethod {
    pub const ALL: [Self; 4] = [Self::Off, Self::Dlss, Self::Fsr, Self::Xess];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Dlss => "DLSS Super Resolution",
            Self::Fsr => "FSR",
            Self::Xess => "XeSS",
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Dlss => "dlss",
            Self::Fsr => "fsr",
            Self::Xess => "xess",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "dlss" => Some(Self::Dlss),
            "fsr" => Some(Self::Fsr),
            "xess" => Some(Self::Xess),
            _ => None,
        }
    }
}

/// The frame generator chosen in Settings → Graphics → "Frame generation".
/// Independent of the upscaler; see [`pairing`] for the legal combinations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameGenMethod {
    #[default]
    Off,
    Dlss,
    Fsr,
    Xess,
}

impl FrameGenMethod {
    pub const ALL: [Self; 4] = [Self::Off, Self::Dlss, Self::Fsr, Self::Xess];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Dlss => "DLSS Frame Generation",
            Self::Fsr => "FSR Frame Generation",
            Self::Xess => "XeSS Frame Generation",
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Dlss => "dlss",
            Self::Fsr => "fsr",
            Self::Xess => "xess",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "off" => Some(Self::Off),
            "dlss" => Some(Self::Dlss),
            "fsr" => Some(Self::Fsr),
            "xess" => Some(Self::Xess),
            _ => None,
        }
    }
}

/// Quality mode for the upscaler. "UltraQuality" maps to DLAA on DLSS
/// (native resolution with temporal accumulation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpscaleQuality {
    UltraQuality,
    #[default]
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl UpscaleQuality {
    pub const ALL: [Self; 5] = [
        Self::UltraQuality,
        Self::Quality,
        Self::Balanced,
        Self::Performance,
        Self::UltraPerformance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::UltraQuality => "Ultra Quality (DLAA on DLSS)",
            Self::Quality => "Quality",
            Self::Balanced => "Balanced",
            Self::Performance => "Performance",
            Self::UltraPerformance => "Ultra Performance",
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::UltraQuality => "ultraquality",
            Self::Quality => "quality",
            Self::Balanced => "balanced",
            Self::Performance => "performance",
            Self::UltraPerformance => "ultraperformance",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "ultraquality" | "ultra_quality" => Some(Self::UltraQuality),
            "quality" => Some(Self::Quality),
            "balanced" => Some(Self::Balanced),
            "performance" => Some(Self::Performance),
            "ultraperformance" | "ultra_performance" => Some(Self::UltraPerformance),
            _ => None,
        }
    }
}

/// Which quality modes each provider documents. XeSS (1.x/2.x) has no Ultra
/// Performance mode; DLSS maps Ultra Quality onto DLAA.
pub fn quality_available(method: UpscaleMethod, quality: UpscaleQuality) -> bool {
    match method {
        UpscaleMethod::Off | UpscaleMethod::Dlss | UpscaleMethod::Fsr => true,
        UpscaleMethod::Xess => quality != UpscaleQuality::UltraPerformance,
    }
}

/// Linear render scale relative to the output resolution, per provider and
/// quality mode. Sources: NVIDIA DLSS (DLAA native, Quality 1.5×, Balanced
/// 1.7×, Performance 2×, Ultra Performance 3×), AMD FSR 2/3 (Ultra Quality
/// 1.3×, Quality 1.5×, Balanced 1.7×, Performance 2×, Ultra Performance 3×),
/// Intel XeSS (Ultra Quality 1.3×, Quality 1.5×, Balanced 1.7×, Performance 2×).
/// Verify against the SDK you obtain; these are the documented defaults.
pub fn internal_factor(method: UpscaleMethod, quality: UpscaleQuality) -> f64 {
    match method {
        UpscaleMethod::Off => 1.0,
        UpscaleMethod::Dlss => match quality {
            UpscaleQuality::UltraQuality => 1.0,
            UpscaleQuality::Quality => 1.0 / 1.5,
            UpscaleQuality::Balanced => 1.0 / 1.7,
            UpscaleQuality::Performance => 0.5,
            UpscaleQuality::UltraPerformance => 1.0 / 3.0,
        },
        UpscaleMethod::Fsr => match quality {
            UpscaleQuality::UltraQuality => 1.0 / 1.3,
            UpscaleQuality::Quality => 1.0 / 1.5,
            UpscaleQuality::Balanced => 1.0 / 1.7,
            UpscaleQuality::Performance => 0.5,
            UpscaleQuality::UltraPerformance => 1.0 / 3.0,
        },
        UpscaleMethod::Xess => match quality {
            UpscaleQuality::UltraQuality => 1.0 / 1.3,
            UpscaleQuality::Quality => 1.0 / 1.5,
            UpscaleQuality::Balanced => 1.0 / 1.7,
            UpscaleQuality::Performance => 0.5,
            UpscaleQuality::UltraPerformance => 0.5,
        },
    }
}

/// The support verdict for one provider on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Support {
    Available(&'static str),
    Disabled(String),
}

impl Support {
    pub fn is_available(&self) -> bool {
        matches!(self, Self::Available(_))
    }

    /// The disabled reason, if any — what the UI shows next to the option.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Available(_) => None,
            Self::Disabled(reason) => Some(reason),
        }
    }
}

fn sdk_reason(sdk: &SdkReport, what: &str) -> String {
    match (&sdk.dir, sdk.dir_is_dir) {
        (Some(dir), false) => format!(
            "SDK folder {} (from {}) does not exist",
            dir.display(),
            sdk.source.label()
        ),
        (Some(dir), true) => format!(
            "{what} not found in {} (from {})",
            dir.display(),
            sdk.source.label()
        ),
        (None, _) => format!(
            "{what} not found; no SDK folder configured (set PW_UPSCALER_SDK_DIR)"
        ),
    }
}

/// Hardware + SDK gate for one upscaler. `Off` is always available.
pub fn support(method: UpscaleMethod, gpu: &GpuFacts, sdk: &SdkReport) -> Support {
    match method {
        UpscaleMethod::Off => Support::Available("always available"),
        UpscaleMethod::Dlss => {
            if gpu.cpu_fallback {
                Support::Disabled("Requires an NVIDIA RTX GPU (software adapter detected)".into())
            } else if !gpu.is_nvidia() {
                Support::Disabled(format!("Requires an NVIDIA RTX GPU (detected: {})", gpu.name))
            } else if !sdk.dlss {
                Support::Disabled(sdk_reason(sdk, "DLSS Super Resolution SDK (nvngx_dlss.dll)"))
            } else {
                Support::Available("DLSS SDK found")
            }
        }
        UpscaleMethod::Fsr => {
            if gpu.cpu_fallback {
                Support::Disabled("Requires a real GPU (software adapter detected)".into())
            } else if !sdk.fsr_upscale {
                Support::Disabled(sdk_reason(
                    sdk,
                    "FSR SDK (ffx_fsr3upscaler_x64.dll or ffx_fsr2_api_x64.dll)",
                ))
            } else {
                Support::Available("FSR SDK found (works on AMD, NVIDIA and Intel GPUs)")
            }
        }
        UpscaleMethod::Xess => {
            if gpu.cpu_fallback {
                Support::Disabled(
                    "Requires an Intel Arc or DP4a-capable GPU (software adapter detected)".into(),
                )
            } else if !sdk.xess {
                Support::Disabled(sdk_reason(sdk, "XeSS SDK (libxess.dll)"))
            } else if gpu.is_intel() {
                Support::Available("XeSS SDK found (Intel Arc)")
            } else {
                Support::Available("XeSS SDK found (DP4a fallback path; best results on Intel Arc)")
            }
        }
    }
}

/// Hardware + SDK gate for one frame generator. `Off` is always available.
/// Frame generation is independent of the upscaler; see [`pairing`].
pub fn support_framegen(
    method: FrameGenMethod,
    upscale: UpscaleMethod,
    gpu: &GpuFacts,
    sdk: &SdkReport,
) -> Support {
    match method {
        FrameGenMethod::Off => Support::Available("always available"),
        FrameGenMethod::Dlss => {
            if gpu.cpu_fallback {
                Support::Disabled(
                    "Requires an NVIDIA RTX 40 series GPU (software adapter detected)".into(),
                )
            } else if !gpu.is_nvidia() {
                Support::Disabled(format!(
                    "Requires an NVIDIA RTX 40 series or later GPU (detected: {})",
                    gpu.name
                ))
            } else if !GpuFacts::supports_dlss_frame_generation(&gpu.name) {
                Support::Disabled(format!(
                    "Requires NVIDIA RTX 40 series or later (detected: {})",
                    gpu.name
                ))
            } else if !sdk.dlss_frame_generation {
                Support::Disabled(sdk_reason(sdk, "DLSS Frame Generation SDK (nvngx_dlssg.dll)"))
            } else {
                Support::Available("DLSS Frame Generation SDK found")
            }
        }
        FrameGenMethod::Fsr => {
            if gpu.cpu_fallback {
                Support::Disabled("Requires a real GPU (software adapter detected)".into())
            } else if !sdk.fsr_frame_generation {
                Support::Disabled(sdk_reason(
                    sdk,
                    "FSR Frame Generation SDK (ffx_framegeneration_x64.dll)",
                ))
            } else if !sdk.fsr_optical_flow {
                Support::Disabled(
                    "FSR 3 frame generation needs ffx_opticalflow_x64.dll next to the SDK DLLs"
                        .into(),
                )
            } else {
                let _ = upscale;
                Support::Available("FSR Frame Generation SDK found (upscaler-agnostic per AMD)")
            }
        }
        FrameGenMethod::Xess => {
            if gpu.cpu_fallback {
                Support::Disabled("Requires an Intel Arc GPU (software adapter detected)".into())
            } else if matches!(
                pairing(upscale, FrameGenMethod::Xess),
                PairingVerdict::Unsupported(_)
            ) {
                Support::Disabled(
                    "XeSS Frame Generation is exposed with XeSS upscaling or Off only".into(),
                )
            } else if !sdk.xess_frame_generation {
                Support::Disabled(sdk_reason(sdk, "XeSS 2 Frame Generation SDK (libxess_fg.dll)"))
            } else {
                Support::Available("XeSS 2 Frame Generation SDK found")
            }
        }
    }
}

/// Verdict on combining an upscaler with a frame generator. Based on the
/// vendors' documentation as of the knowledge available when this was written;
/// **not verified against the SDKs**, which cannot be downloaded here. Unverified
/// or undocumented combinations are rejected rather than faked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PairingVerdict {
    Supported,
    Unsupported(&'static str),
}

impl PairingVerdict {
    pub fn is_supported(self) -> bool {
        matches!(self, Self::Supported)
    }
}

/// The pairing table. DLSS Frame Generation (Streamline `sl.dlss_g`) and FSR 3
/// Frame Generation are both documented by their vendors as usable with other
/// temporal upscalers; XeSS Frame Generation is only documented for use with
/// XeSS itself. Cross-vendor XeSS pairings are therefore rejected.
pub fn pairing(upscale: UpscaleMethod, frame_generation: FrameGenMethod) -> PairingVerdict {
    match frame_generation {
        FrameGenMethod::Off => PairingVerdict::Supported,
        FrameGenMethod::Dlss => PairingVerdict::Supported,
        FrameGenMethod::Fsr => PairingVerdict::Supported,
        FrameGenMethod::Xess => match upscale {
            UpscaleMethod::Off | UpscaleMethod::Xess => PairingVerdict::Supported,
            UpscaleMethod::Dlss | UpscaleMethod::Fsr => PairingVerdict::Unsupported(
                "Intel documents XeSS Frame Generation for use with XeSS only",
            ),
        },
    }
}

/// Coerce a persisted triple into a legal one, dropping only values this
/// build cannot honour. Used at settings-decode time; never rewrites what the
/// user sees beyond these rules.
pub fn sanitise(
    upscale: UpscaleMethod,
    quality: UpscaleQuality,
    frame_generation: FrameGenMethod,
) -> (UpscaleMethod, UpscaleQuality, FrameGenMethod) {
    let quality = if quality_available(upscale, quality) {
        quality
    } else {
        UpscaleQuality::Quality
    };
    let frame_generation = if pairing(upscale, frame_generation).is_supported() {
        frame_generation
    } else {
        FrameGenMethod::Off
    };
    (upscale, quality, frame_generation)
}

/// The provider the settings ask for, if hardware and SDK allow it.
pub fn resolve_active(
    upscale: UpscaleMethod,
    gpu: &GpuFacts,
    sdk: &SdkReport,
) -> Option<UpscaleMethod> {
    (upscale != UpscaleMethod::Off && support(upscale, gpu, sdk).is_available()).then_some(upscale)
}

/// Recommended internal render extent for `(w, h)` output. Pure; used by the
/// frame loop and by the diagnostics.
pub fn provider_extent(
    upscale: UpscaleMethod,
    quality: UpscaleQuality,
    w: u32,
    h: u32,
    max_dimension: u32,
) -> (u32, u32) {
    let factor = internal_factor(upscale, quality);
    let limit = u64::from(max_dimension.max(1));
    let scale = |v: u32| ((f64::from(v.max(1)) * factor).round() as u64).clamp(1, limit) as u32;
    (scale(w), scale(h))
}

/// The extent the world should render at. `off_extent` is the extent the
/// resolution-scale slider resolves to (computed by the caller with the
/// existing `render_scale::scaled_extent`, which this module does not depend
/// on so it stays headless-testable).
pub fn render_extent(
    off_extent: (u32, u32),
    output: (u32, u32),
    max_dimension: u32,
    upscale: UpscaleMethod,
    quality: UpscaleQuality,
    gpu: &GpuFacts,
    sdk: &SdkReport,
) -> (u32, u32) {
    match resolve_active(upscale, gpu, sdk) {
        Some(active) => provider_extent(active, quality, output.0, output.1, max_dimension),
        None => off_extent,
    }
}

/// The frame generator the settings ask for, if hardware, SDK and pairing
/// allow it.
pub fn resolve_active_framegen(
    upscale: UpscaleMethod,
    frame_generation: FrameGenMethod,
    gpu: &GpuFacts,
    sdk: &SdkReport,
) -> Option<FrameGenMethod> {
    let paired = pairing(upscale, frame_generation).is_supported();
    (frame_generation != FrameGenMethod::Off
        && paired
        && support_framegen(frame_generation, upscale, gpu, sdk).is_available())
    .then_some(frame_generation)
}

/// Opaque handle to a GPU resource, as far as a provider is concerned. The
/// renderer assigns these; the provider never dereferences them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextureHandle(pub u64);

/// Everything a temporal upscaler needs for one frame. Exposure is the
/// average scene exposure as `2^exp`-ish linear multiplier; 1.0 is neutral
/// (the viewer has no exposure control yet — fixed neutral exposure is a
/// legal DLSS input).
#[derive(Clone, Copy, Debug)]
pub struct ResolveInputs {
    pub input_color: TextureHandle,
    pub depth: TextureHandle,
    pub motion_vectors: TextureHandle,
    pub output: TextureHandle,
    pub exposure: f32,
    pub jitter: (f32, f32),
}

/// Errors a provider can return.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpscaleError {
    /// The hardware/SDK gate rejected the provider; carries the visible reason.
    Disabled(String),
    /// The SDK is present but this build has no verified FFI for it. See
    /// `docs/blockers.md` (B-3/B-4).
    SdkNotIntegrated,
    /// Inputs failed the provider's preconditions (extents, handles, exposure).
    BadConfiguration(&'static str),
}

impl std::fmt::Display for UpscaleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled(reason) => write!(f, "{reason}"),
            Self::SdkNotIntegrated => write!(
                f,
                "the SDK is present but this build carries no verified integration for it"
            ),
            Self::BadConfiguration(what) => write!(f, "bad provider inputs: {what}"),
        }
    }
}

impl std::error::Error for UpscaleError {}

/// The contract every upscaler implements. `resolve` takes the previous
/// frame's jitter and motion vectors; the provider owns its history buffers.
pub trait Upscaler {
    fn init(&mut self) -> Result<(), UpscaleError>;
    fn query_support(&self) -> Support;
    fn resolve(&mut self, inputs: &ResolveInputs) -> Result<(), UpscaleError>;
    fn shutdown(&mut self);
}

/// Passthrough — the "Off" provider. Copies nothing; the renderer's existing
/// bilinear path does the work. Always available.
#[derive(Default)]
pub struct Passthrough {
    initialised: bool,
}

impl Upscaler for Passthrough {
    fn init(&mut self) -> Result<(), UpscaleError> {
        self.initialised = true;
        Ok(())
    }

    fn query_support(&self) -> Support {
        Support::Available("always available")
    }

    fn resolve(&mut self, inputs: &ResolveInputs) -> Result<(), UpscaleError> {
        if !self.initialised {
            return Err(UpscaleError::BadConfiguration("resolve before init"));
        }
        if inputs.input_color.0 == 0 || inputs.output.0 == 0 {
            return Err(UpscaleError::BadConfiguration("missing colour target"));
        }
        Ok(())
    }

    fn shutdown(&mut self) {
        self.initialised = false;
    }
}

macro_rules! vendor_provider {
    ($name:ident, $method:expr) => {
        /// Vendor provider. `init` reports the real gate instead of faking an
        /// integration: SDKs are absent from this environment, and no DLL is
        /// loaded by this build.
        pub struct $name {
            gpu: GpuFacts,
            sdk: SdkReport,
        }

        impl $name {
            pub fn new(gpu: GpuFacts, sdk: SdkReport) -> Self {
                Self { gpu, sdk }
            }
        }

        impl Upscaler for $name {
            fn init(&mut self) -> Result<(), UpscaleError> {
                match support($method, &self.gpu, &self.sdk) {
                    Support::Available(_) => Err(UpscaleError::SdkNotIntegrated),
                    Support::Disabled(reason) => Err(UpscaleError::Disabled(reason)),
                }
            }

            fn query_support(&self) -> Support {
                support($method, &self.gpu, &self.sdk)
            }

            fn resolve(&mut self, inputs: &ResolveInputs) -> Result<(), UpscaleError> {
                if inputs.input_color.0 == 0 || inputs.output.0 == 0 {
                    return Err(UpscaleError::BadConfiguration("missing colour target"));
                }
                Err(UpscaleError::SdkNotIntegrated)
            }

            fn shutdown(&mut self) {}
        }
    };
}

vendor_provider!(NvidiaDlss, UpscaleMethod::Dlss);
vendor_provider!(AmdFsr, UpscaleMethod::Fsr);
vendor_provider!(IntelXess, UpscaleMethod::Xess);

/// Radical inverse for Halton (2,3) jitter. `index` starts at 1 for frame 0.
pub fn halton(index: u32, base: u32) -> f32 {
    let mut f = 1.0f64;
    let mut r = 0.0f64;
    let mut i = index.max(1);
    while i > 0 {
        f /= f64::from(base);
        r += f * f64::from(i % base);
        i /= base;
    }
    r as f32
}

/// Length of the temporal jitter sequence each provider is configured with.
/// DLSS uses a fixed 64-frame Halton-like sequence; FSR 2/3 and XeSS both
/// document Halton(2,3) with period 8/16 — 64 is a superset that satisfies
/// all of them.
pub const JITTER_PERIOD: u32 = 64;

/// Subpixel jitter for `frame_index`, in pixel units centred on zero
/// (i.e. add it to the pixel-centre offset, or scale by `1/extent` for clip
/// space). Deterministic; identical for the same frame index.
pub fn jitter_offset(frame_index: u32) -> (f32, f32) {
    let index = (frame_index % JITTER_PERIOD) + 1;
    (halton(index, 2) - 0.5, halton(index, 3) - 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nvidia_rtx4070() -> GpuFacts {
        GpuFacts::from_parts(VENDOR_NVIDIA, 0x2784, "NVIDIA GeForce RTX 4070", "dx12", false)
    }

    fn nvidia_gtx1080() -> GpuFacts {
        GpuFacts::from_parts(VENDOR_NVIDIA, 0x1b80, "NVIDIA GeForce GTX 1080", "dx12", false)
    }

    fn amd_rx9070() -> GpuFacts {
        GpuFacts::from_parts(VENDOR_AMD, 0x7550, "AMD Radeon RX 9070 XT", "dx12", false)
    }

    fn intel_arc_a770() -> GpuFacts {
        GpuFacts::from_parts(VENDOR_INTEL, 0x56a0, "Intel Arc A770 Graphics", "dx12", false)
    }

    fn software() -> GpuFacts {
        GpuFacts::from_parts(0xffff, 0, "llvmpipe (LLVM 15.0.7)", "vulkan", true)
    }

    fn no_sdk() -> SdkReport {
        SdkReport::default()
    }

    fn full_sdk() -> SdkReport {
        SdkReport {
            dir: Some(PathBuf::from("C:/sdks/vendor")),
            source: SdkSource::Settings,
            dir_is_dir: true,
            dlss: true,
            dlss_frame_generation: true,
            streamline: true,
            fsr_upscale: true,
            fsr_frame_generation: true,
            fsr_optical_flow: true,
            xess: true,
            xess_frame_generation: true,
        }
    }

    #[test]
    fn tokens_round_trip() {
        for method in UpscaleMethod::ALL {
            assert_eq!(UpscaleMethod::parse(method.token()), Some(method));
        }
        for method in FrameGenMethod::ALL {
            assert_eq!(FrameGenMethod::parse(method.token()), Some(method));
        }
        for quality in UpscaleQuality::ALL {
            assert_eq!(UpscaleQuality::parse(quality.token()), Some(quality));
        }
        assert_eq!(UpscaleMethod::parse("bogus"), None);
        assert_eq!(FrameGenMethod::parse(""), None);
        assert_eq!(UpscaleQuality::parse("ultra_quality"), Some(UpscaleQuality::UltraQuality));
        assert_eq!(
            UpscaleQuality::parse("ULTRAPERFORMANCE"),
            Some(UpscaleQuality::UltraPerformance)
        );
    }

    #[test]
    fn off_is_always_available_everywhere() {
        for gpu in [nvidia_rtx4070(), amd_rx9070(), intel_arc_a770(), software()] {
            assert!(support(UpscaleMethod::Off, &gpu, &no_sdk()).is_available());
            let fg = support_framegen(FrameGenMethod::Off, UpscaleMethod::Off, &gpu, &no_sdk());
            assert!(fg.is_available());
        }
    }

    #[test]
    fn dlss_requires_nvidia_and_sdk() {
        assert_eq!(
            support(UpscaleMethod::Dlss, &amd_rx9070(), &full_sdk()).reason().unwrap(),
            "Requires an NVIDIA RTX GPU (detected: AMD Radeon RX 9070 XT)"
        );
        assert_eq!(
            support(UpscaleMethod::Dlss, &software(), &full_sdk()).reason().unwrap(),
            "Requires an NVIDIA RTX GPU (software adapter detected)"
        );
        assert!(support(UpscaleMethod::Dlss, &nvidia_rtx4070(), &no_sdk())
            .reason()
            .unwrap()
            .contains("nvngx_dlss.dll"));
        assert!(support(UpscaleMethod::Dlss, &nvidia_rtx4070(), &full_sdk()).is_available());
    }

    #[test]
    fn fsr_works_cross_vendor_but_needs_sdk() {
        for gpu in [nvidia_rtx4070(), amd_rx9070(), intel_arc_a770()] {
            assert!(support(UpscaleMethod::Fsr, &gpu, &no_sdk())
                .reason()
                .unwrap()
                .contains("FSR SDK"));
            assert!(support(UpscaleMethod::Fsr, &gpu, &full_sdk()).is_available());
        }
        assert!(support(UpscaleMethod::Fsr, &software(), &full_sdk())
            .reason()
            .unwrap()
            .contains("software adapter"));
    }

    #[test]
    fn xess_needs_sdk_available_on_intel_and_elsewhere() {
        assert!(support(UpscaleMethod::Xess, &intel_arc_a770(), &no_sdk())
            .reason()
            .unwrap()
            .contains("XeSS SDK"));
        assert!(support(UpscaleMethod::Xess, &intel_arc_a770(), &full_sdk()).is_available());
        assert!(support(UpscaleMethod::Xess, &nvidia_rtx4070(), &full_sdk()).is_available());
    }

    #[test]
    fn dlss_frame_generation_requires_rtx_40_series() {
        let support = support_framegen(
            FrameGenMethod::Dlss,
            UpscaleMethod::Off,
            &nvidia_gtx1080(),
            &full_sdk(),
        );
        assert!(support.reason().unwrap().contains("40 series"));
        let support =
            support_framegen(FrameGenMethod::Dlss, UpscaleMethod::Dlss, &amd_rx9070(), &full_sdk());
        assert!(support.reason().unwrap().contains("NVIDIA"));
        let support = support_framegen(
            FrameGenMethod::Dlss,
            UpscaleMethod::Dlss,
            &nvidia_rtx4070(),
            &no_sdk(),
        );
        assert!(support.reason().unwrap().contains("nvngx_dlssg.dll"));
        assert!(support_framegen(
            FrameGenMethod::Dlss,
            UpscaleMethod::Dlss,
            &nvidia_rtx4070(),
            &full_sdk()
        )
        .is_available());
    }

    #[test]
    fn fsr_frame_generation_needs_fg_and_optical_flow() {
        let mut sdk = no_sdk();
        sdk.fsr_frame_generation = true;
        assert!(support_framegen(FrameGenMethod::Fsr, UpscaleMethod::Fsr, &amd_rx9070(), &sdk)
            .reason()
            .unwrap()
            .contains("ffx_opticalflow_x64.dll"));
        sdk.fsr_optical_flow = true;
        assert!(support_framegen(FrameGenMethod::Fsr, UpscaleMethod::Dlss, &amd_rx9070(), &sdk)
            .is_available());
    }

    #[test]
    fn xess_frame_generation_pairing_gate() {
        let support = support_framegen(
            FrameGenMethod::Xess,
            UpscaleMethod::Dlss,
            &intel_arc_a770(),
            &full_sdk(),
        );
        assert!(support.reason().unwrap().contains("XeSS upscaling"));
        assert!(support_framegen(
            FrameGenMethod::Xess,
            UpscaleMethod::Xess,
            &intel_arc_a770(),
            &full_sdk()
        )
        .is_available());
    }

    #[test]
    fn pairing_table_legal_and_illegal() {
        for up in UpscaleMethod::ALL {
            assert!(pairing(up, FrameGenMethod::Off).is_supported());
            assert!(pairing(up, FrameGenMethod::Dlss).is_supported());
            assert!(pairing(up, FrameGenMethod::Fsr).is_supported());
        }
        assert!(pairing(UpscaleMethod::Xess, FrameGenMethod::Xess).is_supported());
        assert!(pairing(UpscaleMethod::Off, FrameGenMethod::Xess).is_supported());
        assert!(!pairing(UpscaleMethod::Dlss, FrameGenMethod::Xess).is_supported());
        assert!(!pairing(UpscaleMethod::Fsr, FrameGenMethod::Xess).is_supported());
    }

    #[test]
    fn sanitise_fixes_illegal_combinations() {
        // XeSS has no Ultra Performance mode.
        let (_, quality, _) = sanitise(
            UpscaleMethod::Xess,
            UpscaleQuality::UltraPerformance,
            FrameGenMethod::Off,
        );
        assert_eq!(quality, UpscaleQuality::Quality);
        // Cross-vendor XeSS frame generation is not exposed.
        let (_, _, fg) = sanitise(
            UpscaleMethod::Fsr,
            UpscaleQuality::Quality,
            FrameGenMethod::Xess,
        );
        assert_eq!(fg, FrameGenMethod::Off);
        // Legal combinations survive untouched.
        let (u, q, f) = sanitise(
            UpscaleMethod::Dlss,
            UpscaleQuality::Performance,
            FrameGenMethod::Dlss,
        );
        assert_eq!(
            (u, q, f),
            (UpscaleMethod::Dlss, UpscaleQuality::Performance, FrameGenMethod::Dlss)
        );
    }

    #[test]
    fn internal_factors_shrink_the_image_monotonically() {
        for method in [UpscaleMethod::Dlss, UpscaleMethod::Fsr, UpscaleMethod::Xess] {
            let factors: Vec<f64> = UpscaleQuality::ALL
                .iter()
                .map(|quality| internal_factor(method, *quality))
                .collect();
            assert!(factors.iter().all(|factor| *factor > 0.0 && *factor <= 1.0));
            for pair in factors.windows(2) {
                assert!(pair[0] >= pair[1], "factor must not grow with lower quality");
            }
        }
        assert_eq!(internal_factor(UpscaleMethod::Off, UpscaleQuality::Quality), 1.0);
        // DLAA renders at native resolution.
        assert_eq!(internal_factor(UpscaleMethod::Dlss, UpscaleQuality::UltraQuality), 1.0);
    }

    #[test]
    fn provider_extent_scales_and_clamps() {
        assert_eq!(
            provider_extent(UpscaleMethod::Fsr, UpscaleQuality::Performance, 1920, 1080, 8192),
            (960, 540)
        );
        assert_eq!(
            provider_extent(UpscaleMethod::Dlss, UpscaleQuality::UltraQuality, 1920, 1080, 8192),
            (1920, 1080)
        );
        let (w, h) = provider_extent(UpscaleMethod::Xess, UpscaleQuality::Balanced, 1, 1, 8192);
        assert_eq!((w, h), (1, 1));
        let (w, h) =
            provider_extent(UpscaleMethod::Fsr, UpscaleQuality::UltraPerformance, 4000, 3000, 4096);
        assert!(w <= 4096 && h <= 4096);
    }

    #[test]
    fn render_extent_keeps_off_path_exact() {
        let gpu = nvidia_rtx4070();
        let sdk = no_sdk();
        // With Off (or an unavailable provider), the caller's slider extent wins.
        assert_eq!(
            render_extent(
                (960, 540),
                (1920, 1080),
                8192,
                UpscaleMethod::Off,
                UpscaleQuality::Quality,
                &gpu,
                &sdk,
            ),
            (960, 540)
        );
        assert_eq!(
            render_extent(
                (960, 540),
                (1920, 1080),
                8192,
                UpscaleMethod::Dlss,
                UpscaleQuality::Quality,
                &gpu,
                &sdk,
            ),
            (960, 540)
        );
        // An available provider overrides the slider extent.
        assert_eq!(
            render_extent(
                (1221, 717),
                (1920, 1080),
                8192,
                UpscaleMethod::Fsr,
                UpscaleQuality::Performance,
                &amd_rx9070(),
                &full_sdk(),
            ),
            (960, 540)
        );
    }

    #[test]
    fn jitter_is_deterministic_and_bounded() {
        for frame in 0..JITTER_PERIOD * 2 {
            let (x, y) = jitter_offset(frame);
            assert!(x.abs() <= 0.5 && y.abs() <= 0.5);
            assert_eq!(jitter_offset(frame), (x, y));
        }
        // The sequence actually moves (first and second frames differ).
        assert_ne!(jitter_offset(0), jitter_offset(1));
        assert_eq!(jitter_offset(0), jitter_offset(JITTER_PERIOD));
    }

    #[test]
    fn halton_matches_known_radical_inverses() {
        assert!((halton(1, 2) - 0.5).abs() < 1e-6);
        assert!((halton(2, 2) - 0.25).abs() < 1e-6);
        assert!((halton(1, 3) - 1.0 / 3.0).abs() < 1e-6);
        assert!((halton(4, 2) - 0.125).abs() < 1e-6);
    }

    #[test]
    fn probe_empty_folder_reports_nothing() {
        let dir = std::env::temp_dir().join(format!("pwe-sdk-probe-{}-empty", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Nothing in the folder: every DLL missing, folder valid.
        let sdk = probe_sdk(dir.to_str().unwrap());
        assert_eq!(sdk.dir.as_deref(), Some(dir.as_path()));
        assert_eq!(sdk.source, SdkSource::Settings);
        assert!(sdk.dir_is_dir);
        assert!(!sdk.dlss && !sdk.fsr_upscale && !sdk.xess && !sdk.streamline);
        std::fs::remove_dir_all(&dir).ok();
        // A missing folder is reported, not fatal.
        let missing =
            std::env::temp_dir().join(format!("pwe-sdk-probe-{}-missing", std::process::id()));
        let sdk = probe_sdk(missing.to_str().unwrap());
        assert!(!sdk.dir_is_dir);
        assert!(support(UpscaleMethod::Dlss, &nvidia_rtx4070(), &sdk)
            .reason()
            .unwrap()
            .contains("does not exist"));
    }

    #[test]
    fn fallback_with_no_sdks_disables_every_provider() {
        // The exact fallback the viewer relies on when no SDK is shipped.
        let sdk = SdkReport::default();
        for gpu in [nvidia_rtx4070(), amd_rx9070(), intel_arc_a770(), software()] {
            for method in UpscaleMethod::ALL {
                let expected = matches!(method, UpscaleMethod::Off);
                assert_eq!(support(method, &gpu, &sdk).is_available(), expected);
            }
            for method in FrameGenMethod::ALL {
                let expected = matches!(method, FrameGenMethod::Off);
                assert_eq!(
                    support_framegen(method, UpscaleMethod::Dlss, &gpu, &sdk).is_available(),
                    expected
                );
            }
        }
        assert!(support(UpscaleMethod::Dlss, &nvidia_rtx4070(), &sdk)
            .reason()
            .unwrap()
            .contains("PW_UPSCALER_SDK_DIR"));
    }

    #[test]
    fn providers_report_the_real_gate_instead_of_pretending() {
        let mut dlss = NvidiaDlss::new(nvidia_rtx4070(), no_sdk());
        assert!(matches!(
            dlss.query_support(),
            Support::Disabled(_)
        ));
        assert!(matches!(dlss.init(), Err(UpscaleError::Disabled(_))));
        // Even with a "complete" SDK, this build does not claim an FFI.
        let mut dlss = NvidiaDlss::new(nvidia_rtx4070(), full_sdk());
        assert!(matches!(dlss.init(), Err(UpscaleError::SdkNotIntegrated)));
        let mut passthrough = Passthrough::default();
        assert!(passthrough.init().is_ok());
        let good = ResolveInputs {
            input_color: TextureHandle(1),
            depth: TextureHandle(2),
            motion_vectors: TextureHandle(3),
            output: TextureHandle(4),
            exposure: 1.0,
            jitter: jitter_offset(0),
        };
        assert!(passthrough.resolve(&good).is_ok());
        let bad = ResolveInputs { output: TextureHandle(0), ..good };
        assert!(matches!(
            passthrough.resolve(&bad),
            Err(UpscaleError::BadConfiguration(_))
        ));
        passthrough.shutdown();
        assert!(matches!(
            passthrough.resolve(&good),
            Err(UpscaleError::BadConfiguration(_))
        ));
    }
}
