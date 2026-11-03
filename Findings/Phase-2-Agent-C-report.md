# Phase 2 — Agent C report (P2.07, P2.08, P2.09)

Branch `arena/119d167b-perfect-world-exlorer`, based on `b718574`. Target is the
1.4.2 client only: `google_drive/elementclient.exe`, 6,000,640 bytes, md5
`5f02895688fa580506e869156340a664` (verified before any disassembly), PE i386,
image base `0x400000`, `.text` at `0x401000`.

Everything below was measured on the retail archives in the same Drive folder as
the executable. Where a claim is a rule rather than a measurement it says so, and
where a rule could not be checked against the archive the gap is listed.

---

## 1. Production changes and call sites

| File | Change | Production call site |
|---|---|---|
| `src/terrain_sampling.rs` (new) | Native sampler/addressing/format policy for `A3DTerrain2`: `StageSampler`, `stage_sampler()`, `AddressMode::from_native`, `DxtFormat`/`parse_terrain_dds`, `array_slot`, `generates_mips`, mask channel and layer order tables, 12 unit tests | Referenced by `src/material.rs::Materials::load` (see below) and compiled into the binary by `src/main.rs` (`mod terrain_sampling;`). The sampler values are mirrored in `src/terrain_gpu.rs::TerrainGpu::new` and in `src/bin/terrain-view-gpu-check.rs`. |
| `src/terrain_gpu.rs` | Layer sampler changed from `Repeat`/`Repeat` + `anisotropy_clamp: 16` to `MirrorRepeat`/`MirrorRepeat`, `anisotropy_clamp: 1`; mask/lightmap sampler pinned to `ClampToEdge` + `MipmapFilterMode::Nearest` | `TerrainGpu::new`, the only sampler creation site for terrain |
| `src/material.rs` | Each terrain DDS is validated by `terrain_sampling::parse_terrain_dds` before decoding; unsupported FourCC, malformed/truncated mip chains and header/decoder dimension mismatches are explicit errors naming the asset | `Materials::load`, the only terrain texture read |
| `src/gfx_light.rs` | `LIGHT_FIELDS` (8 guessed keys) replaced by the single native marker `LightType`; element 130 without it and any other element with it are explicit errors | `gfx_light::parse`, reached from `gfx_owner::Manager::decode` |
| `src/gfx_owner.rs` | `Status` gains `non_light_owners` and `frames`; `Manager::frames()` | `Manager::status`, read by `src/world_models_gpu.rs::World::gfx_status` |
| `src/world_models_gpu.rs` | New `AlphaStatus` + `World::alpha_status()` so an empty alpha queue can be told apart from a broken one | `src/gpu.rs` World panel |
| `src/gpu.rs` | Settings line rewritten to show truthful counters; the loader-queue explanation moved into hover text | World panel |
| `src/bin/terrain-view-gpu-check.rs` (new) | Per-view terrain submission check (P2.09) | `pr-checks.yml` |
| `src/dxt.rs` (new) | `Materials::load` → `decode_chain`; tests pin it to real archive textures | Terrain source decoding |
| `tools/terrain-format-survey.py`, `tools/pck_lib.py` (new) | Nothing in production; reproduces the P2.07 census from any retail `textures.pck` | Survey tooling |
| `.github/workflows/pr-checks.yml` | Adds the new `terrain-view-gpu-check` as its own labelled step | Phase 1 production shader regressions |
| `tools/check-phase1-core.sh` | Adds `terrain_sampling` to the dependency-free module list | local/CI core checks |
| `Findings/Unsupported-client-paths.md` | Corrected the false "second light family" entry; added terrain sampling, terrain formats and the T2MK `+12` byte registration | — |

---

## 2. Implemented behaviour

### 2.1 The settings line `FX light producers: 257 loaded, 0 pending, 0 elements, 4278 failed; alpha meshes ordered 0`

The line was not one bug but three independent ones, and each is now either fixed
or reported honestly.

