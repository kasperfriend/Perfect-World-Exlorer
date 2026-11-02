# Phase 1 — completion checks

A requirement passes only if its entire scope is supported by implementation and evidence. “Ready?” below is an acceptance check, not a confidence estimate. Passing Cargo/CI is not a substitute for an unfinished renderer path.

| Task | Ready? | Check and result / next required work |
|---|---|---|
| P1.01 Complete scene-light selection | No | Grid/candidate/tie/range/flags arithmetic has eight passing CPU regressions and executable traces. Full runtime candidate production/lifecycle is still not integrated. |
| P1.02 Every receiving class and its calculations | No | Ordinary skin/rigid material and light variants have a 47-case RGBA/specular GPU comparison. The world corpus is 14 skins, 41 meshes and 23 materials; full receiving-class eligibility/integration remains open. |
| P1.03 Model shader integration without double lighting | No | Baked BMD and terrain remain unmodified by point lights. The eligible animated-world model renderer is absent; a standalone lighting helper is not integration. |
| P1.04 Regional environment lighting | No | Authored endpoints, clock curve and transitions exist. Full asynchronous region ordering and all material consumers remain unverified. |
| P1.05 Sun curve at proven material consumers only | No | Sky uses the recovered curve without relighting baked surfaces. Material-consumer tracing is not complete. |
| P1.06 Terrain/grass lightmaps | No | Area-center selection and CLAMP U/V are now traced; the client path defaults on with a six-case tile-edge/day-night GPU regression (CI 41436006986 passed). Full atlas/data mapping census and unloaded-pair fallback remain open. |
| P1.07 Baked-model lighting/material/alpha overrides | No | Existing integer endpoint blend was rechecked against `0x7b9bc0`; now its GPU implementation stays integer until normalization, with a new exhaustive byte-weight regression. Complete material stages/alpha/overrides remain open. |
| P1.08 Bloom | No | Corrected tap spacing to `4/full_resolution` from `0x40edd0` and `0x40f053`. Patterned-image GPU tests passed on UNORM/sRGB and odd dimensions; the CPU fixture also rejects the old offsets. Recovered and corrected underwater distortion before downsample/glow; added independent quantized-warp pattern regressions (CI 41436969265 passed). Traced special modes 4/5 now have explicit preview controls, transition tests and GPU endpoint fixtures (CI 41438393668 passed). Caller tint and independent brightness/opacity fade inputs are now wired into the renderer with encoded alpha blending and 144 patterned GPU cases (validation pending). Automatic fade triggers/timing and further effect interactions remain open. |
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
