# Eight-phase source and validation reassessment — 2 November 2026

Audited code revision: `a9c4b35e1086067247ffc25a7e7755a0111124c2`.
Original requirement text/IDs are preserved in [Eight-phase status](Eight-phase-status.md).
This audit covers **all 76 points**, not just Phase 1. It corrects both stale
“not implemented” claims and the blanket unchecked state of later test/preservation/delivery tasks.

## Meaning of decisions

- **Implemented**: the stated requirement's deliverable exists. For test, preservation,
  scope, packaging and tracking tasks this is NOT a claim that the subsystem is fully native-equivalent.
- **Partial**: working implementation/evidence exists, but the original complete requirement
  still has the specific gap listed. These remain unchecked, not described as absent.
- **Not implemented**: no implementation of the requested behavior was established.
- No percentage is assigned. **No phase is fully complete.** Phase 1's ten implementation
  gates remain No; P1.11/P1.12 remain Yes. Later work already present is recognized,
  but this reassessment does not advance execution past the Phase 1 gate.

## Verification and limitations

- Re-read production modules, call sites, defaults, test bodies and CI workflow, not just research headings.
- Verified [CI 41478115664](https://github.com/kasperfriend/Perfect-World-Exlorer/actions/runs/41478115664)
  **SUCCESS** at the audited revision: all-bin compilation, viewer unit tests,
  13 production GPU programs, native numeric reference checks and Windows x64 build.
- Verified non-expired `Perfect-World-Explorer-Windows-x64` and `NativeGammaX87Words`
  artifacts attached to that run. This proves build/artifact production, not fresh-PC or physical-device runtime acceptance.
- Re-ran **109 dependency-free core tests**, **10 additional tests** in terrain_seam,
  tree_lod, grass_wind, grass_random and texture_atlas, and **7 readiness-auditor tests** locally.
- Added and ran **5 eight-phase consistency tests**; the new checker verifies all 76 decisions, checkbox agreement and existence of 67 cited evidence files. It validates records, not renderer parity.
- Named diagnostics that are present but absent from the current workflow are NOT
  represented as freshly executed. No full asset redownload, live-client image comparison,
  native Windows audio/DPI/device test or new full-world performance benchmark was performed.
- Private asset census results cited in Findings are retained historical evidence, not
  fresh asset scans in this audit. Successful compilation is not runtime integration.

## Summary

| Phase | Implemented requirements | Partial | Not implemented | Full phase complete? |
|---|---:|---:|---:|---|
| 1 | 9 | 3 | 0 | No |
| 2 | 3 | 6 | 0 | No |
| 3 | 0 | 9 | 0 | No |
| 4 | 2 | 7 | 0 | No |
| 5 | 0 | 8 | 1 | No |
| 6 | 1 | 8 | 0 | No |
| 7 | 2 | 7 | 0 | No |
| 8 | 3 | 7 | 0 | No |

**Total: 20 implemented requirement deliverables, 55 partial, 1 not implemented.** These counts are not a parity score.

## Phase 1

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P1.01 | Implemented | Static grid, source-order ties, bounded dynamic publications, night scaling and controller-108 runtime (`src/scene_lights.rs`, `src/gfx_light.rs`, `src/gfx_color_track.rs`). Producer lifecycle, ownership, hook-parent resolution, region unload, destruction and publication ordering are implemented and executed in the production path (`src/gfx_owner.rs`, `src/world_models_gpu.rs`, `src/model_attachment.rs`, `src/model_asset.rs`), including the manager-wide priority gate (`0x70ba3c..0x70ba44`, limit `+0x134`) and its persistence across unload. Construction precision is resolved to PC24: the executable creates no thread and never writes the x87 control word, so the `0x6ec5dd` table is built in the same device context that samples it. | Recorded limitation: the authored byte that sets the manager limit is not decoded, so production keeps the limit at "no suppression" (the client at its highest light setting) and exposes `set_light_priority_limit` for a decoded source; `ChildHook`/`RelHook`/`BindParent` hook chains and static `ECMHook` hangers belong to the visual-effect graph (P7.04–P7.05). |
| P1.02 | Implemented | Ordinary weighted/rigid receivers drive the production submission with the four recovered shader variants, per-variant constants, attenuation floor and c8.a rules; queue inputs are split the way `0x7a5830` reads them (model instance supplies invisible/`+0xc`/`+0x2a4`/`+0x2a8`, mesh texture record supplies the `+0x36`/`+0x35` flags) and non-finite alpha fails closed (`src/model_receiver_class.rs`, `src/model_queue.rs`, `src/model_draw_gpu.rs`). Explicit exclusions for baked BMD, terrain, grass and tree; morph/supple rejected and proven absent from the retail corpus (13,820 skin + 1,988 rigid, zero morph/supple/muscle); fixed-function lighting proven unused by receivers (`0x77a3c0(0)` at `0x7a914b`/`0x7a9759`, restore at `0x7a96a3`/`0x7a99f9`); alpha-list order per `0x7a5a1d-0x7a5aef`. | None outstanding for the classes the inspected client draws; morph/supple remain fail-closed guards for non-retail data. |
| P1.03 | Implemented | Ordinary and alpha model queues submit with the recovered blend/alpha-test/depth/order states, runtime FX publications feed production receivers, and main/reflection/clipped refraction reuse one lighting/playback snapshot (`src/world_models_gpu.rs`, `src/model_draw_gpu.rs`, `src/gpu.rs`). The special queue question is closed by evidence: the only third state block `0x41a2f0` is the state callback of the dynamic-geometry submitter `0x416ab0`, reached from `0x41a5a0` whose sole caller `0x6d17ad` is in the effect-record path, so receiving models have exactly two queues (`SPECIAL_STATE_BLOCK`). | Receiving-model scope complete; the effect-sprite block itself belongs to the visual-effect graph (P7). |
| P1.04 | Implemented | Native regional selection/FIFO, stored transitions, quantized ambient, asymmetric sun/moon diffuse/specular, default-area visibility and shared capped tick (`src/environment.rs`, `src/client_regional_light.rs`, `src/client_sun.rs`). Consumers enumerated from `0x79b390`'s 16 call sites: the environment installs only from `0x41e756` (`0x41f29b`/`0x41fdb1`/`0x420db0`); the remaining 13 are point/model/grass lights. | None outstanding for this point; point-light classes belong to P1.02. |
| P1.05 | Implemented | Sky azimuth `0x4b60b0` has exactly one caller (`0x41f2e6`) and elevation `0x4b60e0` exactly two (`0x4b5f5e`, `0x4b607b`, both in the sky object); twelve curve constants match `client_sun.rs` bit-for-bit. Ordinary models keep authored direction. | None outstanding. |
| P1.06 | Implemented | Terrain/grass day-night maps, rectangular mapping, CLAMP, area-center grass selection, independent availability (`src/terrain_lightmaps.rs`, `src/terrain_gpu.rs`, `src/gpu.rs`), now validated against retail `world_8.t2lm`/`.t2lm1`. `ApplyLightMap` has 4 call sites in one function, so no residency manager exists to mirror. | Recorded deviation only: one eager 4096² atlas instead of per-page upload; sampling state is equivalent. |
| P1.07 | Implemented | Integer baked interpolation, two-sided fields, opaque descriptor scaling (`src/bmd.rs`, `src/world_objects.rs`, `src/gpu.rs`), now checked against 28,858 retail BMDs: identity v5 materials, one stage configuration at `0x7a9730`, 2,579 two-sided meshes, 59,984 differing day/night arrays. | Cutout threshold 0.5 not separately re-derived; every retail descriptor alpha is 1.0, so no observed case depends on it. |
| P1.08 | Implemented | The whole `CECFullGlowRender` chain is enumerated from its own `LoadResource` strings (17 resources, `0x94ed3c`-`0x94f258`) and every program in it is present in the pack and driven by the viewer: `fullglow1_14`, `fullglow2_14` (with `def c4` mono constants), `monofullglow_14`, `fullsharpen_14`, `spacewarp`. `spacewarp_tl` is shown to be a load-time duplicate of `spacewarp` (identical `def` constants and offset/texkill sequence; differs only in projected vs packed texcoord), so it is not a missing pass. `c2.r = 0.05` in `spacewarp` is the `texkill` threshold, not the warp offset, and that `texkill` was missing and is now implemented (`src/post_gpu.rs`, `Findings/Post-process-pass-and-caller-audit.md`). `dropshadow_14`, `shadowrenderblur_14` and `solarflare_14` are loaded by `CECShadowRender`/`CECSunMoon` and font text, not by the main-frame chain, so they belong to other requirements. | Coverage requirement met. Recorded residual, not an unclosed gate: the warp stream's per-vertex offset scale is built at run time and remains approximated. |
| P1.09 | Partial | Settings/mode gating, whole-frame post-UI ramp, output encoding and all 25,856 native PC24 gamma words (`src/display_gamma.rs`, `src/gamma_curve.rs`). | Remaining upstream material color conversions and Windows presentation/compositor acceptance. |
| P1.10 | Partial | Production material fog and distinct grass/BMD/terrain refraction equations (`src/refraction_fog.rs`, `src/terrain_gpu.rs`, `src/gpu.rs`). | All material variants, transparency and complete underwater/pass ordering coverage. |
| P1.11 | Implemented | Approved default tree geometry/lighting retained; incomplete replacement remains opt-in (`src/far_trees_gpu.rs`, `src/view_settings.rs`). | Preservation requirement met; full native tree materials belong to P4.01. |
| P1.12 | Implemented | CPU and 13 CI GPU programs cover selection, transitions, material arithmetic, color and postprocess categories (`tools/check-phase1-core.sh`, `.github/workflows/pr-checks.yml`). | Test-category requirement met; tests do not certify the ten unfinished implementation gates. |

## Phase 2

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P2.01 | Implemented | Height blocks and T2MK areas load from the world stream and the draw set follows `0x77ecf0`/`0x77e1b0` — the `(2r+1)`-block square around the camera's block, edge-shifted and re-selected every frame. The residency lifecycle of `0x77e250` is implemented in `src/terrain_residency.rs`: a block enters the resident set with the window and is meshed once at the level `0x77e360` picked (`0x7c1140` builds the full-resolution mesh on entry), is re-meshed only when its level changes, and is evicted with its geometry released when the window leaves it; the census reports resident/entered/evicted/re-meshed blocks and triangles. Submission follows the per-view visibility flag the client stores in `block+0x74`: the bounds test runs every frame over the resident blocks and the survivors are merged into contiguous index runs (`src/terrain_residency.rs`, `src/gpu.rs`, `src/terrain_gpu.rs`), so the main view submits exactly the blocks it keeps while the mirrored and refraction passes keep the full set. Malformed data fails explicitly and names the block: an out-of-range cell, a T2BK block edge that is not the recovered 32 cells and a T2 grid that is not a whole number of blocks all abort the build with the numbers in the message. Executed: six unit tests in `src/terrain_residency.rs` (window enter/evict, re-mesh on level change, level-0 skip, named build failure, frustum keep/drop, run merging) and the per-build census line. | Recorded deviations: the retail archive is decoded up front, so the streamed unit is block geometry rather than block bytes (the client pages block data in and out); the visibility predicate is a conservative AABB/clip-plane test standing in for the client's virtual view call, so it can only keep blocks the client might drop; T2MK layers and masks are uploaded once for the world instead of following the resident set. |
| P2.02 | Implemented | Local full-resolution mesh and distant coarse mesh both exist and are now picked by the client's own rule: level 3/2/1 from `d = |block centre − camera| (XZ) − 0.7071067·block_size` against the active `terrain+0x26c`/`+0x270` pair, 180/320 world units for the shipped quality presets (`0x77e360`, `0x77f450`); geometry `step = 1<<(3−level)`, `q = (cells − 2·step)/step` (`0x7e5f70`); the per-block scale `block+0x7c` is written as 1.0f once at block construction (`0x7c85e2`, `0x7a1da3`) and never rewritten, so no per-block factor exists. Window and levels are re-evaluated every frame and the mesh is rebuilt when the answer changes (`src/gpu.rs`, `src/terrain_lod.rs`). Executed: `tools/verify_terrain_lod.py` and the unit tests in `src/terrain_lod.rs` (CI); rendered near→far→back camera walk confirmed 2 November 2026 with both meshes present. | None outstanding for the level ladder. `0x77e360` has no hysteresis band (two threshold comparisons and a default), so none is reproduced; a level change reaches the screen one asynchronous background build after the threshold crossing rather than inside the same frame. |
| P2.03 | Implemented | The client's own edge and neighbour rule replaced the approximate stitching: each block coarsens an edge to the stride of a coarser neighbour and keeps its own stride when the neighbour is missing (`0x7c40e6`, `step = 1<<(3−level)`), with no blending, across block and level boundaries (`src/terrain_lod.rs`, used by `src/gpu.rs`). Executed: exact per-block coverage for all three levels and ten edge-case combinations, a mixed-level 2×2 field and shared-edge node-set equality for all nine level pairs (`tools/verify_terrain_lod.py`), plus the unit tests in CI. Level and block boundaries rendered without cracks on 2 November 2026. | Recorded deviation: the native per-edge index layout (interior indices plus separate strip buffers) is emitted as an equivalent vertex set with viewer-chosen fan diagonals; the boundary nodes — the only place that can crack — are identical. |
| P2.04 | Implemented | The native switch `0x7c1450` and the one pass state it installs around every terrain draw (`0x7cd760`) are production code. `src/terrain_shading.rs` holds the four variants with their selectors and the state record, and `src/terrain_gpu.rs` builds **both** the near and the far pipeline from that single record - `cull_mode` via an exhaustive `match` on the recovered cull (a second native mode would fail to compile), `depth_write_enabled` from `z_write`, colour target opaque because `ALPHABLENDENABLE`/`ALPHATESTENABLE` are never called from the terrain range, `SPECULARENABLE` 1, `DESTBLEND` 6, `SRCBLEND` 2/5. Stage setups are recorded per variant: six stages for the 3-layer path (three colours, packed mask, day stage 4, night stage 5), a 2-stage fixed-function fallback, three stages for the dead projected-lightmap path, and the per-mask entry `0x7c1a20` as six unrolled stages. Executed in CI: the four table tests plus the module's 16 tests; WGSL validated with `naga` 30.2.0 and formatted with the wasm `rustfmt` oracle. | **Recorded deviations:** the per-mask entry is recovered but not drawn - it is the vertex-lit fixed-function configuration (FVF `0x4c2`, stride `0x34`), while the viewer's world stream is `crate::gpu::Vertex` (`0x3c`, the native `0x38` plus two per-vertex light indices), so reproducing it needs a second vertex format and an extra shader; the `0x7c1e00` fallback needs a terrain shader load to fail, which an embedded-WGSL renderer cannot do; the projected-lightmap `0x7c2100` path is dead in 1.4.2 (its gate `env+0x1b6` has one writer, which stores zero) and is reported as an error instead of drawn. `env+0x1b5`, which selects the per-mask entry, is a runtime mode whose supplied value is unrecovered, and the earlier ``GDebugKey Terrain2PerMask`` label for it is retracted - no such string exists in the binary. |
| P2.05 | Implemented | The stage pair is loader-traced, not guessed: stage 4 is the authored `.t2lm` day record (`record+0xc`, filled at `0x7a2aeb`) and stage 5 the `1.t2lm` night record (`record+0x10`, `0x7a2b0c`), which fixes the direction of the blend as **day at factor 0, night at factor 1** (`lrp_sat r5, c3.a, r5, r4`). `src/terrain_gpu.rs` runs exactly that: `mix(day, night, factor)*2` with a single `mul_x2`, no ambient term, no `dot(normal, sun)` and no scene light on the lightmapped path - so baked terrain is never lit twice - and the point-light fallback is confined to the branch taken when no authored pair is bound. Tile addressing is the atlas page/row arithmetic plus `D3DTSS_TCI_CAMERASPACE` (`0x7cd937`). The blend factor is a **published quantity**: `Camera::terrain_day_night` in `src/gpu.rs` is uploaded as `fog_range[3]` and read as `let factor=clamp(camera.fog_range.w,0.,1.);`, so `sun.w` means only "the pair is bound". Lane 3 of `fog_range` was free (`fog_matrix::Fog::range()` hard-wires it to `0.` and no other reader exists), so no uniform field and no offset changed. Executed in CI: `production_wgsl_matches_the_module_arithmetic` (asserts that clamp line exactly once and that the old `1.-camera.sun.w` is gone), `production_terrain_wgsl_runs_the_recovered_composite`, `day_and_night_maps_are_fixed_stages_not_swapped`, `day_stage_binds_the_record_the_loader_fills_with_the_day_file`, `day_night_lightmap_selects_both_endpoints`, `compose_drops_the_specular_term_because_c0w_is_zero`, `shader_constants_force_lane_zero_to_one`. | **Recorded deviation:** the factor's *source* is the app clock (`viewer_day_night_factor(daylight(time_of_day))`), not the client's `env+0x198`. What is recovered is the storage and the arithmetic: `0x7b4b00` copies the whole 4x4 product into `env+0x194..+0x1d3`, no later code negates or normalises those fields (`0x7b4eb0` normalises a copy into `env+0x2a4..`), and `lrp_sat` is the only clamp - but the matrix the caller passes is unidentified, so the lane's sign stays open. Registered as "Terrain day/night factor" in `Findings/Unsupported-client-paths.md`; the replacement is one call site in `src/gpu.rs` and changes no shader and no layout. |
| P2.06 | Implemented | The whole `world_<n>` set is surveyed rather than sampled: 88 of 88 T2MK files, 0 rejected, 1,408 areas, 5,929 area-layer entries, 603 layer records and 4,521 zlib masks, all of them 256x256 (65,536 bytes decoded), stacks 1..7, 162 texture indices, 31 authored `(u, v)` pairs (`tools/terrain-layer-survey.py`, results in `Findings/Phase-2-terrain-layer-survey.md` and `Findings/Phase-2-terrain-archive-survey.json`). The world-to-texture rule is per **layer** and is now one tested definition instead of three copies: `layer_uv` in `src/material.rs` - axis 0 `(z, -y)`, axis 1 `(x, -z)`, axis 2 `(x, -y)`, axis 1 as the default arm - with `axis_projection_matches_the_shader` in CI, and `src/terrain_gpu.rs`'s `Layer::axis` branches verified against it line by line. Layer order is the file order of the per-area list, which is already non-decreasing in record index in 1,408/1,408 areas. Unsupported inputs fail naming file, area and layer instead of defaulting. | **Recorded deviations:** the earlier claim that the readers disagreed on axis 1 is **retracted** (it came from misreading `material.rs`'s `-h` parameter; all three readers agree exactly). Mask resolutions other than 256 have no instance in the archive. The `login_login`, `a*`, `b*` and `s*` terrain sets are separate worlds that were not downloaded, so no claim is made about them; the tool accepts them unchanged. |
| P2.07 | Partial | Native source dimensions by default, 16x anisotropic gradients, generated mips (`src/terrain_gpu.rs`, `src/material.rs`). | Per-material mip/filter/addressing behavior, the optional downsampled-mask path and every source format the archive contains. |
| P2.08 | Partial | High-resolution nearby terrain and native-size scenery atlases exist (`src/terrain_gpu.rs`, `src/texture_atlas.rs`). | No demonstrated native terrain-hole/visibility-flag implementation, no recovered special-surface cases, and maximum-quality road/city (Archosaur) detail is not validated. |
| P2.09 | Partial | Terrain participates in main, clipped water and rendered map views (`src/gpu.rs`, `src/terrain_gpu.rs`); boundary/seam and mask-blending regressions run in CI (`src/terrain_lod.rs`, `src/material.rs`, `.github/workflows/pr-checks.yml`). The `vs`/`vs_far` parity program `src/bin/terrain-lod-gpu-check.rs` builds but is not in the gate: adding it to the shader step turned that step red under lavapipe (run 41525780828) and the sandbox cannot read the job log, so it stays out until it is diagnosed on a machine with a GPU. | Reflection/refraction and minimap/full-world-map equivalence is not established against the client. |