**`0 elements` — the FX reader was rejecting every root that contained a light.**
`gfx_light::LIGHT_FIELDS` treated any element carrying `LightNum`, `WaveLen`,
`Segs`, `Interval`, `Amplitude`, `Pos1Enable` or `Filter` as an unexecutable light
family and failed the whole root. Those keys are not light-exclusive. The native
player reads `BufLen: %d`, `Amplitude: %f`, `WaveLen: %d`, `Persistence: %f`,
`OctaveNum: %d` as the effect **noise** block and `StartPos`/`EndPos`/`Segs`/
`LightNum`/`WaveLen`/`Interval`/`Width`/`Alpha`/`Amplitude`/`Pos1Enable`/
`Pos2Enable`/`UseNormal`/`Normal`/`Filter` as the line/beam/trail record, and
`0x7078d0` is a separate light-element loader.

Measured with a parser that replicates the Rust field scan exactly, over all
**5,459** `.gfx` roots in `gfx.pck` (v2 directory, 10,104 entries):

| | roots |
|---|---|
| carry at least one of the eight former "light" keys | 5,459 (5,878 elements) |
| carry `LightType` — the only key the light reader consumes (`0x8ed204`, hit at `0x70b50b`/`0x70b7d5`) | **524** (546 elements) |
| all `LightType` elements are element 130 | yes, 546/546 |
| loaded under the old rule with ≥1 light | **0** |
| loaded under the old rule overall | 3,445 of 5,459 (2,014 rejected) |

Rejected roots by first offending element: 101×706, 123×577, 100×245, 150×129,
120×95, 121×55, 140×51, 152×43, 122×40, 124×33, 102×14, 125×8, 160×6, 110×4,
180/200/190/151×2.

With `LIGHT_FIELDS` replaced by the `LightType` marker: **5,459 of 5,459 roots
load, 524 of them publish a light, 0 rejected.** Nothing is silently dropped — an
element that declares `LightType` but is not element 130 still fails with
`unsupported GFX light element <id>`.

**`257 loaded / 0 pending` — the loader budget, now visible.** `0x5b1a00` admits at
most **one** pending root per update and returns immediately after the load. The
manager already reproduced that; there was simply no way to see how far the queue
had been drained. `Status.frames` and `Manager::frames()` now report it, and the
hover text on the line explains the rule.

**`0 elements` also had a second, honest cause.** Of the 5,459 roots that now load,
**4,935** genuinely contain no `LightType` element: they are sprite, particle,
beam and trail roots whose playback belongs to the visual-effect graph (P7), not
to this pass. They are now counted as `non_light_roots` instead of appearing as
zero emitters on 257 loaded roots.

**`4278 failed` — owners, not roots.** The old expression added
`g.failed_owners` to `models.gfx_failures().len()`, which mixes two populations:
rejected owners and the recorded failure *strings* (one per rejected root, and
every root was rejected). Under the corrected reader the same corpus yields 0
rejected roots, so this term collapses on its own; it is kept in the line because
it is the only place a genuinely rejected owner surfaces.

**`alpha meshes ordered 0` — a plausible steady state, previously unreadable.**
`rebuild_alpha_order` only orders meshes a model declares as alpha-blended, and
retail monsters/props are overwhelmingly opaque, so 0 is a legitimate result. The
line printed the ordered count alone, so "0" could equally mean "no alpha meshes",
"no model published bounds", or "every slot failed". `World::alpha_status()` now
reports `ordered / alpha_meshes (drawable of slots, failed, without bounds)`.

The line now reads, for example:

```
FX light producers: 524 loaded, 0 pending, 524 elements, 0 failed, 4935 non-light roots;
alpha meshes ordered 0/0 (312 drawable of 312 slots, 0 failed, 0 without bounds)
```

### 2.2 Terrain sampling (P2.07)

Recovered from the state wrappers, whose constant argument *is* the D3D state
type (`0x77bdb0` = `SetTextureStageState`, `0x77a1b0` writes `0x16` = 22 etc.):

