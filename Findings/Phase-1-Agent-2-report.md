# Phase 1 — Agent 2 report: regional lighting consumers, terrain/grass lightmap lifecycle, baked BMD materials

**Branch** `arena/b2d2741a-perfect-world-exlorer` · **PR** #2 · **Base** `main` (`9aea524`)
**Date** 2 November 2026 · **Scope** P1.04, P1.05, P1.06, P1.07

Phase 1 is **not** closed by this report. Three of the four requirements have a
working, tested production slice with explicit gaps; P1.05 is deliberately
restricted to the celestial consumer because no evidence supports driving
material lighting from the sky-object curve.

## 0. Verification basis and limits

* **No client executable or PCK is present in this workspace.** Every native
  address below is the *recorded* address from
  [`Research-log-02-rendering-and-lighting.md`](Research-log-02-rendering-and-lighting.md)
  and the audit tables; nothing here is a fresh disassembly, and no fresh
  private-asset scan was performed. `elementclient.exe` exists on the supplied
  Drive (6,000,640 B, 2019-05-03) but was not fetched into this workspace.
* **No Rust toolchain could be installed locally** (distribution hosts are
  unreachable and Actions artifact storage is blocked), so compilation and test
  execution happened in CI on the PR branch. Results are the CI results for the
  commit named in each check, not a local run.
* CI used: **run 41491443997** (commit `8c13497`). Both jobs green:
  * `Rust checks and tests (Linux)`: `Check formatting`, `Verify recovered
    regional arithmetic on x87`, `Test requirement status audits`, `Check all
    binary targets`, `Test GFX light dependency audit`, `Run viewer unit tests`
    (**251 passing**), `Run Phase 1 production shader regressions` — all success.
  * `Windows executable (x64)`: MSVC gamma-word capture/compare, standalone
    rustc module tests, release build, artifact + checksum — all success.
  * All thirteen GPU binaries ran green on lavapipe in that job:
    `display-gamma`, `post`, `bloom`, `bmd-baked`, `grass-instance`,
    `model-light`, `skin-pose`, `skin-palette`, `model-texture`,
    `model-receiver`, `model-draw`, `phase1-fog`, `terrain-refraction`.
  * Two temporary, review-only workflows were used to obtain CI feedback in this
    sandbox (job logs are not reachable from it). They are deleted before
    handoff and contribute no product code.

## 1. Task 1 — P1.04 / P1.05: regional lighting and sun-direction consumers

### What was added

`src/light_consumers.rs` (new) is the evidence-backed consumer matrix. Each row
names the native read that justifies it and the direction source a class may use.

| Consumer | Values it may read | Direction |
|---|---|---|
| Ordinary weighted model | ambient, diffuse, specular, authored direction | authored |
| Ordinary rigid model | ambient, diffuse, specular, authored direction | authored |
| Regional tree tint (viewer opt-in) | diffuse, authored direction | authored |
| Sun disc (experimental) | sky-object direction, celestial visibility | sky curve |
| Moon disc (experimental) | sky-object direction, celestial visibility | sky curve |
| Terrain lightmap | day/night factor | none |
| Grass lightmap | day/night factor | none |
| Baked BMD | day/night factor | none |
| Atmospheric fog | day/night factor | none |
| Underwater fog | day/night factor | none |
| Terrain unlit diagnostic (viewer) | — | none |

The tests fail if a baked/lightmapped/fog row ever gains an ambient, diffuse,
specular or authored-direction value, if any non-celestial row gains the sky
curve, if a visibility row appears outside the discs, or if the production
sources stop matching the wiring (`include_str!` guards over `gpu.rs`,
`terrain_gpu.rs`, `grass_instances.rs`, `far_trees_gpu.rs`).

`src/environment.rs` — direction and default-area ownership became typed and
explicit:

* `MaterialDirection` with `travel()` (D3DLIGHT8 direction, what
  `0x79b390 SetLightParam` receives) and `toward_sun()`. The two consumers keep
  opposite signs: materials receive *travel*, the disc azimuth receives
  *toward-sun*.
* `RegionalLight::material_direction()` reproduces the previous inline glam
  `normalize_or_zero` bit-for-bit (reciprocal multiply, `[0,0,0]` when the
  vector is zero/non-finite).
