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
| P1.08 | Partial | Recovered quarter-size UNORM targets, offsets, blur weights, warp-before-glow, sharpen, presenter transitions/fades and encoded compositing (`src/post_gpu.rs`, `src/glow_state.rs`). | Final whole-pass/caller coverage audit; not another missing basic bloom implementation. |
| P1.09 | Partial | Settings/mode gating, whole-frame post-UI ramp, output encoding and all 25,856 native PC24 gamma words (`src/display_gamma.rs`, `src/gamma_curve.rs`). | Remaining upstream material color conversions and Windows presentation/compositor acceptance. |
| P1.10 | Partial | Production material fog and distinct grass/BMD/terrain refraction equations (`src/refraction_fog.rs`, `src/terrain_gpu.rs`, `src/gpu.rs`). | All material variants, transparency and complete underwater/pass ordering coverage. |
| P1.11 | Implemented | Approved default tree geometry/lighting retained; incomplete replacement remains opt-in (`src/far_trees_gpu.rs`, `src/view_settings.rs`). | Preservation requirement met; full native tree materials belong to P4.01. |
| P1.12 | Implemented | CPU and 13 CI GPU programs cover selection, transitions, material arithmetic, color and postprocess categories (`tools/check-phase1-core.sh`, `.github/workflows/pr-checks.yml`). | Test-category requirement met; tests do not certify the ten unfinished implementation gates. |

## Phase 2

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P2.01 | Implemented | Height blocks and T2MK areas load from the world stream, and the draw set follows `0x77ecf0`/`0x77e1b0`: the `(2r+1)`-block square around the camera's block, edge-shifted at the world border and re-selected every frame (`src/terrain_lod.rs`, `src/gpu.rs`, `src/main.rs`), with the persistent whole-world mesh drawn underneath (`src/terrain_gpu.rs`) so no part of the world is clipped. Malformed data fails explicitly instead of drawing substitute geometry: a T2BK block edge that is not the recovered 32 cells and a T2 grid that is not a whole number of blocks both abort the build with both numbers in the message (`src/gpu.rs`). | Recorded deviations accepted at review on 2 November 2026: every block is resident up front, so the window is a draw-set property and there is no streaming eviction; `block+0x74` per-view visibility is not decoded, so every resident block is submitted (the client's level-0 skip is not reached); the whole-world mesh keeps the viewer's previous extent instead of reproducing the client's clip. |
| P2.02 | Implemented | Local full-resolution mesh and distant coarse mesh both exist and are now picked by the client's own rule: level 3/2/1 from `d = |block centre − camera| (XZ) − 0.7071067·block_size` against the active `terrain+0x26c`/`+0x270` pair, 180/320 world units for the shipped quality presets (`0x77e360`, `0x77f450`); geometry `step = 1<<(3−level)`, `q = (cells − 2·step)/step` (`0x7e5f70`); the per-block scale `block+0x7c` is written as 1.0f once at block construction (`0x7c85e2`, `0x7a1da3`) and never rewritten, so no per-block factor exists. Window and levels are re-evaluated every frame and the mesh is rebuilt when the answer changes (`src/gpu.rs`, `src/terrain_lod.rs`). Executed: `tools/verify_terrain_lod.py` and the unit tests in `src/terrain_lod.rs` (CI); rendered near→far→back camera walk confirmed 2 November 2026 with both meshes present. | None outstanding for the level ladder. `0x77e360` has no hysteresis band (two threshold comparisons and a default), so none is reproduced; a level change reaches the screen one asynchronous background build after the threshold crossing rather than inside the same frame. |
| P2.03 | Implemented | The client's own edge and neighbour rule replaced the approximate stitching: each block coarsens an edge to the stride of a coarser neighbour and keeps its own stride when the neighbour is missing (`0x7c40e6`, `step = 1<<(3−level)`), with no blending, across block and level boundaries (`src/terrain_lod.rs`, used by `src/gpu.rs`). Executed: exact per-block coverage for all three levels and ten edge-case combinations, a mixed-level 2×2 field and shared-edge node-set equality for all nine level pairs (`tools/verify_terrain_lod.py`), plus the unit tests in CI. Level and block boundaries rendered without cracks on 2 November 2026. | Recorded deviation: the native per-edge index layout (interior indices plus separate strip buffers) is emitted as an equivalent vertex set with viewer-chosen fan diagonals; the boundary nodes — the only place that can crack — are identical. |
| P2.04 | Partial | One layer/mask/lightmap terrain shader with distinct near/far vertex paths (`src/terrain_gpu.rs`, `src/material.rs`). | Every native variant, stage setup, cull/depth/blend/alpha-test state and the hardware fallback paths; state is not yet recovered as a table. |
| P2.05 | Partial | T2LM/T2LM1 pair availability, rectangular tile mapping and the 2x day/night modulation (`src/terrain_lightmaps.rs`, `src/terrain_gpu.rs`). | Pair-selection edge cases, clamp/wrap, encoded-space arithmetic and the exact combination with regional ambient. |
| P2.06 | Partial | T2MK v2 layer axes, per-layer transforms and mask grid (`src/material.rs`). | Verify every authored projection, layer count and mask format/resolution against the client; world-to-texture mapping is only proven for the exercised areas. |
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
| P3.06 | Partial | Local loading and background whole-world static scenery pages (`src/world_objects.rs`, `src/gpu.rs`). | Native visibility flags, distance selection, LOD/detail reduction and the region lifecycle, including complete eviction and every model class. |
| P3.07 | Partial | Baked scenery is submitted in the main, reflected, fogged and rendered map passes (`src/gpu.rs`). | Every supported static material, especially alpha and special stages, in every pass, with one snapshot per frame. |
| P3.08 | Partial | 4096-square native-size rectangle atlases with background page loading (`src/texture_atlas.rs`, `src/world_objects.rs`). | Bounded texture residency, cross-page deduplication, eviction and avoiding full-world full-resolution residency. |
| P3.09 | Partial | BMD version/truncation/material tests, a sided GPU diagnostic, reversed-depth tests and an exhaustive baked-blend GPU comparison (`src/bmd.rs`, `src/bin/bmd-baked-gpu-check.rs`). | Multi-stage and transparent material coverage, real depth-conflict regressions and CI wiring for every static-model diagnostic. |