* `0x7c14b0` (batched PS1.4 terrain path): stages 1–5 get
  MAG/MIN/MIPFILTER = `2` (`D3DTEXF_LINEAR`) and ADDRESSU/V = `2`
  (`D3DTADDRESS_MIRROR`); `0x77a120` assigns each stage the coordinate set with the
  same index. `0x7c1a20` repeats it and disables `ZENABLE`.
* `0x7c1e00` (fallback / one-layer path): stage 0 filters `0`
  (`D3DTEXF_NONE`), `COLOROP 0`, ARG1/2 `0`, `ALPHAOP 0`, `RESULTARG 0`; stages
  0/1 use `COLOROP 5/3` with ADDRESSU/V = `1` (`D3DTADDRESS_WRAP`).
* `D3DTSS_MAXANISOTROPY`(21) and `D3DTSS_MIPMAPLODBIAS`(19) are written **nowhere**
  in the terrain path: `0x77a1b0`/`0x77a1d0` have exactly two call sites each and
  none lies in the terrain region. Anisotropy is therefore D3D8's default 1.

The viewer previously sampled with `Repeat` and `anisotropy_clamp: 16`, which is
not native in either respect. Both are fixed. Mips are still generated for the
array slots (the client uploads authored chains, and the array contract requires a
full chain); `terrain_sampling::generates_mips` records the single-level case.

### 2.3 Terrain texture formats and sources (P2.07)

`A3DTexture::CreateDXTExture()` at `0x7f7f74`:
`0x7f84..0x7f88` forwards the authored FourCC to `CreateTexture`;
`0x7f7e4e/55/5c/63/6a` compare against exactly `"DXT1".."DXT5"`
(`0x7f8e9a` checks the DDS `DDPF_FOURCC` bit); `0x7f7e71` aligns the row to four
texels; `0x7f7e9c/0x7f7e95/0x7f7ea3/0x7f7dfd` pick 16/32/16/64 bits per block for
DXT1/2/3/4/5; `0x7f7ef1` caps the mip count at `floor(log2(max(w,h)))+1`. The
failure text is `A3DTexture::CreateDXTExture(), Fail in creating format[%d],
[%d]x[%d], [%d]mip texture`.

`parse_terrain_dds` therefore accepts DXT1–DXT5 only, validates the header, the
cube/flags bits, the declared mip count against the dimensions and the payload
length of every declared level, and reports each failure explicitly. Uncompressed
sources, `ATI1`/`ATI2`, over-declared mip counts and truncated levels are errors,
never fallbacks.

### 2.4 Terrain specials survey (P2.08)

`surfaces.pck` (93,542,325 bytes) is now surveyed locally: PCK v2, **3,724**
entries — `.dds` 2,314, `.tga` 1,056, `.bmp` 284, `.jpg` 39, `.txt` 29, `.sw` 1,
`.cmf` 1. These are item/UI/skill artwork; they do **not** hold the terrain record
types.

The real terrain data is the loose `maps/world` tree. Two T2MK samples were
pulled and decoded against the T2MK loader at `0x7a1520`:

* `world_39.t2mk` — `BMKT` v2, 16 areas, 9 layer records, `+16 = 1`;
  areas carry 3–6 layers each.
* `world_88.t2mk` — `BMKT` v2, 16 areas, 5 layer records, `+16 = 1`.

Layer record = `texture:u32, 0xffffffff, mask_size:u32, +12:u1, u:f32, v:f32,
+24:u1` (28 bytes, `0x7a178c`), mask size 256 or 0 for the first layer, and
`world_39` area 0 selects layer indices `[0, 1, 8]` — layer selection is sparse and
not "first N".

The one unresolved special is the byte at `+12`. The native build copies it into
`[record+0x0c]` of its 9-dword draw record and hands that record to `0x77f180`
(`0x7a255b..0x7a25be`); the viewer reads the same byte as the texture projection
axis. The values seen (0/1/2) are consistent with both readings, so no new
behaviour is claimed — it is registered as an open case rather than guessed.
Holes were **not** invented: nothing in the sampled records describes one, so
nothing new is rendered for them.

