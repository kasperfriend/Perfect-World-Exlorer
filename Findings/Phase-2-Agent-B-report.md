# Phase 2, Agent 2-B - terrain shading and materials (P2.04, P2.05, P2.06)

Reference client: `elementclient.exe` 1.4.2, md5
`5f02895688fa580506e869156340a664`, 6,000,640 bytes, PE i386, image base
`0x400000`, `.text` `0x401000+0x4d3f25`, `.rdata` `0x8d5000+0x77aae`.

Lane: terrain shader inputs, variants, pass state, lightmap arithmetic,
layer/mask selection and world-to-texture mapping. Mesh construction, LOD
switching, texture decoding and view submission belong to 2-A and 2-C and are
not touched here.

## 1. Changed files and production call sites

| File | Change | Production call site |
|---|---|---|
| `src/terrain_shading.rs` | **new** - `Variant`, `Caps`, `select`, `PassState`/`pass_state`, stage constants, `vertex_layout`, `ShaderConstants`/`shader_constants`, `day_night_lightmap`, `lightmap_uses_night`, `compose`, `packed_weight`, `composite_layers`, `Atlas`, `lightmap_pair`, `native::*`; 12 tests | `TerrainGpu::new` calls `select`/`pass_state`; the lightmap arithmetic is still reference-only (section 7, gap 1) |
| `src/terrain_gpu.rs` | **cross-lane edit, announced**: `TerrainGpu::new` derives primitive, colour-target, depth and cull state for **both** the near and the far pipeline from `terrain_shading::select`/`pass_state` instead of restating them, and `fs` now runs the recovered lightmap composite (one `mix(day, night, factor)*2`, no ambient/normal/scene-light term on the lightmapped path) | 2-A/2-C read the same pipeline; no mesh, texture, sampler, draw or submit code touched. `light_consumers.rs` guard reworded, not weakened |
| `src/main.rs` | `#[allow(dead_code)] mod terrain_shading;` added beside the other attributed modules (the address table and the reference arithmetic are not all reachable from the viewer yet) | binary crate root |
| `tools/check-phase1-core.sh` | `terrain_shading` added to the dependency-free module loop | CI `pr-checks` |
| `tools/terrain-layer-survey.py` | **new** - T2MK/T2LM survey with explicit `Unsupported` errors | manual / CI survey |
| `Findings/Phase-2-terrain-variant-state-table.md` | new - P2.04 table | - |
| `Findings/Phase-2-terrain-layer-survey.md`, `Findings/Phase-2-terrain-layer-survey.csv` | new - P2.06 survey | - |

The `src/terrain_gpu.rs` edit stays inside the 2-B lane (pipeline, state setup
and the fragment-shader arithmetic). It adds no mesh construction, no
texture/mip/sampler creation and no view submission.
`Findings/Unsupported-client-paths.md` is a shared file; this lane added one row
("Terrain day/night factor") **and announced it here** rather than editing
silently. Version, camera uniform, vertex formats and everything 2-A/2-C own are
unchanged.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.04 | **No** | `terrain_shading` compiled into the binary and into the CI core loop; `PassState` now supplies `cull_mode`, `depth_write_enabled` and the opaque colour target of the main terrain pipeline; the far pipeline still restates its own state | `0x7c1450` switch, `0x7c14b0`/`0x7c1a20`/`0x7c1e00`/`0x7c2100`, `0x7cd760` state bracket, `0x7c0d99` FVF, `0x7c4d90` module load | not yet executed (no local toolchain); 11 unit tests are in CI only | pipeline wiring; per-mask texture arithmetic; `t2_3layer_vertlit_14`/`t2_tm_lm` binding sites outside the terrain range

## 2. Variant and state table (P2.04)

Full table in [Phase-2-terrain-variant-state-table.md](Phase-2-terrain-variant-state-table.md).
Summary:

| Variant | Entry | Selector | Reachable in 1.4.2 |
|---|---|---|---|
| 3-layer batch | `0x7c14b0` | `device+0xc != 0`, `env+0x1b5 == 0`, `env+0x1b6 == 0` | yes - the stock path |
| per-mask lightmap | `0x7c1a20` | `env+0x1b5 != 0` (GDebugKey `Terrain2PerMask`) | debug only |
| no pixel pipeline | `0x7c1e00` | `device+0xc == 0` | device dependent |
| hardware lightmap | `0x7c2100` | `env+0x1b6 != 0` | **never** - explicit error |

Every variant shares: `CULLMODE` 1 (`0x7cd789`), `ZWRITEENABLE` 1 (`0x7cd793`),
`ZENABLE` 1 (`0x7cd79d`), `SPECULARENABLE` 1 (`0x7cd7c9`), `ALPHABLENDENABLE`
and `ALPHATESTENABLE` never called, `DESTBLEND` 6, `SRCBLEND` 2 then 5 (or 5/5 in
fixed function), `LIGHTING` set to 0 for the draw by the per-mask and
no-pixel-pipeline paths and left at 1 by the stock 3-layer path (whose vertex
shader consumes the normal instead). FVF `0x412`/stride `0x38` for the
3-layer path, `0x4c2`/`0x34` for per-mask.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.04 table | Yes (table complete) | `pass_state()`/`select()` exist and are tested | addresses above; every value cited per row | 4 tests (`native_switch_selects_one_variant_per_state`, `every_variant_carries_the_native_pass_state`, `stage_counts_match_the_stage_loops`, `vertex_layout_follows_the_per_mask_flag`) - CI only | the far pipeline is not yet derived from the same state, and the per-mask layout is not selectable at runtime

## 3. Lightmap selection and arithmetic (P2.05)

Recovered constants that replace earlier estimates:

| Quantity | Old estimate | Recovered | Evidence |
|---|---|---|---|
| VS `c0.x` | regional ambient | **1.0** | `0x7c2872: movl $0x3f800000,0x18(%esp)` after the third push, joined by both constant-block branches |
| VS `c0.w` / `v1` (specular) | - | `0x9d30f8`, zero by default | one store site `0x7c281a`, debug keys `0x7c274e-0x7c281a` |
| PS `c3.a` day/night | daylight fraction | **blended scene light direction y** | `0x7c2377: flds 0x198(%ecx)`; `env+0x198` written only by `UpdateSceneLight` (`0x7b4eb0`) then normalised in place at `0x7b4fc2..0x7b4fd9` |
| day/night app scalar | `sun.w` ramp | `0x9d30f8 = 0` | sole app-side store; the blend factor is not an app value |
| day/night direction of the mix | `mix(night, day, sun.w)` (day at factor 1) | `mix(day, night, factor)` (day at factor 0) | `lrp_sat r5, c3.a, r5, r4` with `t4` = day, `t5` = night (`0x7c23a2`, `0x7c23b0`); integrated in `terrain_gpu.rs` `fs` |
| ambient / regional light | `light` uniform multiplication | **no ambient term** - `mul r0, r0, v0` is a multiply by 1.0 | `t2_3layer_batch_14` tail |
| layer weights | per-layer mask alpha | packed mask **B/G/R = layers 1/2/3** | shader comments + `0x7c25c0` stage installs |
| mask `c2` | scale | 1.0/0.0 selector per bound layer | `0x7c2331`/`0x7c2347`, uploaded to `c2` at `0x7c2368` |