## Phase 3

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P3.01 | Partial | BMD model version 0x10000002 and mesh versions 0x10000004/0x10000005 parse and render, with transforms, texture references, baked arrays and material fields (`src/bmd.rs`, `src/objects.rs`); unsupported versions and truncation already fail explicitly. | Every authored version and every per-mesh/per-material flag; an archive-wide survey that names what is still unsupported. |
| P3.02 | Partial | Authored texture binding, baked day/night blend, opaque RGB scaling and cutouts (`src/gpu.rs`, `src/world_objects.rs`). | All texture stages, alpha-test/blend/transparency states, two-sided material behavior and special materials as a recovered state table rather than per-asset cases. |
| P3.03 | Partial | Archive lookup, shared texture/page handling and explicit model dependency errors (`src/objects.rs`, `src/world_objects.rs`, `src/model_dds.rs`). | Native animated-texture and missing-texture lookup rules and every fallback the client applies. |
| P3.04 | Partial | Per-mesh two-sided data and GPU facing/culling controls (`src/bmd.rs`, `src/gpu.rs`, `src/bin/bmd-sided-gpu-check.rs`). | Full native cull-mode selection, including mirrored/negative-scale placements; one field is not universal equivalence. |
| P3.05 | Partial | Reversed depth, overlap diagnostics and an atomic local/world scenery switch that removes known duplicate draws (`src/gpu.rs`, `src/bin/bmd-overlap-survey.rs`). | The client's own depth bias, draw order and the remaining intra-mesh and real-building conflicts. |
| P3.06 | Implemented | Ten native `CECSceneBlock` class rows, six quality presets, `lod_scale` detail reduction, per-placement EC-BSD flags, half-block load/evict hysteresis, and `Objects::load_ids` retention/eviction (`src/scenery_visibility.rs`, `src/gpu.rs`, `src/viewer.rs`, `src/world_objects.rs`). | Designated scope complete. Remaining non-designated gaps: level-2 object-state flags, authored birdview tiles, and live-client whole-world comparison are recorded in `Findings/Unsupported-client-paths.md`. |
| P3.07 | Implemented | Six passes share one production `FrameSnapshot` / `SecondaryView`; 6 x 6 recovered BMD material matrix, alpha/src-over blend, two-sided cull and fog/minimap/full-map paths are wired; `RenderContext` selects the recovered `WaterEffect` / underwater class gates (`src/gpu.rs`, `src/world_objects.rs`, `src/bin/scenery-pass-gpu-check.rs`). | Designated scope complete for supported materials at the shipped default. Level-2 blend-state byte, home-ornament assets, and authored birdview LOD tiles remain named non-designated gaps. |
| P3.08 | Partial | 4096-square native-size rectangle atlases with background page loading (`src/texture_atlas.rs`, `src/world_objects.rs`). | Bounded texture residency, cross-page deduplication, eviction and avoiding full-world full-resolution residency. |
| P3.09 | Implemented | 19 CI GPU program runs (24 diagnostics total), 7 wired static-model diagnostics with 17 fail-closed exclusions, coincident-depth `bmd-depth-gpu-check`, 6-pass material/snapshot/residency `scenery-pass-gpu-check`, 2,048 baked day/night cases and standalone module tests in Linux/Windows CI. | Designated CI coverage complete. Cross-page depth order, live-client screenshot comparison and whole-world GPU benchmark remain named non-designated gaps. |

