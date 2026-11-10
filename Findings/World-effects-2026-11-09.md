# World effects — the 9 November reports, and three decode errors behind them

Second pass over the four reports left open by
[`Every-effect-2026-11-08.md`](Every-effect-2026-11-08.md). The three defects this
pass fixed are all **class-level** defects — they change what nearly every effect
in the game draws, not one root — and all three come with a native citation and a
unit test that fails on the previous code: ring sections, ring pitch, grid UV
spread, and (section 5.2) the ground-draped decal pass.

This environment has no Rust toolchain, so none of this was compiled while it was
written: the tests were reasoned through by hand and the call sites checked with a
parser. CI then built it — run
[`42663541912`](https://github.com/kasperfriend/Perfect-World-Explorer/actions/runs/42663541912)
passes both jobs, "Rust checks and tests (Linux)" (all binary targets compile, the
whole unit suite including the five new tests) and "Windows executable (x64)".

Two of the first attempts did not compile, and the way they failed is worth
recording because it is a property of this code base rather than of the fix:
`world_models_gpu.rs` and `gfx_owner.rs` are also compiled into the survey
binaries, whose crate root has no `navigation` module, so a `navigation::MapSpace`
field there is a hard error; and `advance_with_ground` is a public signature the
walker add-on compiles against, so its seventh argument broke
`modules/walker/gallery.rs`. Both are fixed by keeping the module's own type out
of the struct (a boxed height closure) and by adding `advance_with_terrain`
beside the six-argument entry point instead of changing it.

Everything in the *Measurements* sections was computed here, from the shipped
data.

## 1. The element classes, against the client's own table

`A3DGFXElement.h` names the ids and `A3DGFXElement::CreateEmptyElement`
(`A3DGFXElement.cpp:598`) constructs the class. The viewer's names differ in
places, which has caused confusion in these notes; the behaviour is what matters,
so the table is recorded once, with the viewer's name beside it.

| id | client class | viewer name | corpus elements |
|---|---|---|---|
| 100 | `A3DDecalEx` — 3D decal | sprite | 3,537 |
| 101 | `A3DDecalEx` — 2D (screen-space TL vertices) | sprite | 5,244 |
| 102 | `A3DDecalEx` — billboard | sprite | 83 |
| 110 | `A3DTrail` | ribbon | 2,956 |
| 120–125 | `A3DParticleSystemEx` per emitter class | particle | 12,360 |
| 130 | `A3DGFXLight` | light | 546 |
| 140 | `A3DGFXRing` | cylinder | 1,116 |
| 150 | `A3DGFXLightning` | fractal | 734 |
| 151 | `A3DGFXLtnBolt` | lightning | 11 |
| 152 | `A3DGFXLightningEx` | trail | 156 |
| 160 | `A3DGFXModel` | model | 447 |
| 170 | `A3DGFXSound` | sound | 491 |
| 180 | `A3DLtnTrail` | wave | 79 |
| 190 | `A3DParaboloid` | curve | 208 |
| 200 | `A3DGFXContainer` | child | 1,530 |
| 210 / 211 | `A3DGridDecalEx` (3D / 2D) | grid | 215 / 1 |

Two consequences worth stating: `140` is a **ring band** (a hollow cone-like
strip, no caps), not a solid cylinder, and `150`/`151`/`152` are three different
lightning classes rather than one family with three names.

## 2. Finding: `Sects: 0` is the default, and the default is 36

`A3DGFXRing::InitStreamBuffer`:

```cpp
if (m_nSects) nSect = m_nSects;
else          nSect = _default_sects;      // static const int _default_sects = 36;
```

The editor writes `Sects: 0` when the artist never touches the field, and it does
so on **1,064 of the corpus's 1,116 element-140 records** (`Sects` is `0` on
1,064, `8` on 24, `10` on 11, `30` on 7, and single digits on the rest; no record
omits the key). `emit_cylinder` read the zero literally and clamped it with
`.max(3)`, so **every one of those 1,064 rings was drawn as a three-sided
prism** — three quads spanning 360°, straight edges where a round band belongs.
That is the "angular slab" appearance of the altar bands, the city's light rings,
and every `圆环光`/`图案` ring in the world.

Fixed: `0` now means `DEFAULT_RING_SECTIONS` (36), and the count is only clamped
upwards to 3 for a damaged record. Test
`a_ring_authored_without_sects_is_thirty_six_sections` asserts 36 quads and
fails on the old code, which emitted 3.

## 3. Finding: `Pitch` is a texture pitch spent on the radii

The same function, a few lines down:

```cpp
float fHalfHeight = .5f * m_fHeight;
float fLimit      = m_fRadius / fHalfHeight;
float fTan        = (float)tan(m_fPitch);
if (fTan > fLimit) fTan = fLimit;
else if (fTan < -fLimit) fTan = -fLimit;
float fDeltaRadius  = (float)(fHalfHeight * fTan);
float fUppderRadius = m_fRadius - fDeltaRadius;
float fLowerRadius  = m_fRadius + fDeltaRadius;
```

So the band's **foot is the wide edge and its roof the narrow one**, by
`halfHeight * tan(pitch)`, clamped to `radius / halfHeight`. That is how a flat
ring texture is made to read as a surface seen at an angle.

The viewer instead added `pitch` to the **angle** of the first vertex
(`a0 = … + cylinder.pitch`), which spins the band about its axis, leaves both
rows exactly one radius, and leaves a seam where the strip closes. 1,116
elements author the key; 365 author `0`, the rest land on 15° steps from
`-1.047` to `2.618` rad, so three quarters of the game's rings were affected.

Fixed to the client's construction, with the same clamp and a guard for
`tan(pitch)` at ±π/2. Test `a_ring_pitch_tilts_the_band_through_its_radii`
asserts that a level ring keeps one radius and that `atan(0.5)` widens the foot
and narrows the roof by `tan × height` — half the radius on a unit ring. The old
code satisfied neither. The strip's UVs were also turned over to the client's
order while there (`tv = m_fTexV + m_fTexHei` on the foot row, `m_fTexV` on the
roof row, `tu = i * m_fTexWid / nSects + m_fTexU`), because the viewer had the
two rows swapped and let `u` run past a narrowed flipbook cell.

## 4. Finding: a grid spreads **one** texture rect over its lattice

`A3DGridDecalEx::_setUVCoord` walks the whole vertex lattice exactly once:

```cpp
const float fUStep = (fU1 - fU0) / (m_nWidthVertNum - 1);
const float fVStep = (fV1 - fV0) / (m_nHeightVertNum - 1);
```

with `fU0 = m_fTexU`, `fU1 = m_fTexU + m_fTexWid` (the flipbook cell, already
scrolled by `TexSpeed` in the base class). A 2×4 curtain therefore samples half
the sheet per row, once.

`emit_grid` gave every cell the whole rect instead. On the corpus's dominant 2×4
geometry that drew **three stacked copies of the image** down one curtain and
restarted the `TexSpeed` scroll at each row boundary, so a falling sheet read as
three sheets sliding over each other. Fixed to the per-vertex spread
(`u = u0 + du * col / (columns - 1)`, `v = v0 + dv * row / (rows - 1)`), which is
also exactly what the gravity-fed `大坑_液体`, `水片圆/长` and `汐族光翼` grids
want. Test `a_grid_spreads_one_texture_rect_over_its_lattice` asserts the row
step is half the sheet; the old code emitted the same rect on both rows.