* `Selection { key, target, duration_ms, default_present }` is what both
  `Environments::endpoints()` and `RegionStreaming::endpoints()` now return, so a
  frame that finds **no** default area is distinguishable from one whose default
  area happens to be the local area.
* `FogTransition` publishes `celestial_flags` every frame **only** when a default
  box was actually found, installs the direction on key change **only** from that
  box, and falls back at construction to the viewer direction `[0,-1,0]` and the
  recovered constructor flag word `0x18`. Neither field can ever be filled from
  the local box's unrelated `+0x04/+0x74`.

`src/gpu.rs` — three consumer fixes:

* `retained_regional_light` keeps the last regional light so a frame without a
  selected region still has authored direction/flag data instead of dropping to
  a literal.
* Ordinary model lighting keeps `light.material_direction().travel()` (identical
  arithmetic to the previous `-normalize(toward_sun)`); the animated curve is
  still referenced exactly once, in the sky pass.
* The disc azimuth uses `toward_sun()`; the celestial visibility tick now reads
  the same retained light, so a hidden sun/moon never depends on region
  selection and never gates material light.

### Confirmation check — Task 1

| Field | Result |
|---|---|
| Task | P1.04 regional ambient/diffuse/specular + direction; P1.05 sun-curve consumer boundary |
| Finished | **No** — the ordinary receiving classes and the celestial boundary are done; overlap/priority variants, fixed-function alternatives and per-class audits remain open |
| Production integration | Yes: `environment::endpoints` → `FogTransition::{initial,update_endpoints}` → `RegionalLight` → `gpu::draw_models` / sky pass; `light_consumers::MATRIX` documents the allowed reads |
| Native evidence | default-vs-local split `0x41ebaf..0x41ec65`; init `0x41f244`; key change `0x41fd58/0x41fdb1`; changed-key frame skips shading `0x41fdbe`; flag extraction `0x420992..0x4209bf`; constructor flag default `0x4b2d0c`; visibility tick `0x4b5f63..0x4b6021`; alpha `0x4b3b80`; sky curve `0x4b60b0/0x4b60e0`; material light `0x4208d0..0x420db0`, `0x79b390` (all recorded, not re-derived) |
| Tests executed | `environment::tests::default_box_owns_flags_and_direction_even_for_a_local_region`, `missing_default_box_never_invents_flags_or_direction`, `losing_the_default_box_retains_the_last_ratified_direction_and_flags`, `default_visibility_flags_update_independently_of_material_light`, `teleport_reinitializes_and_interruption_retargets_in_flight_values`, `time_boundaries_stay_finite_periodic_and_asymmetric`, `material_direction_matches_the_previous_renderer_normalization`, `truncated_rejected`; `light_consumers::tests::*` (6) — all pass in the 250-test CI run |
| Remaining gaps | no evidence for additional material consumers of the sky curve; ordinary fixed-function/state-block variants not audited; trees stay opt-in; disc geometry/halo is separate work; nothing re-verified against a live client |

## 2. Task 2 — P1.06: terrain and grass lightmap lifecycle

### What changed

* `terrain_lightmaps::Availability` (`Unbound` 0 / `DayOnly` 1 / `Both` 3). The
  numeric values are the low two bits the grass shader already tests with
  `& 3u`, so they are pinned by a test. `grass_ready()` is true only for `Both`.
* `Pair::read_optional` implements the recovered per-area `ApplyLightMap`
  outcome: a missing **day** record leaves the area unbound (`Ok(None)`), a
  missing **night** record falls back to the day record, and every other I/O or
  format error is still returned. `Pair::read` keeps the strict behaviour for
  callers that require a day map.
* `terrain_gpu::load_lightmaps` now records unbound areas instead of failing the
  whole world, and fills their two atlas slots with a neutral 0.5 tile
  (`0x8410` BC1 blocks / RGBA `128,128,128,255`). The shader still samples every
  slot, so an unbound area renders at unit modulation rather than black; grass
  never reads those slots because their availability is 0.
* The startup log now reports authored-night, day-only and unbound counts.

### Confirmation check — Task 2