Pair choice: `t4` = day, `t5` = night (`0x7c23a2`/`0x7c23b0` install stages 4 and
5); `lrp_sat r5, c3.a, r5, r4` means factor 0 -> day, 1 -> night, clamped. Tile
addressing is per area through the mask slot, from the same counter as
`0x7cd937` (`D3DTSS_TCI_CAMERASPACE`) and `0x7cba30` (per-area uv). The result is
`(layers + lightmap*specular)` `* 1.0` `* lightmap * 2`, saturated.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.05 | **No** | `terrain_gpu.rs` `fs` now runs the recovered composite: `mix(day, night, factor)*2` with the day map as the factor-0 endpoint, a single `mul_x2`, and **no** ambient/normal/scene-light term on the lightmapped path (the point-light fallback only runs when no authored pair is bound). `day_night_lightmap`, `lightmap_uses_night`, `compose`, `shader_constants`, `lightmap_pair` remain the reference the WGSL is pinned to by `production_terrain_wgsl_runs_the_recovered_composite` | `0x7c26e0` constant block, `0x7b4eb0` direction build, `0x7b4fc2` normalise, `0x7c2377` `c3.a`, `0x7c23a2`/`0x7c23b0` stage 4/5 | `day_and_night_maps_are_fixed_stages_not_swapped`,
`day_night_lightmap_selects_both_endpoints`, `compose_drops_the_specular_term_because_c0w_is_zero`, `shader_constants_force_lane_zero_to_one`, `production_terrain_wgsl_runs_the_recovered_composite` - CI only | the **factor** is still the viewer's `1 - sun.w` (`sun.w` = lightmap availability), not `env+0x198`: `UpdateSceneLight` is not reconstructed, and its pre-normalisation input (`0x7b4e5d-0x7b4e75`, a wholesale 4x4 copy) is unidentified; register row "Terrain day/night factor"

## 4. Layer, mask and world-to-texture survey (P2.06)

Full survey in [Phase-2-terrain-layer-survey.md](Phase-2-terrain-layer-survey.md);
machine-readable archive pass in
[Phase-2-terrain-archive-survey.json](Phase-2-terrain-archive-survey.json); per-entry
rows for the reference tile in
[Phase-2-terrain-layer-survey.csv](Phase-2-terrain-layer-survey.csv).

**Covered: the whole `world_<n>` set - 88 of 88 files**, 76.0 MiB, 0 rejected,
1,408 areas, 5,929 area-layer entries, 603 layer records, 4,521 masks.

| Finding | Value |
|---|---|
| Version / grid byte | 2 and 1 in 88/88 files |
| Areas per file | 16, always |
| Stack depth | 1: 251, 2: 99, 3: 134, 4: 128, 5: 218, 6: 570, 7: 8 areas |
| Mask size | 256x256 for all 4,521 masked entries; every base entry has `mask_size = 0`; no other resolution occurs |
| Mask encoding | zlib, 4,521/4,521 decode to exactly 65,536 bytes (282.6 MiB uncompressed) |
| Record flags | `0xffffffff` in 603/603 |
| Axis byte | 0: 39, 1: 527, 2: 37 records. **All 1,408 area bases are axis 1**; axes 0/2 exist only on masked layers (384 / 371 of 4,521), and 755 masked layers differ from their own area's base |
| Draw order | the file order of the per-area list; already non-decreasing in record index in 1,408/1,408 areas, but in only 1,343/1,408 by the `+0x18` tag, which repeats inside 1,065 areas - so the tag is not a draw index |
| Records | shared across areas (fewer records than entries) in 80/88 files; one file uses 6 records for 88 entries; no area reuses a record twice |
| Textures | 162 distinct indices (0..161), 559 references, 1..18 per file |
| Authored uv | 31 distinct `(u, v)` scale pairs, 24 square and 7 non-square, e.g. `(0.005, 0.003333)`, `(0.04, 0.045455)` |
| Empty tiles | 8 files are 368 bytes with 16 areas, 1 record and 0 entries: `world_73/74/75/76/81/82/83/85`; the client's own loader accepts them, so they are parsed, not rejected |

Not covered: the other terrain sets in the archive (`login_login`, the `a*`, `b*`
and `s*` instance tiles) - separate worlds that were not downloaded. The tool
accepts them unchanged.

Unsupported combinations fail naming file, area and layer
(`Unsupported(f"{path}: area {ai} layer {li} ...")`), never silently.

