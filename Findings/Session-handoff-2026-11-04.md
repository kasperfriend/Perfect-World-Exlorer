# Session handoff — 2026-11-04

Both CI jobs are green on this branch. This note records what was wrong,
what fixed it, and the one piece of work that is deliberately not done.

## CI state

| job | result |
|---|---|
| Rust checks and tests (Linux) | **pass** — all steps |
| Windows executable (x64) | **pass** |

Linux steps 6–14 all pass: x87 regional arithmetic, requirement status
audits, `cargo check --bins`, gfx-light audit, viewer unit tests, Vulkan
install, all 22 Phase-1 shader checks, terrain per-view GPU, and
`cargo fmt`. PR #18 reports `MERGEABLE` / `CLEAN`.

Status checker: **39 Implemented / 36 Partial / 1 Not implemented,
consistent, 0 errors, phase 1 complete.**

## Why the branch had never actually been tested

`Check formatting` was the **second** step of the Linux job, so every run
died there and `cargo check`, the unit tests and the whole shader suite
were **skipped on every commit**. Moving formatting to the end exposed a
real backlog:

| defect | file |
|---|---|
| double mutable borrow in the skin fallback loop (E0499) | `model_asset.rs` |
| `EcModel` fixture missing `additional_skins` (E0063) | `tools/fixtures/model_draw.rs` |
| `y` not in scope in the grass emitter (E0425) | `gpu.rs` |
| `is_empty()` on `Option<Material>` (E0599) | `bmd.rs` |
| v5 material fixture on the wrong side of the 48-byte block | `bmd.rs` |
| retail-shader test cut at the wrong `#[cfg(test)]` | `grass_instances.rs` |
| UTF-8 fixture fed to a GBK parser | `ecmodel.rs` |
| `GrassWind::update` missing after the module merge | `gpu.rs` |
| texkill keyed on an alpha the bump map does not have | `post_gpu.rs` |

## Getting the diagnosis out of CI

Raw step logs live on a blob host this sandbox cannot reach, and check-run
annotations are capped per run (the GPU checks fill the cap with
notices). Two things made failures readable:

- `cargo check` keeps its full message format and echoes `error` plus its
  own `-->` line; the test steps echo `test result` and the failing names.
- `cargo fmt --check` **already prints the "Diff in ..." hunks** —
  `rustfmt --diff` is not an option (`Unrecognized option: 'diff'`), which
  is why an earlier attempt posted an empty diff. The check output is now
  captured and posted as a PR comment.

## Outstanding, deliberately not done

`sky_gpu.rs` was taken from `main` wholesale after its rewrite, which
**dropped the `sky_image01..03` config reader**; `SkyGpu::new` hardcodes
the `clear` set. Retreading it onto the new 9-layer geometry atlas is the
next substantive commit. It is a regression against earlier work on this
branch, not a CI failure.

P3.01 (mesh v2/v3) stays `Implemented` on spec + synthetic evidence; it
has **zero corpus occurrences**, and that caveat is written into the row.