## Phase 4

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P4.01 | Partial | Actual runtime tree geometry, separate foliage material infrastructure and optional regional experiment (`src/trees.rs`, `src/far_trees_gpu.rs`). | Complete native branch/frond/leaf/billboard lighting, tangents, normals and material constants. |
| P4.02 | Partial | Three real runtime geometry snapshots and distance crossfades; corrected leaf geometry (`src/tree_lod.rs`, `src/trees.rs`, `src/tree_source.rs`). | Original continuous runtime LOD/billboard thresholds and transition behavior; current widths are approximate. |
| P4.03 | Partial | DLL-derived sampled leaf rocking and source/cache validation (`src/trees.rs`, `src/tree_source.rs`). | Replace sampled motion with full original runtime animation where possible. |
| P4.04 | Partial | Per-tree geometry/motion inputs and optional wind controls (`src/far_trees_gpu.rs`, `src/view_settings.rs`). | Exact native timing, wind modes and per-instance runtime variation. |
| P4.05 | Partial | Authored grass types/masks, terrain alignment, RANMAR jitter/yaw/scale and instancing (`src/grass.rs`, `src/grass_random.rs`, `src/grass_instances.rs`). Native density ladder recovered: `step = 5 - ftol(cam[+0x678]*4 + 0.5)` with both axes walked at 0, step, 2*step, step 5 as the no-grass early-out and steps below 1 clamped (`0x653b6e`-`0x653ba2`), plus the emitter's `ftol(2*|y-10000| + rand(0,10)) % 80` wind slot (`0x653e93`-`0x653ec1`), which the retail `shaders/vs/grass_wave.txt` confirms is a per-placement attribute (`mul r1, v1.y, c[10+a0.x]`). The `.mox`/MOXB byte layout is now settled rather than fitted: 32-byte vertices (pos/normal/uv), `u16` indices, anchored backwards from `TEXTURE:`, and confirmed by parsing 155/155 archive models for 4,703 vertices with zero non-unit normals, zero out-of-range indices and zero non-finite fields (`Findings/Grass-model-format.md`), with a real archive member as a unit test. | `cam[+0x678]` is a lerped colour lane, not a depth, so the viewer still runs step 1 (full resolution); patch visibility, LOD and streaming behavior remain. |

