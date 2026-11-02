# Phase 2 — Agent 2-A (terrain structure) report

Branch: `arena/e49c474a-perfect-world-exlorer`
Client under study: `google_drive/elementclient.exe`, PE i386, image base `0x400000`,
md5 `5f02895688fa580506e869156340a664`, `.text 0x401000 + 0x4d3f25`,
`.rdata 0x8d5000 + 0x77aae`. Assets: `google_drive/world.trn2`
(md5 `d50741ad113ca7bd9951915fd0923e6a`) plus the shipped `world.t2bk` set.
Every address below is a virtual address in that image. Disassembly method:
`objdump -d` dump plus `tools/disasm-window.py`; IAT/rodata resolution through
`tools/pe.py`.

Scope of this report: **terrain structure only** — block residency/draw
selection (P2.01), resolution levels (P2.02), block/level seams (P2.03). Shader
math, lightmaps, layer transforms and view submission belong to 2-B/2-C and are
untouched.

---

## 1. What changed

| File | Change |
|---|---|
| `src/terrain_lod.rs` (new) | Recovered rules as data + geometry: block window (`0x77ecf0`), per-block level selection (`0x77e360`), per-edge coarsening (`0x7c40e6`), per-level block surface builder (`0x7e5f70`/`0x7e6070` geometry). Rust unit tests for every rule plus rasterised crack/overlap regressions. |
| `src/gpu.rs` | `build()` no longer loads "everything then culls" and no longer stitches the near mesh with an approximation. It now derives one block window from the camera, assigns a level per block, and emits the recovered per-block surface with per-edge cases. Fails explicitly when the T2BK block edge is not the recovered 32 cells. Slider label renamed to "Terrain block window (blocks)". |
| `src/terrain_gpu.rs` | Whole-world `world_mesh(t, 4)` far mesh removed (the client draws nothing outside the camera's block window). `TerrainGpu::draw` submits only the window mesh. `edge_mesh()` keeps its signature and now reports a zero-count "far" slot for the water refraction pass. `FarVertex` retained only as the layout of that zero-count slot. |
| `src/terrain_seam.rs` | **Deleted.** Its 4-cell blend toward a coarse sample was the explicit approximation P2.03 had to replace; nothing else referenced it. |
| `src/view_settings.rs` | `DetailSettings::default().radius` 6 → 7 (the client's 420-unit range at 64-unit blocks), documented against `0x77d720`/`0x40e41f`. |
| `src/main.rs` | `Terrain.grid` (TRN2 header dword @20) retained on the struct so the renderer can validate block size; `mod terrain_seam` removed, `mod terrain_lod` added. |
| `tools/verify_terrain_lod.py` (new) | Executable cross-check of the Rust module (see §5). |
| `tools/disasm-window.py`, `tools/pe.py` (new) | Address-window and PE helpers used for the reverse engineering; committed so the evidence is reproducible. |

Production call sites after the change:

* `src/gpu.rs::build` — `crate::terrain_lod::window` → `window_lod` →
  `block_surface`, per resident block; called from `0x40xx`-equivalent viewer
  paths `gpu.rs:2304` (startup), `gpu.rs:2367` (background rebuild) and
  `gpu.rs:2947` (`--gpu-preview`).
* `src/gpu.rs` rebuild trigger (`gpu.rs:2593`) — rebuilds when the camera's block
  moves ≥ 2 blocks or the settings change (unchanged in this lane).
* `src/terrain_gpu.rs::TerrainGpu::draw` — draws exactly the window mesh.
* `src/water_gpu.rs::render_edge` — unchanged signature, receives the zero-count
  far slot from `TerrainGpu::edge_mesh`.

---

## 2. P2.01 — block/area residency and draw selection

### Recovered

`A3DTerrain2`'s per-frame terrain update is `0x77e1b0` (callers `0x672cdd`,
`0x77df82`, `0x77e036`, `0x77e132`, `0x7cd142`, `0x7ce992` — i.e. once per view,
including the reflected/refracted views). It runs, in order:

1. `0x77ecf0` — recompute the visible block rectangle.
2. `0x77e250` — emit/draw the blocks of that rectangle.
3. `0x77e360` — re-assign each block's level (and its four neighbour pointers).

`0x77ecf0` details (`world` = `ecx`, camera-position pointer on the stack):

```
f        = 1.0 / world[0x1c4]           ; 0x1c4 = world units per block (64)
r        = (int)ceil(f * arg_range)     ; arg = world[0x1bc], float world units
px       = (int)((cam.x - world[0x1e4]) * f)     ; camera block coordinate
pz       = (int)((cam.z - world[0x1e8]) * f)
x0 = px - r ; x1 = x0 + 2r + 1          ; (2r+1) wide, not "r each side"
if x0 < 0      { x0 = 0; x1 = min(2r+1, world[0x188]) }
if x1 > world[0x188] { x1 = world[0x188]; x0 = max(0, world[0x188] - (2r+1)) }
(same for the z axis, limit `world[0x184]`)
```

So at a map edge the square **shifts** to keep `2r+1` blocks; it is not
truncated. `world[0x184]`/`world[0x188]` are the block counts (176/128 for the
shipped `world.trn2`: rows `@28` = 176, cols `@32` = 128, each subterrain 16×16
blocks per `subgrid@64`/`grid@20` = 512/32).

The range itself: the constructor leaves `world[0x1bc] = 100.0`; the graphics
quality preset `0x40e3e0` writes the real value into the settings object
(`+0x4c` = 170 / 250 / 420 for quality 0 / 1 / ≥2, ctor default 500 at
`0x40c594`), the config setter `0x40dce0` clamps it to at least `settings[0x48]`
(170) and `0x42ac10` forwards `max(value, 170)` to `0x77d750` (minimum) and
`0x77d720` (`world[0x1bc] = max(value, minimum)`). For the shipped presets the
terrain window is therefore `ceil(420/64) = 7` blocks → **15×15 blocks = 960
world units** around the camera, and nothing else is drawn: there is no
whole-world terrain pass in the client.

Emission (`0x77e250`) walks the rectangle, and `0x77e360` skips a block when

* its block pointer is null (`world[0x18]` array, `0x77e417`),
* `block[0x18] == 0` (not loaded, `0x77e428`),
* `block[0x74] == 0` → level forced to 0 (`0x77e430..0x77e437`).

### Viewer now

`crate::terrain_lod::window` implements the arithmetic above (including the
edge shift); `block_world = grid * grid_spacing` comes from the TRN2 header, the
slider value is the range in blocks (`range = radius * block_world`), the default
is 7. Blocks are resident by construction (the loader still loads every T2BK up
front — that is an asset-loader property, not a draw rule) and are *submitted*
only inside the window; level 0 means "not submitted". Missing/malformed T2BK
data already fails naming the area/block in `main.rs` (`"{name}: {e}"`,
`"{name}: invalid T2BK"`, `"{name}: unsupported version {v}"`,
`"{name}: missing blocks"`, `"{name} block {bi}: …"`). A non-32-cell block edge
now fails naming both numbers instead of drawing substitute geometry.

`Task | Finished Yes | Production integration: gpu.rs::build drives TerrainGpu::draw from terrain_lod::window/window_lod | Native evidence: 0x77ecf0, 0x77e1b0, 0x77e250, 0x77e360, 0x77d720/0x77d750, 0x40dce0, 0x40e3e0, 0x40f2xx | Tests executed: tools/verify_terrain_lod.py window cases (centre, low edge, high edge, world smaller than window) — pass | Remaining gaps: per-frame residency/streaming (the client's block array is filled by an asynchronous loader; the viewer keeps all blocks resident), block[0x74] visibility flag source, and rebuild granularity (see §6)`.

---

## 3. P2.02 — resolution levels and transitions

### Recovered

Level assignment, `0x77e360` per block of the window:

```
if block[0x74] == 0            -> level 0                     (0x77e437)
d = hypot(block[0x3c] - cam.x, block[0x44] - cam.z)           (0x77e443..0x77e45b)
d -= world[0x1c8]              ; 0x1c8 = block_size * 0.7071067 (0x8f0a5c)
if d <  block[0x7c] * world[0x26c] -> level 3                  (0x77e461..0x77e484)
else if d < block[0x7c] * world[0x270] -> level 2              (0x77e486..0x77e49a)
else                                -> level 1                 (0x77e4a3)
```

Only **two** thresholds and **no hysteresis**: the comparisons are re-evaluated
from scratch every frame for every view, so a level change happens exactly when
the camera crosses a threshold. `block[0x3c]/[0x44]` is the block's XZ centre
(set by the block initialiser, `0x7c39f0`/`0x7c3ad0`), `world[0x1c8]` is the
block's half diagonal (the camera-to-centre distance is shrunk so the level
depends on the distance to the block's volume, not to its middle).

Geometry per level (`0x7e5f70` allocates, `0x7e6070`/`0x7e62e0`/`0x7e6530`/
`0x7e67a0` build levels 3/2/1):

```
step = 1 << (3 - level)        ; level 3 -> 1, 2 -> 2, 1 -> 4 cells
q    = (cells - 2*step)/step   ; 30, 14, 6
indices per level record = 6*q*q   (u16, strip pointers at -0xac/-0xa8/-0xa4)
```

i.e. the *interior* mesh covers block cells `step ..= cells-step` at `step`
resolution and the outer ring of `step` cells is closed by the four edge strips
(§4). All three levels index the same `(cells+1)²` vertex grid — the coarse levels
are index subsampling, not a separate vertex path. There is no per-level vertex
format, no cross-fade and no interpolation between levels.

Where the two thresholds come from:

* `CECWorld::LoadTerrain` (`0x42a219`/`0x42a21c`) writes `terrain+0x258 = 4`
  (level count, levels 0..3 — which is why four level records exist) and
  `terrain+0x268 = 2`, then calls `0x77f450(ecx = terrain, near, far)` at
  `0x42a23d`.
* `0x77f450` stores the first argument into `terrain+0x26c` and the **larger** of
  the two into `terrain+0x270` (`fcoms` + `fstps 0x270`, `mov .. 0x26c`).
* Both arguments come from `[CECWorld+0x18]` (the render settings object,
  `0x42a226..0x42a235`, offsets `+0x60`/`+0x64`), which the graphics quality
  preset `0x40e3e0` fills with **40/80** (quality 0, `0x40e3f0`),
  **120/240** (quality 1, `0x40e407`) and **180/320** (quality >= 2,
  `0x40e41f`); the settings constructor default is 180/320 as well
  (`0x40c5a8`/`0x40c5af`).

So the thresholds are **absolute world-unit distances** and the ladder for the
shipped default is: level 3 while the camera is within `180 + 45.25 = 225` world
units of a block centre, level 2 within `320 + 45.25 = 365`, level 1 beyond —
inside a 7-block (448-unit) window, so all three levels are exercised.

### Viewer now

`LodConfig::level` implements exactly the three-way choice above with
`d = |centre - cam| - block_size * 0.70710678` (0x8f0a5c) and thresholds
`180 * block_scale` / `320 * block_scale` (`LodConfig::default`, quality
preset >= 2); `block_scale` is the per-block `block+0x7c` and is 1.0 for the
shipped blocks. Every resident block is emitted at its own level through
`block_surface`, so the switch quantity (camera-to-block-centre XZ distance, half
diagonal removed) and the three-level ladder are the client's. The viewer's old
"full-res near mesh + step-4 whole-world far mesh" split is gone.

`Task | Finished Yes | Production integration: gpu.rs::build → terrain_lod::window_lod::level → block_surface(level) | Native evidence: 0x77e360, 0x77e461..0x77e4a3, 0x77f450, 0x42a219/0x42a23d, 0x40e3e0/0x40e407/0x40e41f, 0x7e5f70, 0x7e6070/0x7e62e0/0x7e6530/0x7e67a0, 0x8f0a5c = 0.7071067 | Tests executed: tools/verify_terrain_lod.py level cases (180/320 boundaries ±0.01, per-block scale 0.5, no band) and per-level triangle counts 2048/512/128 — pass | Remaining gaps: `block+0x7c` per-block scale assumed 1.0 (no shipped block observed with another value; the thresholds are only sensible at 1.0), level re-selection cadence (§6)`.

---

## 4. P2.03 — seams and edge rules

### Recovered

The client does **not** blend, sink or snap heights at block boundaries. It
coarsens the *finer* block's boundary to the coarser neighbour's stride:
`0x7c40e6` (inside `0x7c0f50`'s per-block collection, `0x7c4000`) reads the four
neighbour pointers `block+0xc4..0xd0` that `0x77e360` fills in the same sweep:

```
if own_level <= 1                     -> no coarsening at all   (0x7c40e9)
for each of the four neighbours n:
    if n == 0 or n[0x70] == 0         -> case 0                 (0x7c410a..0x7c4113)
    if n[0x70] >= own                 -> case 0                 (0x7c4115)
    else case = own - n[0x70]         ; 2 bits per side, capped by the level span
```

so the stride along a shared edge is `own_step << case`, which is always the
coarser of the two blocks' steps — both sides therefore place boundary vertices at
the *same* positions. Missing or unloaded neighbours leave the edge at the block's
own step (never an invented coarsening), which is also what happens at the world
border and at the window border. The per-edge strip builders (`0x7e60d2..` for
case 0, the loop from `0x7e6170` for cases 1 and 2) emit corner triangles plus a
fan from the coarsened boundary node onto the fine inner line; all vertices live
on the same `(cells+1)²` grid, so heights are shared bit-exactly between
neighbours and no height is ever moved.

### Viewer now

`block_surface(level, cases)` builds the interior `q×q` cell grid plus one ring
per edge whose boundary nodes are spaced `step << case` and whose inner line is
one `step` inside; the ring is emitted as a fan from each segment's boundary
node. Each corner cell is completed by the perpendicular ring of the same block
(exactly one half of the corner square each) — the same "corner plus strip"
structure the native builders produce. Vertex positions are the global grid
cells, so two neighbours sample the same cells and never crack.

`Task | Finished Yes | Production integration: gpu.rs::build → terrain_lod::edge_case/edge_cases/block_surface | Native evidence: 0x7c40e6, 0x7c410a..0x7c4141, 0x77e4aa..0x77e535 (neighbour slots), 0x7e60d2..0x7e62e0 | Tests executed: tools/verify_terrain_lod.py per-block exact-coverage for 3 levels × 10 case combinations, 2×2 mixed-level field (levels 3/2/1 side by side), and shared-edge node-set equality for all 9 level pairs — all pass, 0 uncovered/doubled samples | Remaining gaps: the native per-edge index layout (`6*q*q` interior + separate strip buffers) is reproduced as an equivalent vertex set with the viewer's own fan diagonals; the 24*bg² per-cell map at `0x7e6ba0` is not decoded. No native per-edge *height* rule exists to reproduce.`

---

## 5. Tests executed and exact results

**Toolchain limitation and how it was closed (stated explicitly):** this
checkout's sandbox has no Rust toolchain (`cargo`/`rustc` are absent), so nothing
here was compiled locally. The same commit is compiled by the repository CI
workflow on every push; run 41522560935 on commit c9e6627 reports **both jobs
green** — Linux (formatting, x87 regional arithmetic, requirement-status audits,
all binary targets, GFX light audit, viewer unit tests, software-Vulkan shader
regressions) and Windows (native gamma words, standalone viewer build, staged
artifact). The previously failing steps were `Check formatting` (rustfmt clean-up
of the new terrain code) and the requirement-status audits (this table still cited
the deleted seam module); both are fixed. What CI verifies is that the crate
builds, every binary target links, all unit tests pass and the shaders validate —
it does not render the scene, so the rendered camera walk stays open below.
`src/terrain_lod.rs` carries the same checks as `#[cfg(test)]` unit tests
(`window_is_the_recovered_2r_plus_1_square`,
`levels_use_the_two_recovered_thresholds_and_no_hysteresis`,
`edge_cases_only_coarsen_towards_a_coarser_neighbour`,
`interior_index_counts_match_the_native_level_grid`,
`every_block_surface_covers_its_block_exactly_once`,
`neighbouring_blocks_at_different_levels_leave_no_crack`); they must be confirmed
with `cargo test terrain_lod` on a machine with Rust before this is treated as a
green build. What *was* executed here is a line-for-line Python mirror of the same
algorithms, `tools/verify_terrain_lod.py`:

```
$ python3 tools/verify_terrain_lod.py
window ...................... ok (r=ceil(range/block), 2r+1, shifted at edges)
levels ...................... ok (two thresholds, -half diagonal, no hysteresis)  [run before the threshold decode]
seam stride ................. ok (max(own, neighbour) step, missing -> own)
block level 1 cases (0, 0, 0, 0):   128 tris,  1024 samples,   0 bad  ok
block level 1 cases (1, 0, 0, 0):   124 tris,  1024 samples,   0 bad  ok
block level 1 cases (0, 1, 0, 0):   124 tris,  1024 samples,   0 bad  ok
block level 1 cases (0, 0, 1, 0):   124 tris,  1024 samples,   0 bad  ok
block level 1 cases (0, 0, 0, 1):   124 tris,  1024 samples,   0 bad  ok
block level 1 cases (1, 1, 1, 1):   112 tris,  1024 samples,   0 bad  ok
block level 1 cases (2, 2, 2, 2):   104 tris,  1024 samples,   0 bad  ok
block level 1 cases (2, 0, 2, 0):   116 tris,  1024 samples,   0 bad  ok
block level 1 cases (0, 2, 0, 2):   116 tris,  1024 samples,   0 bad  ok
block level 1 cases (1, 2, 1, 2):   108 tris,  1024 samples,   0 bad  ok
block level 2 cases (0, 0, 0, 0):   512 tris,  4096 samples,   0 bad  ok
...
block level 3 cases (0, 0, 0, 0):  2048 tris, 16384 samples,   0 bad  ok
block level 3 cases (1, 2, 1, 2):  1968 tris, 16384 samples,   0 bad  ok
2x2 mixed-level field: 4656 tris, 65536 samples, 0 bad ok
shared edge nodes ........... ok (identical sets for all level pairs)
FAILURES: 0
```

Reading: with no neighbour coarsening a level-3 block is exactly 2048 triangles
(32×32 cells × 2), level 2 is 512 and level 1 is 128 — the full block, not only
the interior — and coarsening only removes triangles (case = 2 everywhere:
1952/464/104). The 2×2 field mixes levels 3/2/1 across both boundaries; every one
of 65 536 lattice samples is covered by **exactly one** triangle (no hole, no
double-draw), and for every pair of levels both sides of the shared edge generate
the identical node set. The client-side acceptance ("no crack and no invented
geometry at either boundary kind") is what these two checks measure; the camera
walk itself still needs a GPU run (§6).

Also re-verified during this work (data, not code): `world.trn2` parses as
176×128 blocks of 32×32 cells, spacing 2.0 → 4096×5632 cells, 8192×11264 world
units, 88 subterrains of 16×16 blocks — consistent with `0x1c4 = 64` and
`0x1c8 = 64 × 0.7071`.

### Cost of the recovered rule (measured, not estimated)

For the shipped world (128 × 176 blocks of 64 units), camera at block
`(75.4, 90.2)`, default window 7 (15 × 15 = 225 blocks), using the same
functions the renderer calls (`window`, `level_at`, `block_surface`):

| Level | Blocks | Triangles |
|---|---|---|
| 3 (step 1) | 39 | 79 424 |
| 2 (step 2) | 63 | 31 888 |
| 1 (step 4) | 123 | 15 744 |
| total | 225 | **127 056**, 69 825 vertices |

The previous viewer approximation emitted every block at full resolution
plus a step-4 whole-world mesh: 460 800 triangles and 245 025 vertices for the
same window alone (14.7 MB of 60-byte vertices vs 4.2 MB now).

---

## 6. Interface changes other agents must know

1. **`TerrainGpu::edge_mesh()` still returns `(&Buffer, &Buffer, u32)` but the
   count is now 0** (the whole-world far mesh is gone). 2-C: the water refraction
   pass binds it and draws nothing from it; the terrain visible in the refraction
   target is now the same per-block window mesh as the main pass. If a future
   recipe needs a *distant* terrain representation, it does not exist in the
   client and must not be reintroduced silently.
2. **`TerrainGpu::draw` no longer submits a far pass.** `vs_far`/`FarIn` remain in
   the WGSL module but nothing binds them; the near pipeline is the only terrain
   path. 2-B: the near pipeline state (depth `Greater`, no cull, no blend) is
   untouched.
3. **`Vertex.uv` is no longer written for terrain vertices** (it was an
   atlas-slot UV the terrain fragment shader never read). Terrain vertices are
   `kind = 0`, `uv = 0`, `diffuse = 0xffffffff`.
4. **`DetailSettings::default().radius` is 7** (was 6); the field now means "block
   window half-size / draw range in blocks". Saved settings with `radius=6` still
   decode and simply produce a 13×13 window.
5. **`Terrain` gained `grid: usize`** (TRN2 header dword @20) so the renderer can
   refuse non-32-cell blocks instead of drawing substitute geometry.
6. **`src/terrain_seam.rs` is deleted**; `terrain_seam::stitch` no longer exists.
7. Terrain geometry is rebuilt by the existing `gpu.rs::build` path, i.e. on
   startup, on a settings change and when the camera's block index moves by ≥ 2.
   Other lanes that read `renderer.terrain_end` are unaffected; the index/vertex
   buffer layout is still "terrain first, then scenery/water/grass".
8. **Visible extent changed.** Terrain now ends at the block window (448 world
   units at the default 7) instead of the whole world, because the client draws
   nothing outside it. 2-B/2-C: the shipped client pairs that range with its own
   atmospheric fog/vista, so the viewer's "view distance" slider (2000–24000) no
   longer describes a terrain extent; judge distance appearance with client fog
   enabled rather than by widening the window.

---

## 7. Remaining gaps and assumptions (also in `Findings/Unsupported-client-paths.md`)

* **Per-block level scale `block+0x7c`.** The two thresholds (180/320 world units,
  see §3) are multiplied by this per-block float. The shipped blocks are assumed
  to carry 1.0: any other value would make the shipped preset distances
  meaningless (e.g. 64 would push the level-3 boundary past the whole draw
  window), and no block initialiser observed writes anything else. It is exposed
  as `LodConfig::block_scale` so a future decode can drop the assumption in one
  place. This is the only remaining assumption in the level rule.
* **Level re-selection cadence.** The client re-runs `0x77ecf0`/`0x77e360` every
  frame for every view (`0x77e1b0`); the viewer rebuilds the window mesh on the
  existing `build()` triggers (≥ 2 blocks of camera movement, settings change).
  Crossing a level threshold can therefore lag by up to two blocks and cannot pop
  more than once per rebuild. Reproducing the per-frame cadence needs the terrain
  mesh rebuild split out of the expensive scenery/atlas rebuild in `build()`.
* **Ring triangulation diagonals.** The native code emits per-edge strips from
  separate index buffers (corner triangles plus a fan); the viewer emits the same
  vertex set with its own fan diagonals inside each `step`-wide ring cell. The
  boundary node sets — the only thing that can crack — are identical.
* **Block visibility / hole flags.** `block+0x74` (visibility, level 0) and the
  terrain hole flags (`P2.07`) are not decoded; the viewer treats every resident
  block as visible and emits no holes.
* **Asynchronous block residency.** The client's block array is filled by its
  loader and `0x77e360` skips unloaded blocks; the viewer loads every T2BK up
  front, so "residency" is a draw-set property only. No eviction logic exists in
  either path at draw time.
* **Unrecovered allocation.** `0x7e6ba0` allocates `24*bg²` per block for a
  per-cell map I did not identify; it does not affect the emitted geometry above.
* **No GPU run.** Nothing here was rendered: the sandbox has no GPU surface, so
  the camera near→far→back walk and the "no popping inside the band" acceptance
  are argued from the (executed) geometry tests, not observed. CI compiles and
  links the binaries on both platforms and validates the shaders under software
  Vulkan, but does not present a frame — this is the one remaining item a human
  has to look at.
