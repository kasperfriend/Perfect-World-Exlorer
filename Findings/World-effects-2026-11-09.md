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
| **Cards** | cards, UV, decals, textures, alpha, clock | the geometry and the pixels: corners, scroll window, basis, texture, colour, and whether the window is advancing |
| **Sub-elements** | particles, trails, models, lights, ground, clock | everything that hangs off a root instead of being a card, and the clock that turns it |
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

**Summary** (next to Copy report) merges every channel file into one
`summary.log`: one line per **subject** — the root's archive path plus the element
id, or the shape of a structural line — carrying the first line that subject
wrote and how many lines it wrote in total. This is what makes a waterfall
capture usable: `(x1204) gfx/场景/水门_瀑布.gfx id=100 tile=1 grid=100x100 …` in a
few hundred lines instead of a megabyte of splash cards. At most 80 subjects per
channel, and the file says how many were left out. Copy report's tail is capped at
20 lines per channel for the same reason.

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

## 13. The second batch: four reports, and what they could not say

Four more reports arrived — `Waterfall`, `Wraithgate-2`, `Archosaur`,
`FlickeringRandomSunrays` — each copied from a build whose logging still had the
flood in it. Read together they say one thing about the tool and one thing about
each defect.

**The tool first.** Every one of the four ends with a tail of two hundred lines
that belongs to a single channel: 200 `effect-cards` at the waterfall, 179
`effect-particles` at the Wraithgate, 127 `effect-culling` at the Archosaur, 166
`effect-particles` at the sun rays. The channels that would have answered the
question were ticked — `effect-uv` wrote 4,142 lines, `effect-clock` 56,
`effect-trails` 13 — and are not in the report at all, because the tail is the
last two hundred lines of all of them mixed and one of them always wins.

Two more things the four reports show about the tool:

* every one says `mask 0x0000000000000000` and `0 of 36 on` while naming five
  channels that wrote megabytes — a report copied after the group was unticked.
  The line now prints the live mask next to the count, and says when nothing is on
  but lines exist;
* none of the four could have answered its own question: `Cards` did not include
  the clock when the waterfall was captured, so "did the window advance" had no
  clock in the file to compare against. `Cards` and `Sub-elements` tick
  `effect-clock` now.

| Report | Channels that wrote | What the tail held | What was missing |
| --- | --- | --- | --- |
| Waterfall | cards 2744, uv 4142, textures 12, alpha 36, decals 12 | 200 card lines, all `花瓣分飞.gfx id=121` | the five `水门_瀑布` roots, if they drew at all — 12 texture lines is the whole list, and none of them made the tail |
| Wraithgate-2 | ground 35, particles 280 | 179 particle lines | trails and clock: the ring is neither a particle nor a ground cell |
| Archosaur | frames 18, culling 191, clock 63 | 127 culling lines | every one of the 18 `effect-frames` lines is `烟1.gfx`, not the beam |
| Sunrays | ground 21, particles 168, trails 14, models 3 | 166 particle lines | `effect-frames` was not ticked: the placement group was not on |

**The Archosaur report is the clearest case.** Seventeen `effect-frames` lines came
back and every one of them describes `gfx/场景/烟1.gfx id=120` — a smoke effect at
`scale=11.623` sitting at `(1203.9, 242.4, 1223.6)`. `天煞光束.gfx` is in the same
scene (`effect-clock` proves it: `t=9210…10468ms`, `play_speed=0.400`,
`elements=4 quads=4 particles=0`, `alpha=0.600 scale=6.543`), so the beam builds and
draws four quads, and its frames were dropped by the channel's 250 ms floor before
the file saw them. `effect-frames` is keyed per root and element now: every element
of every root is written once and then at most four times a second, so the next
Archosaur capture contains the beam's own basis columns.

**The sun-ray report adds one fact and one oddity.** `树林光束` is element 100 with
`match_ground=false`, so it is drawn as one flat card — as authored, not as a
missed drape. And `pk场入口青铜剑.gfx id=150` prints `fractal/trail records=0
laid=0 offsets=7` on every sample for twelve seconds: a fractal element that has
seven offsets and lays no records. That is recorded, not explained; it is the one
thing in the four files that looks like a rendering fault rather than a missing
channel.

**What to send next.** Rebuild, fly to the artefact, tick the group, F8, wait five
seconds, F8, **Summary**, and attach `summary.log` plus the pasted report. The
summary lists every root and element that drew, once each, with how many lines it
wrote — which is the list the last four reports did not have.

## 14. The first summaries: four folders, and the numbers they finally carry

The four summaries arrived from a rebuilt viewer — `Archosaur`, `WaterfallAndSunrays`,
`Wraithgate`, `RagingTides`. This is the first round of evidence where the channels
that were ticked are actually *in* the file, and it changes three of the four
questions.