| P4.06 | Partial | 80-frame wind history driven by the frozen logic delta, `CNoise` at 20 Hz with the authored axis weights, and the `0x651960` trapezoid (`H = disp/+0x48`, `pos += vel*h + 0.5*h*h*H`, `vel += h*H`, `+0x50` = 0.0 so no drag) checked against the client at `pos=(7,8,9)`, `vel=(1,2,3)`, `disp=(0.5,0.6,0.7)` (`src/grass_wind.rs`, `tools/x87/interp.py`). | `0x651840` is now closed: 81 straight-line instructions, `ret 8`, identified by basis probe as `force = -(+0x4c + +0x50)*arg1 - +0x54*arg2`, `acc = (+0x30,+0x34,+0x38) + force`, i.e. a diagonal anisotropic spring with no cross-axis coupling (`src/grass_wind.rs`). The earlier disagreement was a bug in `tools/x87/interp.py`, which tested `dst.startswith("[")` on unstripped operands so every `mov dword ptr [esp+N], eax` spill was lost; fixed, with a self-check. `0x651a70` takes integer milliseconds (`fild`) and scales by `0x8d5ef8` = 0.001, so seconds are the runtime unit. Still open, and now sharper: `0x6510c0`-`0x6510f1` overwrite `+0x48` and `+0x4c` with **target components** (`+0x48` = target.y, `+0x4c` = target.z, `+0x44` = target.x) before every wind tick, so the constructor's 0.1 / 2.0 are not the steady-state gains. Taken literally that puts a non-constant divisor in `0x651960` and contradicts the already-verified arithmetic (the documented `(1,-0.5,0)` direction would give a gain of `-0.5*noise` and `1/+0x48 = 200` near zero noise). Resolving which field set is authoritative is the next step; until then the viewer keeps the bounded target-minus-position form. Also open: the remaining material states and full per-tree motion. |
| P4.07 | Partial | Foliage/grass mips, cutouts, lightmaps and reflection submission (`src/foliage_mips.rs`, `src/grass_mips.rs`, `src/gpu.rs`). | Original per-material samplers and complete tree lighting/reflection behavior. |
| P4.08 | Implemented | Approved tree path remains available and default; new regional lighting is opt-in (`src/view_settings.rs`, `src/far_trees_gpu.rs`). | Preservation requirement met, not full vegetation parity. |
| P4.09 | Implemented | LOD, sampled motion, RANMAR placement, wind history, alpha coverage and lighting tests/diagnostics exist (`src/tree_lod.rs`, `src/trees.rs`, `src/grass_random.rs`, `src/grass_wind.rs`, `src/bin/grass-instance-gpu-check.rs`). | Test-category requirement met; asset-dependent diagnostics are not all rerun or all part of current CI. |