## 5. Report status

### 5.1 Waterfalls still read as a still image — mechanism present, source not identified

Measured this pass, from the shipped `world.ecwld` (2,275 effect records, layout
below):

* The City of Raging Tides water gate carries **four `场景/新种族地图/水门_瀑布.gfx`
  placements** at scale 2.736, at `(2619, 239, −4120)` and three neighbours ~13–40
  units away. That root is a waterfall: four element-100 sheets whose textures are
  `纹理\气连续03.tga`, `纹理\流动（通道）.dds`, `normalmap\gas.dds` and
  `物体\humianbowen.dds`, each with a **negative `TexSpeed` V** (−1.5, −1.8, −2.0)
  on top of a `TexRow`/`TexCol` flipbook at 50 ms, plus two element-124 spray
  emitters and a `烟火\沙.tga` sheet. The viewer's `flipbook_uv` applies both
  mechanisms, and the existing test refuses to accept a frame that repeats.
* The **model** waterfall (`瀑布.smd`, the mesh `瀑布.ecm` points at) carries no
  animation at all: `瀑布.ecm` is `MOXTVersion: 13` with `ComActCount: 0`, `AddiSkinCount: 0`,
  `ChildCount: 0` and nothing but a pointer at the `.smd`; the `.ski` material
  block holds one `瀑布.sdr.tga` and a handful of 1.0-ish floats, no UV speed; the
  sheet itself (128×256, 32bpp) is a single vertical fall with a defined top and
  bottom, so it is not built to tile either. A static sheet there is **faithful**
  to the asset.
* The map's only other waterfall content is spray: 26 placements across
  `瀑布水花`, `瀑布水花中间`, `瀑布水花中间2`, `瀑布水花底` and `瀑布水花湍急`
  (scale 1.0–10.4), plus `水柱` ×5 and `水片圆/水片长` at the sea edge.

**Not established** which of the two the report means. The measurement that
settles it is a count of submitted quads at a waterfall, or the name of the root
the camera was pointing at — the viewer's status line reports both. Until then
this pass claims only the two grid/ring fixes above, neither of which is the
waterfall's animation.

### 5.2 "Gas" at the Nightfire altar is a blob — the ground-draped decal path

The client has a second, per-vertex ground pass that this viewer does not have.
`A3DDecalEx::Fill_Verts_3D`, taken only when

```cpp
if (m_bGroundNormal && !m_bGrndNormOnly && m_pGfx->IsCloseToGrnd() && m_pGfx->MatchGrnd())
```

tessellates the quad into `nRow × nCol` cells of `_grnd_grid` units (capped at
`_max_row_or_col`) and asks the terrain for **every vertex's** height:

```cpp
vert.y = AfxGetGrndNorm(*(A3DVECTOR3*)&vert, NULL) + .2f;
```

with `fRow = fHeight2 - i * fGrdSzH` and `fCol = -fWidth1 + j * fGrdSzW` in the
owner's flat X/Z. In other words a ground-matched decal is a **draped sheet**, not
a plane, and it sits 0.2 above the surface it drapes.

The viewer's `ground_frames`/`ground_parent` reproduce the *element-level* frame
selection of `0x6fa7c0` faithfully — height, orientation, `GrndNormOnly` — but
nothing in the sprite path samples the terrain per corner. Every ground-matched
decal is therefore a flat quad at one height. On a bowl-shaped crater floor that
is exactly the reported symptom: the sheet's middle pokes through the ground and
its rim is buried, so a large authored sheet reads as a **small blob**, and the
parts that show are the ones nearest the single sampled height.

The population is large: **923 element-100..104 records** set `MatchGrnd: 1`
across the corpus (skill circles, fire on the ground, altar glows, the fjord's
mist), so this is not a per-location patch either.

**Fixed in this pass.** The two constants are file statics, not header members,
so the missing `A3DGridDecalEx.h` was never needed: `A3DDecalEx.cpp:29-30` gives
`_grnd_grid = 2.0f` and `_max_row_or_col = 4`, i.e. two-unit cells and at most
sixteen quads per sheet.

What landed:

- `gfx_player::GroundHeightQuery` / `GroundSampler` — the surface query the
  sprite path needs, distinct from `GroundHit` because the drape wants a height
  at every lattice point while `GroundHit` is the single sample the owner's own
  frame is built from.
- `Player::advance_with_ground` takes `terrain: Option<GroundHeightQuery>` as a
  new last argument. The near-ground byte the branch gates on (`IsCloseToGrnd`)
  is measured from the query itself — the owner's y minus the surface, signed,
  exactly as `0x6e54a1` does it — so a caller with terrain and no raycast still
  gets the drape, and a caller with neither behaves precisely as before.
- `drape_sprite` reproduces `Fill_Verts_3D`'s branch: element 100 only,
  `MatchGrnd` set, `GrndNormOnly` clear. The sheet is laid out in the **world**
  XZ plane (`x = center.x - Width1 + j*cell`, `z = center.z + Height2 - i*cell`),
  not the element's own plane, so it drops the element's pitch, yaw and 2D spin;
  the cell count is the parent-scaled extent over 2 truncated and clamped to 4;
  each vertex takes `terrain(x, z) + 0.2`; the flipbook rect is spread over the
  whole lattice instead of repeated per cell. A miss anywhere under the sheet
  leaves the whole sheet flat, because half a drape tears.
- `gfx_owner::Manager::effect_quads_with_ground` and
  `world_models_gpu::World::set_terrain` carry the map's `navigation::MapSpace`
  to the world's effects; `gpu.rs` installs it once, in the same block that
  hands the modules the effect catalogue. The heights sit behind an `Arc`, so the
  per-frame copy is a refcount bump.

Two limits to state plainly:

- `YawEffect` (`m_bYawEffectOnGrndNorm`) is not implemented, so 19 of the 906
  ground-matched sprite records stay world-axis-aligned when the client would
  have rotated them by the element's direction. The rest author 0 (586) or carry
  no line at all (301).
- Only the sprite path drapes. `A3DGridDecalEx`/element 210 is a different class
  with its own ground handling and was not touched.

Population actually affected, measured across the corpus: **906** element-100/102
records set `MatchGrnd: 1`; by the client's cell rule 536 of them are one cell
(a single flat quad, still re-oriented into the ground plane) and 370 split into
2×2 (172), 3×3 (60), 4×4 (134), 1×4 (3) and 3×2 (1).

### 5.3 City of Raging Tides: blue glow mis-placed, lightning missing / white blob

Both items sit in the same neighbourhood as the water gate above, and the two
roots there were read element by element:

* `主城光球大.gfx` (four placements at scale 5.97, `(2669, 259, −4115)` and
  neighbours) is 17 elements: six element-140 rings on `纹理\Water00.bmp` stacked
  from 0.78 to 5.97 above the base, two `粒子\guangshu.dds` beams, two element-101
  `normalmap\gas.dds` discs at 3.44 and 2.56, two element-123
  `序列\LightningBallblue3x2.dds` emitters, one `SmallGlowCyan` emitter, and four
  element-200 children. Two of those children are **authored duplicates at the
  same offset** `(0, −0.740011, 0)`; that is in the data, not a viewer fault.
* `主城光球小.gfx` is six more element-140 rings and the same beam/disc set.

