# Graphics technologies: implementation and distribution status

This is the status of the shipped viewer, **not** a list of aspirational menu
options. An unavailable item is disabled rather than quietly running bilinear
scaling under a vendor name.

| Technology | In this tree | What runs |
| --- | --- | --- |
| Resolution scale + bilinear | Yes; default | Existing 10–200% world render and bilinear present. UI, gamma and present encoding stay native. |
| AMD FSR **1** (spatial) | Yes; opt-in | WGSL implementation of AMD's MIT-licensed EASU then RCAS, on the completed world image at 50–99% scale (up to 4x area). Outside that range, the existing bilinear pass runs instead. |
| NVIDIA DLSS SR / DLAA, FG / MFG, RR and other Streamline plugins | **No** | Disabled; no NVIDIA plugin loaded or redistributed. RR also needs a ray-traced input this viewer does not produce. |
| AMD FSR 2/3/4 SDK temporal/ML upscaling, FG, Anti-Lag and other SDK techniques | **No** | Disabled; not the same as this implementation of FSR 1. No AMD SDK components loaded or redistributed. |
| Intel XeSS SR, FG, XeLL | **No** | Disabled; no Intel SDK component loaded or redistributed. |

The Graphics tab places **Upscaling** next to Resolution scale and shows a
separate **Frame Generation** section. The FG entries remain disabled. There is
no frame interpolation, no generated frame in the swapchain, and no support yet
for combinations such as DLSS upscaling with AMD frame generation. Their
absence is explicit in the UI and persisted settings (unknown upscaler tokens
fall back to the legacy bilinear choice). Merely adding a setting, identifying a
GPU, or installing a vendor DLL would not implement the technology.

## Why further work is required

The renderer currently uses wgpu 0.19, a wgpu-owned surface and sRGB
composition. It does not produce the full-resolution per-object motion vectors,
jittered projection, stable previous-frame poses, depth conventions, reactive
and transparency masks, exposure, or history invalidations needed by temporal
upscalers. These inputs must be correct for *every* terrain, sky, water,
vegetation, animation and world-effect pass, including reflections and
teleports. Even a correct native SDK call would produce artifacts with missing
motion/history data.

Frame generation is a **separate** stage after a resolved, native-resolution
world image. It needs vendor-compatible device/queue/resource interop,
optical-flow inputs as required by the chosen SDK, swapchain replacement or
present scheduling, UI compositing without interpolating menus and cursors,
frame pacing, device-loss recovery and latency controls. The current
`surface.get_current_texture()` / `frame.present()` loop presents exactly one
real frame. It does not expose the native swapchain control required by these
SDKs. These are renderer/backend changes, not adjustments to the FSR 1 shader.
Separate upscaler and FG selection is a valid architectural goal, but no SDK
combination should be selectable until both implementations are proven.

## Licensing and release

[LICENSE](LICENSE) remains the **unmodified** GPL-2.0 text; this repository is
marked GPL-2.0-or-later. The FSR 1 port keeps AMD's MIT notice in
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md); Windows packages now include
both notices. The open [FSR 1 source](https://github.com/GPUOpen-Effects/FidelityFX-FSR)
should not be confused with the [newer AMD FSR SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK),
whose license is **file-specific** and whose general license includes
binary-only/no-reverse-engineering terms. [NVIDIA Streamline](https://github.com/NVIDIA-RTX/Streamline)
has open code but DLSS-G uses prebuilt plugins. [Intel XeSS](https://github.com/intel/xess)
distributes SDK software under Intel's separate, binary-only terms. No
proprietary binaries are checked in or packaged. The current GPL cannot simply
be rewritten to grant rights in someone else's code; before shipping any
binary-only SDK or combined executable, obtain rights/approval and a legal
compatibility review for the exact SDK artifacts and distribution method.

## Verification

- `cargo test --locked --bin perfect-world-explorer upscaling` checks settings
  migration, valid mode parsing and FSR 1 scaling eligibility.
- `cargo run --locked --bin fsr1-gpu-check` runs a synthetic image through the
  **production** EASU + RCAS pipelines on a working wgpu adapter. It checks
  color neutrality, edge response, original bilinear fallback and resize.
- These checks do **not** establish DLSS/FSR SDK/XeSS support, visual quality
  in the retail world, or hardware and driver compatibility on Windows.
