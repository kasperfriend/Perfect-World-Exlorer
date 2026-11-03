# Phase 4-C — vegetation materials, authored mips, and CI

## Scope and preservation decision

This change is limited to vegetation texture materials and regression wiring. It does not change tree geometry, leaf animation, grass placement, or grass-wind internals. The existing tree render path remains available and is still the default.

`ViewSettings::vegetation_authored_mips` defaults to `false`. Its UI tooltip says restart is required. The startup value is captured once and passed through the initial and background `build` calls and `Renderer::new`; changing the setting in a running viewer cannot switch the current renderer or a later background rebuild. The regional-tree experiment remains an independent setting. When the authored-mip switch is off, tree atlas generation, the shared world sampler, grass coverage-preserving mip generation, cutouts, and reflection draw submission retain their previous behavior. The original `image` decode still supplies level 0; the new DDS decoder only reads lower levels.

The opt-in experiment uploads authored lower mip levels where the atlas can isolate them, but deliberately retains the current viewer sampler and legacy tree cutout. These are not presented as recovered native values. Unsupported native sampler tuples and tree cutout thresholds are reported explicitly.

## Sampler / mip / cutout policy

| Material | Native sampler evidence | Current viewer sampler (kept in experiment) | Mip handling | Alpha/cutout |
|---|---|---|---|---|
| Bark | Per-material MAG/MIN/MIP, address, and anisotropy tuple **not recovered**. Generic material setup is descriptor-driven (`0x796966`, `0x796e22`); no vegetation-specific anisotropy writer was found. | Linear mag/min/mip; clamp address; anisotropy 16. | Decode authored DDS levels 1–7 and upload without filtering when dimensions exactly match the rectangular tree-atlas slot. Unsupported resized/mismatched chains use the old generated path. | Runtime SpeedTree threshold comes from SGeometry `+0x38`; it is absent from PWTREE cache. Keep legacy shader threshold 0.5 and report the gap. |
| Branch | Same explicit unknown sampler tuple as bark. | Linear mag/min/mip; clamp; anisotropy 16. | Same as bark. | Runtime threshold from SGeometry `+0x38` (`0x656bf5..0x656c01`); legacy 0.5 retained, native pair unsupported. |
| Frond | Same explicit unknown sampler tuple as bark. | Linear mag/min/mip; clamp; anisotropy 16. | Same as bark. | Runtime threshold from SGeometry `+0x38` (`0x656bf5..0x656c01`); legacy 0.5 retained, native pair unsupported. |
| Leaf/composite | Same explicit unknown sampler tuple as bark. | Linear mag/min/mip; clamp; anisotropy 16. | Same as bark. | Runtime threshold from SGeometry `+0x74` (`0x656d3e..0x656d4a`); legacy 0.5 retained, native pair unsupported. |
| Grass | MAG/MIN/MIP and anisotropy descriptor unknown. Native addressing evidence: stage 0 U/V WRAP, stages 1/2 CLAMP (`0x65127a..0x65129c`). Current world atlas uses the shared clamped 16x sampler, so the native-vs-viewer address/sampler combination remains explicitly unsupported. | Shared world sampler: linear mag/min/mip; clamp; anisotropy 16. | Upload authored levels 1–7 only for a square, exact-size atlas slot; rectangular or resampled chains fall back to the existing coverage-preserving generator and emit a warning. | **GREATER, alpha reference 84** (`0x652129`, `0x652361..0x652371`), confirmed exact. The shader's existing alpha value remains 84/255. |

**Atlas limit:** tree and grass atlas textures expose LODs 0–7. Levels deeper than LOD 7 are retained in the decoded cache but cannot be isolated per packed tile because the minimum 128-pixel tile reaches one texel at LOD 7. The current atlas cannot preserve those tail levels without cross-tile collisions. Any such tail is logged, not silently described as uploaded.

**Authored-chain basis:** the `trees.pck` census found 127/127 vegetation DDS files with complete authored chains (all DXT1/DXT3); `grasses.pck` found 105/105 DDS chains (DXT3). `textures.pck` was over the Drive connector's single-file transfer limit; that archive is not used for tree/grass texture loading here.

## Preservation proof and diagnostics