A 5.97-scaled ring whose `Sects` is 0 was, until this pass, a three-sided prism —
and it is drawn **in the plane of the owner's direction**, which points
`(0.85, 0, 0.52)`, i.e. diagonally. A triangular slab 12 units across at that
orientation is a good candidate for "a blue glow in the wrong place on the left",
and the `Water00.bmp`/`流动（通道）` rings are precisely the blue glow. This is a
candidate, not a proof: it is measurable now that the rings are round, by looking
at the same spots again.

The "Lightning" with a "white glow blob" is most likely the `LightningBallblue3x2`
emitters — 2×4 and 0.5×1.7×0.5 extents, `scale 2..4`, i.e. a ball-and-arcs pair —
or the element-152 strips. What makes a *white* blob is already known from
Update 5 of the previous note: a texture that fails to resolve paints the effect
pass's stand-in. The 64 names a case-sensitive sweep had reported inside
`场景/新种族地图/` all resolve once the reader's own rules are applied
(lower-case, slash-normalised, extension fallback): across that folder's **179
roots and 247 distinct texture names, none fails to resolve** — including
`闪电\ShockLightning.dds` (on disk as `tex/闪电/shocklightning.dds`),
`序列\LightningBallblue3x2.dds` and `粒子\SmallGlowCyan.dds`. So the white blob is not a
missing texture in that folder; it has to be a blend/geometry defect in the
element, and both candidates above now draw with the corrected ring and grid
geometry.

### 5.4 The general sweep

Two class-level decode errors fixed (§2, §3) plus one geometry error (§4). A third
gap was found and is recorded rather than fixed (§5.2). One more decode omission
was found and left open deliberately:

**`IsAvgGen` is reached by real records.** The previous note lists
`is_average_generation*` as "no corpus element reaches the routine that would read
them". That is wrong: **45 element-123 records author `IsAvgGen: 1`**, with
`AlphaSeg: 8..19` and `BetaSeg` equal to it. `A3DEllipsoidEmitter::InitParticle`
switches on exactly this flag (`if (m_bIsGenAverage) GenAverage(...)`), and
`GenAverage` places each particle on the ellipsoid's **surface** at the current
`(alpha, beta)` lattice point, scales by a uniform draw only when the emitter is
*not* a surface emitter, and then advances the angles by `2π/AlphaSeg` and
`π/BetaSeg` with wrap rules that restart beta at 0 when it would pass π. The
viewer's `spawn` has no such lattice: it draws one direction per particle. The
difference is a banded, evenly-covered shell against a random one, which matters
for shells whose particles are meant to tile a dome. Not implemented: it needs
per-emitter angular state threaded through `spawn` and the emitter's own record,
and 45 records do not justify an uncompiled change here.

## 6. Measurements used above

### 6.1 `world.ecwld` effect record

60 bytes, then a `u32` byte-length and that many bytes of GBK path with no
terminator:

| offset | field |
|---|---|
| 0 | `u32` export id |
| 4 | `f32` uniform scale |
| 8..20 | `f32[3]` position |
| 20..32 | `f32[3]` direction (unit) |
| 32..44 | `f32[3]` up (unit, perpendicular to direction) |
| 44 | `f32` play speed |
| 48 | `i32` valid time (0 day, 1 night, 2 both) |
| 52 | `f32` alpha |
| 56 | attenuation byte, 57..60 padding |

Scanning the shipped file for that shape yields **2,275 records**, matching the
runtime count, with 1,445 all-day / 154 day-only / 676 night-only placements.

### 6.2 Corpus counts quoted in this note

| quantity | count |
|---|---|
| element-140 records | 1,116 |
| … with `Sects: 0` | 1,064 |
| element-140 `Pitch: 0` | 365 |
| element-100..104 with `MatchGrnd: 1` | 923 (906 of them 100/102, i.e. draped) |
| `.gfx` element keys named `TexU`/`TexV`/`TexWid`/`TexHei` | 0 |
| ECWLD effect records, by authored alpha | 1.0 ×2,008, 0.8 ×57, 0.5 ×45, 2.0 ×41, 0.3 ×34, 5.0 ×14, rest below 3.0 |
| element-123 with `IsAvgGen: 1` | 45 |
| `keyNumber` / `GridAnimationLines` in any corpus `.gfx` | 0 / 0 |
| `场景/新种族地图/` roots / distinct textures / unresolvable | 179 / 247 / **0** |

## 7. What this pass does not claim

* It does not claim the waterfalls are animated in the report's sense, nor that
  the gas blob and the blue glow are *proven* fixed — §5 names the candidate for
  each and the measurement that would close it.
* The two code changes were not compiled here. The tests fail on the previous
  code by construction (3 quads against 36; identical row UVs; equal ring radii),
  but CI is the first place they run.
* No new texture-resolution change is proposed. The earlier case-folded index was
  retracted because the reader was already case-insensitive, and re-measuring the
  tideborn folder through that same rule finds nothing missing.

## 6. The 9 November follow-up report, and the tool that ends the guessing

The second round of screenshots reports four things still wrong: white flashy
glow blobs (in several places), effects misplaced in City of Raging Tides'
floating city, the Nightfire altar unchanged, and static waterfalls. None of them
can be pinned from the files alone, and three cheap hypotheses died on contact
with the corpus, so they are recorded here rather than re-tried:

- **Authored UV sub-rects are not a thing.** The client's `Fill_Verts_3D` uses
  `m_fTexU`/`m_fTexWid`/`m_fTexV`/`m_fTexHei`, which looked like the missing
  piece for "the whole atlas is drawn instead of one cell". The corpus has no
  `TexU`, `TexV`, `TexWid`, `TexHei` or `TexNum` key at all: 30,019 `TexFile`
  lines, and UVs come only from `TexRow`/`TexCol` plus `TexSpeed`, both of which
  the player already honours.
- **The waterfall textures all resolve.** All seven `TexFile` values of
  `水门_瀑布.gfx` map to a member of `gfx.pck` (`纹理\气连续03.tga`,
  `纹理\流动（通道）.dds`, `normalmap\gas.dds`, `物体\humianbowen.dds`,
  `多个烟火\shuizhu01.dds`, `烟火\Effects_Textures_1184-1.bmp`, `烟火\沙.tga`),
  so the animated sheet is not disappearing through a missing texture.
- **Its clock runs.** Every element of `水门_瀑布.gfx` is a single keypoint with
  `TimeSpan: -1`, so `element_cycle` is infinite, `element_phase` returns the
  unclamped local clock, and the `TexSpeed` V scroll (−1.0 … −2.0) is applied
  from a clock that keeps growing. If the waterfall still looks frozen, the
  sheets are not being drawn at all and what is visible is the static BMD mesh —
  and that is a question for the runtime, not for the text.

So this pass adds the runtime tool rather than another guess.

### 6.1 `PWE_EFFECT_CENSUS=1` writes `effect-census.txt`

Set the variable and the world effect pass writes a report every two seconds to
`effect-census.txt` in the working directory (and to stderr where there is one —
the shipped Windows build has no console, hence the file):

```
Effect census: 4218 quads from 137 visible roots
  9.4x11.2 at (123.0,-45.0,88.2) quads=7 tex=纹理\气连续03.tga blend=2/4 alpha=1.00 gfx/场景/新种族地图/水门_瀑布.gfx
  ...
```

One line per visible root, largest single quad first — because an artefact in a
screenshot is a big quad — with its world size, its world centre, its texture,
its blend pair, the sampled alpha, the root's quad count and the root path.
`EffectQuad` carries no element id, but texture + blend + size + root narrows a
corpus search to one element in a couple of minutes.