| Field | Result |
|---|---|
| Task | P1.06 terrain/grass lightmap selection, coordinates, lifetime |
| Finished | **No** — availability, missing-day handling, coordinates and sampling arithmetic are covered; dynamic page residency is not implemented |
| Production integration | Yes: `terrain_gpu::load_lightmaps` → `Availability::bits()` buffer → grass bind group 3; `terrain_gpu` lightmapped branch mixes `night,day` by `camera.sun.w` |
| Native evidence | `ApplyLightMap 0x7c2650` (missing night→day, missing day→unbound); T2LM `0x7a3268..0x7a3286`; grass needs both maps `0x652db4..0x652dc1`; CLAMP `0x651287..0x65129c`; area centre `0x652d99..0x652da0`; terrain night factor `0x42bd6e`; grass `0x42c10d`/`0x651510` |
| Tests executed | `terrain_lightmaps`: `missing_night_falls_back_for_terrain_without_marking_grass_ready`, `missing_day_and_non_missing_night_errors_are_not_hidden`, `missing_day_record_leaves_the_area_unbound_instead_of_failing`, `published_availability_bits_round_trip_and_gate_grass`, `independent_offsets_preserve_mask_indices`, `rejects_invalid_headers_ranges_and_overlap`, `rectangular_world_layout_is_bijective`, `rejects_inconsistent_layout`; `terrain_gpu::tests::neutral_tiles_match_the_atlas_block_layout`, `lod_grid_covers_exact_edges_without_duplicates`; `grass_instances::tests::unavailable_pairs_use_the_unlit_texture_and_never_the_region_light` (also asserts the assembled-shader injection that makes the declarations live) |
| Remaining gaps | the viewer still builds **one eager 4096² atlas** (`2·ceil(areas/256)` layers) for the whole world: there is no per-page load/unload, no runtime residency policy and no eviction test. The client's "missing day keeps the previously bound stage" behaviour is emulated with a neutral tile, not reproduced as a retained D3D texture. No fresh real-asset lightmap run was performed in this change. |

## 3. Task 3 — P1.07: baked BMD materials, alpha and draw state

### What changed

`src/bmd_baked.rs` (new) pins the draw state the production baked path uses:

* `classify(version, textured, two_sided)` accepts only the two mesh versions the
  reader was inspected against (`0x10000004`, `0x10000005`) and returns
  `Stage::{ColorOnly, Modulate2x}`, `AlphaTest::{None, TextureHalf}`,
  `Cull::{BackFace, TwoSided}` and the flat `kind` (`1.0`, `2.0`, `2.1`). A v4
  mesh claiming the v5 no-cull flag is refused rather than guessed.
* `blend(day, night, night_factor)` is a Rust mirror of `bmd_baked_rgb`:
  `(a*(255-w)+b*w)>>8`, including the recovered fact that a full-day blend is
  one 255th dark.
* `src/gpu.rs` world-mesh building now calls `bmd_baked::classify` and logs (not
  silently drops) a mesh it refuses, replacing the previous inline `2.0/2.1`.

### Confirmation check — Task 3

| Field | Result |
|---|---|
| Task | P1.07 baked-model interpolation, material flags, alpha, overrides |
| Finished | **No** — interpolation, two-sided/cull and the texture cutout are covered; the remaining stage/alpha/override set is not |
| Production integration | Yes: `gpu.rs` world builder → `kind` → shared world shader (`bmd_baked_rgb`, MODULATE2X `*2.`, `tex.a < 0.5`, winding drop for `kind < 2.05`) |
| Native evidence | baked arrays `+0x284/+0x288`; integer blend `0x7b9bc0`; day/night weight `0x42b4fe`; MODULATE2X `0x7a9730`; alpha registration `0x7a59c5..0x7a5b6f`; queue eligibility `0x7a5874..0x7a58c2`; v5 material `+0x21c`, flag `+0x26c`, culling `0x7a98c5..0x7a98d6`; DDS alpha flags `0x78ad42..0x78ad62`; world-object depth state `0x42b479..0x42b4a3` |
| Tests executed | `bmd_baked::tests::inspected_versions_classify_and_unknown_versions_are_refused`, `untextured_meshes_never_take_the_modulate_or_alpha_path`, `integer_blend_matches_the_recovered_arithmetic`, `production_wgsl_implements_the_classified_states`; plus the existing `bmd` reader tests (`reads_both_baked_light_arrays`, version-five material/truncation, NaN geometry) |
| Remaining gaps | multi-stage texture blending, `MODULATEALPHA`/alpha-op variants, client-specific material colour overrides for v5, the alpha distance cap `0x497423fe` on non-world queues, and blend/queue ordering for transparent baked content are **not** implemented. Depth (`Greater`, ref `0x54`) and the single opaque world queue are unchanged from before this task. |

