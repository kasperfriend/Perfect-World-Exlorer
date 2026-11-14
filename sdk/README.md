# Vendor Upscaling & Frame Generation SDKs (`sdk/`)

This directory holds redistributable vendor SDK artifacts and license notices for **Perfect World Explorer** (`GPL-2.0-or-later`).

## Summary of Integrated & Supported Technologies

| Technology | Upstream SDK & Verified Version | Source URL | License & Redistribution Status | Runtime Status in Viewer |
|---|---|---|---|---|
| **AMD FSR 1.1 (EASU + RCAS)** | FidelityFX Super Resolution 1.1 (WGSL port in `src/fsr1.wgsl`, `src/fsr1_gpu.rs`) | <https://github.com/GPUOpen-Effects/FidelityFX-FSR> | **MIT License** — fully open-source and GPL-2.0 compatible; compiled directly into the binary | **Genuinely implemented & active** on all GPUs and backends (`Direct3D 12`, `Vulkan`, etc.) |
| **AMD FSR 2 / 3.1 / 4.1 Upscaling & FSR 3.1 / 4.0 Frame Generation** | AMD FSR SDK **v2.4.0 ("Redstone")** (includes FSR 2.3.4, FSR 3.1.6 upscaler/FG, Swapchain 3.1.7/4.0.0, FSR Upscaling 4.1.2 ML, FSR FG 4.0.2 ML) | <https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK> | **MIT-style exception** for signed `signedbin/*.dll` (`AMD_FidelityFX-SDK-2.4.0-LICENSE.md`); included in `sdk/` | **SDK probed & validated**; disabled in UI until native `ID3D12CommandList` / motion-vector hooks are available (beyond `wgpu 0.19`) |
| **NVIDIA DLSS Super Resolution & DLSS Frame Generation** | NVIDIA DLSS SDK **v310.9.x** + NVIDIA Streamline SDK **v2.14.x** (`nvngx_dlss.dll`, `nvngx_dlssg.dll`, `sl.interposer.dll`) | <https://github.com/NVIDIA/DLSS> and <https://github.com/NVIDIAGameWorks/Streamline> | **NVIDIA RTX SDKs License (v. March 14, 2024)** — proprietary; Section 4.e forbids subjecting to GPL/open-source terms. **Not included in Git or releases** | **Hardware & SDK probed** (RTX GPU for SR; RTX 40/50 Ada/Blackwell for FG); disabled in UI without native DX12/motion-vector integration |
| **Intel XeSS Super Resolution & XeSS Frame Generation** | Intel XeSS SDK **v2.1.1 / v3.0.2** (`libxess.dll`, `libxess_fg.dll`, `libxell.dll`) | <https://github.com/intel/xess> | Public headers (`inc/`) are **MIT**; runtime DLLs (`bin/*.dll`) are proprietary under **Intel Simplified Software License (Oct 2022)**. **Binaries not included in Git** | **Hardware & SDK probed** (Intel Arc XMX & SM 6.4 DP4a GPUs); disabled in UI without native DX12/XeLL/motion-vector integration |

## Decoupled Upscaling & Frame Generation Compatibility

- **AMD FSR 3.1 / 4.0 Frame Generation**: Documented by AMD as decoupled from the upscaler (`pairing()` permits `Off`, `Fsr1`, `Dlss`, `Fsr`, or `Xess`).
- **NVIDIA DLSS Frame Generation (`sl.dlss_g`)**: Documented via Streamline as decoupled from the upscaler (`pairing()` permits `Off`, `Fsr1`, `Dlss`, `Fsr`, or `Xess` on RTX 40/50 GPUs).
- **Intel XeSS Frame Generation (`libxess_fg.dll`)**: Requires Xe Low Latency (`libxell.dll`) and is documented for use with `XeSS` or `Off` only; cross-vendor upscaler pairings (`Dlss`, `Fsr`, `Fsr1` + `Xess FG`) are rejected by `upscaling::pairing` and `upscaling::sanitise`.

## How to Supply Non-Redistributable SDKs Locally

1. Download the official SDK release from NVIDIA or Intel and accept the vendor's license terms.
2. Place the runtime DLLs in `<viewer-folder>\sdk\`, or set `PW_UPSCALER_SDK_DIR=<path>`, or set `upscale_sdk_dir=<path>` in `viewer-settings.txt`.
3. Run `cargo run --bin dx12-upscale-check -- --sdk-dir <path>` and `cargo run --bin dx12-framegen-check -- --sdk-dir <path>` to verify detection.
