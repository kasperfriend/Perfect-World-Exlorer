//! Diagnose the upscaling providers on this machine: what the hardware and a
//! user-supplied SDK folder allow, and whether this build can drive them.
//!
//! Run with: cargo run --bin dx12-upscale-check
//!           cargo run --bin dx12-upscale-check -- --sdk-dir D:\sdks\vendor --require-sdk
//!
//! No client and no GPU are required: with neither, the diagnostic prints the
//! SDK probe report and exits 0. `--require-sdk` exits 4 when no vendor DLL is
//! found, `--require-gpu` exits 5 when no adapter can be created. It never
//! fails just because a provider is unavailable — unavailability is the
//! expected outcome on machines without SDKs.
#[path = "../upscaling.rs"]
mod upscaling;

use upscaling::{FrameGenMethod, GpuFacts, Upscaler, UpscaleMethod, UpscaleQuality};

fn adapter_facts() -> Option<GpuFacts> {
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))?;
    let info = adapter.get_info();
    Some(GpuFacts::from_parts(
        info.vendor,
        info.device,
        &info.name,
        &format!("{:?}", info.backend),
        info.device_type == wgpu::DeviceType::Cpu,
    ))
}

struct Args {
    sdk_dir: String,
    require_sdk: bool,
    require_gpu: bool,
}

fn parse_args() -> Args {
    let mut sdk_dir = String::new();
    let mut require_sdk = false;
    let mut require_gpu = false;
    let mut iter = std::env::args().skip(1);
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--sdk-dir" => sdk_dir = iter.next().unwrap_or_default(),
            "--require-sdk" => require_sdk = true,
            "--require-gpu" => require_gpu = true,
            "--help" | "-h" => {
                println!("dx12-upscale-check [--sdk-dir <path>] [--require-sdk] [--require-gpu]");
                std::process::exit(0);
            }
            other => eprintln!("ignored unknown argument: {other}"),
        }
    }
    Args {
        sdk_dir,
        require_sdk,
        require_gpu,
    }
}

fn main() {
    let args = parse_args();
    println!("Perfect World Explorer — dx12-upscale-check");
    println!("platform: {}", std::env::consts::OS);
    println!(
        "requested backends: {}",
        std::env::var("WGPU_BACKEND").unwrap_or_else(|_| {
            if cfg!(windows) {
                "dx12 (Windows default on this branch)".to_string()
            } else {
                "all supported by wgpu (Windows uses dx12)".to_string()
            }
        })
    );
    println!();

    let sdk = upscaling::probe_sdk(&args.sdk_dir);
    println!("SDK probe: {}", sdk.summary());
    println!(
        "  nvngx_dlss.dll: {}   nvngx_dlssg.dll: {}   sl.interposer.dll: {}",
        yes_no(sdk.dlss),
        yes_no(sdk.dlss_frame_generation),
        yes_no(sdk.streamline)
    );
    println!(
        "  FSR upscaler: {}   FSR frame generation: {}   FSR optical flow: {}",
        yes_no(sdk.fsr_upscale),
        yes_no(sdk.fsr_frame_generation),
        yes_no(sdk.fsr_optical_flow)
    );
    println!(
        "  libxess.dll: {}   libxess_fg.dll: {}",
        yes_no(sdk.xess),
        yes_no(sdk.xess_frame_generation)
    );
    println!();

    match adapter_facts() {
        Some(gpu) => {
            println!(
                "GPU: {} (vendor 0x{:04x}, device 0x{:04x}, backend {}{})",
                gpu.name,
                gpu.vendor,
                gpu.device,
                gpu.backend,
                if gpu.cpu_fallback {
                    ", software adapter"
                } else {
                    ""
                }
            );
            println!();
            println!("Upscaler support on this machine:");
            for method in UpscaleMethod::ALL {
                let support = upscaling::support(method, &gpu, &sdk);
                println!("  {} — {}", method.label(), describe(&support));
            }
            println!();
            println!("Frame generation support (with Upscaling = Off):");
            for method in FrameGenMethod::ALL {
                let support =
                    upscaling::support_framegen(method, UpscaleMethod::Off, &gpu, &sdk);
                println!("  {} — {}", method.label(), describe(&support));
            }
            println!();
            println!("Quality modes and internal render factors (1920×1080 window):");
            for method in [UpscaleMethod::Dlss, UpscaleMethod::Fsr, UpscaleMethod::Xess] {
                println!("  {}:", method.label());
                for quality in UpscaleQuality::ALL {
                    let available = upscaling::quality_available(method, quality);
                    let (w, h) =
                        upscaling::provider_extent(method, quality, 1920, 1080, 8192);
                    println!(
                        "    {} — internal {}×{} (factor {:.4}){}",
                        quality.label(),
                        w,
                        h,
                        upscaling::internal_factor(method, quality),
                        if available {
                            ""
                        } else {
                            "  [not offered by this provider]"
                        }
                    );
                }
            }
            println!();
            println!("Temporal jitter (Halton 2,3, period 64), first 8 frames:");
            for frame in 0..8 {
                let (x, y) = upscaling::jitter_offset(frame);
                println!("  frame {frame:2}: ({x:+.5}, {y:+.5}) px");
            }
            println!();
            let mut passthrough = upscaling::Passthrough::default();
            match passthrough.init() {
                Ok(()) => println!("passthrough provider (Upscaling = Off): OK"),
                Err(error) => println!("passthrough provider (Upscaling = Off): {error}"),
            }
            println!("resolution-scale path applies while Upscaling is Off; an active");
            println!("provider renders at its recommended internal resolution instead.");
        }
        None => {
            println!("GPU: no adapter could be created (headless or no driver).");
            println!("The support matrix needs a GPU; the SDK probe above still ran.");
            if args.require_gpu {
                std::process::exit(5);
            }
        }
    }

    let any_sdk = sdk.dlss || sdk.fsr_upscale || sdk.xess;
    if args.require_sdk && !any_sdk {
        println!();
        println!("FAIL: --require-sdk and no vendor SDK DLL was found in the probed folder(s).");
        std::process::exit(4);
    }
    println!();
    println!("Done. Unavailable providers are reported, not faked; see docs/blockers.md.");
}

fn yes_no(value: bool) -> &'static str {
    if value {
        "found"
    } else {
        "absent"
    }
}

fn describe(support: &upscaling::Support) -> String {
    match support {
        upscaling::Support::Available(note) => format!("available ({note})"),
        upscaling::Support::Disabled(reason) => format!("unavailable — {reason}"),
    }
}
