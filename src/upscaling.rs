//! Upscaling and frame-generation policy used by the Graphics tab.
//!
//! This module is deliberately dependency-free: the support matrix, the
//! quality ladder and the temporal jitter sequence are pure arithmetic, so they
//! are unit-testable without a GPU or an adapter. `gpu.rs` builds a
//! [`Hardware`] record from `wgpu::AdapterInfo`; `upscale_gpu.rs` executes the
//! portable WGSL implementation described below.
//!
//! **What this is, and what it is not.** NVIDIA NGX (DLSS / DLSS Frame
//! Generation), Intel XeSS / XeSS-FG and AMD FSR 4/4.1 ship as vendor SDKs with
//! proprietary models and driver hooks. They cannot be reproduced from source,
//! and no vendor binaries are bundled with this viewer. The modes below are
//! *in-house compatibility implementations*: one dependency-free WGSL temporal
//! upscaler and one camera-motion frame interpolator, with per-technology
//! tuning profiles and hardware gates that mirror the vendors' published
//! requirements. See `docs/Graphics-Upscaling.md`.
//!
//! **Hardware matrix (late 2026).** FSR 2/3 and FSR Frame Generation are open
//! (MIT) and cross-vendor. XeSS Super Resolution has always had a DP4a
//! fallback for non-Intel GPUs, and XeSS SDK 2.1 opened XeSS Frame Generation
//! to any GPU with Shader Model 6.4 (XeSS 3 Multi Frame Generation remains
//! Intel-only). FSR 4.1 started on RDNA 4 FP8, reached RDNA 3 and RDNA 2 via
//! an INT8 model in 2026, and its INT8 model is used on non-AMD GPUs by
//! community interop layers. DLSS Super Resolution genuinely needs RTX Tensor
//! cores; DLSS Frame Generation needs RTX 40+ optical-flow hardware, and
//! DLSS 4.5 Multi Frame Generation (up to 6x) needs RTX 50. Those are the only
//! hard vendor locks in this matrix.
//!
//! Upscaling and frame generation are **independent**: any frame generator can
//! be paired with any upscaler (DLSS SR + FSR FG, XeSS SR + DLSS-G, FSR 4.1 +
//! XeSS-FG), and frame generation also works with upscaling off at native
//! resolution. `Auto` merely picks a convenient default.

/// PCI vendor IDs used by the wgpu adapters we care about.
pub const VENDOR_NVIDIA: u32 = 0x10de;
pub const VENDOR_AMD: u32 = 0x1002;
pub const VENDOR_INTEL: u32 = 0x8086;

/// Which upscaler the user selected. `Off` keeps the legacy bilinear
/// `render_scale` path and the manual resolution-scale slider, unless frame
/// generation is enabled (which needs the temporal compositor at 100%).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Upscaler {
    #[default]
    Off,
    Fsr2,
    Fsr3,
    Fsr41,
    Dlss,
    Xess,
}

impl Upscaler {
    pub const ALL: [Upscaler; 6] = [
        Upscaler::Off,
        Upscaler::Fsr2,
        Upscaler::Fsr3,
        Upscaler::Fsr41,
        Upscaler::Dlss,
        Upscaler::Xess,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Upscaler::Off => "Off (bilinear)",
            Upscaler::Fsr2 => "FSR 2",
            Upscaler::Fsr3 => "FSR 3.1",
            Upscaler::Fsr41 => "FSR 4.1",
            Upscaler::Dlss => "DLSS Super Resolution",
            Upscaler::Xess => "XeSS Super Resolution",
        }
    }

    /// Persisted spelling. Unknown keys keep the default.
    pub fn key(self) -> &'static str {
        match self {
            Upscaler::Off => "Off",
            Upscaler::Fsr2 => "FSR2",
            Upscaler::Fsr3 => "FSR3",
            Upscaler::Fsr41 => "FSR41",
            Upscaler::Dlss => "DLSS",
            Upscaler::Xess => "XeSS",
        }
    }

    pub fn parse(text: &str) -> Option<Upscaler> {
        Upscaler::ALL.iter().copied().find(|m| m.key() == text)
    }

    /// The frame generator `Auto` selects for this upscaler. This is only a
    /// default: an explicit Graphics-tab choice always wins.
    pub fn native_frame_generation(self) -> FrameGen {
        match self {
            Upscaler::Off => FrameGen::Fsr,
            Upscaler::Dlss => FrameGen::Dlss,
            Upscaler::Xess => FrameGen::Xess,
            Upscaler::Fsr2 | Upscaler::Fsr3 | Upscaler::Fsr41 => FrameGen::Fsr,
        }
    }

    /// Parameter profile index uploaded to the WGSL temporal pass.
    pub fn profile(self) -> f32 {
        match self {
            Upscaler::Off => 0.,
            Upscaler::Fsr2 => 0.,
            Upscaler::Fsr3 => 1.,
            Upscaler::Fsr41 => 2.,
            Upscaler::Dlss => 3.,
            Upscaler::Xess => 4.,
        }
    }

    /// History weight and disocclusion tolerance for the shared temporal pass.
    /// These are tuning profiles of one portable algorithm, not vendor models.
    pub fn tuning(self) -> (f32, f32) {
        match self {
            Upscaler::Off | Upscaler::Fsr2 => (0.82, 0.020),
            Upscaler::Fsr3 => (0.87, 0.024),
            Upscaler::Fsr41 => (0.91, 0.028),
            Upscaler::Dlss => (0.90, 0.022),
            Upscaler::Xess => (0.88, 0.026),
        }
    }
}