### What the tool itself got wrong, seen from its own output

* **One root placed many times collapsed into one subject.** Keyed by path and
  element id, forty-nine `树林光束` were one line, so a capture of a grove of
  sun-beams reported a single beam and could not say whether the others pointed
  somewhere else. Every keyed channel now carries the placement:
  `gfx/场景/树林光束.gfx id=100 @594,229,3144 …`. In the same spirit, cards and UV
  windows are keyed by element, texture and placement instead of by their slot in
  the frame's quad buffer, which used to split one card into several subjects
  whenever the draw order shifted.
* **The culling list was cut at 80 of 177 subjects** — exactly the list of every
  root in the scene and why it is not drawn. The per-channel cap is 400 now.
* **Nested roots had no name.** A child `GfxPath` player was built without
  `set_name`, so its lines began with a space: `" id=160 meshes=0 vertices=0
  textures=[] at (2553.51,236.73,-4197.57) scale=10.000"` named nothing at all.
  Children are named with their own path now.

### Waterfall: the sheet is scrolling, and the other falls have no sheet

`RagingTides`, at `(2618.9, 239.3, -4119.7)`:

```text
水门_瀑布.gfx id=100 #0 tile=1 grid=3x2 interval=50ms speed=(0.000,-1.500) clock=20230ms
  uv=[(0.000,-30.345),(3.000,-30.345),(0.000,-28.345),(3.000,-28.345)]
```

The tile branch is doing what the client does: `TexCol x TexRow` become the UV
*repeat counts* (3 across, 2 down), and the window starts at `v = -30.345`, which
is `-1.500 × 20.230 s` to three decimals. Over the 20.23 s the player has existed
the window moved exactly at the authored rate, downward, which is the direction a
waterfall falls. The card is `4.92 x 8.43` units (authored `0.900 x 1.250`, scale
2.736 doubled), tinted `0xff79d3ff` = `(0.475, 0.827, 1.000)`, blended `2/4`,
priority 1, texture `gfx/textures/纹理/气连续03.tga`. One repeat of 2 UV over
8.43 units at 1.5 UV/s is about 2.6 units per second — slow for falling water, and
worth comparing against retail, but not stuck.

At the PK-arena waterfall in `WaterfallAndSunrays` `(361.4, 200.6, 3175.8)` there is
**no `水门_瀑布` in any channel at all** — not in the frames, cards, UV, textures,
ground, decals or the 177-subject culling list. What is there is
`瀑布水花`/`瀑布水花底` (25 splash particles each, `speed=4.00`,
`area=(0.3, 3.5, 0.3)`, `ttl=1.00s`), `烟1`, `彩虹`, `树林光束`, `海底飘尘2`,
`花瓣分飞`, `树叶雨`. So that waterfall's water surface is not an effect at all:
the spray is the only thing that moves, and the wall behind it is the map's own
water. The two falls are not the same kind of object, which is why one report said
"the sheets are frozen" and the other had no sheets to look at.

### Archosaur: the beams are vertical now

```text
effect-cards: 天煞光束.gfx id=100 #0 tex=…/circle032.dds a=0.600 26.17x654.86
  c=(1528.04,602.49,1036.61) p0=(1538.40,275.33,1028.61) p1=(1517.67,275.33,1044.60)
  p2=(1538.40,929.66,1028.61) p3=(1517.67,929.66,1044.60)
effect-frames: basis right=(-6.54,0,0) up=(0,0,-6.54) dir=(0,-6.54,0)
record matrix: right=(-6.54,0,0) up=(0,-6.54,0) dir=(0,0,6.54)
```

A 26 by 655-unit shaft whose corners run from y=275 to y=930 — vertical, built
from the record's rows the way §10 derived. The root reports `elements=4 quads=4`
at `play_speed=0.400`, `alpha=0.600`, `scale=6.543`, and only element 100 draws.
The second placement of the same asset (`scale 6.977`, `alpha 1.000`,
`speed 0.100`) is not in this capture's eleven frame subjects, so it was not on
screen at that moment; if it still reads horizontal, its own frames will say so now
that the key includes the placement.

### Wraithgate: the portal root emits no ring

`绿色传送门.gfx` reports `elements=5 quads=115 particles=123 emitters=4`, and only
three elements ever write a line: 123 (`轨迹/风.dds` trail particles, `3.29 x 4.66`),
124 (`粒子/xing6.dds` particles whose cards are `0.01 x 0.02` — one centimetre), and
101 (a `normalmap/gas.dds` sheet, `7.51 x 10.62`, alpha 0.157). No ring card comes
out of this root at all. Either the ring is another root entirely, or two of its
five elements draw nothing — and the capture cannot separate the two yet, because
two elements can share an id, and `effect-elements` (the element table) was not
ticked. That table is the one thing to add for the next Wraithgate capture.

