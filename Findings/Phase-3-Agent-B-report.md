# Phase 3 — Agent B report: static-model material state, culling and depth order

**Branch** `arena/5b7c9c75-perfect-world-exlorer` · **PR** #6 · **Base** `main` (`b718574`)
**Date** 2 November 2026 · **Scope** P3.02, P3.04, P3.05

The lane is *how a parsed static model reaches the device*: material and
texture-stage state, culling/sidedness, depth bias and draw order. File parsing,
texture residency and view/pass selection stay with their owners; §8 lists every
interface this report changes for them.

One of the three sub-tasks is **Yes** (P3.04). P3.02 is **No**: stage, alpha
test, culling and the material-colour question are recovered and implemented,
but the last row — blending — turned out to be owned by the calling pass rather
than by the mesh, and which ornaments select the blended pass is not established
(§2, §7). P3.05 is **No** as well: the recovered depth rules are implemented and
the classification instrument exists, but the retail-archive census the
acceptance criterion requires could not be run in this environment, so the
survey's overlaps are not yet all attributed.

## 0. Verification basis and limits

* **The retail executable was analysed in this session.** `elementclient.exe`
  6,000,640 B, MD5 `5f02895688fa580506e869156340a664` (re-checked with `md5sum`
  before every address below was read), PE i386, image base `0x400000`,
  `.text 0x401000+0x4d3f25`. Disassembled with
  `objdump -d google_drive/elementclient.exe > /tmp/dis/text.asm`; windows were
  read with a helper that matches `^\s+([0-9a-f]+):\t(?:[0-9a-f]{2} )+\s*\t(.*)$`.
  Data references were found by searching the raw file for
  `struct.pack('<I', va)` with the section table applied. The binary is not
  committed (`.gitignore` keeps `/google_drive/` out of the tree).
* **No Rust toolchain exists in this sandbox** (`cargo`, `rustc` and `rustfmt`
  are absent and the distribution hosts are unreachable), so no local build,
  `cargo fmt --check` or test run was possible. Compilation, formatting and test
  execution therefore happened in CI on the PR branch; §6 records the run and
  its exact results. Two local substitutes were used and are reported as such:
  a `tree-sitter-rust` parse of every touched file (0 `ERROR`/`MISSING` nodes)
  and a Python oracle for the integer arithmetic and for the shader's culling
  predicate.
* **The retail asset archives were not available.** `litmodels.pck`
  (397,425,471 B), `models.pck` (888 MB), `building.pck` (326 MB) and the other
  archives exceed the 100 MB per-file limit of the Drive connector, so this
  session re-uses the recorded census (16,335 BMDs, 40,579 meshes, 7,802,464
  vertices; 31,802 v4 + 8,777 v5 meshes; 2,363 v5 two-sided meshes) and the
  recorded overlap survey (174,744 coincident triangles in 5,416 models;
  168,225 opposite-winding, 6,519 same-winding, 12 across meshes). No new
  per-asset claim is made from data this session did not read.

## 1. The recovered static-model draw state

Everything below is written by the ordinary baked-mesh submitter `0x7a9730`,
whose per-mesh loop is `0x7a9808..0x7a99bc`, or by the state pass that precedes
it. `0x7a9120` is the refraction variant of the same submitter and shares every
row except the stage-0 alpha argument.

