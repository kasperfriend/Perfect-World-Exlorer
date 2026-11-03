# Phase 3 Agent C test artifact (P3.06, P3.07, P3.09)

This is the acceptance artifact for the three designated Agent C points. It is
the compiled-viewer/CI test artifact, not a separate test harness: every command
below exercises code reachable from `src/main.rs`'s normal world path or the
same standalone modules that CI compiles.

## What to test

1. Open the viewer from PR #12 (`https://github.com/kasperfriend/Perfect-World-Exlorer/pull/12`)
   with the shipped `configs/angelica.cfg` (`WaterEffect == 0`,
   `detail_scene = 1` → quality preset 2).
2. Move the camera across a region boundary. The World tab's **Scenery quality
   preset** slider changes all ten `CECSceneBlock` class radii; the log line
   reports `10 native classes`, the tree LOD scale, resident placements, loads,
   evictions and distance culls. `Objects::load_ids` retires the ornament
   models/textures that left the resident set.
3. Compare main, fog, reflection, refraction, minimap and full-map views. Hidden
   / reflect-only / refract-only EC-BSD placements are gated by `Visibility`;
   home ornaments and EC models are hidden in reflection/refraction at the
   shipped default. All six views reuse one `FrameSnapshot` (no second time or
   light publication).
4. Run the CI checks below. The Windows release build is the compiled viewer
   artifact; the Linux job runs the static-model GPU programs under software
   Vulkan.

## Commands and local results (2026-11-03)

```
rustc --edition=2021 --test src/scenery_visibility.rs   → 10 passed; 0 failed
rustc --edition=2021 --test src/view_settings.rs        → 19 passed; 0 failed
sh tools/check-phase1-core.sh                            → 27 standalone test binaries, all ok
python3 tools/check-eight-phase-status.py                → consistent true, 23 / 1 / 52, errors []
python3 tools/audit-static-model-gpu-programs.py         → OK: 7 wired, 16 excluded with reasons, 23 total
python3 tools/test_phase1_ready.py                      → 7 OK
python3 tools/test_eight_phase_status.py                → 5 OK
rustfmt --edition 2021 --config skip_children=true --check <changed Rust files> → exit 0
git diff --check                                        → exit 0
Tree-sitter parse of changed Rust files                  → errors 0
```

## CI acceptance

The `pr-checks.yml` workflow for the final commit is recorded here after the
run completes:

- Run: `CI_RUN_ID`
- Linux job: `CI_LINUX_JOB`
- Windows job: `CI_WINDOWS_JOB`
- Conclusion: `CI_CONCLUSION`
- `gh pr checks 12`: `CI_PR_CHECKS`

Workflow steps include `cargo fmt --all -- --check`,
`cargo check --locked --bins`, `cargo test --locked --bin gfx-light-audit`,
`cargo test --locked --bin perfect-world-explorer`, all 17 GPU diagnostic
programs under software Vulkan, the eight-phase and static-model audits, the
standalone `rustc --test src/scenery_visibility.rs` run, and the Windows release
build. Relevant GPU evidence: `scenery-pass-gpu-check` enumerates 6 passes x 6
recovered BMD states (36 rows) plus hidden/reflect-only/refract-only raster
gating and a real eviction; `bmd-depth-gpu-check` rasterizes two coincident
triangles under `GREATER_EQUAL`; `bmd-baked-gpu-check` has 2,048 bit-exact
day/night cases.

## Status

`Findings/Phase-3-completion-checks.md` records the integrated review result:
P3.06, P3.07 and P3.09 are **Yes** for their designated scope. Other Phase 3
rows and the phase completion gate are untouched. Remaining non-designated gaps
are registered in `Findings/Unsupported-client-paths.md`.