Windows: `set PWE_EFFECT_CENSUS=1` in `cmd`, or `$env:PWE_EFFECT_CENSUS=1` in
PowerShell, before starting the viewer; the file lands next to the working
directory, which is the viewer's own folder for the portable build.

### 6.2 A measured anomaly: six roots author a scale no geometry can use

While ranking corpus quads to find the white sheets, one class of value stood
out as impossible rather than merely large. Ten `Scale:` keypoints in six roots
hold values between 4e17 and 1e27:

| root | keypoints | value |
| --- | --- | --- |
| `怪物/宠物通用/大龙虾_水波.gfx` | 1 | 1.104e27 |
| `人物/技能/刺客/回光返照_落光环.gfx` | 3 | 8.263e23 |
| `人物/技能/刺客/回光返照_左手.gfx` | 1 | 8.263e23 |
| `怪物/技能/变异沼泽鸟_技能.gfx` | 1 | 1.284e22 |
| `怪物/宠物通用/麋鹿左角.gfx` | 2 | 4.183e17 |
| `怪物/宠物通用/麋鹿右角.gfx` | 2 | 4.183e17 |

They sit in the last keypoint of otherwise sane ramps (`0.1, 1.3, 1.0, 1.2,
0.95, 8.26e23`), which is the shape of an editor writing an uninitialised float
into the final key, not of an intended size: a 1.9-unit card at 8.26e23 is
1.6e24 units across. `gfx_player`'s `ElementContext::scale` guards only against
non-finite and non-positive values, so these pass through intact and the quad
covers the screen. Both the `ScaleChage` cap (third field, `9999.0` in the same
file) and any client-side clamp remain uncited, so this file records the
measurement and nothing is clamped yet — a clamp without a citation would be the
same kind of guess this audit keeps having to retract.

It is *not* the reported white-blob cause: all six roots are character and
monster effects, and the blob report is about static scenery.

## 7. The census answered it: the ECWLD placement scale and alpha never reached a quad

Seven census logs arrived (City of Raging Tides, Nightfire Altar, Shadow Tide
Camp, Waterbeast Shore, Stairway to Heaven, two Etherblade sweeps). They print,
per visible root, the largest single quad in world units. Compare that against
what the file authors and the arithmetic names the defect in one line.

### 7.1 The measurement

For a 100/101 sprite the client's quad is
`2 x Width x DedaultScale x keypoint Scale x record scale`:

- `A3DDecalEx.h:167-177` — `SetSize` does `fWidth *= 2.0f; m_fWidth1 = m_fOrgPtWidth * fWidth; m_fWidth2 = fWidth - m_fWidth1;`, so the authored `Width` is a **half**-extent and the quad is twice it. `ID_GFXOP_DECAL_WIDTH` (`:125`) returns `m_fWidth*2.0f` for the same reason. The viewer matches this (`gfx_player.rs:2741`), so "the sprites are twice too big" is *not* a defect and never was.
- The census's "height" is the **diagonal**, because `EffectQuad::positions` is a triangle strip and corner 3 is diagonal from corner 0: `彩虹` reads `105.0x115.5` and `sqrt(105^2 + 48^2) = 115.4`, with `Height 8.0 x 2 x 3.0 = 48`.

| root | element `Width` | `DedaultScale` | record scale | client draws | census drew |
| --- | --- | --- | --- | --- | --- |
| `场景/彩虹.gfx` | 17.5 | 3.0 | **1.000** | 105.0 | **105.0** |
| `场景/屋顶八卦.gfx` | 22.4 | 1.0 | 0.304 | 13.6 | 44.8 |
| `场景/八卦.gfx` | 3.85 | 1.0 | 1.561 | 12.0 | 7.7 |
| `场景/新种族地图/白兰树.gfx` | 60.0 | 1.0 | 0.778 | 93.4 | 120.0 |
| `场景/新种族地图/血鲷封印台罩子.gfx` | 40.0 | 1.0 | 0.714 | 57.1 | 80.0 |
| `场景/新种族地图/0远完美浮空城.gfx` | 40.0 | 1.0 | 0.804 | 64.3 | 80.0 |
| `场景/新种族地图/完美远征地主塔_天上大圈.gfx` | 175.0 | 1.0 | 0.701 | 245.4 | 350.0 |

The control row is decisive: where the record scale happens to be 1.000 the
prediction lands on the census to the decimal, and where it is not, the census is
exactly `2 x Width x DedaultScale` — the record scale is simply missing.

### 7.2 Two defects, both in the same install path

**The placement scale was normalized away.** `gfx_owner.rs` builds the owner
transform with `placement_transform` (`0x5b3700` applies the authored scale to
the three basis columns), and `gfx_player.rs:1376` then does

```rust
let base = normalize_basis(transform) * Mat4::from_scale(Vec3::splat(self.root.default_scale * scale));
```

`normalize_basis` strips the scale and only `default_scale * scale` is put back —
and the owner passed a literal `1.` for `scale`. So every world effect drew at
`DedaultScale` and nothing else. The owner now hands the matrix's own column
length over as `placement_scale(&world)`, which is the record scale and nothing
else, because `placement_transform` bakes it into all three columns.

Corpus: of the map's **2,274** effect placements, **1,936 (85%)** author a scale
other than 1.0 — median 1.799, p90 6.816, max 89.8, min 0.164, and **1,158** sit
outside 0.5..2.0. Nothing about this is a rounding detail.

**The record alpha was never installed.** `0x5b33f3` copies the record's alpha
(offset 52) into the GFX node, and `gfx_owner` already folds `Request::outer`'s
fourth component into every element's colour (`:771`), but
`world_models_gpu::install_scene_effects` passed `outer: None`. Every scene
effect therefore ran at 1.0 no matter what the map authored: the seal dome is
authored **0.300**, the floating city **2.000**, the yellow electric ball
**1.500**. 266 of the 2,274 placements differ from 1.0 (0.2 .. 5.0). The
installer now passes `gfx_owner::scene_alpha(effect.alpha)`.

### 7.3 What that explains, report by report

| report | measured cause |
| --- | --- |
| City of Raging Tides: flat white sheet in the courtyard | `血鲷封印台罩子` is authored alpha **0.300** at scale 0.714 — it drew at 1.0 and 1.4x its size, i.e. 3.3x the brightness it was authored for |
| City of Raging Tides: misplaced effects in the floating city | `0远完美浮空城` scale 0.804 and `盆景水晶` scale 0.376 drew at 1.0 — the crystal's effects were **2.7x** their authored size |
| Nightfire Altar | `能量喷泉` scale **5.019** and `能量喷泉5` **2.877** were drawn at 1.0, i.e. a fifth of their authored size, and `白兰树` at 0.778 |
| Stairway to Heaven: misplaced ground yin-yang aura | `屋顶八卦` scale **0.304** drew at 1.0 — 44.8 units wide where the client draws 13.6 |
| Shadow Tide Camp: broken light, invisible in the middle of a beam | `0完美远征地主塔` 0.701 and `远征地血雕大帐` 0.832 drew at 1.0 |
| Waterfalls are static | `水门_瀑布` is authored scale **2.736** and `瀑布水花` 1.879..3.593; the animated sheet drew at 36% of its size, so the only thing big enough to read as falling water was the static mesh behind it |