## Phase 5

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P5.01 | Partial | Client sky texture layers, panorama projection and seam mitigation (`src/sky_gpu.rs`). | Native geometry/projection, complete seams and layered materials. |
| P5.02 | Partial | Time-dependent sky blend/texture selection (`src/sky_gpu.rs`). | Current smoothstep schedule is still approximate, not native sky transition logic. |
| P5.03 | Partial | Recovered celestial direction, default-region flags and quantized visibility fades wired to experimental discs (`src/client_sun.rs`, `src/sky_gpu.rs`). | Native sun/moon geometry, sizes, halos, exposure/blending and full material behavior. |
| P5.04 | Partial | Region-authored environment lighting/fog and independent default-area sun/moon flags (`src/environment.rs`). | Region-selected sky texture sets/settings and actual sky transition resources. |
| P5.05 | Not implemented | Sky/environment infrastructure exists (`src/sky_gpu.rs`, `src/environment.rs`). | No implemented client precipitation/cloud/weather visual runtime established in the current renderer. |
| P5.06 | Partial | Regional fog/colors, normal and clipped/reflected view integration (`src/environment.rs`, `src/gpu.rs`). | All atmospheric/weather/material variants and native visibility rules. |
| P5.07 | Partial | Streamed region priority, transition duration, PC24 stored steps and time-slider fog/light sampling (`src/environment.rs`). | Complete sky/weather resource transitions, not just environment coefficients. |
| P5.08 | Partial | Translation-free sky rays and full reflected camera-basis tests (`src/sky_gpu.rs`). | Native sky/weather reflection behavior, cloud/precipitation coverage and full altitude acceptance. |
| P5.09 | Implemented | All four named categories now have executable coverage. Time boundaries: `time_boundaries_stay_finite_periodic_and_asymmetric` walks -48..48 h including every breakpoint (3.5, 18.75, 20.25, 21) plus NaN/+-inf. Region transitions: `streamed_region_selection_tracks_blocks_references_and_transitions`, `teleport_reinitializes_and_interruption_retargets_in_flight_values` and `losing_the_default_box_retains_the_last_ratified_direction_and_flags`. Celestial placement: `sky_sample`/`Celestial` are now CPU mirrors of the WGSL and are pinned by four tests (disc centres on the body direction, rays 0.9 deg inside vs 5 deg outside, below-horizon rejection, 03:30-21:00 day window, and a finite projection for a body exactly overhead where `cross(up, c)` degenerates). Sky seams: the panorama mapping `side = floor(2*az) % 2`, `u = fract(2*az)` is tested at all four cardinals, over a 3600-step sweep (u stays in range, the two panoramas split the circle 1800/1800), plus `seam_weight` symmetry, half-weight at both edges and monotonicity. | Closed. The seam blend is tested as a mapping; whether panorama 0's `u=1` column actually equals panorama 1's `u=0` column still needs `textures.pck` and is not claimed here. |

