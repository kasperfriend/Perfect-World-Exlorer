# Third-Party Notices

Perfect World Explorer is licensed **GPL-2.0-or-later** (see [LICENSE](LICENSE)).

This file covers the **optional** GPU-upscaling and frame-generation SDK
technologies that Settings → Graphics can use. **None of the vendor SDK
binaries are distributed with this repository or with the viewer packages.**
Every option is disabled with a visible reason until you supply the SDKs
yourself; see [README](README.md#upscaling-and-frame-generation-optional) and
[docs/blockers.md](docs/blockers.md).

Versions below are the ones this build was documented against at the time of
writing. They were **not** downloaded, compiled against or executed in the
build environment. When you download an SDK you get whatever version the
vendor serves; check the license file that comes **inside** your download —
it, not this page, is authoritative for your copy.

## How to supply the SDKs

1. Obtain the SDK from the vendor (links below) and accept its license.
2. Copy the relevant DLLs into **one** of:
   - `sdk\` next to `Perfect-World-Explorer.exe`,
   - the folder named by the `PW_UPSCALER_SDK_DIR` environment variable,
   - the folder set in `viewer-settings.txt` as `upscale_sdk_dir=<path>`
     (the Graphics tab has no text box for it yet; edit the file while the
     viewer is closed). Restart the viewer afterwards.
3. Run `dx12-upscale-check --sdk-dir <path>` and
   `dx12-framegen-check --sdk-dir <path>` to see what was detected.
4. `.gitignore` ignores `*.dll` and `/sdk/`; never commit vendor binaries.
   `tools/package-windows.ps1` stages only the redistributable set described
   below.

Expected file names probed (names vary between SDK releases; the diagnostics
print exactly which names were found):

| SDK component | Probed file name(s) |
|---|---|
| DLSS Super Resolution | `nvngx_dlss.dll` |
| DLSS Frame Generation | `nvngx_dlssg.dll` |
| NVIDIA Streamline (host) | `sl.interposer.dll` |
| AMD FSR API loader (DX12) | `amd_fidelityfx_loader_dx12.dll` |
| AMD FSR upscaler (FSR Upscaling 4 / FSR 3 / FSR 2) | `amd_fidelityfx_upscaler_dx12.dll` |
| AMD FSR Frame Generation (FG 4 / FG Swapchain 4 / FSR 3 FG) | `amd_fidelityfx_framegeneration_dx12.dll` |
| Intel XeSS | `libxess.dll`, `libxess_d3d12.dll` |
| Intel XeSS 2 Frame Generation | `libxess_fg.dll` |

## NVIDIA DLSS Super Resolution / DLSS Frame Generation

- **What it is:** NVIDIA's temporal neural upscalers and frame interpolator
  for RTX GPUs. DLSS Super Resolution raises internal render resolution; DLAA
  runs DLSS at native resolution; DLSS Frame Generation synthesises a
  presented frame between each rendered frame.
- **License:** proprietary — the *NVIDIA DLSS SDK* is distributed to
  registered developers and is governed by the NVIDIA Software License
  Agreement / DLSS SDK license terms included with your download. It is not
  open source.
- **Redistribution:** historically, NVIDIA's DLSS terms permit redistribution
  of the unmodified runtime DLLs (`nvngx_dlss.dll`, `nvngx_dlssg.dll`) as part
  of an application that uses the SDK. **This package does not include them.**
  Do not copy them into a release without reading the license in your SDK
  download; if your license does not grant you redistribution rights, users
  must supply the DLLs themselves. `tools/package-windows.ps1` never stages
  NVIDIA proprietary binaries (it refuses to package any `nvngx*.dll` or
  `sl.*.dll`).
- **Attribution:** "NVIDIA", "DLSS", "DLSS Super Resolution", "DLSS Frame
  Generation", "DLAA" and the NVIDIA logo are trademarks and/or registered
  trademarks of NVIDIA Corporation. "DLSS" technology is powered by NVIDIA RTX.
- **Where to get it:** <https://developer.nvidia.com/rtx/dlss> (developer
  registration required). `sl.interposer.dll` et al. come from the Streamline
  SDK: <https://github.com/NVIDIA/Streamline>.
- **Version documented against:** DLSS SDK 3.x with Streamline 2.x (DLSS SR +
  DLSS-G, RTX 40 series required for frame generation). Unverified in this
  build environment.

## AMD FidelityFX SDK — FSR (upscaling) and FSR Frame Generation

- **What it is:** AMD's open-source upscaler and frame-interpolation
  technology, part of the AMD FSR SDK. One DX12 upscaler DLL provides AMD
  FSR Upscaling 4 (4.1.2), FSR 3.1.6 and FSR 2; one DX12 frame-generation
  DLL provides FSR Frame Generation 4.0.2, FSR Frame Generation (Swapchain)
  4.0.0 and FSR 3 frame generation (3.1.7). Works on AMD, NVIDIA and Intel
  GPUs. (FSR 1 is EASU/RCAS shaders, not a DLL; it is not part of this SDK.)
- **License:** MIT License.
  Copyright © 2021–2024 Advanced Micro Devices, Inc.
  The license text ships in the SDK archive root (`3rdpartynotice.md`) and
  permits use, modification and redistribution, including in binary form,
  provided the copyright and permission notice are preserved.
- **Redistribution:** permitted by the MIT license with the copyright notice
  included. This package does not include the binaries by default. If you
  package them yourself, keep this notice and the AMD copyright line. The
  packaging script can stage **only** the MIT-licensed AMD and Intel DLLs
  (see below); it is not possible to stage NVIDIA binaries with it. The DLLs
  are signed by AMD and ship prebuilt inside the SDK (no build step is
  needed for the effects themselves; the samples and media are what need
  building). Effect DLLs are dispatched through the loader DLL
  (`amd_fidelityfx_loader_dx12.dll`); ship all three together.
- **Attribution:** "FidelityFX", "FSR", "FidelityFX Super Resolution" are
  trademarks of Advanced Micro Devices, Inc.
- **Where to get it:** <https://gpuopen.com/fidelityfx-sdk/> and
  <https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK> (minimal
  packaged release, e.g. `v2.4.0.zip`). The DLLs live under
  `Kits/FidelityFX/signedbin/` inside the SDK archive.
- **Version documented against:** AMD FSR SDK v2.4.0 (FSR Upscaling 4.1.2,
  FSR 3.1.6 upscaling, FSR Frame Generation 4.0.2 / Swapchain 4.0.0 / FSR 3
  FG 3.1.7, `amd_fidelityfx_*_dx12.dll`). Not verified on hardware in this
  build environment. Earlier SDK 1.x DLL names (`ffx_*.dll`) are deprecated
  by AMD and are no longer probed.

## Intel XeSS / XeSS 2

- **What it is:** Intel's temporal upscaler. Native on Intel Arc; runs on
  other GPUs through a DP4a fallback path. XeSS 2 adds XeSS Frame Generation
  (`libxess_fg.dll`).
- **License:** MIT License.
  Copyright © 2022–2024 Intel Corporation.
  The full license text ships in the XeSS repository (`LICENSE`) and permits
  redistribution with the copyright and permission notice preserved.
- **Redistribution:** permitted by the MIT license with notice. This package
  does not include the binaries by default; the packaging script can stage
  `libxess*.dll` when pointed at a user-supplied SDK folder.
- **Attribution:** "XeSS" and "Intel Arc" are trademarks of Intel Corporation.
  Intel XeSS SR technology.
- **Where to get it:** <https://www.intel.com/content/www/us/en/developer/topic/technology/xess.html>
  and <https://github.com/GameTechDev/xess> (see its `LICENSE` file).
- **Version documented against:** XeSS 2.0.x. Unverified in this build
  environment. XeSS has no Ultra Performance mode; the Graphics tab disables
  that choice for XeSS. XeSS Frame Generation is only exposed with XeSS
  upscaling or none — cross-vendor frame-generation pairings are not
  documented by Intel and are not offered.

## Everything else

- The viewer's own code: GPL-2.0-or-later. This project is not affiliated
  with or endorsed by NVIDIA, AMD, Intel or Perfect World.
- Rust dependencies: see `Cargo.lock`; most are permissively licensed
  (MIT/Apache-2.0/BSD/Zlib). Build-time tooling (DXC/V-slim toolchain if you
  add one for a native port) is not part of this repository.
- The Windows CI runner uses Mesa lavapipe for headless GPU checks; it is not
  distributed with the viewer.
- The game client, its DLLs, archives, extracted assets and derived caches
  are **not** included in, or redistributable as part of, this project.
- `Modules/Walker` and `Modules/Populator` are part of this project (GPL-2.0).
