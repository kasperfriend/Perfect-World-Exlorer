# Settings → Graphics: upscaling and frame generation

This document describes the two independent selectors in **Settings →
Graphics**, what runs on which hardware, and — importantly — what these modes
are and are not.

## What is implemented

Vendor upscalers (NVIDIA NGX/DLSS, Intel XeSS, AMD FidelityFX FSR 4/4.1) ship
as proprietary SDKs with neural models and driver integration. They cannot be
reconstructed from source, and **no vendor SDK, DLL or neural model is bundled**
with this viewer. The menu entries are therefore *in-house compatibility
implementations*: a single portable WGSL temporal upscaler and a camera-motion
frame interpolator (`src/upscale_gpu.rs`), with per-technology tuning profiles
and support gates that mirror the vendors' published requirements
(`src/upscaling.rs`).

The pipeline:

1. The 3D world renders into the existing low-resolution `RenderScale` target
   with a Halton sub-pixel projection jitter (16-frame cycle).
2. A WGSL pass reprojects the previous frame using the previous clip matrix and
   scene depth (the client uses reversed depth), clips history to the current
   3×3 YCoCg neighbourhood, rejects disocclusions by depth/motion/luma, and
   accumulates into a full-resolution history texture.
3. A contrast-adaptive RCAS-like sharpen pass writes the full-resolution
   composition view. The UI, minimap, gamma ramp and present encode all still
   run at native resolution.
4. If frame generation is on, `multiplier - 1` extra frames are presented after
   each real frame by warping the two most recent full-resolution frames with
   the depth buffer and re-drawing the cached egui overlay. The world is not
   rendered again and the frame snapshot machine is not advanced twice.

This is **not** optical-flow frame generation, not a vendor neural network, and
not a claim of image-quality parity. Generated frames add presentation latency;
they work best when the real frame rate is already high. They are presented
back-to-back after the real frame with no display pacing timer and no XeLL /
Reflex-style latency control, so a vsynced compositor may drop some of them.

## Independent controls

| Setting | Persisted key | Default |
| --- | --- | --- |
| Upscaler | `upscaler` | `Off` |
| Upscaling quality | `upscale_quality` | `Quality` |
| Upscaler sharpening | `upscale_sharpness` | `0.5` |
| Frame generator | `frame_generation` | `Off` |
| Frame multiplier | `frame_gen_multiplier` (2–6) | `2` |
| Resolution scale (manual) | `resolution_scale` | `100%` |

**Upscaling and frame generation are completely independent.** Any frame
generator can be combined with any upscaler, including:

- DLSS Super Resolution + FSR 3 Frame Generation
- XeSS Super Resolution + DLSS Frame Generation (on RTX 40/50)
- FSR 4.1 + XeSS Frame Generation
- Frame generation with upscaling off (native-resolution FG)

`Auto` only picks a convenient default (the upscaler's own generator, or FSR FG
when upscaling is off); it never overrides an explicit choice. The manual
resolution-scale slider is used only when *both* selectors are `Off`; otherwise
the quality preset (or 100% for FG-only) determines the internal render
resolution. Status lines in the tab show the effective path, internal resolution
and any fallback.

Quality ladder: Native AA 100%, Quality 66.7% (1.5×), Balanced 58.8% (1.7×),
Performance 50% (2×), Ultra Performance 33.3% (3×).

## 2026 hardware support matrix

The gates are based on the vendors' published 2025–2026 requirements, reviewed
for this change. Cross-vendor support is deliberately *not* restricted to one
vendor where the vendor itself (or its SDK) opened a portable path.

| Upscaler | Official path | Compatibility path | Hard lock |
| --- | --- | --- | --- |
| FSR 2 / 3.1 | Open MIT source, any GPU | any hardware GPU | none |
| FSR 4.1 | FP8 on RDNA 4 / RX 9000; INT8 added for RDNA 3 / RX 7000 and RDNA 2 / RX 6000 by FSR SDK 2.3/2.4 in 2026 | INT8-style compatibility profile on other Shader Model 6.4 GPUs (the path community interop layers such as OptiScaler use; unofficial upstream, slower and more artifact-prone) | software adapters |
| DLSS Super Resolution | RTX Tensor cores (20-series and newer), including DLSS 4.5's 2nd-gen transformer SR | none | NVIDIA RTX |
| XeSS Super Resolution | XMX on Intel Arc / Core Ultra; official DP4a fallback on any SM 6.4 GPU | DP4a fallback on NVIDIA/AMD/other GPUs | SM 6.4 |