### RagingTides: two models that never load

Fifteen model elements, thirteen loaded with their meshes and texture names. Two
did not:

* `盆景水晶.gfx id=160 meshes=0 vertices=0 textures=[] at (2688.67,249.99,-4153.43)`
  — a named root whose model will not pose;
* ` id=160 meshes=0 vertices=0 textures=[] at (2553.51,236.73,-4197.57) scale=10.000`
  — a **nested child root**, ninety units from the waterfall, that drew nothing and
  named nothing.

Both are reported by `Player::collect_models` as `element N model <path>: <error>`
on the `effect-problems` channel, which was not ticked. The child now carries its
path in every line, so the next capture names the asset even before the error text
is read.

## 15. The fourth batch: an inverse rotation, a switch, and the binary's own name

Four reports (`Archosaur-2`, `Wraithgate-2`, `WaterfallAndSunrays-2`,
`CityOfRagingTides`) came back with the same verdict: nothing had changed. Two of
the complaints were real and one of them was findable in the client source, not
in the log.

### The axial branch was using the parent's inverse rotation

`A3DDecalEx::Fill_Verts_3D` builds a `RotFromView` card's shaft with one line
(A3DDecalEx.cpp:365, fetched this round from the pw152 source folder):

```cpp
A3DVECTOR3 vDir = RotateVec(GetParentDir() * kp.m_vDir, _unit_z);
```

`GetParentDir()` is the owner's `A3DQUATERNION` and `kp.m_vDir` is the
keypoint's — line 464 of the same file assigns that product straight into an
`A3DQUATERNION` — so the product is the **placement's own rotation** and
`* _unit_z` takes its Z axis. The branch instead did

```rust
let parent = context.placement * Mat4::from_quat(keypoint.inverse());
let dir = parent.transpose().transform_vector3(keypoint * Vec3::Z)
```

which multiplies the parent's **transpose** — for a rotation, its inverse — and
applies the keypoint a second time. A transpose is not visible on an axis-aligned
placement, which is why every test still passed; on a placement that is tilted it
mirrors the shaft about its centre. That is exactly the reported symptom: the
forest's 49 `树林光束` records author directions like `(0, 0.565, 0.825)`, 34
degrees **up**, and the cards were drawn 12–34 degrees **down**. Every shaft in
the map was pointing the wrong way whenever its placement was rotated, and no
amount of swapping keypoint vs. parent could fix that, because the error was the
matrix, not the operands.

The branch now returns the placement's Z axis for the client rule. `RootDir`,
`WorldUp` and `Horizontal` are kept beside it under a settings switch (see below),
and a test pins all four against corners the builder actually returns.

### A switch, because a log cannot settle a picture

The Archosaur beam is the case the log cannot decide. `天煞光束` element 100's
keypoint turns Z onto +Y, so the client rule — correctly implemented now — gives
a **vertical** 655-unit shaft, and the archive footage in the report shows the
same summoning column standing up. The report says both beams should be
horizontal. Both readings are consistent with the same log, because the card's
size, basis and clock are identical either way; only the picture separates them.

So `Settings ▸ World ▸ RotFromView card axis` now offers the four candidates:

| Row | Rule | What it answers |
| --- | --- | --- |
| Client (placement Z) | `q_parent * q_keypoint * _unit_z` | the faithful answer, default |
| Root direction | `q_parent * _unit_z` | ignores the .gfx keypoint |
| World up | `+Y` | stands every shaft up |
| Horizontal | client axis, rise removed | sweeps in the ground plane |

It is persisted as `effect_shaft=` (`view_settings::EffectShaft`), published to
the player through `gfx_player::set_shaft_mode`, and **printed in the decal
channel** as `shaft=client|root|up|flat`, so the next report says which rule
produced the picture it describes. Flipping the row and re-shooting one
screenshot settles the beam in a single round instead of another blind rebuild.

### The summary now names the binary that wrote it

Four rounds of "nothing has been fixed" cannot be told apart from four rounds of
a stale executable: the same numbers come out of both. `summary.log` therefore
starts with

```
# build:    E:\pw142\Perfect World Explorer\pwe.exe (5.2 MB, written 2026-11-09 19:12:44 UTC)
```

— the path, size and write time of the running executable, read from
`std::env::current_exe`. If the next report's header is older than the branch it
was taken for, that is the whole explanation and no matrix has to be blamed.
(`utc_stamp` is Hinnant's civil-from-days, tested against the epoch, one known
instant and 2024-02-29.)

### What is still open

* **The waterfall still does not flow.** `水门_瀑布.gfx id=100` scrolls its UVs at
  `-1.500 × clock` and the card is 4.92 × 8.43 of `纹理/气连续03.tga`; the
  picture is a flat white sheet. The UVs are moving, so the next question is the
  sampled texels (a `.tga` whose alpha the loader may be dropping) and not the
  clock. `effect-textures` reports the path and nothing about its contents; the
  texture channel needs a line that says whether alpha survived decode.