| State | Value for a baked mesh | Native write | Viewer consequence |
|---|---|---|---|
| `LIGHTING` (id 0x89) | 0 | `0x77a3c0(0)` at `0x7a9759` | Material colours and `Power` from the v5 block never contribute. |
| `SPECULARENABLE` (id 0x1d) | 0 | `0x77b570(0)` at `0x7a9762` | No specular term. |
| stage 0 `COLOROP` | 5 `MODULATE2X` | `0x779fc0(0,5)` at `0x7a976d` | `texture.rgb * vertex.rgb * 2`, saturated. |
| stage 0 `COLORARG1/2` | `TEXTURE`, `DIFFUSE` | `0x779fe0(0,2,1)` at `0x7a977a` | Same. |
| stage 0 `ALPHAOP` | 4 `SELECTARG2` | `0x77a020(0,4)` at `0x7a9785` | Alpha is not blended; nothing reads it downstream. |
| stage 0 `ALPHAARG1/2` | `TEXTURE`, `DIFFUSE` | `0x77a040(0,2,1)` at `0x7a9792` | Same. |
| stages 1–7 | disabled (1) | `0x779fc0(1,1)`/`0x77a020(1,1)` at `0x7a979e`/`0x7a97aa` | Single-stage material. |
| `CULLMODE` | saved, then 1 `NONE` when `mesh+0x26c` ≠ 0 | save `0x168(%ecx)` → `0x14(%esp)` at `0x7a98bb`; `0x77a540(1)` at `0x7a98cf`; restore `0x77a540(saved)` at `0x7a9a26`/`0x7a9a49` | Two-sided meshes keep both windings; every other mesh keeps the pass cull mode. |
| `ALPHATEST` | 0 | `cmp $0xff,0x2cc(%edi)` at `0x7a98f1`; `0x77a320(0)` at `0x7a992b` | No cutout for baked meshes. |
| `ALPHATEST(1)` + `ALPHAREF` | only when `mesh+0x2cc` ≠ 0xff | `0x77a320(1)` at `0x7a98f8`; `80*alpha/255+1` at `0x7a990a..0x7a9920` (`imul 0x80808081`, `shl 4`/`lea` = ×80, `sar 7` = ÷255, `+1`), `0x77a380` at `0x7a9924` | Unreachable from files; the reader refuses such a value. |
| `ALPHABLENDENABLE` | **owned by the calling pass**, never by the mesh loop | no `0x77a290` write exists between `0x7a9808` and the draws it gates; the passes write it: `1` at `0x42c650` (only when `[obj+0x95] == 2`, `0x42c5f3`) and `0` at `0x42b474`, `0x42b4df`, `0x42c32e`, `0x42c509` | Not decided by this table. The viewer draws every page in the unblended pass. |
| `ZBIAS` (id 0x2f) | not written | the only call sites are `0x77a250(2)` at `0x417453` and the `0x77a250(0)` restore at `0x417471`, both in the effect path | Bias 0. |
| `ZFUNC` (id 0x17) | not written | only `0x796753` and `0x796cd2` (water/effect paths) | D3D8 default `LESSEQUAL`. |
| Draw order | mesh array order, one indexed submission per mesh | loop head `0x7a9808`, per-mesh submit `0x7b9ca0` at `0x7a98e3` | Unchanged from the client: insertion order, later primitives last. |

`mesh+0x2cc` is written only by the mesh constructor (`0x7b9408`,
`movl $0xff,0x2cc(%esi)`); no BMD reader in the image stores to that offset.
`mesh+0x26c` is the v5 two-sided byte the reader already exposes.

## 2. Task P3.02 — material and texture-stage state

### Recovered rule

A baked static mesh has exactly one material row: one enabled stage combining
the texture with the baked vertex colour, doubled; lighting off; specular off;
no alpha test; culling from `mesh+0x26c`. Blending is *not* part of this row —
the queue owns it (§ "Not finished" below). The v5 material block the
reader parses (`Ambient`/`Diffuse`/`Emissive`/`Specular`/`Power`/`2Sided` at
`0x7b67b0`, copied by `0x7b66d0`) contributes only `2Sided` to the draw, because
`LIGHTING(0)` at `0x7a9759` removes the fixed-function material from the
pipeline — the colours are parsed, stored and never read by the submitter. That
is the recovered *rule*, not a per-asset exception, and it replaces the old
assumption that a non-default material colour would modulate the surface.

### Implementation

