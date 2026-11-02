# Phase 1 audit — 2 November 2026

This is a source/evidence audit, not a new client comparison. Phase 1 is **incomplete**; later phases are not signed off. Historical research logs describe earlier revisions and must not be read as the current implementation checklist. No percentage is assigned.

## Requirement-by-requirement findings

| Requirement | Existing evidence / implementation | Remaining acceptance work |
|---|---|---|
| Complete scene-light selection | `scene_lights.rs` parses records, builds 10×10 camera grid, caps runtime candidates at three per cell, selects strictly nearer candidates, scales flag-1 static lights by night and overrides attenuation. Synthetic unit tests exist. | Recover producer lifetimes and complete object consumers, validate actual runtime input ordering. Selection is not a range cutoff; distinguish shader and fixed-function paths. |
| Identify receiving classes and material calculations | `model_light.rs` has isolated CPU/WGSL arithmetic; `scene-model-light-survey` queries world placements diagnostically. | Class/state eligibility, material-combined constants, fixed-function range/ambient/diffuse semantics are not fully established. A successful diagnostic candidate is not proof of a receiving class. |
| Integrate lights without double-lighting | BMD baked colors and terrain lightmaps already render. | Eligible animated world renderer is absent. Do not wire point lights into BMD, terrain or approved trees to make this box appear complete. |
| Regional ambient, direction, overlap and transitions | `environment.rs`, `client_region_map.rs`, optional streamed selection in `gpu.rs`; authored endpoints and durations, optional regional tree tint. | Asynchronous insertion order and full material consumers remain unverified. Tree tint is an opt-in approximation. |
| Sun curve only at proven material consumers | `client_sun.rs` recovers sky-object direction; optional sky discs consume it. | No proof that this sky-object curve should drive every material. Leave baked material lighting unchanged. |
| Terrain/grass lightmaps, coordinates and sampler arithmetic | `terrain_gpu.rs` and `gpu.rs` implement day/night maps; grass area-center selection is optional. | Exact atlas mapping, boundary addressing, unloaded-page rules, sampler state and specular variants need client verification. |
| Baked model interpolation, flags and alpha | `bmd.rs`, `world_objects.rs`, `gpu.rs`; dual color arrays, per-mesh two-sided flag, cutouts and day/night blend. | All material/texture stages, alpha state and client-specific overrides are not complete. |
| Bloom constants, UVs, formats, transitions, order | `post_gpu.rs` has recovered blur weights and ordinary/underwater glow transitions. Existing `post-gpu-check` exercises arithmetic. | Exact target formats, texel alignment/filtering, special modes and original full pass ordering remain open. |
| Gamma/display chain | `display_gamma.rs` renders composed fullscreen world/UI through encoded UNORM input and optional sRGB output decode. | This run fixes the windowed scene-only ramp discrepancy. Monitor/compositor behavior, other color conversions and live Windows comparison remain unverified. See changes below. |
| Applicable per-pass fog | Current terrain/world/tree shaders already contain material fog; `phase1-fog-gpu-check` exercises production functions. `post_gpu.rs` retains screen-fog support. | Do not infer the entire pipeline is still screen-space from old log entries. Complete pass/variant coverage, reflection, transparency and underwater ordering still require tracing and comparison. |
| Preserve approved trees | Existing default tree lighting and geometry are left untouched. Regional tint remains optional. | Complete branch/frond/leaf/billboard material reconstruction before replacing the fallback. |
| Automated validation | Existing unit tests and diagnostic GPU binaries cover portions of these paths. | Tests do not establish live-client parity. Add real-asset/reference coverage, execute all GPU checks, and validate Windows presentation. |

## Concrete correction in this run

The UI previously enabled `camera.client_gamma` in windowed mode. That reached the scene postprocess, applying a gamma curve before overlays. The recorded executable evidence at `0x77b7c0` says the windowed device path skips ramp construction. The viewer now enables the emulated display ramp **only when both the option and fullscreen are enabled**. Scene-only gamma is disabled in the interactive viewer. Fullscreen still applies it once after world, postprocess, map and UI composition. The postprocess diagnostic API retains its explicit gamma argument for arithmetic testing, not as a windowed viewer feature.

The display and diagnostic post shaders now share one WGSL LUT implementation in `gamma_curve.rs`, with a CPU oracle for the recovered integer setting, 16-bit entries, interpolation and sRGB decoding. Invalid settings uploaded to either pass fall back to 41. Defaults and saved-setting keys have not changed. The graphics label now states “fullscreen only.”

### Validation actually executed

A local Rust 1.88 compiler and standard library were acquired from npm-hosted `@rustbin` component archives into ignored `.cache/`; no repository build dependencies were altered.

Standalone `rustc --test` runs:

- `gamma_curve.rs`: **4 passed** (all 101 settings × 256 LUT entries for monotonicity/endpoints; mode gating; integer/fallback settings; interpolation and sRGB boundary).
- `scene_lights.rs`: **4 passed**.
- `client_region_map.rs`: **2 passed**.
- `client_sun.rs`: **1 passed**.
- `view_settings.rs`: **2 passed**.

These 13 tests do not compile the application or validate WGSL. Full Cargo/GPU/Windows validation was **not run** in this environment. The new `display-gamma-gpu-check` binary tests the actual production pass on UNORM and sRGB targets, resize/view recreation, RGB endpoints, near-black values, integer settings and invalid-setting fallback (40 readback cases, ±1 encoded byte tolerance). It is provided but **not yet executed**.

## Client access and sequencing

Drive listing confirmed `Games/pw142/element` and split `models.zip` / `litmodels.zip` archives in `Games`. Listing metadata is not asset inspection; no new executable bytes or client files were acquired or analyzed in this run. No proprietary assets have been added to Git.

The eight-phase checklist is preserved in [Eight-phase-status.md](Eight-phase-status.md). Phase 1 remains the gate. This run does not implement or claim completion of terrain parity, static-material parity, vegetation runtime parity, sky/weather, water, animated world models/effects, full-world optimization or Windows delivery. Completing those requires substantially more reverse engineering and validation than a gamma correction or a parser survey.