### 7.4 Tests

- `gfx_owner::the_ecwld_placement_scale_reaches_the_quads` — the same root placed
  at scale 2.5 must draw 2.5x wider; it draws 1.0x before the fix.
- `gfx_owner::the_ecwld_record_alpha_dims_its_quads` — `scene_alpha(0.3)` reaches
  the quad as 76/255, `scene_alpha(2.0)` clamps at the byte the shader reads.

### 7.5 Hypotheses that died on the way, so nobody re-tries them

- **Sprite dimensions doubled.** Correct behaviour, cited above; the viewer
  already matches.
- **`OrgPtWidth`/`OrgPtHeight` pivots ignored.** Neither key occurs in any of the
  5,459 corpus files, so every pivot is the 0.5 default and the quad is symmetric.
- **TGA alpha lost.** Measured: `flarewhite_alpha.tga`, `通道粒子2.tga`,
  `通道粒子3.tga` and the smoke sheets are 128..810 px 32-bit TGAs with a real
  alpha channel (mean alpha 44/255 on the flare, 8/255 on `烟雾1`) — not an
  opaque decode.
- **Texture paths failing to resolve** (case, doubled separators). `texture_path`
  goes through `ecmodel::normalize`, which collapses `\\`, drops empty segments
  and lowercases — the archive is lowercased too, so the lookup matches.
- **Scrolling UVs clamped, which would freeze a waterfall.** The effect sampler
  is already `AddressMode::Repeat` on both axes (`gfx_effect_gpu.rs:140-146`).

## 8. Round three: the scale was right for world geometry and wrong for cards

Eight more reports arrived with eight fresh censuses. Two of them are direct
measurements of the §7 change, one praises it and one rejects it:

| root | placement scale | before §7 | after §7 | report |
| --- | --- | --- | --- | --- |
| `场景/新种族地图/能量喷泉.gfx` | **5.019** | 10.5 | 52.7 | "finally has correct size gas" |
| `场景/树林光束.gfx` | 8.776 | 0.7 x 14.0 | **6.1 x 123.0** | "sun rays became too big" |

Both cannot be right with one rule, so the rule has to be split by path — and the
client splits it. `A3DDecalEx::Fill_Verts_2D` (`/tmp/decal.cpp:714-760`) builds a
**screen-space card**: the centre comes from `GetParentTM() * kp.m_vPos`, but the
size is `m_fWidth1 * kp.m_fScale * fScale` where `fScale` is the projected size
of one world unit times **`m_pGfx->GetActualScale()`** — the GFX's own
`DedaultScale`, not the owner's map scale. Everything built in the parent frame —
particles, models, rings, grids, the `Fill_Verts_3D` ground path — inherits the
map scale through `GetParentTM()` instead.

That is what the two rows above measure: `能量喷泉` is elements 160 and 123
(models and emitters, world geometry, scale honoured) and `树林光束` is element
100 only (`DedaultScale: 0.7`, 0.5 x 10 cards, owner scale 8.776).

### 8.1 What changed

- `emit_sprite` now divides the owner's placement scale back out of the card's
  extents. The card keeps its authored `DedaultScale` size; the placement still
  moves its centre, because the centre goes through the placement matrix. A ring
  or a particle still follows the map scale.
- `scene_alpha` clamps the authored alpha to 0..1. The node's alpha is a byte
  once it reaches a vertex, and records authoring **2.0** (`0远完美浮空城`,
  `主城光球小`) and **5.0** were pushed past what retail draws — a half-alpha
  element multiplied by two saturates where the client leaves it at half. That is
  the "too bright blue effect on other craters".

Tests: `a_ring_follows_the_owner_scale_where_a_sprite_does_not` (a ring at 3x is
3x, a card is not) and `the_ecwld_placement_scale_does_not_resize_a_sprite`.

### 8.2 Still open, and what each needs

Recorded so the next round starts from the measurement rather than from a guess:

- **Waterfalls are static.** The census at Etherblade, Waterbeast Shore and the
  new pair shows only `瀑布水花`/`瀑布水花底` (the splash emitters) and never a
  `水门_瀑布` sheet, i.e. the thing that reads as falling water is the static BMD
  mesh. The next question is whether `水门_瀑布`'s seven quads are drawn at all at
  those spots, and the census already answers it: they are absent from every log.
  Either the owners are not activated, or their cards are drawn where the mesh
  hides them.
- **Effects rotated 90 degrees** (`天煞光束`, Archosaur). Both placements are in
  the corpus with different `direction`/`up` pairs; the fix needs those two
  vectors read next to the shot, because a 90-degree flip is a basis assignment,
  not a magnitude.
- **Forest of Songs "Shroom Lights" too low.** `蘑菇灯` places at 9.1..12.2 units
  now; "too low" is a height, so it is the record's `position.y` against the
  element's local offset — the same class of question as the 90-degree flip.
- **Wraithgate: spinning green effect, floating blue fires.** `绿鬼火` draws
  28.7 x 40.5 cards and `骷髅光柱` 30 x 42.4; a card that spins is a `Rad_2D` or
  billboard-orientation question, and "floating" is again height.
- **Summoning stations: white flickering blobs.** The census taken there is
  dominated by `蓝火` (alpha 1.00) and `岩浆火焰` (296 quads); the flicker is
  likely the flipbook `TexInterval`, which the census does not report yet.

## 9. The remaining five, measured from the world file

§8.2 listed them as questions. The ECWLD record scan answers several of the
questions outright, so the next round starts from the record rather than from a
guess. Record layout as `scene::Effect::read` reads it: id `[0]`, scale `[4]`,
position `[8..20)`, direction `[20..32)`, up `[32..44)`, speed `[44]`,
valid_time `[48]`, alpha `[52]`, attenuation `[56]`, then a length-prefixed GBK
path.

### 9.1 The waterfall is not an effect

Of the map's 2,275 placements, **31 name a `瀑布` file**: 28 splash/mist emitters
(`瀑布水花`, `瀑布水花底`, `瀑布水花中间`, `瀑布水花中间2`, `瀑布水花湍急`) and 3
`场景\新种族地图\水门_瀑布.gfx` tiles. **No record names `瀑布.gfx`,
`瀑布大/小/1..4/子/小慢.gfx`**, although all of them are in the archive
(`gfx/场景/新种族地图/瀑布.gfx` … — 22 archive entries match `瀑布`). At Etherblade
(blocks 32,70 and 38,69) the world places exactly four splash owners and nothing
else, which is what the census reported: the viewer draws every effect the map
puts there.

So the standing sheet is scenery — an ornament or a water piece — and its motion
lives in the scenery/water path, not in `gfx_player`. The three `水门_瀑布`
owners are small: five `100` cards of 0.9 x 1.25 at scale 2.736, i.e. 2.5 x 3.4
units, `TileMode: 1` with `TexSpeed` -1.0 / -1.5 / -1.8 / -2.0 on V and a 2x3 or
2x2 `TexRow`/`TexCol` grid, plus two `124` emitters. Those scroll correctly in
`flipbook_uv`; they are simply not what the eye reads as the waterfall.

The scenery pack that holds the sheet is not in this workspace (`gfx.pck` has
`.gfx` and its textures only), so this item is blocked on the asset, not on a
reading of the client.

### 9.2 The Archosaur beam pair is a 180-degree roll in the world data