* `src/bmd_baked.rs` is now the single table: `Stage` (`ColorOnly`,
  `Modulate2x`), `AlphaTest` (`Disabled`), `Cull` (`BackFace`, `TwoSided`),
  `Blend` (`Opaque`), `DrawState`, `classify()`, `alpha_test()`,
  `cutout_reference()`, plus `native` constants naming every write site above.
* `classify()` is the only producer of the flat `kind` float that the shared
  world shader consumes; `src/gpu.rs` (`build()`, nearby scenery) and
  `src/world_objects.rs` (`run()`, whole-world pages) both call it, so the old
  duplicated `if two_sided { 2.1 } else { 2.0 }` inline rule is gone.
* Untextured meshes used the water colour path (`kind` 1.0, `i.color.rgb` =
  atlas rectangle) — a silent wrong colour. They now get their own kinds
  (1.1 single-sided, 1.2 two-sided) and the shader returns
  `clamp(baked_colour * 2)` through `bmd_pass_fog`, i.e. `MODULATE2X` against an
  unbound (white) stage.
* The baked band was removed from the shader's alpha test. Before:
  `((i.kind>1.5 && i.kind<2.5) || i.kind>3.5) && tex.a<…`. After:
  `i.kind>3.5 && tex.a<…`, so only grass/trees keep a cutout.
* Unrepresentable combinations are explicit errors: any `mesh+0x2cc` other than
  `0xff` is refused by `alpha_test()` with the reference the client would have
  computed, and a v4 mesh carrying a two-sided flag keeps failing.

### Not finished: blending

The first version of this table claimed baked meshes are opaque because the mesh
loop never writes `ALPHABLENDENABLE`. That claim does not survive inspection of
the callers: `0x7a9730` has four call sites and one of them, the block entered
when the drawn object's byte `+0x95` equals 2 (`0x42c5f3`), sets
`ALPHABLENDENABLE(1)` at `0x42c650`, `ALPHATEST(0)` at `0x42c660`, blend factors
through `0x42c66c` -> `0x42c000`, and only then draws at `0x42c683`. Other call
sites (`0x42b474`, `0x42b4df`, `0x42c32e`, `0x42c509`) disable it. Blending is
therefore a *queue* property, `+0x95` is the byte that selects the blended queue,
and its meaning (and the value retail ornaments carry) is outside what this
session established. `DrawState.blend` records the state as
`Blend::QueueOwned`; the viewer still draws every page unblended, which is now a
declared gap instead of an assumption.

### Acceptance

`bmd-sided-gpu-check` proves the two material axes the mesh owns in one raster
pass: opaque shading (`kind` 2.0), two-sided shading (`kind` 2.1) and a
zero-alpha atlas that must still shade (the cutout state cannot be entered).
"Blended" is not covered because it is not a mesh property: `blend` is
`QueueOwned` and the regression that pins it is
`every_classified_mesh_has_no_alpha_test_and_owns_no_blend` plus the shader
assertion that the doubled alpha-test band is gone. Per-object blending stays an
open gap.

## 3. Task P3.04 — culling, winding, sidedness

### Recovered rule

There is no per-asset culling decision. The submitter reads one byte,
`mesh+0x26c` (`0x7a98c5`), and either leaves the pass cull mode alone or sets
`CULLMODE(1)`/`NONE` for that one mesh (`0x7a98cf`), restoring the saved value
(`0x168(%ecx)` read at `0x7a98bb`) in every exit path (`0x7a9a26`, `0x7a9a49`,
and the two early exits at `0x7a96f2`/`0x7a9716`). Winding is never rewritten:
the client relies on the placement's own determinant, so a mirrored
(negative-determinant) placement produces the opposite `front_facing` and the
same single-sided rule then keeps the other face. No branch in the client
compensates for mirroring.

### Implementation

* `DrawState.cull` is the only culling input; the shader's winding drop is gated
  by `kind` bands that correspond one-to-one to `Cull::BackFace`
  (`1.05..1.15` for colour-only, `1.5..2.05` for textured). `camera.clip.w`
  still disables it globally, `camera.clip.z` still selects the projected
  parity for the reflected pass; neither was changed.