| Frame generator | Supported hardware | Frame multiplier ceiling |
| --- | --- | --- |
| FSR 3 Frame Generation | open/cross-vendor, any hardware GPU | 2× (one generated frame) |
| DLSS Frame Generation / MFG | RTX 40+ optical flow; DLSS 4.5 Dynamic/6× MFG is RTX 50 (Blackwell) only | 2× on RTX 40, up to 6× on RTX 50 |
| XeSS-FG / XeSS 3 MFG | XeSS SDK 2.1 opened XeSS-FG to any SM 6.4 GPU, not just Intel; XeSS 3 Multi Frame Generation stays Intel-only (Arc A/B, Core Ultra / Panther Lake) and requires XeLL | 2× on non-Intel, up to 4× on Intel XMX |

Notes that shaped the gates:

- **XeSS is not Intel-only.** XeSS SR has had a DP4a fallback for non-Intel GPUs
  since 1.x, and Intel's XeSS 2.1 SDK (2025) opened XeSS Frame Generation to
  AMD/NVIDIA GPUs with Shader Model 6.4 (GTX 10-series / RX 5000-series and
  newer; RTX 3000 / RX 6000 recommended). XMX remains Intel-only.
- **FSR 3 is not AMD-only.** FSR 2/3 and FSR Frame Generation are MIT-licensed
  and cross-vendor by design.
- **FSR 4.1 is not RDNA 4-only any more.** The FP8 model needs RDNA 4, but AMD
  shipped an INT8 model for RX 7000 (RDNA 3) and RX 6000 (RDNA 2) in 2026;
  community interop layers use that INT8 model on NVIDIA/Intel GPUs. The
  compatibility profile is labelled as such in the UI.
- DLSS is the one genuine vendor lock: Super Resolution needs Tensor cores, FG
  needs the optical-flow accelerator on RTX 40+, and 6× MFG needs Blackwell.
- Software adapters (lavapipe, SwiftShader, WARP) get none of the vendor modes;
  the viewer falls back to the legacy bilinear path.

### Shader Model 6.4 detection

wgpu 0.19's `DownlevelCapabilities::shader_model` only expresses SM 2/4/5, so
it cannot express SM 6.4 or `VK_KHR_shader_integer_dot_product`. `gpu.rs`
therefore derives a conservative `shader_model_64` flag from the backend and
adapter class: a hardware adapter on Vulkan, D3D12 or Metal is treated as SM
6.4-capable; GL/WebGPU/software adapters are not. This gates only the
DP4a/INT8 compatibility paths and is reported verbatim in the Graphics tab so
the user can see what was assumed.

### Fallbacks

If a requested upscaler is unavailable, the viewer falls back to FSR 3 (or FSR
2), then to bilinear. If a requested frame generator is unavailable, it falls
back to FSR FG, then to off. A multiplier above the technology's hardware
ceiling is clamped. **These fallbacks are independent** — a failed DLSS request
does not disable a separately selected FSR FG, and vice versa. Each fallback is
reported in the tab.

## Verification status

- `src/upscaling.rs` is dependency-free and has standalone unit tests covering
  the support matrix, cross-vendor combinations, untied selectors, MFG ceilings,
  fallbacks, software adapters and the Halton jitter bounds.
- `src/view_settings.rs` has unit tests for persistence, sanitisation and the
  independent selectors.
- The WGSL sources were parsed and validated with naga (via `naga-wasm`) and are
  checked for binding/layout agreement by `src/upscale_gpu.rs` unit tests.
- Full-app compilation and on-device visual verification require the normal
  Cargo/Windows build; do not treat the policy tests as proof that the vendor
  SDKs or their image quality are present.

## Sources reviewed (late 2026)

- Intel XeSS 2.1 SDK announcements for cross-vendor XeSS-FG / XeLL (KitGuru,
  Overclock3D, XDA, 2025).
- XeSS 3 Multi Frame Generation scope: Intel developer documentation coverage
  and 2026 reporting (FPSHQ XeSS 3 overview, WindowsForum analysis, 2026).
- AMD FSR SDK 2.3/2.4 and FSR 4.1 INT8 rollout (CGChannel, The Verge,
  KitGuru, Overclock3D, 2026).
- OptiScaler 0.9.4 release notes for the FSR 4.1.1 INT8 compatibility path and
  `Fsr4ForceEnableInt8` on non-AMD GPUs (GitHub, 2026).
- DLSS 4.5 SR/FG/MFG hardware scope (WCCFtech, BottleneckPC, 2026).

These are secondary sources; before claiming SDK-level compatibility or
redistributing vendor binaries, confirm against the primary Intel XeSS SDK, AMD
FidelityFX SDK/GPUOpen and NVIDIA NGX documentation and licensing terms.