* **The portal has no ring.** `绿色传送门.gfx` still emits no card for two of its
  five elements. `effect-elements` (the element table) is the channel that would
  name them, and it was not ticked in any of the four captures.
* **Models that never load.** `盆景水晶.gfx id=160 meshes=0` is reported on
  `effect-problems`, which was also not ticked.

## 16. The engine's own element and control tables, read from the pw152 source

The `GfxCommon` folder in the report (`pw152source/GfxCommon`) is the real
engine, not a decompilation: `A3DGFXEx.cpp`, `A3DGFXElement.{h,cpp}`,
`A3DDecalEx.cpp`, `A3DGFXRing.cpp`, `A3DGFXKeyPoint.cpp`, `A3DGFXLight.cpp`,
`A3DGFXExMan.cpp` and the rest. Three of this round's fixes come straight out of
it, and so do two corrections to what the viewer thought its elements were.

### The element table (`A3DGFXElement.h:26-51`)

| Id | Class | Viewer before this round |
| --- | --- | --- |
| 100/101/102 | `A3DDecalEx` 3D / 2D / billboard | correct |
| 110 | `A3DTrail` | correct |
| 120..125 | particle emitters | correct |
| 130 | `A3DGFXLight` | correct |
| **140** | **`A3DGFXRing`** | parsed as `Cylinder`, and rendered as one |
| 150 | `A3DGFXLightning` | called `Fractal` |
| 151 | `A3DLtnBolt` | called `Lightning` |
| 152 | `A3DGFXLightningEx` | called `Trail` |
| 160 | `A3DGFXModel` | correct |
| 170 | sound | correct |
| 180 | `A3DLtnTrail` | called `Wave` |
| **190** | **`A3DGFXParaboloid`** | called `Curve`, and built from the *root's* `Vec` pair |
| 200 | `A3DGFXContainer` | correct |
| 210/211 | grid decal 3D / 2D | correct |
| 220/221 | physics emitters | not parsed |
| 230 | `A3DGFXECModel` | not parsed |

The 140 row is only a naming debt — `A3DGFXRing::InitStreamBuffer` and
`FillVertexBuffer` are what `emit_cylinder` already reproduces, including the
`Sects: 0` default of 36, the `fDeltaRadius = fHalfHeight * tan(pitch)` clamp and
the strip's `tu = i * TexWid / nSect`. **The 190 row is not.** Element 190 is a
paraboloid, and the viewer has been rendering it as a "curve" that takes its two
endpoints from the owner's `Vec:` pair. That is why the floating city, whose
roots are full of 190s, is the one place the effect reads worst.

### The control table (`a3dgfxkeypoint.h:34-47`)