### Acceptance

`bmd-sided-gpu-check` runs six instances per case: single-sided CCW,
single-sided CW (index order reversed), two-sided CCW, two-sided CW,
**single-sided with a mirrored placement** (positions mirrored in X, the
equivalent of `x = y.cross(z)` under a negative determinant) and two-sided
mirrored. Expected coverage, derived from the shader predicate and confirmed by
the Python oracle in §6:

| `clip.z` | `clip.w` | expected pixels |
|---|---|---|
| 0 | 0 | drawn, dropped, drawn, drawn, **dropped**, drawn |
| 1 | 0 | dropped, drawn, drawn, drawn, **drawn**, drawn |
| 0 | 1 | all drawn |
| 1 | 1 | all drawn |

The mirrored single-sided quad follows its neighbours' rule exactly — it is
dropped when the unmirrored CCW quad is kept and kept when the projection
parity flips — which is the per-case evidence the task asks for.

## 4. Task P3.05 — depth, bias, ordering, overlap

### Recovered rule

* **Bias:** none. `ZBIAS` is written only by the effect path (`0x417453`
  value 2, restored `0x417471`); the earlier note that the client used a
  default bias of 50 does not survive inspection and is withdrawn. `ZFUNC` is
  never written by the baked path, so it keeps the D3D8 default `LESSEQUAL`.
* **Precision:** the client's depth buffer is D3D8 fixed-point; the viewer keeps
  its documented reversed-`f32` buffer. That deviation is unchanged and stays
  reported.
* **Order:** mesh array order, one submission per mesh, no sorting by texture or
  distance in the submitter (`0x7a9808` increments through `[mesh+0x2c0]`).
* **Ties:** equal depth must reach the *later* submission (`LESSEQUAL`), so a
  duplicate triangle drawn second wins.

### Implementation

* `src/gpu.rs` gains `scenery_pipeline`: identical to the world pipeline except
  `depth_compare: GreaterEqual`, used for `terrain_end..local_end` and both
  page lists. Terrain keeps `Greater`, so terrain-vs-terrain behaviour is
  untouched and terrain-vs-scenery ties resolve to the scenery, as in the
  client.
* `bmd_baked::overlap_outcome()` classifies a coincident pair as `Culled`
  (opposite winding and at least one mesh single-sided — exactly one surface
  survives for any camera side), `LaterSubmissionWins` (both rasterise and the
  pair is drawn in the client's mesh order) or `Unresolved` (both rasterise but
  the pair is not proved to share a submission). It is unit-tested for all five
  cases.
* `src/bin/bmd-overlap-survey.rs` now reads `two_sided` per mesh, applies the
  classifier to every pair and prints
  `resolved_by_cull=` / `resolved_by_order=` / `resolved_cross_mesh=` alongside
  the existing counters, plus the outcome on each of its twelve example lines.

### Acceptance status

Not met. The classifier and counters exist, but the survey could not be run
against the retail archive here (connector limit, §0), and the recorded
aggregate is not enough to attribute pairs: the 168,225 opposite-winding pairs
are `Culled` *unless both meshes of a pair are two-sided* (2,363 of 40,579
meshes are two-sided, but the pair-wise incidence is not recorded), and the
6,519 same-winding pairs need the per-pair mesh order to be attributed. Running
`cargo run --bin bmd-overlap-survey -- <path>/litmodels.pck` prints the three
new counters and closes the gap; the numbers from that run must replace this
paragraph.

## 5. Changed files and production call sites

