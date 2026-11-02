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
| `src/terrain_gpu.rs` | **cross-lane edit, announced**: `TerrainGpu::new` now derives primitive, colour-target, depth and cull state from `terrain_shading::select`/`pass_state` instead of restating them | 2-A/2-C read the same pipeline; no mesh, texture, sampler or submit code touched |
| `src/main.rs` | `#[allow(dead_code)] mod terrain_shading;` added beside the other attributed modules (the address table and the reference arithmetic are not all reachable from the viewer yet) | binary crate root |
| `tools/check-phase1-core.sh` | `terrain_shading` added to the dependency-free module loop | CI `pr-checks` |
| `tools/terrain-layer-survey.py` | **new** - T2MK/T2LM survey with explicit `Unsupported` errors | manual / CI survey |
| `Findings/Phase-2-terrain-variant-state-table.md` | new - P2.04 table | - |
| `Findings/Phase-2-terrain-layer-survey.md`, `Findings/Phase-2-terrain-layer-survey.csv` | new - P2.06 survey | - |

The `src/terrain_gpu.rs` edit stays inside the 2-B lane (pipeline and state
setup only). It adds no mesh construction, no texture/mip/sampler creation and no
view submission, and it leaves the WGSL untouched.
`Findings/Unsupported-client-paths.md` needs one new row (section 7); it was not
edited because that register is shared and the edit is proposed, not applied.

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
| ambient / regional light | `light` uniform multiplication | **no ambient term** - `mul r0, r0, v0` is a multiply by 1.0 | `t2_3layer_batch_14` tail |
| layer weights | per-layer mask alpha | packed mask **B/G/R = layers 1/2/3** | shader comments + `0x7c25c0` stage installs |
| mask `c2` | scale | 1.0/0.0 selector per bound layer | `0x7c2331`/`0x7c2347`, uploaded to `c2` at `0x7c2368` |

Pair choice: `t4` = day, `t5` = night (`0x7c23a2`/`0x7c23b0` install stages 4 and
5); `lrp_sat r5, c3.a, r5, r4` means factor 0 -> day, 1 -> night, clamped. Tile
addressing is per area through the mask slot, from the same counter as
`0x7cd937` (`D3DTSS_TCI_CAMERASPACE`) and `0x7cba30` (per-area uv). The result is
`(layers + lightmap*specular)` `* 1.0` `* lightmap * 2`, saturated.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.05 | **No** | `day_night_lightmap`, `lightmap_uses_night`, `compose`, `shader_constants` exist and are tested; the WGSL `fs` still uses `mix(night,day,sun.w)*2` | `0x7c26e0` constant block, `0x7b4eb0` direction build, `0x7b4fc2` normalise, `0x7c2377` `c3.a` | `day_and_night_maps_are_fixed_stages_not_swapped`,
`day_night_lightmap_selects_both_endpoints`, `compose_drops_the_specular_term_because_c0w_is_zero`, `shader_constants_force_lane_zero_to_one` - CI only | what feeds `UpdateSceneLight` before `0x7b4e60` (`env+0x194..+0x19c` are overwritten wholesale by a 4x4 copy there, so the pre-normalised source is still open); the WGSL still has to use the recovered factor `env+0x198` instead of `sun.w`

## 4. Layer, mask and world-to-texture survey (P2.06)

Full survey in [Phase-2-terrain-layer-survey.md](Phase-2-terrain-layer-survey.md);
row data in [Phase-2-terrain-layer-survey.csv](Phase-2-terrain-layer-survey.csv).

Covered: 1 file (`world_35.t2mk`, v2), 16 areas, 90 area-layer entries, 11 layer
records, 10 texture indices, 74 masks - all 256x256 and all zlib, decoding to
exactly `size*size`. Stacks are 4-7 layers with a mask-free base. Axes present:
1 (9 records at stack level: 8 distinct records), 0 (2), 2 (1).