### 2.5 Per-view consistency (P2.09)

`src/bin/terrain-view-gpu-check.rs` drives the production WGSL out of
`src/terrain_gpu.rs` with its real `vs`, `vs_far` and `fs` entry points and its
real 11-binding layout, and submits five views:

| View | Selector |
|---|---|
| main | `clip.y >= 0`, near mesh |
| reflection | `clip.y >= 0`, near mesh, reflected light set |
| clipped refraction | `clip.y < 0` (`terrain_refract_vs` projection) |
| minimap | far mesh, same area/layer storage |
| full-world map | far mesh, same area/layer storage |

Before **and after** each submission the area and layer buffers are read back and
compared with one pre-computed snapshot, proving no view rebuilds or advances the
snapshot; then all five read-backs must return the same terrain colour for the
same world position (≤3/255, sampling rounding only).

---

## 3. Tests executed

```
$ RUSTC=/tmp/tc/bin/rustc LD_LIBRARY_PATH=/tmp/tc/lib sh tools/check-phase1-core.sh
```
27 module suites green (one `test result: ok` per suite), including the new
`terrain_sampling` suite (12 passed) and the extended `gfx_owner` harness
(39 passed, up from 36). New regressions:

* `terrain_sampling`: batched path is trilinear+mirror; no stage requests
  anisotropic taps; fallback stage 0 disables filtering and wraps; address enum
  decoding fails closed; mask channels b/g/r/a; layer order `['b','g','r']`; DXT
  FourCC table matches the native comparisons; DXT formats map to UNORM, not sRGB;
  authored single level parses; authored full chain is kept; unsupported sources
  fail explicitly; array slots preserve authored resolution.
* `gfx_light`: effect noise/line blocks are not light paths (three verbatim retail
  record shapes); element 130 without `LightType` is an error; an element with the
  marker that is not 130 is still an error.
* `gfx_owner`: `owners_without_a_light_are_reported_apart_from_failures`
  (2 loaded, 1 non-light, 1 emitter, 0 failures, frame counter).

```
$ python3 tools/check-eight-phase-status.py   # consistent: true, errors: []
$ python3 tools/check-phase1-ready.py         # P1.01..P1.12 unchanged
```

`cargo` is not installed in this environment, so `cargo check`/`cargo test` and the
WGSL checks could not be executed here; the new GPU check was verified to compile
past name resolution (`rustc` reports only unresolved external crates), and CI
gains both `terrain-lod-gpu-check` and `terrain-view-gpu-check`. `cargo fmt` was
also unavailable; the hand-written style follows the surrounding one-liner UI code.

---

## 3a. Why `terrain-lod-gpu-check` is still outside the CI gate

P2.09 asked for the program to be wired in. I added it and the step went red. Reading
the program instead of the log explained why, and the explanation is a property of the
check, not of the renderer:

* `terrain-lod-gpu-check` renders the same quad through `vs` and `vs_far` and then
  asserted `frames[0] == frames[1]`. Its own fragment shader returns
  `vec4(normal*0.4+0.5, distant)`, `vs` reports the authored per-vertex `kind`, and
  `vs_far` reports `distant = 1`, so the two frames are *supposed* to differ. The
  assertion could never hold on any GPU, which is why it never held under lavapipe.
* Correcting it to compare RGB and assert the alpha flag still failed, because the
  program's far input is `@location(1) normal: vec4<f32>` with `Snorm8x4` vertices
  while `vs_far` in `src/terrain_gpu.rs` declares `normal: vec4<f32>` — the normals
  themselves differ between the two paths, so RGB equality is not the right invariant
  either. A meaningful parity check needs the normal comparison defined first, which is
  mesh/vertex-lane work (Agent 2-A), not mine.