| File | Change | Production call site |
|---|---|---|
| `src/bmd_baked.rs` | Draw-state table, `classify`, `alpha_test`, `cutout_reference`, `overlap_outcome`, `native` write-site constants, 8 unit tests | imported by `gpu.rs`, `world_objects.rs`, `bin/bmd-overlap-survey.rs`, `bin/world-preload-check.rs` |
| `src/gpu.rs` | shader: culling bands extended to the colour kinds, colour-only `MODULATE2X` branch, baked alpha test removed; new `scenery_pipeline` (`GreaterEqual`) created in `Renderer::new` and used by `draw_opaque` | `draw_opaque()` (`set_pipeline`, `terrain_end..local_end` + `local_pages` + `world_pages`) |
| `src/world_objects.rs` | whole-world page vertices classify through the table instead of inline `2.0/2.1` | `run()` page builder |
| `src/bin/world-preload-check.rs` | declares `mod bmd_baked` (the shared table it now needs) | diagnostic only |
| `src/bin/bmd-overlap-survey.rs` | per-pair overlap classification and counters | diagnostic only |
| `src/bin/bmd-sided-gpu-check.rs` | six instances per case, mirrored placement, zero-alpha atlas | diagnostic only |
| `.github/workflows/pr-checks.yml` | adds `cargo run --locked --bin bmd-sided-gpu-check` to the lavapipe step — **cross-lane notice**: it only adds my own check, no existing step changed | CI gate |
| `Findings/Unsupported-client-paths.md` | scenery row updated with the recovered facts; new row for the unreachable cutout state (both rows stay open where the work is open) | — |

Nothing outside the static-model pipeline was touched: no terrain pipeline
change, no water/effect state, no texture residency, no view/pass selection.

## 6. Tests executed and results

**Not executable locally** — there is no Rust toolchain in this sandbox (§0).
The evidence actually produced here, and the CI run that produces the rest:

| Check | How | Result |
|---|---|---|
| `md5sum google_drive/elementclient.exe` | local | `5f02895688fa580506e869156340a664` (6,000,640 B) ✓ |
| Syntax of every touched file | `tree-sitter-rust` parse of `src/gpu.rs`, `src/bmd_baked.rs`, `src/world_objects.rs`, `src/bin/{bmd-sided-gpu-check,bmd-overlap-survey,world-preload-check}.rs` | 0 `ERROR`/`MISSING` nodes ✓ |
| Integer alpha-reference arithmetic | Python oracle of `80*a/255+1` | `a=0→1`, `1→1`, `128→41`, `0xfe→80`, `0xff→81`, `u32::MAX→255` — matches the Rust test expectations ✓ |
| Day/night blend arithmetic | Python oracle of `(d*(255-w)+n*w)>>8` | `[9,19,29]`, `[199,209,219]`, `[127,127,127]`, `[254,254,254]` — matches `integer_blend_matches_the_recovered_arithmetic` ✓ |
| Culling predicate of the six fixture instances | Python oracle of the shader condition | `(0,0)→[T,F,T,T,F,T]`, `(1,0)→[F,T,T,T,T,T]`, `(0,1)`/`(1,1)`→all `T` — matches the Rust expectations ✓ |
| `python3 tools/test_phase1_ready.py` | local | `Ran 7 tests … OK` ✓ |
| `python3 tools/test_eight_phase_status.py` | local | `Ran 5 tests … OK` ✓ |
| `python3 tools/check-eight-phase-status.py` | local | `errors: []`, `complete_phases: []` ✓ |

CI executed the rest on this branch. The sequence, including one real defect the
run caught, is:

| Run | Head | Result |
|---|---|---|
| 41520738614 | first commit | `Check formatting` **failed** — `bmd_baked.rs` test asserts were not in rustfmt's shape |
| (formatting helper, deleted) | `131c642` | `cargo fmt --all` applied; only `src/bmd_baked.rs` changed |
| 41521409838 | `249ec9e` | `Check formatting` ✓, `Verify recovered regional arithmetic on x87` ✓, `Test requirement status audits` ✓, `Check all binary targets` ✓ (`cargo check --locked --bins`), `Test GFX light dependency audit` ✓, `Run viewer unit tests` **failed** |
| 41522066048 | `ddc3839` | `Viewer unit tests` ✓, `Baked scenery raster regression` ✓ (`bmd-sided-gpu-check` on lavapipe) |
| **41523033865** | `991fe2d` | **both jobs green.** `Rust checks and tests (Linux)`: `Check formatting` ✓, `Verify recovered regional arithmetic on x87` ✓, `Test requirement status audits` ✓, `Check all binary targets` ✓, `Test GFX light dependency audit` ✓, `Run viewer unit tests` ✓, `Run Phase 1 production shader regressions` ✓ (all fourteen lavapipe binaries, including the extended `bmd-sided-gpu-check` now wired into that step). `Windows executable (x64)` ✓ |

The unit-test failure was real and is fixed in `ddc3839`:
`cutout_reference(u32::MAX)` multiplied in 32 bits and panicked on overflow in a
debug build ("attempt to multiply with overflow"); the client's `imul` at
`0x7a9910` is 32x32→64, so the function now widens to `u64` before the
multiply. After the fix, the eight `bmd_baked` tests and every pre-existing
viewer unit test pass, and the extended `bmd-sided-gpu-check` passes on lavapipe
— the six-instance expectation table of §3 is therefore confirmed by raster
output, not only by the Python oracle. The final head `991fe2d` repeats all of
that inside the ordinary PR gate (run 41523033865) and stands green on both
jobs.

Two temporary, review-only workflows were used to get feedback into this sandbox
(`cargo fmt` and the failure text, because Actions logs and check annotations
are not reachable from it). Both are deleted before handoff and contribute no
product code; the formatting they produced is kept.

## 7. Remaining unsupported cases and assumptions

* **Face culling is implemented in the fragment shader, not the rasteriser.**
  `primitive.cull_mode` stays `None` because the same pipeline draws terrain,
  water and scenery; the discard is equivalent for coverage but does not save
  raster work, and it depends on `front_facing` being derived from the same
  winding convention in every pass. Recorded, not hidden.
* **Blended scenery is not reproduced.** The blended pass exists in the client
  (`0x42c650..0x42c683`, gated by `[obj+0x95] == 2`); the viewer draws every
  page in the unblended pass, and the meaning of `+0x95` is unestablished, so no
  ornament is classified into the blended queue yet.
* **The alpha-test branch is unimplemented by construction.** A `mesh+0x2cc`
  other than `0xff` raises an error (`bmd_baked::alpha_test`). If a future
  archive ever carries one, the mesh is skipped with a message rather than
  drawn wrong.