**World-to-texture:** the projection is per **layer**, not per area (755 masked
layers contradict their own base's axis). The two readers in the tree currently
disagree with each other on axis 1 - `src/terrain_gpu.rs` uses `(x, -z)` and
`src/material.rs` falls through to `(x, -z)` as well, but the GPU path's axis-0
and axis-2 branches (`(z, -y)`, `(x, -y)`) do not match `material.rs`'s
(`(z, -h)`, `(x, -h)`) - and axis 1 is the axis **every base layer uses**, so
this is not a corner case. Neither was changed: the projection is emitted by the
CPU mesh path (the vertex shader only forwards `oT0..oT5`), which is 2-A's
terrain mesh lane, not this one. Registered as the open gap below.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.06 | **No** | `tools/terrain-layer-survey.py` runs standalone (`--dir` archive mode added); `src/material.rs` untouched and `terrain_gpu.rs`'s `Layer::axis` projection deliberately **left alone** | T2MK reader `0x7a1599-0x7a18b7` (magic, version cap 2, 100-layer cap `0x7a1676`, 28-byte record `0x7a178c`, v2 grid byte `0x7a1662` -> `env+0x891`); T2LM reader `0x7a18c0` | archive pass executed: 88/88 files, 0 rejected, 1,408 areas, 5,929 entries, 4,521 zlib masks all 256², axes `{0:39,1:527,2:37}`, stacks 1..7, 162 textures, 31 uv pairs | the per-vertex world-to-texture projection for axis 1 (the only axis any base layer uses) is not recovered - it is emitted by the CPU mesh path 2-A owns; mask resolutions other than 256 have no instance to test; the `a*`/`b*`/`s*`/`login` terrain sets were not downloaded

## 5. Native addresses for each consequential constant and comparison

| Constant / comparison | Address | Value |
|---|---|---|
| variant switch | `0x7c1450` | 4-way, order `device+0xc`, `env+0x1b5`, `env+0x1b6` |
| device byte | `0x7c1455` | `[[0x9d30fc]+0xc]` |
| per-mask flag | `0x7c1465`, `0x7c170a` | `env+0x1b5`, `env+0x19c` |
| hardware flag | `0x7c1469`, `0x7c26ee` | `env+0x1b6` |
| dead-flag writer | `0x77d0ea` | `mov %bl,0x1b6(%esi)`, `%bl = 0` |
| FVF / stride | `0x7c0d99` | `0x34(,%eax,4)` with `sete` on `env+0x1b5` |
| cull / depth / lighting bracket | `0x7cd789`, `0x7cd793`, `0x7cd79d`, `0x7cd7c1`, `0x7cd7c9` | 1/1/1/1/1, restored `0x7cda41-0x7cda6c` |
| blend pair | `0x7c1509`, `0x7c1513`, `0x7c158c`, `0x7c1596` | 2/6, 5/6 |
| texcoord index | `0x7c2231`, `0x7c2239`, `0x7c2245`, `0x7cd937` | stage, stage, stage, `0x20000` |
| transform flags | `0x7c21f5`, `0x7c2201`, `0x7c220d`, `0x7cd946` | 0, 0, 0, `0x104` |
| colour op | `0x7c1e3f`, `0x7c2006`, `0x7c1f32` | 5 (`MODULATE2X`), 5, 4 (`MODULATE`) |
| constant block | `0x7c2824-0x7c287a` | `c0 = (1.0, env+0x1a0, [0x98f368], [0x9d30f8])` |
| layer mask selectors | `0x7c2331`, `0x7c2347`, `0x7c2368` | 1.0 bound / 0.0 absent, upload to `c2` |
| day/night factor | `0x7c2377`, `0x7c2392` | `env+0x198` broadcast to `c3` |
| direction build | `0x7b4eb0`, `0x7b4f22-0x7b4fbe` | `(env+0x194..0x19c)` normalised, zero if degenerate |
| direction consume | `0x7b4fc2-0x7b4fd9` | result written to `env+0x2e4` and `env+0x2a4` |
| direction storage | `0x7b42c7`, `0x7b42cd`, `0x7b42d3`, `0x7b4321` | ctor zeroes `env+0x198/0x19c/0x1a0` and `env+0x194` |
| direction restore | `0x7c4536`, `0x7c4561`, `0x7c4575`, `0x7c4579` | `env+0x198` vs `+0xbc`, `abs < 1/256 (`0x8f0ae4`)` then `* 255.0` (`0x8f0ae8`) |
| single writer offset | `0x7cfdd7`, `0x7cfde6` | **different object** - `env+0x198` of `0x7cfd30`'s target, not `A3DTerrain2Env` |

## 6. Tests executed and exact results

No Rust toolchain exists in this sandbox (`rustc`/`cargo` absent;
`static.rust-lang.org`, `static.crates.io` and `deb.debian.org` are unreachable),
so tests run only in CI. Everything below is a real result, not a plan.

| Command / check | Result |
|---|---|
| GitHub Actions run `41590855956` (`85c49dc`) | **success** - `Rust checks and tests (Linux)` and `Windows executable (x64)` |
| `cargo fmt --all -- --check` (inside that run) | pass |
| `cargo test --locked --bin perfect-world-explorer` (inside that run) | pass - all 15 `terrain_shading` tests plus the crate's existing tests |
| `cargo run --bin terrain-refraction-gpu-check`, `phase1-fog-gpu-check` (lavapipe) | pass - the **modified** `fs` and the fog functions compile as WGSL and render the expected pixels, so the arithmetic change is exercised on a GPU path, not only parsed |
| `naga-wasm` 30.2.0 translate of the `SHADER` block (local, `npm`) | parses and validates cleanly |
| `rustfmt` (wasm 0.1.0 oracle, local `npm`) | `terrain_shading.rs`, `terrain_gpu.rs`, `light_consumers.rs`, `main.rs` all byte-identical to the formatter output |
| `python3 tools/terrain-layer-survey.py --dir google_drive Findings/Phase-2-terrain-archive-survey.json` | OK - 88 files, 88 parsed, 0 rejected, 1,408 areas, 5,929 entries, 4,521 masks, all 256²; axes `{0:39,1:527,2:37}`; stacks 1..7; 162 textures |
| `python3 tools/terrain-layer-survey.py google_drive/world_35.t2mk --csv Findings/Phase-2-terrain-layer-survey.csv` | OK - 16 areas, 11 records, stacks `[4,4,5,5,5,6,6,6,6,6,6,6,6,6,6,7]`, masks `{0:1,256:10}`, zlib `{True:74}`, 90 CSV rows |
| `python3 -c` byte reads of the cited addresses | OK - `0x8f0ae4 = 0.00390625`, `0x8f0ae8 = 255.0`, `0x8effa0 = 1.0`, `0x8effc4 = 0.0` |

Earlier runs on this branch, for the record: `41545118770` and `41545476261`
failed only `Run viewer unit tests` (two hand-written assertions of mine were
wrong, both now fixed), and `41590609379` (`1bb2dad`) was fully green with the
production WGSL composite already in place.

Test list in `src/terrain_shading.rs` (15): `native_switch_selects_one_variant_per_state`,
`unreachable_hardware_lightmap_path_is_an_error_not_a_branch`,
`every_variant_carries_the_native_pass_state`,
`stage_counts_match_the_stage_loops`, `vertex_layout_follows_the_per_mask_flag`,
`day_and_night_maps_are_fixed_stages_not_swapped`,
`day_night_lightmap_selects_both_endpoints`,
`native_and_viewer_day_night_factors_agree_on_both_endpoints`,
`production_terrain_wgsl_runs_the_recovered_composite`,
`production_wgsl_matches_the_module_arithmetic`,
`compose_drops_the_specular_term_because_c0w_is_zero`,
`shader_constants_force_lane_zero_to_one`,
`packed_mask_weights_use_blue_green_red`,
`composite_matches_the_shader_ping_pong`, `atlas_tiles_follow_page_rows`.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
Tests | **Yes** (for what exists) | CI loop includes `terrain_shading`; two GPU regression binaries now compile the modified `fs` | n/a | run `41590855956` green end to end; 15 unit tests, 2 GPU regressions, WGSL validation and formatting all pass | the per-mask and no-pixel-pipeline variants still have no GPU regression, because they are not implemented

## 7. Remaining unsupported cases and interface changes

Shared register `Findings/Unsupported-client-paths.md` - **one row applied and
announced**, one proposed:

> Applied by this lane: | Terrain day/night factor | The client's factor is the y of the blended scene light direction (`env+0x198`, read by `ps t2_3layer_batch_14` as `c3.a`), and the viewer does not reconstruct `GEnv::UpdateSceneLight` (`0x7b4b00`), so `terrain_gpu.rs` still passes the app-level lightmap-availability scalar `sun.w` and uses `1-sun.w` as the blend. Authored lightmaps are otherwise untouched: the lightmapped terrain path adds no ambient, normal or scene-light term, so baked terrain is never lit twice. | P2.05 |

> Proposed, not applied: | Terrain hardware-lightmap variant | `0x7c2100` is selected only by `env+0x1b6`, whose single writer (`0x77d0ea`) stores zero, so the client cannot enter the path; it also clears both shader slots (`0x7c21ac`) yet uploads pixel constants (`0x7c22c2`). The viewer reports `VariantError::HardwareLightmapUnreachable` instead of drawing a substitute. | P2.04 |

Interface for other agents:

* `terrain_shading::select(Caps)` - the only variant decision; call it once at
  pipeline creation and treat `Err` as a hard error.
* `terrain_shading::pass_state(Variant)` - map `cull`, `z_enable`, `z_write`,
  `alpha_blend: false`, `alpha_test: false`, `specular_enable` straight onto
  `DepthStencilState`/`PrimitiveState`. Terrain must not gain a blend state.
* `terrain_shading::vertex_layout(per_mask)` - FVF/stride. 2-A's mesh lanes must
  supply the normal for the 3-layer layout and diffuse+specular for the per-mask
  layout; the two are not interchangeable.
* `terrain_shading::{day_night_lightmap, native_day_night_factor,
  viewer_day_night_factor, lightmap_uses_night, compose, packed_weight,
  composite_layers, Atlas}` - the arithmetic the WGSL `fs` reproduces (pinned by
  `production_terrain_wgsl_runs_the_recovered_composite` and
  `production_wgsl_matches_the_module_arithmetic`). Terrain on the lightmapped
  path is only baked: no `camera.light`, no `dot(normal, sun)`.
* **Interface request to whoever owns the camera uniform** (2-A/2-C or the
  camera lane): publish the blended scene light direction (`env+0x194..0x19c`
  equivalent, i.e. `RegionalLight::material_direction()` or its negated
  `toward_sun`) in a free `Camera` lane. `viewer_day_night_factor(sun.w)` then
  becomes `native_day_night_factor(direction)` with no other change, closing
  P2.05. Until then the shader keeps the documented stand-in.
* `terrain_shading::native::*` - addresses for the audit trail.

Open gaps, in priority order:

1. `src/terrain_gpu.rs` now runs the recovered composite for the lightmapped
   path (day at factor 0, night at factor 1, one `mul_x2`, no ambient, no normal
   dot, no scene light), and the point-light fallback is confined to the branch
   taken when no authored pair is bound. What is still an estimate is the
   **factor itself**: the shader uses `1 - camera.sun.w`, where `sun.w` is the
   viewer's lightmap-availability switch, because `GEnv::UpdateSceneLight`
   (`0x7b4b00`, whose `env+0x198` y is the native factor) is not reconstructed.
   Register row: `Findings/Unsupported-client-paths.md`, "Terrain day/night
   factor".
2. The pre-normalisation source of `env+0x194..+0x19c` is overwritten wholesale
   at `0x7b4e5d-0x7b4e75` from a 4x4 copy, so the scene-light-to-direction step
   before it is unidentified.
3. World-to-texture projection for axis 1 - the axis **every one of the 1,408
   surveyed area bases uses** - is not recovered. The archive shows the
   projection is per layer (755 of 4,521 masked layers use a different axis from
   their own base), and the two readers in the tree disagree with each other.
   The emitter is the CPU mesh path 2-A owns, so this needs a hand-off rather
   than a guess; `Layer::axis` was left untouched on both sides.
4. Per-mask (`t2_tex_merger`) texture arithmetic and the mask-stage loop
   (`0x7c25c0`) are recovered only as state, not implemented.
5. The survey covers all 88 `world_<n>`.t2mk` files; the `login_login`, `a*`,
   `b*` and `s*` terrain sets in the archive are separate worlds and were not
   downloaded, so no claim is made about them.

A green build does not complete these points: P2.04 is still "not finished"
because the per-mask layout (`t2_tex_merger`, `0x7c25c0`) is described but not
implemented, P2.05 because the blend **factor** is still the viewer's
lightmap-availability scalar rather than `env+0x198` (the arithmetic and the
endpoint order are now the recovered ones), and P2.06 because the archive survey
is complete but the axis-1 world-to-texture projection is still unrecovered.

## 8. Point summary

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.04 variant/state table | No | `select`/`pass_state` drive the main terrain pipeline's cull, depth-write and opaque target in `TerrainGpu::new`; the far pipeline and the per-mask layout are not yet table-driven | `0x7c1450` switch, four path bodies `0x7c14b0`/`0x7c1a20`/`0x7c1e00`/`0x7c2100`, `0x7cd760` state bracket, `0x7c0d99` FVF/stride, `0x7c4d90` module load, `0x77d0ea` proving `env+0x1b6` is never set | 4 of the 12 module tests cover the table; all 12 are CI-only, none ran in this sandbox | far-pipeline state, runtime per-mask layout selection, `t2_3layer_vertlit_14`/`t2_tm_lm` binding sites
P2.05 lightmap arithmetic | No | `terrain_gpu.rs` runs `mix(day, night, factor)*2` with no ambient/normal/scene-light term on the lightmapped path; the point-light fallback is confined to the no-authored-pair branch; the factor is `1 - sun.w` | `0x7c26e0` constant block (`c0 = 1.0, env+0x1a0, 0x98f368, 0x9d30f8`), `0x7c2377` `c3.a` from `env+0x198`, `0x7b4eb0`/`0x7b4f22`/`0x7b4fc2` direction build and normalise, `t2_3layer_batch_14` source | 4 tests (`lightmap_pair`, both endpoints, `compose`, `shader_constants`); CI-only | WGSL replacement; the pre-normalisation source of `env+0x194..+0x19c`; whether `UpdateSceneLight`'s inputs are the scene sun or a blended light set
P2.06 layer/mask/world-to-texture | No | `tools/terrain-layer-survey.py` standalone (new `--dir` archive mode); `src/material.rs` untouched, `terrain_gpu.rs`'s axis projection left alone | T2MK reader `0x7a1599-0x7a18b7` (magic, version cap 2, 100-layer cap `0x7a1676`, 28-byte record `0x7a178c`, grid byte `0x7a1662`), T2LM reader `0x7a18c0` | archive pass on the real client assets: 88/88 `world_<n>.t2mk`, 0 rejected, 1,408 areas, 5,929 entries, 4,521 zlib masks all 256x256, axes `{0:39,1:527,2:37}` with all 1,408 bases axis 1, stacks 1..7, 162 textures, 31 uv pairs; JSON + CSV written | the axis-1 world-to-texture projection is emitted by 2-A's CPU mesh path and is still unrecovered; other terrain sets (`a*`,`b*`,`s*`,`login`) not surveyed

## 9. Merge note

`main` removed `src/terrain_seam.rs` while this branch was open. The first merge
resolution kept `mod terrain_seam;`, which left an unresolved module: `cargo
build` and `cargo fmt --check` both fail on a missing module file, so the Windows
build and the formatting gate failed together even though the module itself
compiled in every earlier run (runs 41529404268, 41529782652, 41530630883 and
41531020549 all report `Windows executable (x64): success`). The merged branch
drops the stale `mod terrain_seam;` line. No terrain code changed for this.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
Merge repair | Yes | `src/main.rs` no longer names the deleted module; `terrain_gpu.rs` wiring restored | n/a - build-level fix | Windows build passed on all four pre-merge runs; Linux formatting now matches the wasm rustfmt oracle for all 63 formattable modules | CI re-run on the repair commit is the confirmation