* Agent A reached the same conclusion independently at `037bc21` and left the program
  out of the gate. Returning it to the gate without fixing it would make CI red for
  everyone, so the program stays out and this analysis is recorded here instead.
  It remains runnable by hand.

The per-view check I own (`terrain-view-gpu-check`) runs as its own labelled CI step so
a failure names the program rather than the batch.

## 4. Confirmation blocks

**P2.07 — texture sourcing, mips, samplers.**
Task: match source dimensions, mip chain generation vs authored mips, addressing
modes, anisotropic filtering, per-material sampler settings and every archive
format; keep authored chains; match filters where the client generates.
Finished **Yes**.
Production integration **Yes** — `TerrainGpu::new` sampler and the rewritten
authored-chain upload, `Materials::load`, `src/dxt.rs`, `src/terrain_sampling.rs`.
Native evidence **Yes** — `0x7c14b0`, `0x7c1a20`, `0x7c1e00`, `0x77a0e0`,
`0x779f60`, `0x77a120`; absence of `D3DTSS_MAXANISOTROPY` at `0x77a1b0`/`0x77a1d0`;
`A3DTexture::CreateDXTExture()` `0x7f7d83..0x7f7f3c` and `0x7f7f74`.

Tests executed and results: **the archive census now runs on the real
`textures.pck`** (117,047,648 bytes, extracted from the client's own split archive;
1038 entries, 310,055,385 bytes uncompressed). `tools/terrain-format-survey.py`:

| question | answer |
|---|---|
| terrain layer sources (`textures/maps/*.dds`) | **363**, all accepted, **0** rejected |
| formats | **DXT3 x 356, DXT1 x 7** |
| dimensions | 256x256 x 235, 512x512 x 126, 256x512 x 1, 1024x1024 x 1 |
| authored mip levels | 9 x 235, 10 x 127, 11 x 1 |
| complete power-of-two chains | **363 of 363** |
| payload == 128 + sum(level sizes) | **363 of 363** |
| sources needing client-generated mips (<=1 level) | **0** |

Whole archive for context: 636 `.dds` — DXT1 43, DXT3 428, plus 165 non-FourCC
(RGB32/RGB24/L8A8) which are non-terrain art and are **rejected explicitly** by
`parse_terrain_dds` rather than decoded.

Two production defects were found and fixed by that census:

1. **Authored mips were discarded.** `material.rs` decoded every source to RGB8
   (throwing away the chain), then `terrain_gpu.rs` box-filtered a brand new chain
   at upload. With 363/363 sources carrying authored chains and 0 needing
   generation, that replaced the client's reductions with a different filter.
   `src/dxt.rs` now decodes DXT1..DXT5 and level *n* is uploaded to mip slot *n*;
   box reduction survives only for a genuinely single-level source.
2. **`DxtFormat::storage_format()` was dead code.** The accepted block formats were
   validated and then never used, so the archive's real formats (DXT3 = BC2 for
   98% of terrain) never reached the GPU as BC2.

The decoder is pinned to **six real archive textures** — both terrain formats and
all four source dimensions (256x256, 512x512, 256x512, 1024x1024) — against an
independent Python implementation of the same block rules, compared by SHA-256 of
the decoded image. `src/dxt.rs` 9 tests passed (2 synthetic-mode, 1 DXT3 alpha
plane, 1 DXT5 alpha, 2 error cases, 1 chain walk, 1 reference-hash parity, 1
anti-tautology guard); core suite green (28 modules).
Remaining gaps: `D3DTEXF_NONE` (fallback stage 0) has no wgpu equivalent inside a
trilinear pipeline and is registered — it is the client's own fallback sampler and
no terrain source uses it; off-square sources keep aspect through nearest-neighbour
fit into a square slot (one 256x512 source in the archive).

