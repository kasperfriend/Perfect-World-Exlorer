# Blockers

Each entry: step, error, what was tried, workaround chosen. A blocker keeps
the viewer working with the affected provider disabled; it is never marked
done.

## B-1 — No Rust toolchain in the working environment (Phases 0–6)

- **Step:** Phase 0/1 — `cargo build --release`, `cargo test --bin perfect-world-explorer`.
- **Error:** `cargo: command not found`; no `rustup`; no `rustc`; apt is not
  usable (`Permission denied` on the package lists and the Debian mirrors are
  unreachable); the sandbox's outbound hosts are limited to github.com,
  codeload.github.com, api.github.com, registry.npmjs.org, pypi.org and
  files.pythonhosted.org — none of which serve a Rust toolchain.
- **Tried:** `find / -name cargo`, `apt-get update`, checking
  `~/.cargo`, `/usr/local/cargo`. Installed `web-tree-sitter` +
  `tree-sitter-rust` from the npm registry to get a real Rust *parser*
  (`tools/check-rust-syntax.js`), which catches syntax regressions in the
  files edited here; it is not a type checker and does not replace `cargo
  check`.
- **Workaround:** code is written against the existing source with surgical
  edits; every touched `.rs` file is run through the tree-sitter syntax
  check. Compile/test verification is deferred to the PR's CI (Linux job runs
  `cargo check --bins` + unit tests; Windows job runs
  `cargo build --release`). Local `cargo` results in the report are
  **not verified**.
- **Status:** open (workaround in place).

## B-2 — No GPU (Phases 1, 2, 3, 4, 6)

- **Step:** Phase 1 GPU checks (`water-pass-gpu-check`, `simple-water-gpu-check`,
  `post-gpu-check`, `display-gamma-gpu-check`, …), and any runtime verification
  of an upscaler/frame-gen provider.
- **Error:** neither `nvidia-smi` nor `/dev/dri` exists in this sandbox; no
  Vulkan/D3D12 adapter can be created locally.
- **Tried:** `nvidia-smi`, `ls /dev/dri` — both absent.
- **Workaround:** GPU checks are marked **not verified** locally; CI runs the
  existing suite headless on lavapipe. The new `dx12-upscale-check` and
  `dx12-framegen-check` diagnostics are written to succeed with no GPU (they
  print the support matrix and SDK probe report, exit 0), so CI exercises
  their logic paths without hardware.
- **Status:** open (needs a Windows RTX/AMD/Intel machine + the SDKs).

## B-3 — Vendor SDKs cannot be downloaded here (Phases 3, 4, 5)

- **Step:** Phase 3/4 — integrate NVIDIA DLSS/Streamline, AMD FSR, Intel XeSS
  SDKs; Phase 5 — redistribution terms.
- **Error:** the only reachable hosts are github.com, codeload.github.com,
  api.github.com, registry.npmjs.org, pypi.org, files.pythonhosted.org. Vendor
  developer portals (developer.nvidia.com, amd.com/gpuopen, intel.com) are not
  reachable. DLSS in particular is not published as a downloadable SDK on a
  reachable host.
- **Tried:** reachability is defined by the sandbox proxy; no fetch to vendor
  hosts was attempted beyond the allowlist (they are unreachable).