* **Depth-buffer format and precision still deviate** (reversed `f32` with
  `Greater`/`GreaterEqual` versus the client's fixed-point `LESSEQUAL`). The
  tie rule now matches; the quantisation does not.
* **The overlap census is not re-run** (§4). Until it is, no claim is made that
  every surveyed pair is attributed.
* **Untextured baked meshes** are now rendered with the recovered stage, but no
  retail mesh of that shape was read in this session, so the branch is covered
  by code and state tests only, not by a corpus example.
* **Two state writes in the submitter are still unmapped.** `0x77baf0` and
  `0x77bad0` are called once, unconditionally, at `0x7a97b2`/`0x7a97b7` before
  the `0x785550` cull gate; neither has an identified render-state id yet, so
  no row of §1 is claimed for them. They are the only unclassified writes in
  `0x7a9730`.
* **Animated/instanced submitter variants of `0x7a9730`** (for example any
  per-frame state block a caller installs before entering the loop) were not
  enumerated; only the loop's own writes are claimed.

## 8. Interface changes for other agents

* `bmd_baked::classify(version, textured, two_sided) -> Result<DrawState, String>`
  — the only supported way to obtain the `kind` float. The `kind` encoding is
  now: `0` terrain base, `0.5–1.0` water/quad colour, **`1.1`/`1.2` untextured
  baked (single/two-sided)**, `2.0/2.1` textured baked, `3.75` grass, `5`
  far-tree billboards, `>4.5` rotating billboards. Anything that emits vertices
  must not invent values inside `1.05..1.15`, `1.05..1.25` or `1.5..2.05`.
* `bmd_baked::overlap_outcome(same_winding, first_two_sided, second_two_sided,
  same_submission)` — reuse it for any other coincident-geometry report rather
  than re-deriving the rule.
* `Renderer` gained a private `scenery_pipeline`; the water depth copy at
  `gpu.rs:1454` still uses `pipeline` (`Greater`). If another agent needs the
  equal-depth tie elsewhere, ask for a pipeline with `GreaterEqual` instead of
  changing the shared one.
* `.github/workflows/pr-checks.yml` gained one step; the workflow owner should
  keep `bmd-sided-gpu-check` in the list because it is the only raster proof of
  the cull table.

## 9. Per-point acceptance lines

P3.02 material and texture-stage state — one table drives both static-model
draws, the material-colour question is answered by `LIGHTING(0)`, the cutout row
is pinned as unenterable rather than assumed, and the blending row was moved to
its real owner (the calling queue) with the blended pass located but not yet
selected per ornament.

`Task P3.02 | Finished No | Production integration gpu.rs build()+draw_opaque, world_objects.rs run() via bmd_baked::classify | Native evidence 0x7a9759 LIGHTING(0), 0x7a9762 SPECULAR(0), 0x7a976d COLOROP 5, 0x7a977a/0x7a9792 args, 0x7a979e stage 1 off, 0x7a98f1/0x7a992b ALPHATEST off, 0x7b9408 mesh+0x2cc=0xff, no 0x77a290 write inside the mesh loop | Tests executed and results bmd_baked unit tests (8) in CI runs 41522066048 and 41523033865, bmd-sided-gpu-check opaque/two-sided/zero-alpha cases | Remaining gaps blending is queue-owned: the blended pass at 0x42c650 is gated by an unestablished per-object byte +0x95, so no ornament is classified into it; alpha-test branch refused; untextured branch not seen in the corpus`

P3.04 culling, winding, sidedness — no per-asset decision exists: one mesh
byte selects `CULLMODE(1)`, the pass value is restored on every exit path, and a
mirrored placement simply reverses `front_facing` under the same rule.

`Task P3.04 | Finished Yes | Production integration bmd_baked::DrawState.cull drives the shader winding bands; scenery_pipeline keeps cull_mode None and the fragment discard reproduces CULLMODE | Native evidence 0x7a98c5 test of mesh+0x26c, 0x7a98cf CULLMODE(1), 0x168(%ecx) save at 0x7a98bb, restores 0x7a9a26/0x7a9a49/0x7a96f2/0x7a9716 | Tests executed and results bmd-sided-gpu-check six-instance cases incl. mirrored placement (CI runs 41522066048, 41523033865); Python oracle of the predicate reproduced all four expected patterns | Remaining gaps culling is a fragment discard, so the mirrored case is proved by raster output rather than by hardware cull state`

P3.05 depth, bias, ordering, overlap.

`Task P3.05 | Finished No | Production integration gpu.rs scenery_pipeline GreaterEqual for terrain_end..local_end and both page lists; bmd_baked::overlap_outcome; bmd-overlap-survey counters | Native evidence no ZBIAS write outside 0x417453/0x417471, no ZFUNC write in the baked path (only 0x796753/0x796cd2), mesh-order loop 0x7a9808 with per-mesh submit 0x7b9ca0 | Tests executed and results coincident_pairs unit test (5 cases), coincident_duplicates_are_won_by_the_later_draw, Python oracle of equal-depth semantics; the archive survey was not run | Remaining gaps every surveyed overlap is not yet attributed: run bmd-overlap-survey on litmodels.pck and replace the aggregate paragraph; the earlier "default bias 50" claim is withdrawn`
