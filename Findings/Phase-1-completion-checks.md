# Phase 1 — completion checks

A requirement passes only if its entire scope is supported by implementation and evidence. “Ready?” below is an acceptance check, not a confidence estimate. Passing Cargo/CI is not a substitute for an unfinished renderer path.

| Task | Ready? | Check and result / next required work |
|---|---|---|
| P1.01 Complete scene-light selection | No | Grid/candidate/tie/range/flags and publication lifecycle have eleven passing CPU regressions and executable traces. Native-verified consume/reset/unregister bridge is implemented for world receivers, GPU bridge CI 41460362278 passed; same-frame multipass CI 41461016032 also passed. Retail light-element parsing/execution now covers all 546 actual archive elements, including native controller-108 LUTs; 19 combined runtime/selection CPU tests pass and decoded-light GPU/Windows CI 41463476329 passed. General GFX graph/scene/model-event ownership and resolved-parent lifecycle integration remain open. |
| P1.02 Every receiving class and its calculations | No | Ordinary skin/rigid material and light variants have a 47-case RGBA/specular GPU comparison. The world corpus is 14 skins, 41 meshes and 23 materials. Successful-load eligibility and ordinary render defaults are now traced; retained poses and 41 DDS references/383 authored mip levels validate. Direct-skeleton rigid pose/draw support is now implemented with inverse bind and virtual slot; 48 native CPU tests and GPU/Windows CI 41459370262 passed. Full receiving-class/draw integration remains open. |
| P1.03 Model shader integration without double lighting | No | Baked BMD and terrain remain unmodified by point lights. Retained mesh/material/texture GPU draws and atomic pose/frame updates now pass CI 41448989595; the viewer retains actual ECWLD model dependencies. Ordinary world receivers now default on and submit main/reflection/underwater-camera refraction with source clocks and initial phase ticks. Those GPU pass checks passed CI 41452346626. Full material/queue coverage and runtime FX light integration remain open. Default/UI/status CI 41453546142 passed; opaque descriptor RGB scaling passed CI 41454850805. |
| P1.04 Regional environment lighting | No | Authored endpoints, clock curve and transitions exist. Full asynchronous region ordering and all material consumers remain unverified. |
| P1.05 Sun curve at proven material consumers only | No | Sky uses the recovered curve without relighting baked surfaces. Material-consumer tracing is not complete. |
| P1.06 Terrain/grass lightmaps | No | Area-center selection and CLAMP U/V are now traced; the client path defaults on with a six-case tile-edge/day-night GPU regression (CI 41436006986 passed). Checked rectangular mapping and independent T2LM record-length parsing now have four CPU regressions. Actual TRN2 maps 88 subterrain pairs to 32x44 mask areas; All 176 payloads / 2,816 DDS tiles now validate, with zero missing/invalid records ([manifest](Phase-1-lightmap-payload-manifest.csv)). Per-area grass availability and terrain-only missing-night fallback passed CI 41456559295. Missing-day terrain binding and dynamic loading/unloading lifecycle remain open. |
| P1.07 Baked-model lighting/material/alpha overrides | No | Existing integer endpoint blend was rechecked against `0x7b9bc0`; now its GPU implementation stays integer until normalization, with a new exhaustive byte-weight regression. Opaque descriptor RGB scaling is native-verified and passed CI 41454850805. Retail alpha-list insertion now has CPU regressions (including the 1.4.2 versus 1.5.2 weight/AABB difference); it is not yet connected to alpha scene submission. Complete material stages/alpha/overrides remain open. |
| P1.08 Bloom | No | Corrected tap spacing to `4/full_resolution` from `0x40edd0` and `0x40f053`. Patterned-image GPU tests passed on UNORM/sRGB and odd dimensions; the CPU fixture also rejects the old offsets. Recovered and corrected underwater distortion before downsample/glow; added independent quantized-warp pattern regressions (CI 41436969265 passed). Traced special modes 4/5 now have explicit preview controls, transition tests and GPU endpoint fixtures (CI 41438393668 passed). Caller tint and independent brightness/opacity fade inputs are now wired into the renderer with encoded alpha blending and 144 patterned GPU cases (CI 41440063378 passed). The +6 postprocess flag is now identified and implemented as the authored fullscreen sharpen pass, after encoded glow and before display gamma; CI 41457607134 passed all GPU tests and Windows. Automatic fade triggers/timing and further effect interactions remain open. |
| P1.09 Gamma and color chain | No | Windowed gating and final fullscreen ramp are corrected and tested; encoded UNORM/sRGB output tests pass. This is not verification of every upstream material color conversion and Windows compositor path. |
| P1.10 Per-pass and underwater fog | No | Main per-material fog exists. Grass refraction uses traced exponential vertex fog in the actual clipped pass (CI 41434813651 passed). Opaque BMD refraction has its distinct above-plane-depth clamp (CI 41435206213 passed). Near/far terrain has the traced plane projection, depth darkness and vertex fog (CI 41435695632 passed). Complete material-variant/underwater pass coverage remains open. |
| P1.11 Preserve approved trees while tracing separately | Yes, preservation requirement | Tree shader, geometry and default lighting were not changed. Existing fallback remains; the incomplete replacement has not been enabled or represented as parity. This does not mean the full tree material path is recovered. |
| P1.12 Automated tests in requested categories | Yes, category coverage | CPU tests cover selection, transitions and color arithmetic; CI exercises production gamma, postprocess, fog and model-light shaders. New bloom UV and BMD byte-blend checks extend coverage. These tests do not close the implementation gaps in P1.01–P1.10. |