## 4. Handoff

1. **Changed files.** `src/light_consumers.rs` (new), `src/bmd_baked.rs` (new),
   `src/environment.rs` (`MaterialDirection`, `Selection`, `FogTransition`
   retention, tests), `src/gpu.rs` (retained light, model/sky direction split,
   celestial flag retention, `bmd_baked::classify` in the world builder),
   `src/terrain_lightmaps.rs` (`Availability`, `read_optional`, tests),
   `src/terrain_gpu.rs` (unbound areas + neutral tiles + block-layout test),
   `src/grass_instances.rs` (`GRASS_LIGHT_WGSL` hoisted + availability test),
   `src/main.rs` (module registrations), `tools/check-phase1-core.sh` (module
   list).
2. **Behaviour.** Frames without a selected region no longer overwrite the
   ratified direction/flag word; a default box that leaves the active set keeps
   its last values; hidden discs still leave material light untouched; a
   lightmap area with no day map is unbound instead of aborting the world;
   grass keeps its unlit fallback unless both authored maps exist; baked meshes
   keep integer day/night blending and never enter the receiving path.
3. **Native addresses/assets.** See the tables above; all recorded in
   `Research-log-02-rendering-and-lighting.md`, none re-derived here. No new
   asset payloads were downloaded or committed.
4. **Tests and exact results.** CI run **41491443997** at `8c13497`:
   `cargo test --locked --bin perfect-world-explorer` = **251 passed, 0
   failed**; `cargo check --locked --bins` clean (warnings only); `cargo fmt
   --all -- --check` clean; all thirteen GPU regression binaries green on
   lavapipe; Windows release build and MSVCRT gamma cross-check green. One
   intermediate commit briefly dropped the grass shader's lightmap-injection
   patch while hoisting the WGSL constant; `grass-instance-gpu-check` caught it
   (`area LM clamp mode=false daylight=0: 222, expected 12`) and the injection is
   now restored and asserted by a test.
5. **Unsupported paths and assumptions.** (a) Terrain/grass/BMD dynamic
   residency is unimplemented — the atlas is eager. (b) Missing-day areas are
   emulated with a neutral tile, not a retained D3D texture. (c) `default_present`
   is a diagnostic accessor; the viewer does not yet surface it in the UI.
   (d) Baked multi-stage/alpha-op variants and v5 light overrides are refused or
   left unmodelled rather than guessed; `bmd_baked::classify` returns an error
   for out-of-set versions. (e) Nothing here was compared against a running
   client.
6. **Shared-interface changes.** `terrain_lightmaps::Pair::availability()`
   changed return type from `u32` to `Availability` (callers: `terrain_gpu`,
   tests); `Pair::read_optional` is new. `environment` gained `MaterialDirection`
   and an internal `Selection`; `FogTransition` gained a private
   `default_present` field plus accessor. `gpu.rs` gained
   `retained_regional_light` and now calls `bmd_baked::classify`. No WGSL
   uniform layout, no colour-space convention and no material constant layout
   was changed; shader arithmetic is unchanged.
7. **Final status.** P1.04 — **No** (slice complete, audit open). P1.05 —
   **No** (boundary enforced; no further consumers proven). P1.06 — **No**
   (availability/lifecycle slice; residency open). P1.07 — **No** (draw-state
   slice; full stage/override set open).

## 5. Recommended next work

1. Terrain/grass **residency**: per-page upload/eviction with a retained-binding
   policy, exercised by a headless harness that loads, drops and reloads a
   region and asserts the sampled texel.
2. Baked BMD **stages and overrides**: enumerate the remaining texture-stage ops
   and v5 material colour overrides from the recorded `0x7a9xxx` block, then
   extend `bmd_baked::DrawState` instead of adding more magic kinds.
3. Regional **overlap/priority** variants and fixed-function alternatives, with
   a real client comparison for the default-vs-local split.
4. Surface `FogTransition::default_present()` in the viewer status line so a
   reviewer can see when the fallback direction is in use.