| export | scale | position | direction | up |
|---|---|---|---|---|
| 19583 | 6.543 | (1528.0, 602.5, 1032.3) | (0, 0, 1) | (0, **-1**, 0) |
| 29056 | 6.977 | (491.1, 526.2, -876.9) | (0, 0, 1) | (0, **1**, 0) |

Both are the same root (`天煞光束`, element 100, 2 x 50) and the two `up` vectors
are exact opposites, orthogonal to `direction` in both cases (cosine 0.000). The
world therefore does author a flip between the two instances, and
`placement_transform` reproduces it exactly — its columns are
`right = up x forward`, `up`, `forward`, so a negated `up` negates the first two
columns, which is a 180-degree roll about the beam axis. Nothing in the viewer
inverts it, and nothing in the viewer can tell which of the two the shot shows.
Every `树林光束` record in the corpus is orthogonal in the same way (62 records,
cosine 0.000), so a degenerate basis is not the mechanism either.

### 9.3 The summoning-station flames are additive, and their colour is honoured

`场景/岩浆火焰.gfx` is three `121` emitters, all `SrcBlend: 5` / `DestBlend: 2`
(source alpha, **one** — additive):

| element | texture | rate | TTL | area | colour range | scale |
|---|---|---|---|---|---|---|
| Unnamed2 | `particle017.dds` | 150 | 0.8 s | 28 x 24 x 1 | `-112840`..`-32704` = `#FE47B8`..`#FF7FC0` | 0.1..1.0 |
| Unnamed3 | `smoke001.dds` | 100 | 1.5 s | 28 x 24 x 1 | `-922804224`..`-931135488` | 0.5..1.0 |

`particle_color` honours both ends (`random_color_hsv`/`random_color_rgb`), so the
white is not a colour default. The count is not inflated either: `rate x TTL` is
150 x 0.8 + 100 x 1.5 = **270** live particles for one instance, and the census
measured **296** quads at alpha 0.77. So the emitter is doing what the file says
and the white is the additive sum of ~270 overlapping quads (`DestBlend: one`)
inside a 28 x 24 area — the element is authored that way, and what makes it read
as a blob is where it sits, not what it emits.

### 9.4 `蘑菇灯` is placed where the map says, so "low" is local

Twelve placements, scale 0.443..0.935, `y` 253.5..272.3, `direction` (0,0,1),
`up` (0,1,0) for all twelve. The census reports the `circle030白` cards at
`y` 251..270 — within a unit of the record, and the cards are 11..17 units tall,
so the root is not offset. "Too low" is therefore inside the root: `蘑菇灯` is
`160` (a model element) + `100` (`SmallGlowPink`) + `140`, and the model element's
own geometry decides where the cap sits relative to the glow.

### 9.5 What each still needs

- Waterfall: the scenery pack (the sheet BMD), or the shot that shows which mesh
  is frozen.
- Beam flip: one pair of shots of the two `天煞光束` instances.
- `蘑菇灯`: the `160` element's model, to measure the cap against the glow.
- Wraithgate spin/floating: the root's affector list for `绿鬼火2` (12 placements,
  scale 2.0..8.954, all `up` (0,1,0)) — a card that spins is `Rad_2D`, and "floating"
  is height again.
- Summoning stations: whether the client caps the emitter per frame; 296
  overlapping additive quads is the measured starting point.

## 10. The client source settles the sun-ray size, and three other readings

The 1.4.2 client is readable on the user's Drive (`pw142/element`, and the
`GfxCommon` tree of `pw152source`). Four functions decide everything §8 and §9
were guessing at.

### 10.1 The record scale *does* reach a card — §8's divisor was wrong

```cpp
// CECOrnamentMan::LoadGFXFromFile — EC_ManOrnament.cpp:950-952
A3DMATRIX4 mat = a3d_TransformMatrix(pNode->vDir, pNode->vUp, pNode->vPos);
pGFX->SetScale(pNode->fScale);
pGFX->SetPlaySpeed(pNode->fSpeed);
pGFX->SetParentTM(mat);
```

`pNode->fScale` is the ECWLD record's `[4]` field. `SetScale` feeds
`CalcActualScale` (`A3DGFXEx.h:197-200`, `m_fActualScale = m_fDefScale * m_fScale`),
and `SetParentTM` (`A3DGFXEx.cpp:1589-1595`) does the opposite of what §8 assumed:
it keeps the translation and **normalizes** the three basis rows, so the placement
matrix's own scale is discarded and the node carries the scale on its own.

So a card's extent is `Width x Height` doubled, times the keypoint scale, times
`GetActualScale() = DedaultScale x record scale`. For `场景/树林光束.gfx`
(`DedaultScale 0.7`, 0.5 x 10 cards, eight `RotFromView: 1` elements) at this
map's 8.776 that is a **6.1 x 123 unit shaft**, and it is meant to be. §8's
divide-out removed a factor the client applies; it is reverted here, and
`the_ecwld_record_scale_reaches_a_card` pins it (2.5x record scale is a 2.5x card).

### 10.2 `RotFromView` is an axial billboard, not a screen billboard

```cpp
// A3DDecalEx::Fill_Verts_3D, RotFromView branch — A3DDecalEx.cpp:357-413
A3DVECTOR3 vPos  = GetParentTM() * kp.m_vPos;
A3DVECTOR3 vDir  = RotateVec(GetParentDir() * kp.m_vDir, _unit_z);
A3DVECTOR3 vView = vPos - pView->GetCamera()->GetPos();
A3DVECTOR3 vUp   = CrossProduct(vView, vDir); vUp.Normalize();
verts[1] = vPos + (vDir * fHeight2) + (vUp * fWidth2);
verts[3] = vPos + (vDir * fHeight2) - (vUp * fWidth1);
verts[0] = vPos - (vDir * fHeight1) + (vUp * fWidth2);
verts[2] = vPos - (vDir * fHeight1) - (vUp * fWidth1);
```

The card's **height runs along the shaft direction** and only its width turns to
the camera. That direction is *not* the element's Z axis: Angelica is a
row-vector engine, `SetDirAndUp` writes the basis as **rows**
(A3DGFXEx.cpp:1574-1585: `SetRow(0, right)`, `SetRow(1, up)`, `SetRow(2, dir)`),
so `RotateVec(m_qDir * kp.m_vDir, _unit_z)` is the third *column* of that
row-major basis — the parent's inverse rotation applied to the keypoint
direction, `(right.z, up.z, dir.z)`.

The shipped `world.ecwld` decides this on its own records (read 9 November,
2,275 effect placements, `google_drive/world.ecwld`):

| Record | `dir` | `up` | Third column | Reads as |
| --- | --- | --- | --- | --- |
| `天煞光束` (1528.0, 602.5, 1032.3) | (0, 0, 1) | (0, -1, 0) | (0, 0, 1) | horizontal shaft |
| `天煞光束` (491.1, 526.2, -876.9) | (0, 0, 1) | (0, 1, 0) | (0, 0, 1) | horizontal shaft |
| `树林光束` (232.3, 229.1, 2960.8) | (0, 0.565, 0.825) | (0, 0.825, -0.565) | (0, -0.565, 0.825) | 34 degrees **down** |