## New verified work

- PR title: **Phase 1 — Lighting and display pipeline**. No “in progress” label in the title or description heading; the remaining gates are still stated explicitly.
- Commit `909df63`: arbitrary-size bloom offset correction and independent patterned-image reference. [CI run 41431460966](https://github.com/kasperfriend/Perfect-World-Exlorer/actions/runs/41431460966) passed Linux compilation/unit/GPU checks and the Windows executable build.
- Follow-up: production BMD blend uses integer channel extraction/products/shifts rather than unpack-normalize-multiply-floor round trips. `bmd-baked-gpu-check` compares 2,048 cases (eight endpoint pairs × every 8-bit night weight), without a tolerance. CI also now executes the existing grass/BMD rendered-image regression. Results are tracked on the PR; do not infer a pass before the run finishes.
- Dependency-free core script includes four CPU bloom reference tests, including an explicit rejection of the old tap-spacing formula.

## Exit condition

**Not met.** P1.01–P1.10 must all pass their full acceptance checks before claiming Phase 1 completion. No empty-list trick, renamed status or passing arithmetic test changes that condition.

### Receiver/material follow-up

- `3ed6592` recovers non-specular skin/rigid material constants and the
  ordinary-versus-point-light alpha distinction. [CI 41432875230](https://github.com/kasperfriend/Perfect-World-Exlorer/actions/runs/41432875230)
  passed Linux checks (including the expanded 31-case RGBA GPU comparison)
  and the Windows executable build.
- [Actual model/material census](Phase-1-model-material-census.md) records
  the full-archive and world-only checks, including two malformed archive
  records. The world's 14 referenced skins/41 skin meshes/23 materials pass
  the parser/material-binding check. This does not close P1.03's renderer gate.

- `184f7d8`: [CI 41433732886](https://github.com/kasperfriend/Perfect-World-Exlorer/actions/runs/41433732886) passed Linux checks, the 47-case model shader comparison, and the Windows build.

## Confirmation gate

Run `python3 tools/check-phase1-ready.py` before each confirmation checkpoint.
It returns nonzero while any full-scope requirement is No, missing,
duplicated or invalid. A checkpoint in that state must report **No — continue
implementation**, not offer completion approval. Repeat implementation and
verification when No remains. Only an all-Yes audit can justify the completion
confirmation poll. Build success cannot override an unfinished requirement.
The script checks recorded acceptance decisions, not renderer parity on its
own; evidence still has to justify every Yes. Confirmation cannot waive a No.

Latest checkpoint: the user selected **No — continue implementation**.
The subsequent cycle implements caller glow modulation/fade inputs and
keeps the unresolved full-scope gates open.

2026-11-02 continuation confirmation: **No — continue implementation**.
Core 83 tests; combined runtime/selection 19; combined world/model/scene
50 after preserving complete scene-effect owner records. Retail GFX light
execution and native controller-108 are now verified through production
weighted/rigid receiver GPU pixels. CI 41464185202 is green for e59268e.
Owner record retention does not close graph/scene/model-event lifecycle gaps.
The readiness audit still has ten No gates; no full-scope No was relabeled Yes.