/// Industry-standard quality ladder. The divisor is the linear upscale factor;
/// `percent` is the internal render resolution as a percentage of the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Quality {
    NativeAa,
    #[default]
    Quality,
    Balanced,
    Performance,
    UltraPerformance,
}

impl Quality {
    pub const ALL: [Quality; 5] = [
        Quality::NativeAa,
        Quality::Quality,
        Quality::Balanced,
        Quality::Performance,
        Quality::UltraPerformance,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Quality::NativeAa => "Native AA (100%)",
            Quality::Quality => "Quality (1.5x)",
            Quality::Balanced => "Balanced (1.7x)",
            Quality::Performance => "Performance (2.0x)",
            Quality::UltraPerformance => "Ultra Performance (3.0x)",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Quality::NativeAa => "NativeAA",
            Quality::Quality => "Quality",
            Quality::Balanced => "Balanced",
            Quality::Performance => "Performance",
            Quality::UltraPerformance => "UltraPerformance",
        }
    }

    pub fn parse(text: &str) -> Option<Quality> {
        Quality::ALL.iter().copied().find(|q| q.key() == text)
    }

    pub fn divisor(self) -> f32 {
        match self {
            Quality::NativeAa => 1.0,
            Quality::Quality => 1.5,
            Quality::Balanced => 1.7,
            Quality::Performance => 2.0,
            Quality::UltraPerformance => 3.0,
        }
    }

    pub fn percent(self) -> f32 {
        100.0 / self.divisor()
    }
}

/// Frame-generation technology. This is **independent** of [`Upscaler`]:
/// every combination is allowed. `Dlss` covers DLSS Frame Generation and DLSS
/// 4.5 Multi Frame Generation, `Xess` covers XeSS-FG and XeSS 3 MFG, and `Fsr`
/// covers the open FSR 3 Frame Generation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FrameGen {
    #[default]
    Off,
    Auto,
    Fsr,
    Dlss,
    Xess,
}

impl FrameGen {
    pub const ALL: [FrameGen; 5] = [
        FrameGen::Off,
        FrameGen::Auto,
        FrameGen::Fsr,
        FrameGen::Dlss,
        FrameGen::Xess,
    ];

    pub fn label(self) -> &'static str {
        match self {
            FrameGen::Off => "Off",
            FrameGen::Auto => "Auto (match upscaler)",
            FrameGen::Fsr => "FSR 3 Frame Generation",
            FrameGen::Dlss => "DLSS Frame Generation / MFG",
            FrameGen::Xess => "XeSS Frame Generation / MFG",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            FrameGen::Off => "Off",
            FrameGen::Auto => "Auto",
            FrameGen::Fsr => "FSR",
            FrameGen::Dlss => "DLSS",
            FrameGen::Xess => "XeSS",
        }
    }

    pub fn parse(text: &str) -> Option<FrameGen> {
        FrameGen::ALL.iter().copied().find(|f| f.key() == text)
    }

    /// Short name used in status lines.
    pub fn short(self) -> &'static str {
        match self {
            FrameGen::Off => "off",
            FrameGen::Auto => "auto",
            FrameGen::Fsr => "FSR FG",
            FrameGen::Dlss => "DLSS-G",
            FrameGen::Xess => "XeSS-FG",
        }
    }
}

/// Which code path a technology runs on. `Accelerated` is the vendor's
/// hardware-accelerated profile, `Compatible` is the documented cross-vendor
/// fallback (DP4a / INT8 / portable WGSL), `Fallback` means the requested
/// technology was replaced, and `Disabled` means it is off.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Path {
    Accelerated,
    Compatible,
    Fallback,
    Disabled,
}