## Phase 4

| ID | Decision | Already implemented / evidence | Remaining acceptance / boundary |
|---|---|---|---|
| P4.01 | Partial | Actual runtime tree geometry, separate foliage material infrastructure and optional regional experiment (`src/trees.rs`, `src/far_trees_gpu.rs`). | Complete native branch/frond/leaf/billboard lighting, tangents, normals and material constants. |
| P4.02 | Partial | Three real runtime geometry snapshots and distance crossfades; corrected leaf geometry (`src/tree_lod.rs`, `src/trees.rs`, `src/tree_source.rs`). | Original continuous runtime LOD/billboard thresholds and transition behavior; current widths are approximate. |
| P4.03 | Partial | DLL-derived sampled leaf rocking and source/cache validation (`src/trees.rs`, `src/tree_source.rs`). | Replace sampled motion with full original runtime animation where possible. |
| P4.04 | Partial | Per-tree geometry/motion inputs and optional wind controls (`src/far_trees_gpu.rs`, `src/view_settings.rs`). | Exact native timing, wind modes and per-instance runtime variation. |
| P4.05 | Partial | Authored grass types/masks, terrain alignment, RANMAR jitter/yaw/scale and instancing (`src/grass.rs`, `src/grass_random.rs`, `src/grass_instances.rs`). | Complete native density, patch visibility, LOD and streaming behavior. |
| P4.06 | Partial | 80-frame spring/noise wind history, source fade and cutout arithmetic (`src/grass_wind.rs`, `src/gpu.rs`). | Exact client wind update timing/precision and all material states. |
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
| P5.09 | Partial | Clock, region transitions, celestial curve/visibility and reflection ray tests (`src/client_sun.rs`, `src/environment.rs`, `src/sky_gpu.rs`). | Automated sky seam and regional sky resource transition coverage. |

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