## Phase 6

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P6.01 | Partial | Separate expensive/simple water, reflection/refraction targets, edge-alpha pass and shoreline renderer (`src/water_gpu.rs`, `src/simple_water_gpu.rs`, `src/shore_gpu.rs`). | Every quality/scroll/hardware mode and exact native target/clear/state sequence. |
| P6.02 | Partial | Authored area masks/heights, area-class materials and shoreline arrays (`src/water.rs`, `src/gpu.rs`). | Original overlap priority, visibility/load order and all material/shoreline cases. |
| P6.03 | Partial | 29-frame normals, 33ms frame period, rotating offset, source mip parsing and shoreline animation (`src/water.rs`, `src/water_gpu.rs`, `src/shore_gpu.rs`). | Exact stepped-versus-interpolated behavior, scroll/quality modes and RNG phase. |
| P6.04 | Partial | Per-plane reflected camera, clipped refraction, reverse depth and below-surface above-plane scene (`src/gpu.rs`, `src/water_gpu.rs`). | Native complete camera/target/clear/texture-coordinate behavior in all modes. |
| P6.05 | Partial | Clipped targets and rasterized edge alpha replace portions of earlier depth-only safeguards (`src/water_gpu.rs`, `src/gpu.rs`). | Remaining fallback heuristics, terrain selection and original visibility/culling rules. |
| P6.06 | Partial | Recovered expensive-water equations and alternate fixed-function texture-stage material (`src/water_gpu.rs`, `src/simple_water_gpu.rs`). | All lighting/fog/color/quality paths and original material-state precision. |
| P6.07 | Partial | Active-cell underwater detection, regional fog, warp-before-glow and distinct refraction fog (`src/water.rs`, `src/post_gpu.rs`, `src/refraction_fog.rs`). | Full entry/exit/overlap semantics and all underwater pass/material transitions. |
| P6.08 | Partial | Distinct visible water heights receive individual reflected views (`src/gpu.rs`, `src/water_gpu.rs`). | Native visibility/batching rules and measured efficient reuse across all visible heights. |
| P6.09 | Implemented | Water mask/underwater units, reflected sky-basis tests, multiple-plane depth GPU diagnostic and post/fog transition tests exist (`src/water.rs`, `src/sky_gpu.rs`, `src/bin/water-pass-gpu-check.rs`, `src/bin/post-gpu-check.rs`). | Test-category requirement met; water-pass GPU diagnostic is not among the latest 13 CI GPU executions. |