| Id | Class | Effect on the keypoint |
| --- | --- | --- |
| 100 | `A3DGFXCtrlMove` | position += Dir * dist |
| 101 | `A3DGFXCtrlRot` | **`Rad_2D` += dist** (the card's own 2D spin) |
| 102 | `A3DGFXCtrlRotAxis` | `m_vDir = Q(axis, dist) * m_vDir`, position orbits the axis line |
| 103 | `A3DGFXCtrlRevol` | same shape |
| 104 | `A3DGFXCtrlCentriMove` | pulls toward `Pos` |
| 105 | `A3DGFXCtrlColorChange` | per-channel colour rates |
| 106 | `A3DGFXCtrlScaleChange` | scale ramp with min/max |
| 107..112 | noise / colour transfer / scale transfer / curve move | **parsed as load errors** |

Two things fell out of that table:

* `A3DGFXCtrlRotAxis::Process` builds the spin as `Q(m_vAxis, fDist)` and
  **left**-multiplies the keypoint direction. The player accumulated a scalar
  and multiplied `Quat::from_axis_angle(Vec3::Z, orbit)` on the *right*: a
  different rotation for every axis an artist did not author as Z, and about the
  wrong frame even then. That is the Wraithgate "spinning lights" shape of
  error — a spin that turns the card about an axis nobody authored.
* `CtrlType: 112` is `ID_KPCTRL_NOISE_BASE`, and 241 retail controls are exactly
  that. The parser rejects 107..112 as "unsupported", so **every element that
  carries one fails to load** — a silent whole-root loss, not a cosmetic
  difference. This is the next thing to parse.

### Texture animation (`A3DGFXElement.h:521-577`, `A3DGFXElement.cpp:1250-1276`)

```cpp
void CalcTexAnimation() {          // tile: wid = TexCol, hei = TexRow
    if (m_bTileMode) { m_fTexWid = (float)m_nTexCol; m_fTexHei = (float)m_nTexRow; }
    else if (m_nTexRow == 1 && m_nTexCol == 1) m_bNeedCalcTex = false;
    else { m_fTexWid = 1.f / m_nTexCol; m_fTexHei = 1.f / m_nTexRow; }
}
void TexAnimation(DWORD dwTickTime) {
    if (m_bTileMode) {
        m_fTexU += m_fUOffsetSpeed * dwTickTime * .001f;   // scroll, tile only
        m_fTexV += m_fVOffsetSpeed * dwTickTime * .001f;
    } else {
        m_dwTexTimeRemain += dwTickTime;
        if (m_dwTexTimeRemain < m_dwTexInterval) return;
        m_dwTexTimeRemain -= m_dwTexInterval;
        if (++m_nCurTexCol >= m_nTexCol) { if (++m_nCurTexRow >= m_nTexRow) { row = 0; col = 0; } else col = 0; }
        m_fTexU = m_fTexWid * m_nCurTexCol;                 // assigned, never scrolled
        m_fTexV = m_fTexHei * m_nCurTexRow;
    }
}
bool TickAnimation(DWORD dwTickTime) {
    m_dwTexTickCount += dwTickTime;
    if (m_bTileMode || m_dwTexTimeTotal == 0 || m_dwTexTimeTotal > m_dwTexTickCount)
        TexAnimation(dwTickTime);      // past the total: the sheet holds its frame
}
```

The viewer did both halves of that at once: it stepped the flipbook cell **and**
added `TexSpeed * time` on top. `水门_瀑布` authors `TexSpeed (0, -1.5)` with
`TileMode 0` and a `3x2` atlas, so after twenty seconds its UV window sat thirty
texture heights below the sheet and the card sampled nothing but the edge. That
is the flat white sheet. The atlas was stepping the whole time; the scroll on top
is what hid it.

`TexAnimMaxTime` also stops the walk where it stands rather than shortening the
loop, which the old `limit = clamp(total / interval, 1, cells)` got wrong.

## 17. The sun rays and the waterfalls, against the census numbers

### The shafts are the authored size after all

`Sunrays.txt` (431 quads, 34 roots) lists every `场景/树林光束.gfx` card with its
census size. Two of them, from different placements:

```text
7.4x147.4 at (2408.9,269.6,-1588.5) quads=7 tex=…/圆环光/circle004.dds a=0.33
5.7x114.1 at (2234.6,266.8,-1907.4) quads=7 tex=…/圆环光/circle013.dds a=0.50
```

Both are `Width 0.5 x Height 10`, `DedaultScale 0.7`, doubled, times the
placement scale — one application of the record scale, which is what §10.1
derived from `SetScale`/`SetParentTM`. The card in `WaterfallAndSunrays.log`
frame 17 (`right=(-2.14,3.33,3.70) up=(3.77,3.71,-1.16) dir=(-3.25,2.12,-3.79)`)
draws `5.42x108.51` running `p0=(630.36,214.88,3105.68)` to `p2=(556.36,238.12,3181.39)`,
i.e. `(-74.0, 23.2, 75.7)` over 108.5 units — exactly
`-normalize(right.z, up.z, dir.z) * 108.5`, the third **column** of the row-major
basis, times the height. The shaft lies where the client's
`RotateVec(GetParentDir() * kp.m_vDir, _unit_z)` puts it, leaning 12 degrees up
along the tree line, and it is 108 units long because the map scaled it to
108 units. Nothing about the geometry or the size is left to change; what the
four `effect_shaft` rows exist for is proving that in one screenshot.

### The `水门_瀑布` fix is now pinned by a test

`a_waterfall_flipbook_never_scrolls_off_its_sheet` builds exactly the authored
record — a 3x2 atlas, `TileMode 0`, `TexSpeed (0, -1.5)` — and walks it for forty
half-second frames, which is the twenty seconds the field report ran. Every
corner must stay inside the sheet, and the sheet must still change cell: twenty
seconds of the old code failed the first assertion with every corner reading the
edge texel, the flat white card.

### The grid decal keeps its scroll, because it is a different class

The first CI run after §16 caught the one place the removed scroll had to stay:
element 210/211 is `A3DGridDecalEx`, whose curtain fixtures author
`TexRow: 1, TexCol: 1, TexSpeed: -0.4` — a 1x1 sheet has no cells to step, so
`A3DGFXElement::TexAnimation`'s flipbook rule cannot be what moves it. The
scroll now lives in `emit_grid`, next to the class that owns it, and
`a_grid_builds_its_authored_sheet_and_scrolls_it` still passes.

### The portal's silent elements are not the 107-112 controls

§16 named `CtrlType 107..112` as the leading hypothesis for `绿色传送门`'s two
silent elements. `Wraithgate-2.log` kills it: the frame line reports
`elements=5 quads=115 particles=123 emitters=4 unrendered=0` while only ids 123,
124 and 101 write a card. A rejected control fails its **element's load**, so the
root would report three elements, not five — the two silent ones loaded and built
nothing. The element table (`set_name`, `EffectElements`) already prints **every**
element regardless of quads, so the next capture with that channel ticked names
them outright; nothing needs to change in the code for that answer.

Still open, in this order: `A3DParaboloid` (element 190 — our `Spec::Curve`
retains the authored `coefficient` and `height`, so the data is there but the
surface topology needs `A3DParaboloid.cpp`), the 230 `ECModel` and 220/221
physics emitters, and `盆景水晶.gfx`'s `meshes=0`.

## 18. What the client can draw *on top of* a fall

The theory to test was that the sheet is drawn internally and the game paints
something over it. Two client paths can do that, and the sources separate them.

### The effect-side overlay: a projected screen-space pass

`A3DGFXSurfaceRender::Render` (pw152 `GfxCommon`, fetched this round) takes an
element's own vertices and re-draws them as a **screen-space projected** card:

```cpp
m_pCamera->SetProjectionParam(-fHalfWidth, fHalfWidth, -fHalfHeight, fHalfHeight, -100.0f, 100.0f);
m_pCamera->SetPos(vCenter + A3DVECTOR3(0, fRadius * 2.0f, 0));
pA3DDevice->SetZWriteEnable(false);
pA3DDevice->SetTextureCoordIndex(0, D3DTSS_TCI_CAMERASPACEPOSITION);
pA3DDevice->SetTextureTransformFlags(0, A3DTTFF_COUNT4 | A3DTTFF_PROJECTED);
pA3DDevice->SetTextureAddress(0, A3DTADDR_BORDER, A3DTADDR_BORDER);
matProjectedView = InverseTM(pHostCamera->GetViewTM()) * m_pCamera->GetVPTM() * matScale;
```

A card drawn through that path has no world orientation at all: its texture is
projected onto the view, so it reads as a flat picture pasted over the scene.
It is the `SetupSteam` path — the client's water-steam/水花 overlay.
It is **not** the decal path: `A3DGFXElement` (base class, downloaded and
grepped this round) names neither `SurfaceRender` nor `Steam` nor `MatchSurface`
anywhere, so no element flag reaches it from the sprite record. Whatever uses it
is a class of its own, and it is not what draws `水门_瀑布`.

### The world-side path: water drawn on static meshes

The terrain-water renderer has a static-mesh mode, and that is the one that can
paint a fall:

```cpp
// A3DTerrainWater.cpp:67-84
bool A3DTerrainWater::InternalInit(..., bool bStaticMesh) {
    if (!m_pWaterRender->Init(this, pA3DDevice, pHostViewport, pHostCamera, nMaxVerts, nMaxFaces, bStaticMesh))
        return false;
    // Set static mesh flag only if water render really support it
    m_bStaticMesh = m_pWaterRender->m_pWaterStaticVertexShader ? true : false;
}
```

and the shader it uses, `water_static_mesh_vs.txt`, is fed the mesh's own
vertices and texcoords (2 KB, fetched this round):

```asm
; v0 = position in world space   v1 = normal   v2 = texture coordinates for base layer
mov r0, v0
mad r0.x, r0.x, c34.w, c34.x     ; redesign the position into the water block's cuboid
mov r0.y, c34.y
mad r0.z, r0.z, c34.w, c34.z
m4x4 oPos, r0, c0
mov r0, v2                       ; the mesh's own texcoords …
m3x2 oT0, r0, c6                 ; … through layer-0's transform matrix
m3x2 oT1, r0, c10                ; … and layer-1's
```

Six layer matrices (`c6`..`c29`) are available and two are used; the pixel
shader `water_cheap_scroll.txt` adds the fresnel term and takes its alpha from
the reflect map's edge channel. Water is therefore **geometry with a scrolling
water material**, not only the flat grid our loader knows: `src/water.rs` reads
`A3DWaterArea` records as `height, grid_size, columns, rows, center, mask,
shore_waves` — a horizontal grid at one height, with no static-mesh input at
all.

### Consequence

A waterfall built as water-on-a-static-mesh is invisible in the viewer: no
`A3DWaterArea` covers it, so the only thing left on screen at that spot is
whatever `gfx` root the map also placed — the animated sheet, or nothing at all
(`WaterfallAndSunrays` §14 found only spray). What the game shows there is the
water material, sampled from the mesh's own UVs through a scrolling layer
transform, and it *looks* static because a layer transform that neither loader
nor renderer applies leaves one fixed image on the geometry.

Decisive next check, which needs the 1.4.2 map data rather than more source: is
there a static mesh carrying the water material at the fall? The loader has no
static-mesh water record at all today, so the work is to read
`A3DTerrainWaterRender.cpp`'s static-mesh submission (69 KB, on the Drive) and
add that path — not to touch the effect again.

## 19. What the 1.4.2 data on the Drive can and cannot answer

Read this round, from the Drive itself:

* `world.ecwld` (2,454,726 bytes, the map our captures come from) — decoded its
  GBK strings: **501 CJK names mention water, and every one of them is an effect
  root** (`瀑布水花`, `瀑布水花底`, `海底水泡`). No scenery or model path in the
  file names a fall.
* No water file exists in the tree: `name contains '.wtr'` returns 0 files.
  `A3DWaterArea` is not a separate asset — `src/water.rs` reads it from each
  **block** (`block.water`), so it ships inside the map data, always as
  `height + grid_size + columns + rows + centre + mask + shore_waves`, i.e. a
  horizontal grid at one height.

So the Drive answers the question in the negative only: the map data it holds
contains no record that could describe a vertical water fall. `A3DTerrainWater`'s
static-mesh path takes its geometry from somewhere this data set does not carry,
which is exactly why the viewer draws nothing there and why the effect sheet is
the only thing left in our frame.

### The channels that would have said so were never wired

`SceneWater` and `RenderWater` were declared in the Logging tab and **emitted
nowhere** (`grep SceneWater|RenderWater src --include=*.rs` outside
`diagnostics.rs` returned no hits before this commit). A capture could not say
whether a water area exists at a spot, so the question stayed open for rounds.
`RenderWater` now prints one line per uploaded area:

```text
export=41 colour=0xff336699 simple=false height=201.500 grid=8.000 cells=16x16
wet=143 of 256 centre=(361.00,201.50,3176.00) shore_waves=0
```

That is the number that settles §18 without another guess: **no line within a
hundred units of a fall, while the game shows water there, means the surface is
the static-mesh water**, and the loader needs `A3DTerrainWaterRender.cpp`'s
static-mesh submission next.

### How to capture it

1. Build and stand in the viewer at the fall, within about 50 units, facing it —
   the same spot the field report names, e.g. `(361.4, 200.6, 3175.8)` in the
   PK-arena map.
2. Logging tab: tick **Water** (group *Terrain, sky and water*) and
   **Batches ▸ RenderWater**, plus **Scene ▸ SceneEffects** and
   **Effects ▸ Effect elements** if the sheet is also in question.
3. Let the world finish loading (the water upload happens once per world build),
   then move the camera once so a fresh frame is drawn.
4. Send `summary.log` — it stamps the binary that wrote it (§15), so a stale
   executable cannot be mistaken for a missing record.

## 20. The water group could not be ticked, and element 190 is a bowl

### Why "Terrain, sky and water" flipped itself back

The group tick box only touched members whose channel `wired()`; a group where
*no* member has an emitter could therefore never become "all on", so the box
unticked itself on the next frame. The water group was exactly that: `SceneWater`
and `SceneSky` have no emitters and `RenderWater` was not in `wired()` either.
The box now writes the whole member set, and `RenderWater` is wired — it prints
one line per uploaded `A3DWaterArea` (`export`, colour, `simple`, `height`,
`grid`, `cells`, `wet of`, `centre`, `shore_waves`), which is the line §19 asked
for.

### Element 190 is `A3DParaboloid`, and its geometry is 40 lines

`A3DParaboloid.cpp` was on the Drive after all (`fullText contains
'A3DParaboloid'`, 6,742 bytes). `Build`:

```cpp
m_nCircles = int(m_fHeight / _grid_sz);          // _grid_sz = .1, capped at 8
float fGrdSz = m_fHeight / m_nCircles;
float fRadius = (float)sqrt(m_fCoefficient * i * fGrdSz);
ver.x = fRadius * fCos[j]; ver.y = fRadius * fSin[j]; ver.z = fGrdSz * i;
ver.tu = fUC + fTexX * fCos[j];  ver.tv = fVC - fTexY * fSin[j];
```

`_segs = 360 / 15 = 24`, the UV centre is the middle of the current window, and
the keypoint scale is applied by `FillVertexBuffer`'s
`matWorld = mat.Scale(kp.m_fScale) * GetTranMatrix(kp) * GetParentTM()` — once,
to the finished point.

The viewer had been building element 190 from the **root's `Vec:` pair**, which
element 190 does not own: a 0.5-wide ribbon along a chord that most files do not
author. `emit_paraboloid` now builds the bowl exactly as above, so
`水柱.gfx` — `elements=3 quads=0` in `StrayLights.log` — has geometry for the
first time, and the floating city's sheets stop being an unrelated smear.
`an_element_190_is_a_paraboloid_bowl_of_the_authored_height` pins the ring count
(3 rings × 24 segments for 0.3 units) and the shape (widest radius / height =
√0.3 / 0.3, a scale-free ratio).

### The sun-ray log says the axis *is* the record's direction

`Sunrays.log`, one placement:

```text
basis right=(3.48,0.00,0.00) up=(0.00,2.90,-1.94) dir=(0.00,1.94,2.90)
card  3.48x69.78  p0=(949.32,253.43,3310.89)  p2=(949.32,214.66,3252.98)
decal basis=axial shaft=client rot_from_view=true
```

`p2 - p0` normalises to `(0, 0.556, 0.831)`, which is the record's `dir` row to
three places, and the record is tilted 34° about X — the authored tilt of that
`树林光束` placement. So the shaft follows `GetParentDir() * kp.m_vDir * _unit_z`
= the third row of the row-vector basis. If the picture still reads as random,
the next datum is one screenshot per `effect_shaft` row at the same spot; the
decal line already prints which row produced it.

## 21. Sweep of the retail package (`gfx.pck`, pw142)

The packaged effects, not the editor samples, are the specification. The PW 1.4.2
`gfx.pck` on the Drive (`1lDQSlXt78bPq5QaayQ5S8m6r0Yp0PQ_e`, 97,723,094 bytes,
`MOXTVersion 0x20002`, 10,104 entries) holds **5,459 `.gfx` roots**. Reading every
one of them with `tools/pck_lib.py` gives the element and controller census the
parser has to cover:

| element id | class (`A3DGFXElement.h:26-51`) | records |
|---|---|---|
| 123 | `PARTICLE_CYLINDER` | 9,870 |
| 101 | `DECAL_2D` | 5,244 |
| 100 | `DECAL_3D` | 3,542 |
| 110 | `TRAIL` | 2,956 |
| 124 | `PARTICLE_ELLIPSOID` | 1,748 |
| 200 | `CONTAINER` | 1,530 |
| 120 | `PARTICLE_POINT` | 1,315 |
| 140 | `RING` | 1,116 |
| 150 | `LIGHTNING` | 734 |
| 130 | `LIGHT` | 546 |
| 170 | `SOUND` | 491 |
| 160 | `MODEL` | 447 |
| 125 | `PARTICLE_CURVE` | 382 |
| 121 | `PARTICLE_BOX` | 320 |
| 210 | `GRID_DECAL` | 215 |
| 190 | `PARABOLOID` | 208 |
| 152 | `LIGHTNING_EX` | 156 |
| 102 | `DECAL_BILLBOARD` | 83 |
| 180 | `LTNTRAIL` | 79 |
| 122 | `PARTICLE_MULTIPLANE` | 40 |

Controller census: 105 ColorChange 16,125 · 106 ScaleChange 15,592 · 104
CentriMove 9,463 · 102 RotAxis 6,720 · 108 ClTrans 6,594 · 101 CtrlRot 6,453 ·
100 Move 6,375 · 109 ScaleNoise 3,818 · 110 CurveMove 1,918 · 111 ScaleTrans
1,858 · 107 ClNoise 1,137 · 112 NoiseBase 241 · 103 Revol 225.

Two results worth recording:

* **Nothing in the package is outside the parser.** Every one of the twenty
  element ids above and all thirteen controller types are dispatched in
  `src/gfx_file.rs`; a scan for the keys the parser marks *required*
  (`GfxPath`, `ColorMin`, `Width`/`Height`, `Segs`/`LightNum`/`Interval`,
  `wNumber`/`hNumber`/`fGridSize`) finds no missing key on any of the 5,459
  roots. The lightning family writes its three widths as three separate `Width`
  lines, in file order start, end, mid — the order `Fractal::widths` keeps.
* **`水门_瀑布.gfx` is a tile sheet, not a flipbook**: `TileMode 1`, `TexCol 3`,
  `TexRow 2`, `TexSpeed (0, -1.5)`, `OrgPt (0.5, 0)`, `SrcBlend 2`/`DestBlend 4`,
  cards `0.90 x 1.25`. `flipbook_uv`'s tile branch already slides that window and
  `a_tiled_sheet_repeats_and_scrolls_where_a_flipbook_steps_cells` runs on these
  exact numbers, so the effect side of the waterfall is faithful. A waterfall
  that still looks frozen is therefore the terrain-water mesh (§18/§19), not a
  texture animation.

And one that explains a blank rather than a bug: `gfx/场景/水柱.gfx` is
`RepeatCount -1` with `RepeatDelay 30000` and a single 1,750 ms keypoint, so the
client itself draws the spray for 1.75 s and then waits 28 s
(`A3DGFXKeyPointSet::TickAnimation`, `if (m_bInDelay) { m_dwCurDelay += dwTickTime; ... }`
at `A3DGFXKeyPoint.cpp:1363-1382`). `element_cycle`/`element_phase` reproduce that,
so `quads=0 particles=0` at t = 20,940 ms is the authored behaviour, not a
regression.