Not covered: every other `.t2mk` in the archive (the 32x44 grid needs 176 files /
2,816 tiles), every `.t2lm` except `world_39`, mask resolutions other than 256,
and axis bytes outside 0/1/2.

Unsupported combinations fail naming file, area and layer
(`Unsupported(f"{path}: area {ai} layer {li} ...")`), never silently.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.06 | **No** | `tools/terrain-layer-survey.py` runs standalone; nothing in `src/material.rs` changed | T2MK reader `0x7a1599-0x7a18b7` (magic, version cap, 100-layer cap, 28-byte record, `rep movsl` 7 dwords); T2LM reader `0x7a18c0` | survey executed on `world_35.t2mk`: 16 areas, 11 layers, stacks 4/5/6/7 = 2/3/10/1, masks 0x1/256x10, zlib 74/74, outputs above | archive-wide survey; `src/terrain_gpu.rs` axis mapping (`0 -> (z,-y)`, `1 -> (x,-z)`, `2 -> (x,-y)`) does not match the recovered axial list and must not be changed until the sprite-side reader for the same byte is checked

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
so the unit tests did **not** run here. They run in CI through
`tools/check-phase1-core.sh`, which now includes `terrain_shading`.

| Command | Result |
|---|---|
| `python3 tools/terrain-layer-survey.py google_drive/world_35.t2mk --csv ...` | OK - 16 areas, 11 layers, stacks `[4,4,5,5,5,6,6,6,6,6,6,6,6,6,6,7]`, axes `{0:2,1:8,2:1}`, masks `{0:1,256:10}`, zlib `{True:74}`, 90 CSV rows |
| `python3 -c` byte reads of the cited addresses | OK - `0x8f0ae4 = 0.00390625`, `0x8f0ae8 = 255.0`, `0x8effa0 = 1.0`, `0x8effc4 = 0.0` |
| `rustc --test src/terrain_shading.rs` | **not run** - no toolchain |

Test list in `src/terrain_shading.rs` (12): `native_switch_selects_one_variant_per_state`,
`unreachable_hardware_lightmap_path_is_an_error_not_a_branch`,
`every_variant_carries_the_native_pass_state`,
`stage_counts_match_the_stage_loops`, `vertex_layout_follows_the_per_mask_flag`,
`day_and_night_maps_are_fixed_stages_not_swapped`,
`day_night_lightmap_selects_both_endpoints`,
`compose_drops_the_specular_term_because_c0w_is_zero`,
`shader_constants_force_lane_zero_to_one`,
`packed_mask_weights_use_blue_green_red`,
`composite_matches_the_shader_ping_pong`, `atlas_tiles_follow_page_rows`.

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
Tests | **No** | CI loop updated | n/a | survey run locally (above); 11 Rust tests unexecuted here, CI-only | first CI run must confirm the module compiles standalone

## 7. Remaining unsupported cases and interface changes

Proposed row for `Findings/Unsupported-client-paths.md` (not applied - shared
register):

> | Terrain hardware-lightmap variant | `0x7c2100` is selected only by `env+0x1b6`, whose single writer (`0x77d0ea`) stores zero, so the client cannot enter the path; it also clears both shader slots (`0x7c21ac`) yet uploads pixel constants (`0x7c22c2`). The viewer reports `VariantError::HardwareLightmapUnreachable` instead of drawing a substitute. | P2.04 |

Interface for other agents:

* `terrain_shading::select(Caps)` - the only variant decision; call it once at
  pipeline creation and treat `Err` as a hard error.
* `terrain_shading::pass_state(Variant)` - map `cull`, `z_enable`, `z_write`,
  `alpha_blend: false`, `alpha_test: false`, `specular_enable` straight onto
  `DepthStencilState`/`PrimitiveState`. Terrain must not gain a blend state.
* `terrain_shading::vertex_layout(per_mask)` - FVF/stride. 2-A's mesh lanes must
  supply the normal for the 3-layer layout and diffuse+specular for the per-mask
  layout; the two are not interchangeable.