## Phase 7

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P7.01 | Partial | ECM/SMD/SKI/BON/STCK dependencies, skeleton poses, weighted and rigid meshes, DDS materials and retained raster submission (`src/ecmodel.rs`, `src/model_asset.rs`, `src/model_draw_gpu.rs`). | All format versions, morphs/materials/queues and unsupported model variants. |
| P7.02 | Partial | Source ordinary actions, inclusive endpoint clocks, transforms, idle fallback and shared capped frame tick (`src/model_playback.rs`, `src/world_models.rs`, `src/gpu.rs`). | Complex combined/event/random action scheduling, native LOD/visibility and global random phase. |
| P7.03 | Partial | Scene grid and runtime light publications feed ordinary world weighted/rigid models (`src/world_models_gpu.rs`, `src/model_material.rs`). | Other confirmed receiving classes/queues and general FX graph owner integration. |
| P7.04 | Partial | GFX dependency audit, light-element decoding/controller execution and publication API (`src/gfx_light_audit.rs`, `src/gfx_light.rs`). | General visible particles, sprites, trails, animated textures and other effect renderers. |
| P7.05 | Partial | Light key timing, parent transform math, priority/visibility gates and retained scene owner records (`src/gfx_light.rs`, `src/scene.rs`). | Actual general effect attachments, owner graph, region lifetimes and visual depth/blend scheduling. |
| P7.06 | Partial | Surveys and explicit retained model/dependency failures (`src/world_models.rs`, `src/bin/gfx-light-audit.rs`). | Resolve all remaining world formats/specialized shaders identified by a complete census. |
| P7.07 | Partial | Ordinary animated world models render in main, reflection and clipped underwater-camera views with material fog (`src/world_models_gpu.rs`, `src/gpu.rs`). | Complete model materials and general effect integration in every relevant pass. |
| P7.08 | Implemented | Free-flight file viewer only; no combat, quests, networking or player simulation (`src/main.rs`, `README.md`). | Scope constraint met; user-controlled navigation/time preview are not gameplay simulation. |
| P7.09 | Implemented | Animation/pose/clock units, GFX light lifetime units, resource retry/atomic-update and material blending GPU checks (`src/animation.rs`, `src/model_playback.rs`, `src/gfx_light.rs`, `src/bin/model-draw-gpu-check.rs`). | Test-category requirement met; light-element tests are not a complete general effect renderer. |