impl Path {
    pub fn label(self) -> &'static str {
        match self {
            Path::Accelerated => "Accelerated path",
            Path::Compatible => "Compatibility path",
            Path::Fallback => "Fallback",
            Path::Disabled => "Off",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceClass {
    Cpu,
    Virtual,
    Integrated,
    Discrete,
    Other,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Backend {
    Vulkan,
    Dx12,
    Metal,
    Gl,
    BrowserWebGpu,
    Other,
}

/// Adapter facts needed by the support matrix. Built in `gpu.rs`; kept free of
/// wgpu types so this module can be tested with plain `rustc --test`.
///
/// `shader_model_64` approximates "Shader Model 6.4 / DP4a capable", which is
/// the requirement Intel publishes for XeSS on non-Intel GPUs and the INT8
/// FSR 4.1 model uses. wgpu does not expose SM 6.4 directly, so `gpu.rs` sets
/// it from the backend and adapter class; see `docs/Graphics-Upscaling.md`.
#[derive(Clone, Debug)]
pub struct Hardware {
    pub vendor_id: u32,
    pub device_id: u32,
    pub name: String,
    pub device_class: DeviceClass,
    pub backend: Backend,
    pub shader_f16: bool,
    pub shader_model_64: bool,
}

impl Hardware {
    pub fn new(
        vendor_id: u32,
        device_id: u32,
        name: String,
        device_class: DeviceClass,
        backend: Backend,
        shader_f16: bool,
        shader_model_64: bool,
    ) -> Self {
        Self {
            vendor_id,
            device_id,
            name,
            device_class,
            backend,
            shader_f16,
            shader_model_64,
        }
    }

    pub fn vendor(&self) -> Vendor {
        match self.vendor_id & 0xffff {
            VENDOR_NVIDIA => Vendor::Nvidia,
            VENDOR_AMD => Vendor::Amd,
            VENDOR_INTEL => Vendor::Intel,
            _ => Vendor::Other,
        }
    }

    /// A software rasteriser (lavapipe, SwiftShader, WARP) cannot run vendor
    /// features and is never counted as "supported hardware".
    pub fn is_software(&self) -> bool {
        matches!(self.device_class, DeviceClass::Cpu | DeviceClass::Virtual)
            || self.backend == Backend::Other
    }

    fn lower_name(&self) -> String {
        self.name.to_ascii_lowercase()
    }

    fn has_any(&self, needles: &[&str]) -> bool {
        let n = self.lower_name();
        needles.iter().any(|p| n.contains(p))
    }

    /// Turing-or-newer RTX parts carry the Tensor cores DLSS SR needs. GTX
    /// 16xx is Turing without Tensor cores and is deliberately excluded.
    pub fn nvidia_rtx(&self) -> bool {
        if self.vendor() != Vendor::Nvidia || self.is_software() {
            return false;
        }
        self.has_any(&["rtx", "quadro rtx", "a100", "a40", "a6000", "h100"])
            && !self.has_any(&["gtx"])
    }

    /// Ada (RTX 40) and Blackwell (RTX 50) carry the optical-flow accelerator
    /// DLSS Frame Generation requires.
    pub fn nvidia_ada_or_newer(&self) -> bool {
        if !self.nvidia_rtx() {
            return false;
        }
        self.has_any(&[
            "rtx 40",
            "rtx 50",
            "rtx 60",
            "ada",
            "blackwell",
            "l40",
            "l4 ",
            "gb20",
            "ad10",
        ]) || matches!(self.device_id & 0xffff, 0x2684 | 0x2704 | 0x2b81 | 0x2c02)
    }

    /// DLSS 4.5 Dynamic/6x Multi Frame Generation is Blackwell-only.
    pub fn nvidia_blackwell(&self) -> bool {
        self.nvidia_rtx()
            && (self.has_any(&["rtx 50", "rtx 60", "blackwell", "gb20"])
                || matches!(self.device_id & 0xffff, 0x2b81 | 0x2c02))
    }

    /// FSR 4 FP8 model: RDNA 4 / Radeon RX 9000.
    pub fn amd_rdna4(&self) -> bool {
        self.vendor() == Vendor::Amd
            && !self.is_software()
            && (self.has_any(&[
                "rx 9",
                "radeon rx 9",
                "gfx12",
                "rdna 4",
                "rdna4",
                "r9700",
                "ai pro r9700",
            ]) || matches!(self.device_id & 0xffff, 0x7550 | 0x7551 | 0x7580 | 0x7581))
    }

    /// FSR 4.1 INT8 model: RDNA 3 / RX 7000 and RDNA 2 / RX 6000 (officially
    /// supported in 2026), plus AMD APUs.
    pub fn amd_rdna2_or_3(&self) -> bool {
        self.vendor() == Vendor::Amd
            && !self.is_software()
            && self.has_any(&[
                "rx 6",
                "rx 7",
                "radeon rx 6",
                "radeon rx 7",
                "gfx11",
                "gfx10",
                "rdna 2",
                "rdna 3",
                "rdna2",
                "rdna3",
                "radeon 780m",
                "radeon 890m",
                "ryzen ai",
            ])
    }

    /// Intel Xe with XMX (Arc Alchemist / Battlemage, Core Ultra / Panther
    /// Lake). XeSS 3 Multi Frame Generation is limited to these parts.
    pub fn intel_xmx(&self) -> bool {
        self.vendor() == Vendor::Intel
            && !self.is_software()
            && self.has_any(&[
                "arc",
                "battlemage",
                "alchemist",
                "xe2",
                "xe3",
                "b580",
                "a770",
                "core ultra",
                "panther lake",
                "lunar lake",
            ])
    }

    /// XeSS SR (DP4a) and XeSS-FG 2.1 need Shader Model 6.4, available on
    /// essentially every hardware GPU wgpu can drive through a modern backend.
    pub fn supports_dp4a(&self) -> bool {
        !self.is_software() && self.shader_model_64
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Support {
    pub available: bool,
    pub path: Path,
    pub reason: &'static str,
}

impl Support {
    fn accelerated(reason: &'static str) -> Self {
        Self {
            available: true,
            path: Path::Accelerated,
            reason,
        }
    }
    fn compatible(reason: &'static str) -> Self {
        Self {
            available: true,
            path: Path::Compatible,
            reason,
        }
    }
    fn no(reason: &'static str) -> Self {
        Self {
            available: false,
            path: Path::Fallback,
            reason,
        }
    }
}

impl Hardware {
    pub fn supports(&self, mode: Upscaler) -> Support {
        match mode {
            Upscaler::Off => Support::compatible("Bilinear scaling (manual resolution slider)"),
            Upscaler::Fsr2 | Upscaler::Fsr3 => {
                if self.is_software() {
                    Support::no("Temporal upscaling needs a hardware GPU")
                } else {
                    // FSR 2/3 is open (MIT) and runs on every vendor.
                    Support::compatible("FSR 2/3 is open and cross-vendor")
                }
            }
            Upscaler::Fsr41 => {
                if self.is_software() {
                    Support::no("FSR 4.1 needs a hardware GPU")
                } else if self.amd_rdna4() {
                    Support::accelerated("FSR 4.1 FP8 profile (RDNA 4 / RX 9000)")
                } else if self.amd_rdna2_or_3() {
                    Support::compatible("FSR 4.1 INT8 profile (RDNA 2/3)")
                } else if self.supports_dp4a() {
                    Support::compatible(
                        "FSR 4.1 INT8 compatibility profile (third-party GPU; unofficial upstream)",
                    )
                } else {
                    Support::no("FSR 4.1 needs an INT8/DP4a capable GPU")
                }
            }
            Upscaler::Dlss => {
                if self.nvidia_rtx() {
                    Support::accelerated("DLSS SR Tensor-core profile (RTX)")
                } else {
                    // Tensor cores are a genuine hardware requirement; DLSS is
                    // the one upscaler that does not have a portable fallback.
                    Support::no("DLSS requires NVIDIA RTX Tensor cores")
                }
            }
            Upscaler::Xess => {
                if !self.supports_dp4a() {
                    Support::no("XeSS needs a Shader Model 6.4 (DP4a) GPU")
                } else if self.intel_xmx() {
                    Support::accelerated("XeSS XMX profile (Intel Arc / Core Ultra)")
                } else {
                    Support::compatible("XeSS DP4a fallback (official on non-Intel GPUs)")
                }
            }
        }
    }

    pub fn supports_frame_gen(&self, fg: FrameGen) -> Support {
        match fg {
            FrameGen::Off => Support::compatible("Frame generation disabled"),
            FrameGen::Auto => Support::compatible("Resolved against the active upscaler"),
            FrameGen::Fsr => {
                if self.is_software() {
                    Support::no("FSR Frame Generation needs a hardware GPU")
                } else {
                    Support::compatible("FSR 3 Frame Generation is open and cross-vendor")
                }
            }
            FrameGen::Dlss => {
                if self.nvidia_ada_or_newer() {
                    if self.nvidia_blackwell() {
                        Support::accelerated("DLSS 4.5 FG/MFG on RTX 50 (up to 6x)")
                    } else {
                        Support::accelerated("DLSS Frame Generation on RTX 40 (optical flow)")
                    }
                } else {
                    Support::no("DLSS Frame Generation requires RTX 40 or newer")
                }
            }
            FrameGen::Xess => {
                if !self.supports_dp4a() {
                    Support::no("XeSS-FG requires a Shader Model 6.4 GPU")
                } else if self.intel_xmx() {
                    Support::accelerated("XeSS 3 FG/MFG on Intel XMX hardware (up to 4x)")
                } else {
                    Support::compatible("XeSS-FG 2.1 DP4a path (any SM 6.4 GPU, 2x)")
                }
            }
        }
    }
}

/// The persisted Graphics-tab inputs. `manual_percent` is the legacy
/// resolution-scale slider, used only with `Upscaler::Off` and no frame
/// generation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Config {
    pub upscaler: Upscaler,
    pub quality: Quality,
    pub sharpness: f32,
    pub frame_generation: FrameGen,
    /// Requested presented frames per rendered frame, 2..6. Clamped by the
    /// selected technology's hardware ceiling at runtime.
    pub multiplier: u8,
    pub manual_percent: f32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            upscaler: Upscaler::Off,
            quality: Quality::Quality,
            sharpness: 0.5,
            frame_generation: FrameGen::Off,
            multiplier: 2,
            manual_percent: 100.,
        }
    }
}

/// What the renderer will actually do this frame, after all hardware gates and
/// fallbacks have been applied.
#[derive(Clone, Debug, PartialEq)]
pub struct Selection {
    pub requested: Upscaler,
    pub upscaler: Upscaler,
    /// WGSL tuning profile actually used (may differ on compatibility paths).
    pub profile: Upscaler,
    pub upscaler_path: Path,
    pub quality: Quality,
    /// Internal 3D render resolution, percent of the window size.
    pub render_percent: f32,
    pub sharpness: f32,
    /// True when the temporal compositor replaces the bilinear blit. This is
    /// independent of `upscaler`: native-resolution frame generation needs it.
    pub temporal_active: bool,
    pub frame_generation: FrameGen,
    pub frame_gen_path: Path,
    /// Effective frame multiplier: 1 = off, 2 = one generated frame, etc.
    pub multiplier: u32,
    pub upscaler_fallback: bool,
    pub frame_gen_fallback: bool,
    /// Status lines for the Graphics tab. Never empty.
    pub upscaler_note: String,
    pub frame_gen_note: String,
    pub note: String,
}

/// Largest frame multiplier a technology can produce on this adapter.
/// DLSS 4.5 reaches 6x on RTX 50, XeSS 3 reaches 4x on Intel XMX hardware,
/// FSR FG and the cross-vendor XeSS-FG 2.1 path produce one extra frame.
pub fn max_multiplier(fg: FrameGen, hw: &Hardware) -> u32 {
    match fg {
        FrameGen::Dlss if hw.nvidia_blackwell() => 6,
        FrameGen::Dlss => 2,
        FrameGen::Xess if hw.intel_xmx() => 4,
        FrameGen::Xess => 2,
        FrameGen::Fsr => 2,
        FrameGen::Off | FrameGen::Auto => 1,
    }
}

/// Resolve a user selection into a safe, hardware-supported plan.
///
/// The two features are resolved independently: a frame generator is never
/// replaced just because a different vendor's upscaler is active.
pub fn resolve(config: &Config, hw: &Hardware) -> Selection {
    let requested = config.upscaler;
    let mut notes: Vec<String> = Vec::new();
    let mut upscaler_fallback = false;

    let (upscaler, upscaler_path, upscaler_note) = if requested == Upscaler::Off {
        (Upscaler::Off, Path::Disabled, "Upscaling off".to_string())
    } else {
        let support = hw.supports(requested);
        if support.available {
            (
                requested,
                support.path,
                format!("{} — {}", requested.label(), support.reason),
            )
        } else {
            // Fall back to the widest cross-vendor path, then to bilinear.
            upscaler_fallback = true;
            let fallback = if hw.supports(Upscaler::Fsr3).available {
                Upscaler::Fsr3
            } else if hw.supports(Upscaler::Fsr2).available {
                Upscaler::Fsr2
            } else {
                Upscaler::Off
            };
            (
                fallback,
                Path::Fallback,
                format!(
                    "{} — {} — using {} instead",
                    requested.label(),
                    support.reason,
                    fallback.label()
                ),
            )
        }
    };

    // A compatibility FSR 4.1 path uses the FSR 3 tuning profile; the FP8
    // profile stays on the FSR 4.1 tuning (see `Upscaler::tuning`).
    let profile = if upscaler == Upscaler::Fsr41 && upscaler_path == Path::Compatible {
        Upscaler::Fsr3
    } else {
        upscaler
    };

    let wanted = if config.frame_generation == FrameGen::Auto {
        upscaler.native_frame_generation()
    } else {
        config.frame_generation
    };
    let mut frame_gen_fallback = false;
    let mut frame_gen_path = Path::Disabled;
    let mut frame_generation = FrameGen::Off;
    let mut multiplier = 1;
    let mut frame_gen_note = "Frame generation off".to_string();

    if wanted != FrameGen::Off {
        let support = hw.supports_frame_gen(wanted);
        if support.available {
            frame_generation = wanted;
            frame_gen_path = support.path;
            frame_gen_note = format!("{} — {}", wanted.short(), support.reason);
        } else {
            frame_gen_fallback = true;
            frame_gen_path = Path::Fallback;
            let fallback = if hw.supports_frame_gen(FrameGen::Fsr).available {
                FrameGen::Fsr
            } else {
                FrameGen::Off
            };
            frame_generation = fallback;
            frame_gen_note = format!(
                "{} — {} — using {} instead",
                wanted.short(),
                support.reason,
                fallback.short()
            );
            if fallback != FrameGen::Off {
                frame_gen_note.push_str(&format!(
                    " — {}",
                    hw.supports_frame_gen(FrameGen::Fsr).reason
                ));
            }
        }
        if frame_generation != FrameGen::Off {
            let requested_multiplier = u32::from(config.multiplier.clamp(2, 6));
            let ceiling = max_multiplier(frame_generation, hw);
            multiplier = requested_multiplier.min(ceiling);
            if multiplier < requested_multiplier {
                frame_gen_fallback = true;
                notes.push(format!(
                    "{} supports up to {}x on this GPU",
                    frame_generation.short(),
                    ceiling
                ));
            }
            notes.push(format!(
                "{} active: {}x presented frames",
                frame_generation.short(),
                multiplier
            ));
        }
    }

    let temporal_active = upscaler != Upscaler::Off || frame_generation != FrameGen::Off;
    let render_percent = if upscaler != Upscaler::Off {
        config.quality.percent()
    } else if frame_generation != FrameGen::Off {
        // Native-resolution frame generation: the temporal compositor still
        // runs (it owns the history buffers FG warps), but no upscaling occurs.
        100.
    } else {
        config.manual_percent.clamp(10., 200.)
    };
    let sharpness = if config.sharpness.is_finite() {
        config.sharpness.clamp(0., 1.)
    } else {
        0.5
    };

    if upscaler != Upscaler::Off {
        notes.push(format!(
            "{} at {:.0}% internal resolution ({})",
            upscaler.label(),
            render_percent,
            config.quality.label()
        ));
    } else if frame_generation != FrameGen::Off {
        notes.push("Native resolution (100%); frame generation without upscaling".to_string());
    } else {
        notes.push(
            "Bilinear resolution scaling; manual slider controls the 3D resolution".to_string(),
        );
    }
    if hw.is_software() {
        notes.push("Software adapter detected: vendor features disabled".to_string());
    }

    Selection {
        requested,
        upscaler,
        profile,
        upscaler_path,
        quality: config.quality,
        render_percent,
        sharpness,
        temporal_active,
        frame_generation,
        frame_gen_path,
        multiplier,
        upscaler_fallback,
        frame_gen_fallback,
        upscaler_note,
        frame_gen_note,
        note: notes.join(" • "),
    }
}

/// Halton low-discrepancy sequence value for a 1-based index.
pub fn halton(index: u32, base: u32) -> f32 {
    let mut result = 0.0f32;
    let mut denominator = 1.0f32;
    let mut i = index.max(1);
    while i > 0 {
        denominator *= base as f32;
        result += (i % base) as f32 / denominator;
        i /= base;
    }
    result
}

/// Jitter for frame `frame_index`, in UV units (positive = right/down), sized
/// for a `low_w` × `low_h` internal render target. One 16-frame Halton cycle.
pub fn jitter_uv(frame_index: u32, low_w: u32, low_h: u32) -> [f32; 2] {
    let i = (frame_index % 16) + 1;
    let w = low_w.max(1) as f32;
    let h = low_h.max(1) as f32;
    [(halton(i, 2) - 0.5) / w, (halton(i, 3) - 0.5) / h]
}

/// The same jitter as an NDC offset used to offset the projection matrix.
pub fn jitter_ndc(frame_index: u32, low_w: u32, low_h: u32) -> [f32; 2] {
    let j = jitter_uv(frame_index, low_w, low_h);
    [2.0 * j[0], -2.0 * j[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hardware(vendor_id: u32, name: &str) -> Hardware {
        Hardware::new(
            vendor_id,
            0,
            name.to_string(),
            DeviceClass::Discrete,
            Backend::Vulkan,
            true,
            true,
        )
    }

    #[test]
    fn quality_ladder_matches_the_published_scale_factors() {
        assert_eq!(Quality::NativeAa.percent(), 100.);
        assert!((Quality::Quality.percent() - 66.6667).abs() < 0.001);
        assert!((Quality::Balanced.percent() - 58.8235).abs() < 0.001);
        assert_eq!(Quality::Performance.percent(), 50.);
        assert!((Quality::UltraPerformance.percent() - 33.3333).abs() < 0.001);
        for q in Quality::ALL {
            assert_eq!(Quality::parse(q.key()), Some(q));
        }
        assert_eq!(Quality::parse("Ultra"), None);
    }

    #[test]
    fn enum_keys_round_trip_and_unknown_keys_do_not_change_anything() {
        for m in Upscaler::ALL {
            assert_eq!(Upscaler::parse(m.key()), Some(m));
        }
        for f in FrameGen::ALL {
            assert_eq!(FrameGen::parse(f.key()), Some(f));
        }
        assert_eq!(Upscaler::parse("DLSS 4"), None);
        assert_eq!(FrameGen::parse("FSR4"), None);
        assert_eq!(Upscaler::default(), Upscaler::Off);
        assert_eq!(FrameGen::default(), FrameGen::Off);
        assert_eq!(Quality::default(), Quality::Quality);
    }

    #[test]
    fn xess_and_xess_fg_run_on_any_sm64_gpu() {
        // XeSS SR always had the DP4a fallback and SDK 2.1 opened XeSS-FG to
        // NVIDIA/AMD GPUs with Shader Model 6.4.
        for gpu in [
            hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090"),
            hardware(VENDOR_AMD, "AMD Radeon RX 6800 XT"),
            hardware(VENDOR_INTEL, "Intel(R) Arc(TM) B580 Graphics"),
            hardware(0x1af4, "Generic DX12 GPU"),
        ] {
            assert!(gpu.supports(Upscaler::Xess).available, "{}", gpu.name);
            assert!(
                gpu.supports_frame_gen(FrameGen::Xess).available,
                "{}",
                gpu.name
            );
        }
        let arc = hardware(VENDOR_INTEL, "Intel(R) Arc(TM) B580 Graphics");
        let rtx = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        assert_eq!(arc.supports(Upscaler::Xess).path, Path::Accelerated);
        assert_eq!(rtx.supports(Upscaler::Xess).path, Path::Compatible);
        assert_eq!(
            rtx.supports_frame_gen(FrameGen::Xess).reason,
            "XeSS-FG 2.1 DP4a path (any SM 6.4 GPU, 2x)"
        );
    }

    #[test]
    fn fsr3_is_cross_vendor_and_fsr41_has_an_int8_compatibility_path() {
        let rtx = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        let rdna2 = hardware(VENDOR_AMD, "AMD Radeon RX 6800 XT");
        let rdna3 = hardware(VENDOR_AMD, "AMD Radeon RX 7800 XT");
        let rdna4 = hardware(VENDOR_AMD, "AMD Radeon RX 9070 XT");
        let arc = hardware(VENDOR_INTEL, "Intel(R) Arc(TM) A770 Graphics");

        for gpu in [&rtx, &rdna2, &rdna3, &rdna4, &arc] {
            assert!(gpu.supports(Upscaler::Fsr2).available);
            assert!(gpu.supports(Upscaler::Fsr3).available);
            assert!(gpu.supports(Upscaler::Fsr41).available, "{}", gpu.name);
            assert!(gpu.supports_frame_gen(FrameGen::Fsr).available);
        }
        assert_eq!(rdna4.supports(Upscaler::Fsr41).path, Path::Accelerated);
        assert_eq!(rdna3.supports(Upscaler::Fsr41).path, Path::Compatible);
        assert_eq!(rdna2.supports(Upscaler::Fsr41).path, Path::Compatible);
        assert_eq!(rtx.supports(Upscaler::Fsr41).path, Path::Compatible);
        assert!(rtx
            .supports(Upscaler::Fsr41)
            .reason
            .contains("third-party GPU"));
        // Compatibility runs the FSR 3 tuning profile; FP8 keeps 4.1 tuning.
        assert_eq!(
            resolve(
                &Config {
                    upscaler: Upscaler::Fsr41,
                    ..Config::default()
                },
                &rtx
            )
            .profile,
            Upscaler::Fsr3
        );
        assert_eq!(
            resolve(
                &Config {
                    upscaler: Upscaler::Fsr41,
                    ..Config::default()
                },
                &rdna4
            )
            .profile,
            Upscaler::Fsr41
        );
    }

    #[test]
    fn dlss_remains_the_only_real_vendor_lock() {
        let rtx4090 = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        let rtx2080 = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 2080 Ti");
        let gtx = hardware(VENDOR_NVIDIA, "NVIDIA GeForce GTX 1660");
        let rdna4 = hardware(VENDOR_AMD, "AMD Radeon RX 9070 XT");
        let arc = hardware(VENDOR_INTEL, "Intel(R) Arc(TM) B580 Graphics");

        assert!(rtx4090.supports(Upscaler::Dlss).available);
        assert!(rtx2080.supports(Upscaler::Dlss).available);
        assert!(!gtx.supports(Upscaler::Dlss).available);
        assert!(!rdna4.supports(Upscaler::Dlss).available);
        assert!(!arc.supports(Upscaler::Dlss).available);
        assert!(rtx4090.supports_frame_gen(FrameGen::Dlss).available);
        assert!(!rtx2080.supports_frame_gen(FrameGen::Dlss).available);
        assert!(!rdna4.supports_frame_gen(FrameGen::Dlss).available);
    }

    #[test]
    fn frame_generation_is_fully_untied_from_upscaling() {
        let rtx4090 = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        // DLSS upscaling + FSR frame generation.
        let mixed = resolve(
            &Config {
                upscaler: Upscaler::Dlss,
                frame_generation: FrameGen::Fsr,
                ..Config::default()
            },
            &rtx4090,
        );
        assert_eq!(mixed.upscaler, Upscaler::Dlss);
        assert_eq!(mixed.frame_generation, FrameGen::Fsr);
        assert!(!mixed.upscaler_fallback);
        assert!(!mixed.frame_gen_fallback);
        assert!(mixed.temporal_active);

        // XeSS upscaling + DLSS frame generation.
        let mixed2 = resolve(
            &Config {
                upscaler: Upscaler::Xess,
                frame_generation: FrameGen::Dlss,
                ..Config::default()
            },
            &rtx4090,
        );
        assert_eq!(mixed2.upscaler, Upscaler::Xess);
        assert_eq!(mixed2.frame_generation, FrameGen::Dlss);
        assert!(!mixed2.frame_gen_fallback);

        // FSR 4.1 upscaling + XeSS frame generation on an AMD GPU.
        let rdna4 = hardware(VENDOR_AMD, "AMD Radeon RX 9070 XT");
        let mixed3 = resolve(
            &Config {
                upscaler: Upscaler::Fsr41,
                frame_generation: FrameGen::Xess,
                ..Config::default()
            },
            &rdna4,
        );
        assert_eq!(mixed3.upscaler, Upscaler::Fsr41);
        assert_eq!(mixed3.frame_generation, FrameGen::Xess);
        assert!(!mixed3.frame_gen_fallback);

        // Frame generation with no upscaler at all: native resolution, but the
        // temporal compositor is active so the generated frames have history.
        let native_fg = resolve(
            &Config {
                upscaler: Upscaler::Off,
                manual_percent: 50.,
                frame_generation: FrameGen::Fsr,
                ..Config::default()
            },
            &rdna4,
        );
        assert_eq!(native_fg.upscaler, Upscaler::Off);
        assert_eq!(native_fg.frame_generation, FrameGen::Fsr);
        assert_eq!(native_fg.render_percent, 100.);
        assert!(native_fg.temporal_active);
        assert_eq!(native_fg.multiplier, 2);
        assert!(native_fg.note.contains("without upscaling"));
    }

    #[test]
    fn auto_follows_the_upscaler_but_is_only_a_default() {
        let cases = [
            (
                hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4080"),
                Upscaler::Dlss,
                FrameGen::Dlss,
            ),
            (
                hardware(VENDOR_INTEL, "Intel(R) Arc(TM) B580 Graphics"),
                Upscaler::Xess,
                FrameGen::Xess,
            ),
            (
                hardware(VENDOR_AMD, "AMD Radeon RX 9070 XT"),
                Upscaler::Fsr41,
                FrameGen::Fsr,
            ),
            (
                hardware(VENDOR_AMD, "AMD Radeon RX 6800 XT"),
                Upscaler::Off,
                FrameGen::Fsr,
            ),
        ];
        for (gpu, upscaler, expected) in cases {
            let plan = resolve(
                &Config {
                    upscaler,
                    frame_generation: FrameGen::Auto,
                    ..Config::default()
                },
                &gpu,
            );
            assert_eq!(plan.upscaler, upscaler, "unexpected upscaler fallback");
            assert_eq!(plan.frame_generation, expected);
            assert!(!plan.frame_gen_fallback);
        }
    }

    #[test]
    fn unsupported_vendor_requests_fall_back_without_touching_the_other_feature() {
        let rdna3 = hardware(VENDOR_AMD, "AMD Radeon RX 7800 XT");
        let plan = resolve(
            &Config {
                upscaler: Upscaler::Dlss,
                frame_generation: FrameGen::Dlss,
                ..Config::default()
            },
            &rdna3,
        );
        assert_eq!(plan.upscaler, Upscaler::Fsr3);
        assert_eq!(plan.frame_generation, FrameGen::Fsr);
        assert!(plan.upscaler_fallback);
        assert!(plan.frame_gen_fallback);
        assert!(plan.upscaler_note.contains("Tensor cores"));
        assert!(plan.frame_gen_note.contains("RTX 40"));
        // Both fallbacks are independent: neither feature is disabled.
        assert!(plan.temporal_active);
        assert_eq!(plan.multiplier, 2);
    }

    #[test]
    fn multi_frame_generation_ceilings_follow_the_2026_hardware() {
        let blackwell = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 5090");
        let ada = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        let arc = hardware(VENDOR_INTEL, "Intel(R) Arc(TM) B580 Graphics");
        let rtx_on_xess = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        let rdna4 = hardware(VENDOR_AMD, "AMD Radeon RX 9070 XT");
        let dlss = |multiplier: u8| Config {
            upscaler: Upscaler::Dlss,
            frame_generation: FrameGen::Dlss,
            multiplier,
            ..Config::default()
        };
        assert_eq!(resolve(&dlss(6), &blackwell).multiplier, 6);
        assert_eq!(resolve(&dlss(6), &ada).multiplier, 2);
        assert_eq!(max_multiplier(FrameGen::Xess, &arc), 4);
        assert_eq!(max_multiplier(FrameGen::Xess, &rtx_on_xess), 2);
        assert_eq!(max_multiplier(FrameGen::Fsr, &rdna4), 2);
        let xess4 = resolve(
            &Config {
                upscaler: Upscaler::Xess,
                frame_generation: FrameGen::Xess,
                multiplier: 4,
                ..Config::default()
            },
            &arc,
        );
        assert_eq!(xess4.multiplier, 4);
        let xess_on_rtx = resolve(
            &Config {
                upscaler: Upscaler::Xess,
                frame_generation: FrameGen::Xess,
                multiplier: 4,
                ..Config::default()
            },
            &rtx_on_xess,
        );
        assert_eq!(xess_on_rtx.multiplier, 2);
        assert!(xess_on_rtx.frame_gen_fallback);
    }

    #[test]
    fn software_adapters_never_report_vendor_support() {
        let cpu = Hardware::new(
            VENDOR_NVIDIA,
            0,
            "llvmpipe (LLVM 15.0.7)".to_string(),
            DeviceClass::Cpu,
            Backend::Vulkan,
            false,
            false,
        );
        for m in Upscaler::ALL {
            assert_eq!(cpu.supports(m).available, m == Upscaler::Off);
        }
        assert!(!cpu.supports_frame_gen(FrameGen::Fsr).available);
        assert!(!cpu.supports_frame_gen(FrameGen::Xess).available);
        let plan = resolve(
            &Config {
                upscaler: Upscaler::Dlss,
                frame_generation: FrameGen::Auto,
                ..Config::default()
            },
            &cpu,
        );
        assert_eq!(plan.upscaler, Upscaler::Off);
        assert_eq!(plan.frame_generation, FrameGen::Off);
        assert_eq!(plan.multiplier, 1);
        assert!(!plan.temporal_active);
        assert!(plan.upscaler_fallback);
    }

    #[test]
    fn off_keeps_the_manual_slider_and_never_generates_frames() {
        let gpu = hardware(VENDOR_NVIDIA, "NVIDIA GeForce RTX 4090");
        let plan = resolve(
            &Config {
                upscaler: Upscaler::Off,
                manual_percent: 73.5,
                frame_generation: FrameGen::Off,
                multiplier: 6,
                ..Config::default()
            },
            &gpu,
        );
        assert_eq!(plan.upscaler, Upscaler::Off);
        assert_eq!(plan.render_percent, 73.5);
        assert_eq!(plan.frame_generation, FrameGen::Off);
        assert_eq!(plan.multiplier, 1);
        assert!(!plan.temporal_active);
    }

    #[test]
    fn dangerous_config_values_are_sanitised() {
        let gpu = hardware(VENDOR_AMD, "AMD Radeon RX 9070 XT");
        let plan = resolve(
            &Config {
                upscaler: Upscaler::Fsr3,
                sharpness: f32::NAN,
                frame_generation: FrameGen::Auto,
                multiplier: 200,
                manual_percent: -1.,
                ..Config::default()
            },
            &gpu,
        );
        assert_eq!(plan.sharpness, 0.5);
        assert_eq!(plan.multiplier, 2);
        assert!(plan.render_percent > 10. && plan.render_percent <= 100.);
    }

    #[test]
    fn jitter_is_bounded_by_half_a_low_resolution_pixel_and_cycles() {
        assert!((halton(1, 2) - 0.5).abs() < 1e-6);
        assert!((halton(1, 3) - 0.3333333).abs() < 1e-6);
        for i in 0..64u32 {
            let uv = jitter_uv(i, 960, 540);
            assert!(uv[0].abs() <= 0.5 / 960. + 1e-6);
            assert!(uv[1].abs() <= 0.5 / 540. + 1e-6);
            let ndc = jitter_ndc(i, 960, 540);
            assert!((ndc[0] - 2. * uv[0]).abs() < 1e-9);
            assert!((ndc[1] + 2. * uv[1]).abs() < 1e-9);
        }
        assert_eq!(jitter_uv(0, 960, 540), jitter_uv(16, 960, 540));
        assert_ne!(jitter_uv(0, 960, 540), jitter_uv(1, 960, 540));
        // A degenerate extent still yields finite values.
        assert!(jitter_uv(3, 0, 0).iter().all(|v| v.is_finite()));
    }
}