- Default setting is false and has a round-trip/default test in `src/view_settings.rs`.
- The default tree path continues to generate the atlas mips and use the existing 16x sampler. The default grass path uses the same existing alpha-coverage generator. The viewer's legacy tree alpha threshold stays 0.5; grass stays 84/255.
- The regional tree-lighting switch and reflection submission are untouched by this change.
- `tree-lod-gpu-check` remains the baseline synthetic regression for LOD transitions, wind/time variation, regional RGB, and default-lighting preservation.
- `grass-instance-gpu-check` remains wired for the integrated grass instance path (including placement/wind behavior).
- `grass-gpu-mip-check` is extended to read back and compare the exact authored LOD-7 payload in addition to its existing independent-tile/coverage checks.
- `tree-authored-mip-gpu-check` exercises the opt-in `FarTrees::new(..., authored_mips=true)` upload path with synthetic authored chains. The asset-dependent `tree-gpu-mip-check` remains excluded with a named reason because CI has no user `trees.pck` and baked PWTREE cache.
- `vegetation_material.rs` tests the GREATER/84 boundary, default sampler/cutout values, generated-vs-authored distinction, runtime tree-cutout rejection, and explicit sampler gaps. The module is included in both dependency-free core-test loops.

## CI and local verification

| Check | Command | Result / run identity |
|---|---|---|
| Phase status audits | `python3 tools/test_phase1_ready.py` | Passed (7 tests) |
| Phase status audits | `python3 tools/test_eight_phase_status.py` | Passed (5 tests) |
| Phase status consistency | `python3 tools/check-eight-phase-status.py` | Passed; no errors |
| GPU diagnostic audit | `python3 tools/audit-static-model-gpu-programs.py` | Passed: 10 wired, 15 excluded with reasons, 25 total |
| Shell syntax / whitespace | `bash -n tools/check-phase1-core.sh`; `git diff --check` | Passed |
| Rust/GPU CI | PR #17, run `41656708248` — https://github.com/kasperfriend/Perfect-World-Exlorer/actions/runs/41656708248 | **Passed**. Linux job `125988836765`; Windows job `125988837508`. |

Successful CI commands/results:

- Linux job `125988836765`: `cargo fmt --all -- --check`; `cargo check --locked --bins`; `cargo test --locked --bin gfx-light-audit`; `cargo test --locked --bin perfect-world-explorer`; Python status/audit commands above; and all configured GPU checks. The Phase 4 GPU commands were `cargo run --locked --bin tree-lod-gpu-check`, `cargo run --locked --bin grass-gpu-mip-check`, and `cargo run --locked --bin tree-authored-mip-gpu-check`; all passed in the green run.
- Windows job `125988837508`: the nine existing standalone CPU tests plus `vegetation_material` passed; `cargo build --locked --release --target x86_64-pc-windows-msvc --bin perfect-world-explorer` and the `bmd-overlap-survey` release build passed; artifact staging/checksum passed.

The sandbox has no local `cargo`, `rustc`, or `rustfmt`; Rust compilation, Rust tests, and GPU checks were verified by the Linux/Windows CI run.

## Exact changed files

- `.github/workflows/pr-checks.yml`
- `Findings/Phase-4-C-vegetation-materials.md`
- `src/bin/grass-check.rs`
- `src/bin/grass-gpu-mip-check.rs`
- `src/bin/grass-texture-check.rs`
- `src/bin/tree-authored-mip-gpu-check.rs`
- `src/bin/tree-gpu-mip-check.rs`
- `src/bin/tree-lod-gpu-check.rs`
- `src/bin/tree-motion-check.rs`
- `src/bin/tree-texture-check.rs`
- `src/bin/windows-tree-setup-check.rs`
- `src/far_trees_gpu.rs`
- `src/gpu.rs`
- `src/grass.rs`
- `src/grass_mips.rs`
- `src/main.rs`
- `src/trees.rs`
- `src/vegetation_material.rs`
- `src/vegetation_textures.rs`
- `src/view_settings.rs`
- `tools/audit-static-model-gpu-programs.py`
- `tools/check-phase1-core.sh`
- `tools/static-model-gpu-programs.txt`

## Remaining gaps

1. Native material sampler tuples (MAG/MIN/MIP, per-stage addressing, anisotropy) are not retained in the vegetation code path; the viewer sampler is preserved rather than claimed as an exact match.
2. Tree alpha references are runtime SpeedTreeRT SGeometry fields and were not serialized by PWTREE05/06/07. The corresponding DLL bake/probe cannot be run in this sandbox (no Wine or MinGW toolchain), so tree thresholds remain explicitly unsupported.
3. Grass is packed into square atlas tiles and shares the world sampler. Rectangular/resampled grass authored chains cannot be represented faithfully in this atlas and use generated fallback mips.
4. Atlas LODs beyond 7 cannot be kept isolated per tile; deeper authored tail levels remain cached but are not uploaded.
5. Tree regional lighting remains the existing opt-in approximation; native per-material lighting constants are not recovered. Reflection submission was left unchanged and has no new standalone vegetation-reflection GPU diagnostic.