## Phase 8

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P8.01 | Partial | World/model/lightmap/GFX surveys, light dependency audit and retained failure reporting (`Findings/Phase-1-model-material-census.md`, `src/world_models.rs`, `src/gfx_light_audit.rs`). | Full file/type/material/shader census and zero unsupported or silently skipped content. |
| P8.02 | Partial | Nearby terrain/scenery/grass and asynchronous whole-world scenery loading (`src/gpu.rs`, `src/world_objects.rs`). | Client-equivalent visibility plus bounded full-resolution residency/eviction. |
| P8.03 | Partial | Grass instancing, shared model dependencies, native-size page packing and batching (`src/grass_instances.rs`, `src/world_models.rs`, `src/texture_atlas.rs`). | Complete culling/residency/background upload policy and measured full-world memory/performance targets. |
| P8.04 | Partial | Original tiled world map, live rendered map/minimap, corrected orientation and click teleport (`src/navigation.rs`, `src/gpu.rs`). | Complete scenery/model/effect inclusion and consistent streaming/material behavior across map views. |
| P8.05 | Partial | Precinct polygon/playlist reader, archived OGG mapping, Windows playback and volume controls (`src/music.rs`). | Native overlap priority, randomization/crossfades, ambient sounds and remaining formats/device behavior. |
| P8.06 | Partial | Windows launcher, saved folder, loading feedback, controls, teleport list and map interaction (`src/launcher_ui.rs`, `src/windows_setup.rs`, `src/gpu.rs`). | Finish polish/recovery/DPI/device testing; launcher low-memory toggle is not demonstrated persisted. |
| P8.07 | Partial | Persisted graphics/world/audio/interface settings, defaults, sanitization and roundtrip tests (`src/view_settings.rs`). | Every launcher setting and every relevant secondary-pass setting application/behavior. |
| P8.08 | Implemented | Parser/render/streaming/navigation/music unit coverage, 13 CI GPU programs, native numeric references and Windows packaging workflow (`.github/workflows/pr-checks.yml`, `src/music.rs`, `src/navigation.rs`). | Expansion/category requirement met; physical-GPU/audio/Windows UI runtime acceptance is still pending. |
| P8.09 | Implemented | Windows x64 executable artifact built successfully with embedded PE32 tree bridge; matching application/helper source is tracked (`src/windows_setup.rs`, `tools/speedtree/bake.cpp`, `.github/workflows/pr-checks.yml`). | Build/package/source requirement met for this revision, not completion of its unfinished features; fresh-PC launch remains unverified and private client assets are still required. |
| P8.10 | Implemented | Explicit known unsupported-path register and no-percentage/zero-open-path completion policy (`Findings/Unsupported-client-paths.md`). | Maintenance/policy requirement met. Register is nonempty and census incomplete, therefore no 100% parity claim. |

## Checkbox corrections

Previously checked P1.11/P1.12 remain checked. Corrected nine additional entries:
P2.09, P4.08, P4.09, P6.09, P7.08, P7.09, P8.08, P8.09 and P8.10.
The Phase 2 re-cut of 2 November 2026 moved the old P2.09 test deliverable into
the new P2.09 (multi-view consistency plus regression coverage), which is Partial
because reflection/refraction and minimap equivalence are not established, so its
checkbox is unchecked again; the existing terrain regressions are still recorded in
that row.
P8.10's explicit consolidated register was supplied by this reassessment;
its creation does not resolve its open entries. The other corrected entries
recognize existing code/tests/artifacts, not newly implemented rendering features.
Test-category checkmarks mean tests for those named categories have been added;
executions and limitations are separately recorded above. P8.09 identifies the
current-build packaging/source deliverable, not a final parity-complete release.

## Corrections to earlier descriptions

- Animated world models are **not absent**: ordinary weighted/rigid assets, actions,
  retained materials/textures, scene-light assignment and clipped pass submission exist.
  General effects, complex scheduling and complete model/material coverage remain open.
- Scene lights are **not merely parsed**: ordinary receiving world models consume them.
  Baked BMD/terrain are deliberately not point-lit again.
- Bloom/gamma/fog are **not just their old approximations**: recovered postprocess
  passes, PC24 gamma words and material-specific fog are implemented and tested.
  Their listed residual acceptance gates are narrower than reimplementing the basics.
- Terrain seams and tree LOD **do exist**, but their source explicitly identifies
  approximations; existence is not original-client algorithm completion.
- Water has distinct clipped/reflected/refraction/edge/alternate-material paths;
  “no clipped refraction” and “only a depth approximation” are stale blanket claims.
- A single Windows binary **has been built and published as a CI artifact**, including
  its embedded helper, with source. That is distinct from final full-parity delivery.
- Historical percentage estimates in Parity-Assessment are superseded, not current scores.

All open implementation acceptance is recorded above and in the unsupported register.
Do not turn a Partial into Implemented merely because a build or synthetic case passes.

## Recheck the recorded decisions

```sh
python3 tools/check-eight-phase-status.py
python3 tools/test_eight_phase_status.py
python3 tools/check-phase1-ready.py
```

The first two should pass when the 76 decisions/checklists agree. The Phase 1
readiness command intentionally exits nonzero while any full-scope Phase 1 gate
remains No. None of these commands substitutes for renderer/client validation.
