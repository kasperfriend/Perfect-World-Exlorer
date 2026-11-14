//! Diagnose the frame-generation pairings on this machine.
//!
//! Run with: cargo run --bin dx12-framegen-check
//!           cargo run --bin dx12-framegen-check -- --sdk-dir D:\sdks\vendor --require-sdk
//!
//! No client and no GPU are required. The check prints the full
//! upscaler × frame-generator matrix with the reasons each combination is
//! unavailable, the SDK probe report, and the frame-pacing contract the
//! shipped viewer is held to. `--require-sdk` exits 4 when no frame-generation
//! DLL is found, `--require-gpu` exits 5 when no adapter can be created.
#[path = "../upscaling.rs"]
mod upscaling;

use upscaling::{FrameGenMethod, GpuFacts, PairingVerdict, UpscaleMethod};

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
                println!("dx12-framegen-check [--sdk-dir <path>] [--require-sdk] [--require-gpu]");
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
    println!("Perfect World Explorer — dx12-framegen-check");
    println!("platform: {}", std::env::consts::OS);
    println!();

    let sdk = upscaling::probe_sdk(&args.sdk_dir);
    println!("SDK probe: {}", sdk.summary());
    println!(
        "  DLSS FG: {}   FSR FG: {} (optical flow: {})   XeSS 2 FG: {}",
        yes_no(sdk.dlss_frame_generation),
        yes_no(sdk.fsr_frame_generation),
        yes_no(sdk.fsr_optical_flow),
        yes_no(sdk.xess_frame_generation)
    );
    println!();

    println!("Pairing matrix (vendor documentation; NOT verified against the SDKs):");
    println!("                       {:>22} {:>22} {:>22}", "DLSS FG", "FSR FG", "XeSS FG");
    for upscale in UpscaleMethod::ALL {
        let cells: Vec<&'static str> = [
            FrameGenMethod::Dlss,
            FrameGenMethod::Fsr,
            FrameGenMethod::Xess,
        ]
        .iter()
            .map(|fg| match upscaling::pairing(upscale, *fg) {
                PairingVerdict::Supported => "supported*",
                PairingVerdict::Unsupported(_) => "not supported",
            })
            .collect();
        println!("  {:>18} {:>22} {:>22} {:>22}", upscale.label(), cells[0], cells[1], cells[2]);
    }
    println!("* Supported by the vendor's documentation, but only exposed when the SDK DLLs are");
    println!("  present and the hardware gate passes; this build never fakes an interpolation.");
    for upscale in UpscaleMethod::ALL {
        for frame_gen in FrameGenMethod::ALL {
            if let PairingVerdict::Unsupported(reason) = upscaling::pairing(upscale, frame_gen) {
                println!(
                    "unsupported pairing: {} + {} — {reason}",
                    upscale.label(),
                    frame_gen.label()
                );
            }
        }
    }
    println!();

    match adapter_facts() {
        Some(gpu) => {
            println!(
                "GPU: {} (vendor 0x{:04x}, backend {}{})",
                gpu.name,
                gpu.vendor,
                gpu.backend,
                if gpu.cpu_fallback {
                    ", software adapter"
                } else {
                    ""
                }
            );
            println!();
            println!("Frame generation support per pairing on this machine:");
            for upscale in UpscaleMethod::ALL {
                for frame_gen in [FrameGenMethod::Dlss, FrameGenMethod::Fsr, FrameGenMethod::Xess] {
                    let support = upscaling::support_framegen(frame_gen, upscale, &gpu, &sdk);
                    println!(
                        "  {} + {} — {}",
                        upscale.label(),
                        frame_gen.label(),
                        describe(&support)
                    );
                }
            }
        }
        None => {
            println!("GPU: no adapter could be created (headless or no driver).");
            println!("The per-pairing matrix needs a GPU; the SDK probe above still ran.");
            if args.require_gpu {
                std::process::exit(5);
            }
        }
    }

    println!();
    println!("Frame pacing contract (verified when a provider is active):");
    println!("  - generated frames present in order, after the real frame that produced them;");
    println!("  - the UI is drawn on the real frame only and never interpolated, so menus,");
    println!("    the minimap and text are never doubled or smeared;");
    println!("  - the UI input rate is not halved by interpolation;");
    println!("  - the diagnostics TimingFrame / EffectFrames channels (F8 markers) remain the");
    println!("    probe for present order and frame time while frame generation is on.");
    println!();
    println!("Frame generation never presents UI frames twice: the UI layer is composed after");
    println!("the world upscale/interpolation pass and is excluded from interpolation.");

    let any_sdk = sdk.dlss_frame_generation
        || sdk.fsr_frame_generation
        || sdk.xess_frame_generation;
    if args.require_sdk && !any_sdk {
        println!();
        println!(
            "FAIL: --require-sdk and no frame-generation SDK DLL was found in the probed folder(s)."
        );
        std::process::exit(4);
    }
    println!();
    println!("Done. Unavailable pairings are reported, not faked; see docs/blockers.md.");
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