The Archosaur pair is horizontal in both variants, which is what the footage
shows, and the 49 `树林光束` placements all tilt along the column rather than
along their authored `dir`. Reading `dir` directly tilted every forest shaft the
wrong way through the vertical — the "sun rays point in random directions"
report. `a_rot_from_view_shaft_follows_the_row_major_basis` pins all three rows
of that table. Drawing these as screen-facing quads is what turned
`树林光束` into 123-unit walls over the tree tops (the "sun rays are too big"
report) and what made the Archosaur shafts read horizontally in one spot and
vertically in the next: both `天煞光束` records author `direction (0, 0, 1)` with
`up (0, ±1, 0)`, and the branch never looks at `up` at all — the shaft lies along
Z either way, so the 180 degrees between the two records' `up` vectors is
invisible in retail.

594 element lines in 232 roots author `RotFromView: 1`. The 2D spin is a roll
about the shaft axis, and a camera exactly on the axis gives the client a zero
`vUp` — the corner builder returns a degenerate quad rather than a NaN one.

### 10.3 `TileMode` is a scrolling tile, not a flipbook

```cpp
// A3DGFXElement::CalcTexAnimation — A3DGFXElement.h:521-540
if (m_bTileMode) { m_fTexWid = (float)m_nTexCol; m_fTexHei = (float)m_nTexRow; }
else if (m_nTexRow == 1 && m_nTexCol == 1) m_bNeedCalcTex = false;
else { m_fTexWid = 1.f / m_nTexCol; m_fTexHei = 1.f / m_nTexRow; }

// A3DGFXElement::TexAnimation — A3DGFXElement.h:539-566
if (m_bTileMode) { m_fTexU += m_fUOffsetSpeed * dt; m_fTexV += m_fVOffsetSpeed * dt; }
else { /* step m_nCurTexCol / m_nCurTexRow every TexInterval */ }
```

and `Fill_Verts_3D` writes `fU = {m_fTexU, m_fTexU + m_fTexWid}` straight into
the vertices, so a tiled element's UV window is `TexCol x TexRow` texture widths
— the sampler's `Repeat` makes that many copies — and it slides. `TexRow`/`TexCol`
are **repeat counts** under this flag and a grid of frames without it.

`flipbook_uv` applied the grid unconditionally, so every tiled sheet cut from
cell to cell at `TexInterval` instead of flowing: 579 element lines in 197 roots,
including all four tiled elements of `场景/新种族地图/瀑布.gfx`, nine of
`瀑布大.gfx`, and the five of `水门_瀑布.gfx` that this map places three times at
(2618.9, 239.3, -4119.7) and its neighbours.

### 10.4 `NoChangeDir` (unchanged, for the record)

`A3DGFXEx::SetParentTM` skips the basis copy when `m_bNoChangeDir` is set
(A3DGFXEx.cpp:1592). None of the reported roots authors it — `蘑菇灯` is the only
one in the corpus that even prints the key, at `NoChangeDir: 0` — so it is not
the mechanism behind any of the six reports.

## 11. Settings ▸ Logging: live, per-subsystem logging the viewer can be asked for

A screenshot says "there is a blue disc in the crater" and nothing else. The
status line counts quads but not which root drew one. Both gaps are closed by a
logging tab that is off by default and can be switched on while the viewer runs:

**Settings ▸ LOGGING** (fifth tab, next to Graphics/World/Sound/Interface) has
sub-tabs **Effects · Scene · Models · Render · Timing** and 36 channels. Each
channel is a tick box with a tooltip that says exactly what its lines contain,
a live count of lines written, and how many lines its throttle dropped. A
channel with no call site in this build is marked *no hook in this build yet*, so
an empty file can never be mistaken for a healthy subsystem.

Every channel writes its own file in `<explorer folder>/pwe-logs/`, with a header
naming the run and the channel, a `[+12.345]` timestamp on every line, and an
8 MiB rotation (`.log.1`). The folder belongs to the explorer, next to its
executable (a `cargo run` checkout resolves `target/debug` back to the checkout
itself), never to the client install it is reading: the game directory is not
where a report should have to be found. A folder saved by an older build that
still points inside the client root is treated as unset, so the move needs no
manual clearing of the box.

The client install is refused rather than discouraged: a folder saved by an
older build, a typed one, or a default that lands there all resolve to the
explorer's own folder, and if the executable itself sits in the game folder the
logs go to `%LOCALAPPDATA%\Perfect-World-Explorer\pwe-logs`. The page says so
under the path instead of writing somewhere the report cannot be found.

`session.log` is written whether or not a channel is ticked. It carries the run
header (client root, explorer root, version, mask), every group ticked, every
folder change, and every mark — which is what makes a folder of channel files
explainable a week later.

The tick boxes and the folder persist in `viewer-settings.txt` (`logging_mask`,
`logging_dir`), so a set of ticks survives a restart.

### Groups: one click per family of channels

Five buttons sit above the channel list. The names are general — they say what a
line *is*, not which defect prompted it — because a group of records answers every
question about those records. A group **replaces** the mask rather than adding to
it: a group with a leftover channel in it reads as one long log with no boundary.

| Button | Ticks | Answers |
| --- | --- | --- |
| **Placement** | roots, elements, frames, culling, clock | where a root is, how it was built, the basis its cards are drawn in, and whether it draws at all |
| **Cards** | cards, UV, decals, textures, alpha | the geometry and the pixels: corners, scroll window, basis, texture, colour |
| **Sub-elements** | particles, trails, models, lights, ground | everything that hangs off a root instead of being a card |
| **Scene** | scene-effects, effect draws, model queue, problems | what the map placed, what reached the GPU, what failed |
| **Everything** | every channel that can write | a report that has not been narrowed down yet |

Each group heading on the Effects page — **Where it is**, **Cards**,
**Sub-elements**, **Diagnosis** — carries the same tick box, and so does each
page. Every bulk switch touches only the channels that actually have a hook: with
36 channels and 19 hooks, the other 17 would be empty files and no answer.

### Nothing floods a file

Four limits, and all four are counted on the row so nothing disappears silently.

* **One line per thing, not per frame.** Every channel that names things — cards,
  UV windows, textures, decals, models, lights, particles, trails, ground
  drapery, the clock per root, a root that was culled — writes a key the first
  time it is seen and then at most once per its own interval (`emit_keyed`,
  `diag_keyed!`). Before this, `effect-textures` and `effect-models` streamed every
  element of every root on every frame and buried the rest of the file; a
  five-root waterfall put the same texture lines down sixty times a second. Now a
  file is the *complete list* of what drew, once each, which is the form a
  screenshot artefact can actually be matched against.
* An unchanged line is **collapsed** and the run of duplicates summarised once
  (`(240 identical lines collapsed)`).
* Each channel has a **budget** of ~200 lines a second with a 4,000-line burst, so
  a whole census can land at once and a call site in a loop still cannot flood.
* The per-frame channels keep their **throttle** underneath.

The row reads `1,204 lines, 12 budgeted, 3,880 repeats collapsed`: lines plus
repeats is the number of times the thing was seen, so *40 lines, 3,900 repeats* is
a complete list, not a channel that stopped working.

**F8** drops a `---- MARK camera (x, y, z) yaw … pitch … time … ----` line into
`session.log` *and* into every channel that is on, past the throttle and past the
mask. Two marks bracket the window being reported, so the log names the position
and the moment instead of describing them.

### The Effects page, channel by channel