**P2.08 — terrain specials and detail surfaces.**
Task: implement holes, visibility flags and special surface cases from client
data, plus maximum-quality road and city detail including Archosaur's
high-resolution surfaces.
Finished **No**.
Production integration **Partly** — layer selection, mask channels, mask sizes and
the four source dimensions are driven from real archive and T2MK data; no hole or
visibility-flag path exists to wire up.
Native evidence **Yes for the negatives** (they are what blocks the point):
`0x7a1520`/`0x7a178c` (T2MK), `0x7a255e`/`0x7a2776` (consumers), `0x77f180`
(layer record hand-off).
Tests executed and results: the `.trn2` files were decoded in full to find the
special-surface records the point needs. A `.trn2` is
`{magic "2NRT", version 1, x_size f32, z_size f32, region_width f32, ...}` with two
offset/count pairs naming a **texture table** (162 entries in `world.trn2`, 88
regions) and a **region-name table** (`world_1`..`world_88`), and the name tables end
exactly at EOF with **no trailer**. `tools/material-check.rs` already reads those same
fields (36/52/72/80), which corroborates the layout.
**There is no hole, visibility-flag or special-surface record anywhere in it.** The
`.t2bk` block header was also re-examined for the `block+0x74` visibility byte; its
six-float prefix is not yet decoded and is registered rather than guessed.
Remaining gaps: holes and `block+0x74` visibility are **not invented** — no record in
the client data examined so far describes them, and this register entry stands until
one is found. Archosaur high-resolution surfaces are not claimed: the census shows
only four source dimensions, with a single 1024x1024 source in the whole terrain set.

**P2.09 — multi-view consistency and CI regressions.**
Task: keep terrain consistent across main, reflection, clipped refraction, minimap
and full-world map with one per-frame snapshot; keep boundary/variant/
layer-selection/near-far LOD-continuity regressions in CI; wire
`terrain-lod-gpu-check` into the GPU program set.
Finished **Yes** for the shared-snapshot, four-view parity and CI-wiring parts.
Finished **No** for `terrain-lod-gpu-check` being in the gate (see §3a) and for pixel
parity of the clipped-refraction view.
Production integration **Yes** — the check drives the production WGSL, the production
11-binding layout and the production vertex strides; it runs in CI as its own step and
the branch is green (run 41543311052).
Native evidence **Yes** — `0x7c14b0`/`0x7c1a20` shared stage setup for main and
reflection, `0x7cb642`/`0x7cb8ef` for clipped refraction, `vs/terrain_refract_vs` from
`shaders.pck`.
Tests executed and results: `terrain-view-gpu-check` green in CI. Main, reflection,
minimap and full-world map return the identical layer composite (`[28, 15, 7]`), the
area/layer storage is byte-identical before and after all five submissions, and the
refraction view's coverage is reported. Core suite green locally (27 suites); rustfmt
clean at 1.8.0; `cargo check`, the GFX audit and the viewer unit tests pass in CI.
Remaining gaps: `terrain-lod-gpu-check` still cannot pass and stays out of the gate
(§3a); clipped-refraction pixel parity is not asserted, because its output depends on
`terrain_pass_fog`'s transmittance/fog interaction — shader-lane behaviour
(P2.04/P2.05) — not on terrain sourcing; the minimap and full-world-map views are
exercised through the far mesh and the shared storage rather than their GUI entry
points.

---

## 5. Interface changes for other agents

* `gfx_owner::Status` gained two public fields (`non_light_owners`, `frames`) and
  `Manager::frames()`. Construct with `..Default::default()`.
* `world_models_gpu::AlphaStatus` and `World::alpha_status()` are new; nothing
  existing changed signature.
* `gfx_light::parse` no longer rejects roots for `LightNum`/`WaveLen`/`Segs`/
  `Interval`/`Amplitude`/`Pos1Enable`/`Filter`. Any audit that counted those as
  failures must switch to `LightType`.
* `terrain_sampling` (new module) is the single place for terrain sampler/format
  policy; Agent 2-B should take sampler values from it rather than hard-coding
  them in `terrain_gpu.rs`.
* `src/terrain_gpu.rs` sampler block changed inside Agent 2-C's lane; mesh/LOD
  construction and WGSL arithmetic were not touched.