* `terrain_shading::{day_night_lightmap, lightmap_is_night, compose,
  packed_weight, composite_layers, Atlas}` - the arithmetic the WGSL `fs` must
  reproduce. In particular `terrain_gpu.rs` must stop multiplying terrain by
  `camera.light` or by `max(dot(normal, sun), 0)`: terrain is never normal-lit and
  never point-lit, only baked.
* `terrain_shading::native::*` - addresses for the audit trail.

Open gaps, in priority order:

1. `src/terrain_gpu.rs` still applies `mix(night, day, sun.w)`, a `baked`
   multiply and a regional `light` term. P2.05 arithmetic is recovered but **not
   integrated**; until it is, the viewer's terrain lighting is still an estimate.
2. The pre-normalisation source of `env+0x194..+0x19c` is overwritten wholesale
   at `0x7b4e5d-0x7b4e75` from a 4x4 copy, so the scene-light-to-direction step
   before it is unidentified.
3. The axis mapping in `terrain_gpu.rs` does not match the surveyed axial list.
4. Per-mask (`t2_tex_merger`) texture arithmetic and the mask-stage loop
   (`0x7c25c0`) are recovered only as state, not implemented.
5. Archive-wide T2MK/T2LM survey (175 more `.t2mk` files) not run.

A green build does not complete these points: P2.04 is still "not finished"
because the per-mask layout and the far pipeline are not driven by the table,
P2.05 because the WGSL arithmetic is not yet the recovered one, and P2.06 because
one file out of 176 is surveyed.

## 8. Point summary

Task | Finished Yes/No | Production integration | Native evidence | Tests executed and results | Remaining gaps
P2.04 variant/state table | No | `select`/`pass_state` drive the main terrain pipeline's cull, depth-write and opaque target in `TerrainGpu::new`; the far pipeline and the per-mask layout are not yet table-driven | `0x7c1450` switch, four path bodies `0x7c14b0`/`0x7c1a20`/`0x7c1e00`/`0x7c2100`, `0x7cd760` state bracket, `0x7c0d99` FVF/stride, `0x7c4d90` module load, `0x77d0ea` proving `env+0x1b6` is never set | 4 of the 12 module tests cover the table; all 12 are CI-only, none ran in this sandbox | far-pipeline state, runtime per-mask layout selection, `t2_3layer_vertlit_14`/`t2_tm_lm` binding sites
P2.05 lightmap arithmetic | No | reference functions only; WGSL still uses `mix(night, day, sun.w)*2` and a regional `light` term | `0x7c26e0` constant block (`c0 = 1.0, env+0x1a0, 0x98f368, 0x9d30f8`), `0x7c2377` `c3.a` from `env+0x198`, `0x7b4eb0`/`0x7b4f22`/`0x7b4fc2` direction build and normalise, `t2_3layer_batch_14` source | 4 tests (`lightmap_pair`, both endpoints, `compose`, `shader_constants`); CI-only | WGSL replacement; the pre-normalisation source of `env+0x194..+0x19c`; whether `UpdateSceneLight`'s inputs are the scene sun or a blended light set
P2.06 layer/mask/world-to-texture | No | `tools/terrain-layer-survey.py` standalone, `src/material.rs` untouched | T2MK reader `0x7a1599-0x7a18b7` (magic, version <= 2, 100-layer cap, 28-byte record, v2 grid byte `env+0x891`), T2LM reader `0x7a18c0` | survey of `world_35.t2mk` executed: 16 areas, 11 layer records, 90 area-layer entries, stacks 4/5/6/7 = 2/3/10/1, masks 0 or 256x256, 74/74 zlib; 90-row CSV written | 175 of 176 `.t2mk` files unsurveyed; the `terrain_gpu.rs` axis mapping contradicts the surveyed axial list; mask sizes other than 256 untested

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