- **Workaround:** **no SDK binaries are committed** (they would also violate
  the packaging rule for non-redistributable DLLs). Instead:
  - `src/upscaling.rs` implements the full option state machine, pairing
    table, jitter sequence, per-provider internal resolutions and quality
    modes — all unit-tested logic, compiled into the viewer.
  - `src/upscale_sdk.rs` probes a user-selected SDK directory
    (`PW_UPSCALER_SDK_DIR`, the `upscale_sdk_dir` settings key, or
    `<exe>\sdk\`) for the documented DLL names and reports what it finds;
    every provider stays disabled with a visible reason until a real SDK is
    present.
  - The actual vendor FFI calls (`slDLSSInit`, `ffxFsr2ContextCreate`,
    `xessContextCreate`, …) are **not** implemented, because the SDKs could
    not be obtained, compiled against or executed anywhere in this
    environment — writing them untested would be fabrication, not
    integration. Selecting a provider whose SDK is present shows
    "SDK integration not available in this build" and the viewer falls back
    to the existing bilinear path.
  - Licenses/attribution are recorded in `THIRD_PARTY_NOTICES.md` from the
    vendor terms as published (with the requirement to re-check the license
    shipped inside the SDK you download).
- **Status:** open (needs the SDKs on a Windows machine; see README "Upscaling
and frame generation").

## B-4 — Native pass-by-pass DX12 rewrite (Phase 2)

- **Step:** Phase 2 — device/swapchain, descriptor heaps, root signatures,
  PSOs, command lists, fences, barriers, upload path, and the terrain /
  static / SpeedTree / water / post / UI passes as native D3D12.
- **Error:** the change touches ~20 GPU modules and ~90 diagnostic binaries
  and cannot be compiled or tested here (B-1, B-2). A rewrite that "mostly
  looks right" but does not compile would break `main` for every user and is
  worse than the current `wgpu` path, which already runs on the D3D12 API on
  Windows through wgpu-hal (naga → HLSL → DXIL/DXBC).
- **Tried:** evaluated a from-scratch `d3d12`/`windows-rs` port — rejected:
  the crates are unreachable (crates.io is not an allowed host), so even the
  *binding* layer cannot be added to `Cargo.toml`.
- **Workaround:** §6 of `docs/dx12-port-notes.md` — DX12 is made the explicit
  default backend on Windows through wgpu; the native port is specified in
  §7 of that document but deliberately not attempted.
- **Status:** open (blocked on toolchain + GPU + Windows; crates.io
  unreachable).

## B-5 — Motion vectors / jitter / depth-in-SDK-format (Phase 3.14)

- **Step:** Phase 3.14 — enable jitter in the projection matrix, output motion
  vectors for all moving geometry (incl. Walker and Populator actors), write
  depth in the SDK-required format.
- **Error:** requires touching the shared projection used by every pass and
  every vertex shader, plus per-pass velocity outputs (skinning path included)
  and depth-format changes. Cannot be compiled or visually verified (B-1, B-2),
  and any mistake is a full render breakage, not a partial failure.
- **Tried:** scoped the change to: a unit-tested jitter sequence + matrix
  helper (`src/upscaling.rs::jitter_*`) and a documented target/format
  requirement in `docs/dx12-port-notes.md` §7. The projection and shaders are
  **not** modified.
- **Workaround:** jitter/MV exist as tested pure functions consumed by the
  diagnostics; production projection unchanged. Upscalers are disabled in UI
  until an SDK is present anyway (B-3).
- **Status:** open.

## B-6 — Open PR overlap (Phase 0.3)

- **Step:** Phase 0.3 — read the open PR and avoid duplicating it.
- **Finding:** **two** PRs are open. `#38` (Teleport List worlds; unrelated).
  `#37` "Graphics: FSR1 upscaling and full vendor option lists (SDKs pending)"
  adds `src/upscaling.rs`, `src/render_scale.rs` changes, an FSR1 EASU/RCAS
  WGSL pass, Graphics-tab lists for DLSS/FSR1/FSR2/FSR3/FSR4.1/XeSS and an
  independent frame-generation list, `THIRD_PARTY_NOTICES.md`, a README
  section and `GRAPHICS_TECHNOLOGIES.md`. It is **not** merged; this branch is
  cut from `main` and therefore does not contain any of it.
- **Impact:** my implementation also creates `src/upscaling.rs` (provider
  state machine, pairing table, jitter) plus `src/upscale_sdk.rs` (SDK
  discovery) — #37 creates `src/upscaling.rs` too (option lists + worker
  plumbing), so that file conflicts textually, as expected for two PRs
  building the same feature from `main`. The other conflicting files:
  `README.md` (license section + graphics bullet),
  `tools/package-windows.ps1`, `src/gpu.rs` (Graphics tab),
  `src/view_settings.rs` (settings keys), `THIRD_PARTY_NOTICES.md` (same
  path, different scope). `.github/workflows/pr-checks.yml` is **not**
  touched by this branch. Resolving the overlap means choosing one Graphics
  tab implementation; both must be reconciled by the maintainer before
  either ships. This branch deliberately does **not** contain #37's FSR1
  EASU/RCAS pass (someone else's reviewable work) and does not pretend its
  vendor SDKs either — this is the SDK-integration follow-up to #37, not a
  duplicate of it.
- **Status:** open (PRs must be rebased/merged in order by the maintainer).

## B-7 — Frame pacing / UI input rate (Phase 4.21)

- **Step:** Phase 4.21 — with FG on, present frames in order and never drop
  the UI input rate; test with the diagnostics frame counter.
- **Error:** requires a working frame-generation provider and present loop
  (B-3) plus a Windows GPU (B-2). No FG presents frames here, so there is
  nothing to pace.
- **Tried:** diagnostics `TimingFrame`/`EffectFrames` channels exist
  (`src/diagnostics.rs`) and are the right probe; the pacing contract is
  documented for the implementer in `README.md` and
  `docs/dx12-port-notes.md` §7. The `dx12-framegen-check` diagnostic prints
  the pacing expectations it would enforce.
- **Status:** open (blocked by B-2/B-3/B-4).

## B-8 — Visual parity against the shipped client (Phases 1.7, 6.32)

- **Step:** Phase 1.7 — capture reference frames at fixed camera positions for
  terrain, SpeedTree, water, sky, Walker and Populator actors; Phase 6.32 —
  compare against the port.
- **Error:** no game client is available in the environment (and none may be
  committed), and there is no GPU (B-2). Reference frames therefore cannot be
  captured or compared.
- **Workaround:** parity is **not measured**. The unchanged rendering path is
  preserved by construction (every pixel pipeline in the world/UI chain is
  byte-identical when `Upscaling = Off`; only the Windows backend *selection*
  changes), but no frame-level evidence exists.
- **Status:** open (needs client + Windows GPU).