| Channel | What a line is |
| --- | --- |
| `effect-roots` | Root built, activated, released: path, warning/texture/model counts |
| `effect-elements` | The element table of a root, written once when it is named: id, class, `TexFile`, W/H, grid, interval, tile flag, speeds, blend, keypoint count |
| `effect-frames` | Per element: keypoint position/direction/scale, clock, and the world basis (`right`/`up`/`dir` columns) the card is drawn in |
| `effect-cards` | Every card: four world corners, centre, width x height, texture, blend, priority |
| `effect-uv` | The UV window: tile flag, `TexCol x TexRow`, interval, speeds, clock, four UV corners |
| `effect-textures` | Texture resolution, hit or unusable path, blend pair |
| `effect-alpha` | Authored colour, `color_scale`, final RGBA, the cutoff and whether the card survived it |
| `effect-ground` | Ground branch: why a sheet stayed flat, or cell count/size, quad count and the sampled surface |
| `effect-particles` | Live/quota/rate/ttl/size/origin, 3D and billboard, direction, speed, acc, ground, surface, bind, area, radius |
| `effect-trails` | Ribbon/wave segment counts, curve length/segments/half-width, fractal and trail record counts |
| `effect-models` | Element-160/161 meshes: vertex and texture counts, placement, scale |
| `effect-lights` | Lights published, with position, colour, alpha, clock |
| `effect-culling` | Every owner: drawn with its record scale/alpha/speed/basis, or skipped with `active` and `start` |
| `effect-clock` | The whole root once a frame: time, dt, play speed, finished/loops, per-class counts, alpha, scale, drape and ground state |
| `effect-decals` | Which basis built a card (`element`/`axial`/`billboard`), the `RotFromView` and `FaceToViewer` flags, origin, Z offset |
| `effect-problems` | Every non-fatal failure and warning, in order |

The Scene page carries `scene-effects` — the `world.ecwld` records themselves,
with origin, path, speed, start/span, alpha, position and basis — which is the
counterpart to `effect-roots`: what the map asked for, and what the loader did
with it. The Models page carries `model-queue` (one root admitted per frame, and
what was rejected), and Render carries `render-effects`, where the effect census
also lands instead of only going to `effect-census.txt`.

### How to log an issue properly

The page spells this out under **How to capture a report**, so it does not live
only in this file:

1. **Click the group** that covers the question. The mask, the tooltip and
   `session.log` all name the set, so there is no doubt what was running.
2. **Fly to the spot, get the artefact on screen, press F8.** The mark carries
   the camera pose: position, yaw, pitch and the app clock.
3. **Wait a few seconds** — two or three frames of a waterfall, one full turn of
   a ring — then **press F8 again**. The pair of marks is the window; everything
   between them is the evidence.
4. **Copy report.** It carries the build, the client root, the folder, every
   switch that is on, what each of the 36 channels wrote (lines and throttled
   lines) and the live tail — one block, ready to paste. A log without that
   header is most of what usually has to be asked about afterwards.
5. **Attach the two or three files the report names**, not the whole folder, if
   the tail was not enough. `Clear files` between defects: two sessions in one
   folder read as a single session with no boundary.

A channel that says *no hook in this build yet* has no call site, so ticking it
produces nothing on purpose; that belongs in the report rather than in a wait.

### What each open report needs, exactly

| Report | Group | Read it like this |
| --- | --- | --- |
| Sun rays point at random | **Placement** | `effect-frames` prints each element's keypoint and the basis columns; `effect-cards` prints the corners, so the shaft's direction is a number. Compare with the `dir`/`up` pair `scene-effects` prints for the same placement. |
| Archosaur beams are both horizontal | **Placement** | Two placements of one root — `alpha 0.60 @ speed 1.0` and `alpha 1.00 @ speed 0.100`. `effect-clock` shows whether the second one starts late, `effect-frames` shows both bases. |
| Waterfall does not move | **Cards** | `effect-uv` against `effect-clock`: the window has to advance while the clock advances. If the clock runs and the window does not, the sheet is pinned; if neither runs, the element is asleep. |
| Crater is pure blue | **Cards** | `scene-effects` names what the map placed there; `effect-cards` says which root built the disc; `effect-alpha` says what its colour fold produced; `effect-textures` says whether the texture resolved at all. |
| Wraithgate ring spins fully | **Sub-elements** | `effect-trails` for the ribbon, `effect-uv` for the scroll along it, `effect-clock` for the turn. |
| Shroom lights sit too low | **Sub-elements** | `effect-cards` gives the world Y of every card; `effect-ground` says whether the drape moved it. |

## 12. What the first batch of live reports showed

Five captures came back from the field — the stuck waterfall, the random sun
rays, the archosaur beam, the spinning Wraithgate ring, the City of Raging Tides.
The single most useful thing in them was a **defect in the logging itself**, which
is why the fix in §11 is the one that mattered:

* **The channel with the most to say drowned everything else.** `effect-textures`
  and `effect-models` are facts about a load, so they were given no throttle — and
  every element of every root wrote one line *per frame*. A waterfall with a
  splash root and five water-sheet roots put the splash's `id=123 … textures=[…]`
  line down sixty times a second and pushed the sheet roots out of both the file
  and the tail. The evidence that was wanted — the five `水门_瀑布` roots and their
  `TexSpeed` — was the thing being buried. Both channels are now keyed: one line
  per root, element and texture, refreshed every five seconds.
* **Culling said the same thing forever.** `光运.gfx skipped: active=false
  elapsed=…ms start=0ms` repeats for every root that is not drawn, every frame, and
  the elapsed count in it changes every frame so the repeat collapse could not
  catch it. It is now keyed by root: a root states once why it is not drawn, and
  again only every ten seconds while that stays true.
* **`effect-cards` on the splash proved the splash is not the water.** Forty-nine
  to fifty particle billboards inside `area=Some(Vec3(0.3, 3.5, 0.3))` at
  `speed=4.00`, `ttl=1.00s`, with `effect-uv` for that root always
  `tile=0 grid=1x1 speed=(0.000,0.000)` — a spray of droplets with no scroll of its
  own, whose corners jump every frame. The running water is the five 100-sheet
  tiled roots from §10, and the scroll lives in their `TexSpeed` (−1…−2 on V).
* **A model element with no path at all** appears at City of Raging Tides:
  `effect-models` prints `id=160 meshes=0 vertices=0 textures=[] at
  (2555.24, 238.20, −4182.58) scale=10.000` for an element whose model path is
  empty. `Player::collect_models` answers a model that will not pose with an empty
  mesh list plus the warning `element 160 model : <error>`, so `effect-problems`
  names the cause; whether the record has no path or the reader lost it is what
  that one line decides. It is not yet established which, so it is recorded here
  rather than guessed at.

### What the next capture has to answer

One capture at the waterfall with **Cards** on now settles the whole question,
because the file lists every root that drew:

1. `grep 水门_瀑布 effect-uv.log` — five roots, `tile=1 grid=100x100
   speed=(…, −1.5)`. If they are there, the sheets draw and the window is a
   function of the clock; if they are absent while the splash is present, the
   sheets are never submitted and the water on screen is not them.
2. `grep 水门_瀑布 effect-clock.log` — `t=` climbing, `play_speed` non-zero. A clock
   that does not move pins a window that is otherwise correct.
3. `(N identical lines collapsed)` on a `水门_瀑布` UV line is the shortest
   possible statement of the defect: the sheet is drawn and its window is not
   advancing.

A second capture at City of Raging Tides with **Sub-elements** on gives
`effect-problems` the empty-path model element, with the element index and the
reader's own error text.
