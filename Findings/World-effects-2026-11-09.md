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

## 22. Lightning ribbons: two defects the class source settles

`A3DGFXLightning` is element **150** (`m_nEleType = ID_ELE_TYPE_LIGHTNING`,
`A3DGFXLightning.cpp:40`) — 734 records in the retail package, plus element 152
(`A3DGFXLightningEx`, 156) which reuses the same strip. Two of its rules were
wrong in the viewer, and both are readable straight out of the class.

### 22.1 `Width`/`Alpha` are `[start, end, midpoint]`, not `[end, midpoint, start]`

The header declares the fields in this order (`A3DGFXLightning.h:60-65`):

```cpp
float m_fWidthStart;  m_fWidthEnd;  m_fWidthMid;
float m_fAlphaStart;  m_fAlphaEnd;  m_fAlphaMid;
```

`Load` fills them in file order (`A3DGFXLightning.cpp:467-500`): line 1 →
`m_fWidthStart`, line 2 → `m_fWidthEnd`, the version-35 line 3 →
`m_fWidthMid`; and `FillVertexBuffer` ramps `Start → Mid` over the first half of
the path and `Mid → End` over the second (`:888-903`). So the **third** authored
line is the apex. `profile_at` was reading line 2 as the midpoint, which put the
taper of a retail record like `Width: 0 / Width: 0 / Width: 0.4`
(`gfx/models/暗语者营地/暗语者营地.gfx`) at the far end instead of in the middle —
every such ribbon was a wedge rather than a spindle.

### 22.2 The noise stays perpendicular to the ribbon's own axis

`BuildPath` (`:1005-1052`) displaces along the *perpendicular* only:

```cpp
A3DVECTOR3 vUp(CalcVertVec(vDir));
m_pData[0][i] = j / fSegs;                             // along the path
m_pData[1][i] = fRand[0] * A3D_2PI;                    // twist about it
m_pData[2][i] = filter(m_nFilterType, i/fSegs, fRand[1]);
```

and `FillVertexBuffer` composes the cross-section as

```cpp
vPos = vStart + vDir*(m_pData[0][i]*fDist)
     + RotateVec(A3DQUATERNION(vDir, m_pData[1][i]), vUp) * (m_pData[2][i] * m_fAmplitude);
```

with the two endings taken verbatim from `vStart`/`vEnd` (`nMod == 0` and
`nMod == m_nSegs`). The viewer instead displaced every sample — endpoints
included — along the fixed world diagonal `(1, 0.5, 0.25)` and scaled it by the
*first* `Amplitude`. The first four keys (`BufLen`, `Amplitude`, `WaveLen`,
`Persistence`, `OctaveNum`) belong to `m_NoiseCtrl`, which `Load` reads before
the ribbon keys (`:421`); the second `Amplitude` is the ribbon's own excursion
and was being dropped entirely.

Consequences, in the field: a `Fractal` left its own plane, drifted the same way
on every rebuild (so it read as a sheet sliding in one direction), and could
reach several world units on effects whose authored excursion is one — the
sheets hanging off trees and the blobs crossing the floating city. `emit_fractal`
now builds the perpendicular basis, twists each sample about the axis, leaves
both authored ends on `StartPos`/`EndPos`, and multiplies by the noise amplitude
and the ribbon amplitude in the client's order. `lightning_filter` is the
`filter` free function above `BuildPath` (`:983-1003`) verbatim, with the enum
from `A3DGFXLightning.h:26-29` (`FT_TRIANGLE` 0, `FT_RECTANGLE` 1, `FT_SIN` 2,
`FT_TRAPEZIA` 3).

Two tests pin the law: `a_lightning_ribbon_is_displaced_perpendicular_to_its_own_axis`
and `the_lightning_filter_matches_the_client_free_function`.

### 22.3 What `GenDirection` actually does (checked, no change needed)

`A3DParticleEmitter::GenDirection` (`:72-83`) looks like a one-sided fan but is
not:

```cpp
vDir = (m_bBind ? _unit_z : RotateVec(m_qParentDir, _unit_z));
float fAngle = _UnitRandom() * m_fAngle;            // tilt, drawn first
A3DVECTOR3 vUp = (m_bBind ? _unit_x : RotateVec(m_qParentDir, _unit_x));
A3DQUATERNION q(vDir, _UnitRandom() * A3D_2PI);     // azimuth, drawn second
vUp = RotateVec(q, vUp);
q.ConvertFromAxisAngle(vUp, fAngle);
vDir = RotateVec(q, vDir);
```

`vUp` is spun about `vDir` by a uniform azimuth *before* the tilt, so the tilt
direction covers the full circle: the distribution is a cone of half-angle
uniform in `[0, Angle]` with uniform azimuth, which is exactly `cone_sample`.
Draw order (angle first, azimuth second) matches `cone_direction`. The cyan
tree lights are therefore not a direction-law bug in this path.

## 23. Element 180 is `A3DLtnTrail`, and element 110 checked against `A3DTrail`

### 23.1 180 was drawing element 110's ribbon

`A3DLtnTrail` (`A3DLtnTrail.cpp`) is not an accumulating strip. Its whole state is
at most three cross-sections:

```cpp
// AddPoint, :264-333
if (nCount > 1) { m_TrailLst.RemoveHead(); nCount--; }        // never more than two
A3DVECTOR3 vInter = ldStart.m_vCenter * fR + vCenter * fPortion;   // midpoint
A3DVECTOR3 vAmp = RotateVec(A3DQUATERNION(vDir, _UnitRandom()*A3D_2PI), CalcVertVec(vDir));
vAmp *= m_fMinAmp + (m_fMaxAmp - m_fMinAmp) * _UnitRandom();       // perpendicular jitter
vInter += vAmp;
...
ldCenter.m_fU = ldStart.m_fU + _tex_delta;                        // 0.2, wraps at 1
```

so each tick inserts a **jittered midpoint** between the last centre and the
current one, then the current one: three rows, two quads, refreshed every
`_inner_tick = 40` ms, each row's `U` advancing by `_tex_delta = 0.2`.
`FillVertexBuffer` (`:336-410`) writes `tu = ld.m_fU` on **both rails** with
`tv` 0 and 1 — the texture runs along the trail, across the rails, the opposite
way round from element 110 — and the alpha is the keypoint alpha times
`m_nSegLife / SegLife`.

The viewer routed 180 through element 110's emitter: an unbounded history of
cross-sections laid on a 30 Hz clock, no jitter, and the age fraction in `V`.
That is the difference between a 1-second zigzag trail and a long smooth sheet,
and it is what `gfx/人物/技能/妖兽/巨浪释放2.gfx` and the other 78 element-180
records were drawn as.

There is now a `Runtime::LtnTrail` with its own state and `emit_ltn_trail`
implementing the client's order of operations — age and expiry first, then one
`AddPoint` per 40 ms of accumulated time, then the head-drop and the three-row
cap. One viewer-only adaptation is marked in the code: the client slerps a
rotation between samples and applies it to the authored rails, while the viewer
interpolates the rails themselves; for the rigid placements the corpus authors
that is the same motion, and it keeps one shared jitter offset.
`an_ltn_trail_keeps_three_cross_sections_and_scrolls_its_u` pins the three-row
cap, the 0.2 step in `U`, both rails sharing one `U` per row, and `tv` spanning
0..1.

### 23.2 110 and 140 verified, no change

`A3DTrail` (element 110, 2,956 records) stores one cross-section per frame built
from `OrgPos1`/`OrgPos2` — `vCenterOrg = (p1+p2)*.5`, `vHalf = (p2-p1)*(fScale/2)`
(`:147-155`) — with `SegLife` as the per-segment age, `tv` keyed to the remaining
life fraction and `tu` spanning `m_fTexU .. m_fTexU + m_fTexWid`
(`FillVertexBuffer:700-740`). `emit_ribbon` already matches all of that (rails
from the two `OrgPos` points, linear age fade, `U` across the rails, `V` along
the length). `SegLife` in the retail package is authored 200–2000 ms on every
record, so the zero-life guard never swallows a real trail.

`A3DGFXRing` (element 140, 1,116 records) matches too: `Sects: 0` → 36
(`_default_sects`), the pitch shifting into the two radii, `NoRadScale`/
`NoHeiScale` bypassing the keypoint scale, `tu` running around the
circumference (`:291`). Nothing to change.

## 24. The particle classes, read from the four emitter sources

The four area emitters cover 13,253 of the package's particle records (123 alone
is 9,870). Read against the classes, the viewer had **ids 123 and 124 swapped**:
the class that samples a cylinder was drawing an ellipsoid and vice versa, and
both direction rules travelled with the wrong id.

| id | class | surface sample | volume sample | direction |
|---|---|---|---|---|
| 120 | `A3DPointEmitter` | cone at exactly `Angle`, one draw | `GenDirection` | `m_vMoveDir` = the cone, `m_vPos = m_vMoveDir * 0.001` |
| 121 | `A3DBoxEmitter` | face chosen by `fRand < fS1`, else `fRand < fS2` (non-cumulative) | box | `GenDirection` |
| 123 | `A3DCylinderEmitter` | `(cos θ, sin θ, z)` | disc in XY, `z` symmetric | `GenDirection` |
| 124 | `A3DEllipsoidEmitter` | unit **sphere** point | unit ball | `-v` for surface, **none** in volume |

### 24.1 123 is the cylinder, 124 the ellipsoid

`A3DCylinderEmitter::GenSurface` (`:32-48`) turns `_unit_x` about `_unit_z` by a
full random turn and takes `z = _SymmetricRandom()`:

```cpp
A3DQUATERNION q(_unit_z, _UnitRandom() * A3D_2PI);
A3DVECTOR3 v = RotateVec(q, _unit_x);
float z = _SymmetricRandom();
pParticle->m_vPos = v.x * m_vXRange + v.y * m_vYRange + z * m_vZRange;
```

and its `InitParticle` calls `GenDirection` for both branches, so the emission
cone still applies. `A3DEllipsoidEmitter::GenSurface` (`:94-107`) spins `_unit_x`
about `_unit_y`, then `_unit_y` about that, and writes `m_vMoveDir = -v`; its
`InitParticle` never calls `GenDirection`, and the base
`A3DParticleEmitter::InitParticle` (`A3DParticleEmitter.h:145-155`) leaves
`m_vMoveDir` alone — so a volume ellipsoid emits particles that do not move at
all.

The viewer had `123` sampling the sphere and negating it, and `124` sampling the
cylinder and keeping the cone: both shapes and both directions inverted. They are
now the right way round, `cylinder_volume_sample` reproduces `GenTotal`'s
rejection loop (three symmetric draws per attempt, rejected until
`x*x + y*y <= 1`), and a volume ellipsoid is born with a zero direction.

### 24.2 120 and 121 checked, unchanged

`A3DPointEmitter::InitParticle` is its own override: the surface branch builds
`q1 = axis_angle(vUp, π/2 - Angle)` and stores the *product* `q1 * q`, then
`m_vMoveDir = RotateVec(q1 * q, _unit_y)` — one draw, exactly `surface_cone`,
which `surface_cone_spends_one_draw` and `id_120_surface_emission_is_a_cone_one_unit_out`
already pin. The only difference left is the `_init_delta = .001` nudge that
pushes a 120 particle one thousandth of a unit along its own direction; it is
smaller than a texel at every world scale and is not modelled.

`A3DBoxEmitter::GenSurface` selects a face with `fRand < fS1` and then
`fRand < fS2`, where `fS2` is already normalised by the sum — *not* cumulative,
so the X faces are reachable only when `YZ > XY`. `box_surface_sample` reproduces
that skew (and the sign draw before the two in-face draws), so nothing changed.

The corpus test `corpus_unbound_emission_carries_the_basis_scalar` now asserts the
cylinder's shape for id 123 and the ellipsoid inequality for the classes that
share the ball, which is the check that would have caught the swap.

## 25. Element 152: the append step and its second condition

`A3DGFXLightningEx` derives from `A3DGFXLightning` (element 150) and adds the ring
— which is what the viewer's `Trail` spec modelled from the binary. Two of its
rules were wrong, and both are one screen of source.

### 25.1 Appending needs `m_bCurvedMethod`, not just `isappend`

`A3DGFXLightningEx::TickAnimation` (`A3DGFXLightningEx.cpp:198-218`):

```cpp
if (m_bCurvedMethod && m_bIsAppendly) { TickAdd(dwTickTime); TickList(dwTickTime); }
else if (m_lstCurvePoints.GetDataCount() != m_nVertPairNum)
{
    m_lstCurvePoints.RemoveAll();
    for (int nIndex = 0; nIndex < m_nVertPairNum; ++nIndex)
        m_lstCurvePoints.AddData(LnCurvePoint(nIndex, m_nVertsLife));
}
```

`m_bCurvedMethod` is **not a file key**: `A3DGFXLightning::InitBuf` sets it when
the first keypoint carries a curve-move control (CtrlType 110). So an `isappend: 1`
trail whose keypoint has no 110 control is rebuilt whole every frame and never
animates — the viewer animated every `isappend` record, 138 of 156 in the corpus.
`update_trail` now requires both, and the corpus test asserts that only trails
with both flags can change length.

### 25.2 The step is `TimeSpan / (Segs * LightNum + 1)`

`TickAdd` (`:240-260`):

```cpp
m_nTimeStep += dwTickTime;
int nTimePerVertAdd = m_nTimeSpan / m_nVertPairNum;
if (nTimePerVertAdd == 0) return;
if (m_nCurrentIndex < m_nVertPairNum && m_nTimeStep > nTimePerVertAdd)
{
    int nNewAddedNum = m_nTimeStep / nTimePerVertAdd;
    m_nCurrentIndex += nNewAddedNum;  a_ClampRoof(m_nCurrentIndex, m_nVertPairNum);
    m_nTimeStep %= nTimePerVertAdd;
    ...
}
```

`m_nVertPairNum` is `m_nSegs * m_nLightNum + 1` (`A3DGFXLightning::InitBuf`), so
the divisor is the strip's cross-section count plus one — not `Segs`, which is what
the viewer divided by. A `Segs: 10, LightNum: 1` trail therefore steps every
`TimeSpan/11` rather than `TimeSpan/10`, and a `TimeSpan` shorter than the pair
count lays nothing at all (`nTimePerVertAdd == 0`), where the viewer would still
have animated it. The old code recorded the interval as unquotable — "two
dereferences through a field that is ... never written" — because the disassembly
was reading a member the source names plainly.

`TickList` (`:219-238`) is what the viewer already had: age every record, and with
`istaildisappear` set drop the oldest run from the first record that reached its
life.

The timing tests were rewritten on the recovered step (a 100 ms clock over four
segments and one light is five pairs, so 20 ms a vertex): a 20 ms frame is not
past a step because the comparison is strict, 40 ms lays two, 60 ms lays three,
and the reset test now accounts for the 16 ms frame still sitting in the
accumulator when the ring is emptied.

## 26. `盆景水晶` from the shipped package: the map is under `textures/`

The potted-crystal root is `gfx/场景/新种族地图/盆景水晶.gfx` (25 elements: sheets,
trails, rings and two element-160 records). Both model elements are identical in
the fields that matter:

```text
GFXELEMENTID: 160   TexFile:            ModelPath: 盆景\盆景水晶.SMD
                                   ModelActName:   (empty)   WriteZ: 0 / 1
```

and the archive stores the model beside the root in the same package:

| member | size | header |
|---|---|---|
| `gfx/models/盆景/盆景水晶.smd` | 140 | `MOXBDMSA` v8 |
| `gfx/models/盆景/盆景水晶.bon` | 880 | `MOXBEKSA` v6 |
| `gfx/models/盆景/盆景水晶.ski` | 5,966 | `MOXBIKSA` v9 |
| `gfx/models/盆景/textures/邢盆景水晶.dds` | — | the only map |

Two details of that listing are worth stating because both are silent failures:
the effect writes `.SMD` in upper case while the member is `.smd`, and the map is
**not** in the model's own folder but in `textures/` below it. The first is
handled (`gfx_catalog::find` folds case); the second was not.

### 26.1 `texture_paths()[0]` pointed at a member that does not exist

`ModelSkin::texture_paths` returns two candidates for a skin map:

```rust
ecmodel::normalize(&format!("{parent}/tex_{stem}/{texture}"))   // [0]
ecmodel::normalize(&format!("{parent}/textures/{texture}"))     // [1]
```

`posed_model_meshes` took `paths[0]` unconditionally. In the shipped `gfx.pck` the
`tex_<stem>/` layout exists for 21 folders (29 files: `交叉的剑`, `冰刺`, `冰晶`,
`凌杀`, `天师大冰块`, `泰山`, `火鸟`, `飞剑`, …), while **77 model folders keep
their maps in `textures/`** — 2,409 files, the potted crystal among them. So every
one of those models asked the archive for `gfx/models/盆景/tex_盆景水晶/邢盆景水晶.dds`,
got nothing, and the effect pass drew its flat white stand-in for an unreadable
texture: a white crystal, a white `盆景火`, and so on for the bulk of the skinned
effect models.

The path is now probed through the same reader that loads the skins — the first
candidate the archive actually contains wins, and the element's own `TexFile` is
still the fallback for a mesh with no texture index at all.

### 26.2 The check runs on the real file

`the_real_potted_crystal_poses_its_model` opens `google_drive/gfx.pck`, reads the
root, builds the player, resolves the models through the pack and asserts

* two element-160 records were collected,
* at least eight posed meshes (the skin is four rigid meshes on each of the two
  elements) with vertices,
* and the resolved map is `gfx/models/盆景/textures/邢盆景水晶.dds`.

Without the package on the machine the test says so and skips — the same rule the
corpus tests follow — so CI (which has no `gfx.pck`) still runs the synthetic
`one_triangle_model` chain unchanged, and only a machine holding the retail
package proves the shipped model.

The skin itself is the shape the loader supports: `MOXBIKSA` v9, four rigid
meshes, one texture, one material, **zero bones**, no morph/supple/muscle skin —
the bind pose is the whole model.

## 27. `A3DGridDecalEx`: the class behind every waterfall, and the tint it applies

Elements 210/211 were the last big class handled on inference. The real source is
in `GfxCommon` on the Drive and is now recovered: `A3DGridDecalEx.h` (8,073 bytes)
and `A3DGridDecalEx.cpp` (37,996 bytes, binary MIME — the header carries the two
inline functions that matter).

### 27.1 What the header states

```cpp
typedef struct GRID_VERTEX_DATA { A3DVECTOR3 m_vPos; A3DCOLOR m_dwColor; } GridData;
inline int _calcCurRectNum() { return (m_nHeightVertNum - 1) * (m_nWidthVertNum - 1); }
```

* a lattice is `m_nWidthVertNum * m_nHeightVertNum` **vertices** holding a position
  and a colour each — one quad per `(w-1)*(h-1)` cell, which is the arithmetic the
  reader already uses;
* `_setInitVertPos()` synthesises the un-authored lattice in the **XY plane**
  (`z = 0`), `fHalfWid = max((w-1)/2, 1) * m_fGridSize` split around the origin and
  `Y` running from `+fHalfHei/2` **down** to `-fHalfHei/2`, so the default step is
  `fGridSize/2`, not `fGridSize`;
* `_setColorComposed(diffuse, nVertNum)` multiplies every vertex colour by the
  keypoint colour, channel by channel, written as `(vertex * key) & 0xff00` then
  `>> 8` — full white on full white therefore lands on 254;
* `getVertRotFromView` builds `m_vPos - v.y*m_vDir + v.x*m_vUp + v.z*m_vView` for a
  `RotFromView` lattice, and `_fillGfxVertexMatchGround` re-samples terrain height
  per vertex for `GroundHeight`;
* the class owns an `A3DGfxGridAnimation*`, i.e. a second, per-vertex animation
  channel on top of the UV scroll.

### 27.2 The waterfall, exactly as shipped

`gfx/场景/新种族地图/瀑布.gfx` (version 73, 52 elements) is the retail waterfall:

| part | count | detail |
|---|---|---|
| element 210 curtains | 4 | `物体\waterfall.dds`, 2×4 lattice, `TileMode: 1`, `TexSpeed (0, -0.4)` |
| element 124 emitters | 48 | `烟火\Dust.tga`, `多个粒子\waterfalldrops.tga`, `多个粒子\shuizhu.dds` |

One curtain, verbatim:

```text
-58.299149 35.844849 9.029253   dwColor: ffffffff
-34.900089 35.844849 9.029253   dwColor: ffffffff
...
-58.299149 -0.006341 9.029253   dwColor: 88caff
-34.900089 -0.006341 9.029253   dwColor: 88caff
fGridSize: 0.500000   Color: -32640 (this one: -13261313)
```

23.399 units wide, 35.851 tall, in the XY plane at z = 9.029, foot row `88caff`
with alpha 0 — the sheet dissolves at its foot because that row's alpha is zero,
not because of a gradient. `CtrlMethodCount: 0`: the curtain has **no** grid
animation, so all of its motion is the `TexSpeed` V scroll the player already
reproduces.

### 27.3 The bug: the keypoint tint never reached the sheet

`emit_grid` coloured each quad from the file's vertex colour alone:

```rust
let color = rgba(*value, context.color_scale);   // vertex RGB, alpha only scaled
```

`_setColorComposed` multiplies the vertex colour by the keypoint colour, and 179 of
the pack's 216 grid elements carry a keypoint that is **not** white:

| keypoint colour | grids | example |
|---|---|---|
| `c800805c` | 21 | |
| `ffffffff` | 18 | the only untinted group |
| `64ffffff` | 12 | |
| `c800b9ff` | 8 | `暗语者营地.gfx` |
| `6400e6ff` | 8 | `暗语者营地.gfx` |
| `ff00805c` | 8 | |
| `ff57507c` | 5 | |
| `ff35a5ff` | — | `瀑布.gfx`, the curtain above |

So the waterfall's white vertices are drawn white here and cyan-blue in the
client, and `冷凝术_冰块.gfx` (`ff5580aa`, `ff79abc6`, `ff5555aa`) draws pale
slabs where the client draws blue ice. That is the "white blob" class: the geometry
and the blend were right, the colour multiply was missing.

`emit_grid` now composes `grid_rgba(vertex, key, color_scale)`, which reproduces
the client's `& 0xff00 >> 8` multiply byte for byte and folds the root alpha in the
same place `rgba` does. New test
`a_grid_multiplies_each_vertex_by_the_keypoint_colour` builds a 2×2 lattice of
`ff808080` against a `ffff8080` keypoint and requires exactly 127, 64, 64, 254 —
the values the client's expression produces.

### 27.4 Three audits that came back clean

Run over all 5,459 roots of the shipped `gfx.pck`, and recorded so nobody repeats
them:

* **Grids**: 216 elements, and every one writes exactly `wNumber * hNumber` vertex
  rows — none empty, none short. The authored path is the only live one, so the
  `_setInitVertPos` fallback stays damage tolerance rather than a rendering path.
* **Textures**: 1,610 distinct `TexFile` values, 27,575 references; 1,606 resolve
  under `gfx/textures/`. The four that exist nowhere in the pack are
  `序列/闪电竖.tga` (14 references), `序列/闪电.tga`, `图案/155.dds` and
  `图案/场景3紫.dds` — a lightning sheet and two decals, not a systematic gap.
* **Hard requirements**: the parser rejects a root only for a missing `GfxPath`
  (elements 200–204) or a missing `ColorMin` (particles 120–125). Neither occurs
  once in the corpus, so no shipped root is turned away by them.

`A3DGFXEx.h` also came back with the root-level properties, which is where the next
gap is: `m_bFaceToViewer`, `m_bNoChangeDir`, `m_b2DRender` (whose setter forces
`m_bZTestEnable = !b`), `m_b2DBackLayer`, `m_bUseOuterColor`/`m_OuterColor`,
`m_bFadeByDist` with its start/end pair, `DeriveParentProperty` for children, and
`SetAlphaTransition`. The reader already parses `NoChangeDir`, `2DRender` and
`2DBackLayer`, but nothing in the player reads them yet.

## 28. Sizes: the file's `Width`/`Height` are half extents, and the tool's own box says so

The report's "many oversized effects" is a question about one convention, so it is
worth settling with data rather than taste. `A3DDecalEx.h` (5,005 bytes, recovered
from `GfxCommon` on the Drive) states it twice:

```cpp
float m_fWidth;      //  Half width
float m_fHeight;     //  Half height

void SetSize(float fWidth, float fHeight)
{
    m_fWidth = fWidth;  m_fHeight = fHeight;
    fWidth  *= 2.0f;    fHeight  *= 2.0f;      //  -> the full card
    m_fWidth1 = m_fOrgPtWidth  * fWidth;  m_fWidth2  = fWidth  - m_fWidth1;
    m_fHeight1 = m_fOrgPtHeight * fHeight; m_fHeight2 = fHeight - m_fHeight1;
}
```

and `GetProperty(ID_GFXOP_DECAL_WIDTH)` returns `m_fWidth * 2.0f`, so the editor's
"width" is the card and the file's `Width` is the member — the **half** extent. A
`Width: 0.25` card is therefore 0.5 units across; the shipped waterfall emitters
(`ParticleWidth` is a different key, unaffected) and every beam and slash card in
the game are twice their authored number.

### 28.1 The `Vec:` box agrees, root by root

Every root ships the AABB its own editor measured. In the twelve roots whose only
geometry is one sprite with an identity `Direction`, position zero, `Scale: 1`,
`DedaultScale: 1`, `OrgPt: 0.5` and no `RotFromView`, the box's half extents equal
`Width` and `Height` exactly:

| root | Width × Height | half extents |
|---|---|---|
| `18品双剑_剑柄.gfx` | 0.130 × 0.500 | 0.130, 0.500, 0.000 |
| `18品双剑_剑身高光.gfx` | 0.035 × 0.750 | 0.035, 0.750, 0.000 |
| `紫色剑光长.gfx` | 0.130 × 1.000 | 0.130, 1.000, 0.000 |
| `绿色剑光.gfx` | 0.250 × 0.500 | 0.250, 0.500, 0.000 |
| `蓝色剑光{,长}.gfx` | 0.250 / 0.130 × 0.500 / 1.000 | same, same, 0.000 |
| `黄色剑光.gfx` | 0.300 × 0.600 | 0.300, 0.600, 0.000 |
| `水族箱玻璃.gfx` | 3.200 × 2.150 | 3.200, 2.150, 0.000 |
| `炼狱凶神_眼神片.gfx` | 0.020 × 0.010 | 0.020, 0.010, 0.000 |

If a card were `Width` wide, the tool would have to pad every one of those boxes by
exactly two on both axes and leave Z at zero; it pads nothing else anywhere. So the
card is `2 × Width` by `2 × Height`, which is what `quad_corners` and the static
reader both draw. New test `a_sprite_spans_twice_its_authored_width_and_height`
runs `绿色剑光.gfx` verbatim and requires the drawn half extents to equal that
root's own `Vec:` half extents, so a later "correction" cannot halve the game.

### 28.2 The sunbeams: the scatter is authored, the aiming is not

`gfx/场景/新种族地图/树林日光一道.gfx` (version 73, 4 elements) is the forest
sunlight: three element-100 cards of `圆环光\光束13.dds`, `Height: 7.5`,
`MaxExtent: 11.25`, **`RotFromView: 1`**, plus one element-121 dust emitter. Each
card carries its own keypoint quaternion:

| element | `Direction` | card |
|---|---|---|
| `Unnamed16` | `-0.707107, 0.000000, 0.000000, 0.707107` | 0.5 × 7.5 |
| `Unnamed6` | `-0.560986, -0.430459, -0.430459, 0.560986` | 0.25 × 7.5 |
| `Unnamed19` | `-0.701057, -0.092296, -0.092296, 0.701057` | 0.5 × 7.5 |

So the shafts *are* meant to fan, but each one fans by an authored quaternion, not
by chance: `Unnamed16` is a ninety degree turn about X (the shaft lies along Z),
and the other two are small tilts. A viewer that drops those quaternions — or that
forces every shaft to face the camera — turns three deliberate angles into what
reads as random aiming. With `RotFromView` the client keeps the shaft's height on
that direction and takes only its width from the view
(`A3DDecalEx.cpp:357-413`), which is the rule `emit_sprite` implements.

`MaxExtent: 11.25` is `1.5 × 7.5`, i.e. the loader's default for an authored zero
(`0x71e121`), not a size — it is the cull/LOD radius. Half of the 15-unit shaft is
7.5, so the extent covers it.

### 28.3 Where the current tree stands on the three reported symptoms

* **Static waterfall** — the curtain is element 210 and its motion is the
  `TexSpeed` V scroll; `a_grid_builds_its_authored_sheet_and_scrolls_it` runs 600
  frames and fails on any frame that repeats the previous UV. The colour multiply
  added in §27 was the other half of that report.
* **Random sunrays** — the aiming above; the four shaft modes are still selectable
  so the faithful `client` rule can be compared against the alternatives.
* **Oversized effects** — the sizing above is confirmed, not changed: the doubling
  is the client's, and the new test pins it to the root's own `Vec:` box.
* Still unread from `A3DGFXEx.h`: `NoChangeDir`, `2DRender` (whose setter also
  clears the Z test) and `2DBackLayer` are parsed but consumed by nothing.

## 29. `ZFUNC`: the effect pass was using `LESS` where the client uses `LESSEQUAL`

Flicker is a depth question before it is a blending question, and the client answers
it with a device default.

* `A3DGFXRenderSlot.h` shows what a slot carries: `m_nRenderLayer`, `m_VertSize`,
  `m_PrimType`, `m_strTexPath`, `m_Shader`, `m_bTileMode`, **`m_bZTestEnable`**,
  `m_nRef`, `m_TotalVertCount`, the element array, the texture, the pixel shader
  and `m_enumFogFlag`. There is a ZENABLE byte and **no Z-function field**, so
  every effect slot runs on whatever `D3DRS_ZFUNC` the device holds — the D3D8
  default, `D3DCMP_LESSEQUAL`. (The repo already records the only depth-state
  writes the effect path makes: the `ZBIAS` 2 / 0 pair at `0x417453`/`0x417471`;
  `ZFUNC` is never written.)
* The viewer's world-effect pass asked for `wgpu::CompareFunction::Greater`
  (`world_models_gpu.rs`, the reverse-Z equivalent of `LESS`). Under reverse-Z,
  `LESSEQUAL` is `GreaterEqual`.

What `LESS` costs, in a blended pass where nothing writes depth: a sheet drawn at
the same depth as one already there is **dropped**. Decals and their overlay glows,
paired cards, the coplanar layers of a portal, a curtain and its spray all sit at
identical or near-identical depth, so the loser disappeared — and because the depth
is computed per vertex in floating point, one hair of camera movement swaps the
comparison and the sheet blinks back. That is the flicker, and it is exactly the
tie rule the repo had already spelled out for baked scenery
(`gpu.rs`: "a duplicate baked triangle submitted after an identical one must win,
exactly as a second draw at equal depth passes `LESSEQUAL`") but had not applied to
the blended pass, where a lost tie is a vanished sheet rather than an identical one
underneath.

The effect pass now compares `GreaterEqual`. Two blended draws at equal depth both
pass, which is what the client does.

Still unmodelled from the same disassembly note: the `ZBIAS` 2 / 0 pair. It biases
effect depth for a reason (almost certainly to lift a coplanar layer above the
surface it decorates), but the unit of a D3D8 `ZBIAS` of 2 does not map onto a
`DepthBiasState` constant without measurement, so no bias is invented here.

## 30. Element 160 draws through a skin, not through a quad slot: `WriteZ` was ignored

§29 established that a *quad* slot carries no depth-function and never writes
depth. A model element is not a quad slot. `A3DGFXECModel.h` (element 230) shows
the shape of that family — the element owns an `m_pECModel` and an
`UpdateECModel(A3DMATRIX4, DWORD dwDiffuse, DWORD dwTickTime)` that takes the
element's transform and *diffuse*, then overrides `Render`, `TickAnimation` and
`PrepareRenderSkinModel`; it renders an `A3DSkinModel`, not a vertex buffer of
its own. Element 160 (`A3DGFXModel`) is the same idea for an `.SMD`.

That matters because the element saves its own `ZWRITEENABLE` state: the corpus
authors `WriteZ` on `160` blocks. §31, written once `A3DGFXModel.cpp` arrived,
corrects the arithmetic guessed here: 115 of the 447 shipped models author
`WriteZ: 1`, 131 author `0`, and 201 predate the version-49 key, whose
constructor value is **false** — see §31. The
viewer parsed `WriteZ`, `AlphaCmp` and `Use3DCam` and then never read any of them
(`grep` over `src/` found no consumer), so a crystal, an ice block or a stone
model went through the batched effect path with `depth_write_enabled: false`.

In a blended pass that never writes depth, a model cannot occlude anything drawn
after it: the waterfall sheet behind a crystal still passes the depth test (the
buffer holds only the terrain's depth) and is painted over the model. The fix
uses the authored state instead of a constant:

* `EffectMesh` carries `write_depth`, filled from `ModelRef::write_z` when
  `emit_model` wraps the posed vertices into quads (the load-time pose has no
  element yet, so its value is `false`).
* `PipelineKey` gains `write_depth`, so a writer and a non-writer in the same
  layer, blend pair and texture do not share a pipeline.
* Inside a `RenderLayer` a depth writer now sorts *before* the blended sheets,
  ahead of `Priority`: the writer has to establish the buffer the sheets are
  tested against, or a model drawn last would still be painted over.
* Quad slots keep `depth_write_enabled: false`, as §29 requires.

`AlphaCmp` and `Use3DCam` remain parsed and unread — the alpha test of a model
draw and the camera choice need `A3DGFXModel.cpp`, which is not in hand yet.

## 31. `A3DGFXModel.cpp`: what a model element actually is, in the client's words

The source for element 160 arrived in `pw152source` as `A3DGFXModel.cpp`
(13,274 B, Drive id `1ggPY4Gk2BSb2O7yfGnQX8PWmQ87PT_VX`). It settles everything
§30 had to infer, and corrects one number in it.

**Constructor values, which are the meaning of an absent key.** `A3DGFXModel::A3DGFXModel`
initialises `m_nLoops(1)`, `m_bAlphaCmp(false)`, **`m_bZWriteEnable(false)`**,
`m_bStart(false)`, `m_bIsUse3DCamera(false)`, `m_bIsFacingDir(false)`,
`m_iMatMethod(MTL_ORIGINAL)`, `m_MatScale(1,1,1,1)`, `m_fTransparent(-1)`.

**Load gates**, all of them text lines on `dwVersion`: `ModelActName` ≥ 19,
`AlphaCmp` ≥ 29 (read as `int`, `!= 0`), `Loops` ≥ 31, `WriteZ` ≥ 49,
`Use3DCam` ≥ 77, `FacingDir` ≥ 99. The shipped corpus (versions 33 – 79) can
therefore carry `FacingDir` in no file, and a census of `160` blocks agrees: no
`FacingDir` key exists in `gfx.pck`.

**Census of the 447 element-160 blocks in `gfx.pck`:**

| key | values |
| --- | --- |
| `WriteZ` | `1`: 115, `0`: 131, absent: 201 |
| `AlphaCmp` | `1`: 9, `0`: 438 |
| `Loops` | `-1`: 174, `1`: 72, absent: 201 |
| `Use3DCam` | `0`: 133, absent: 314 |
| `SrcBlend`/`DestBlend` | 5/2: 253, 5/6: 179, 2/4: 6, 2/2: 9 |

So §30's rule was right and its default was wrong: `write_z` must default to
**false**, and only those 115 authored models write depth. `gfx_file.rs` now takes
the constructor value, and `model_defaults_do_not_write_depth` pins the three
defaults a `160` block can omit.

**Blend state.** `Play()` registers the slot with a *fixed*
`SRCALPHA`/`INVSRCALPHA`, but that is the slot's own prim state; the skin model is
given the authored pair at load (`ChangeModel` → `SetSrcBlend(m_Shader.SrcBlend)`)
and again every tick for a shared model (`PrepareRenderSkinModel`). The authored
pair therefore drives the model's draws, which is what the viewer already does.
`WriteZ` is different: `PrepareRenderSkinModel` ends with an unconditional
`m_pDevice->SetZWriteEnable(m_bZWriteEnable)` for every model, so it applies to
shared and private models alike.

**Colour and lighting.** `TickAnimation` builds
`diffuse = kp.MultiplyAlpha(m_pGfx->GetActualAlpha())` — keypoint colour times the
root's actual alpha — then multiplies by the root's outer colour when
`IsUsingOuterColor()`. That diffuse becomes `MTL_SCALED` material scale
(`rgb/255`) and, when its alpha is not 255, `m_fTransparent = 1 - a/255`. The skin
model is then given `SetLightInfo` from `AfxGetLightparam()`: device ambient,
the directional light's direction, diffuse and specular, with point lights off,
and `ChangeModel` calls `EnableSpecular(true)` and
`SetAutoAABBType(AUTOAABB_INITMESH)`.

The viewer's model path is **unlit**: `EffectVertex` carries position and UV only,
so the posed normals `pose_bind` already computes are dropped, and no light
uniform reaches the effect pass. A model is therefore drawn at full texture
brightness with no directional shading and no specular, whatever the region's
light is. That is a real difference, but the fix needs the light snapshot the
scenery shader uses (`gpu.rs` `region_ambient`/`region_diffuse`/`region_direction`
are a *regional* grid, not the single `A3DLIGHTPARAM` the client hands the model),
so it is left undone rather than approximated.

**Still not implemented, all now with a named client entry point:**

* `ModelActName` + `Loops` — `PlayActionByNameDC(action, loops, 0)` on the first
  tick after `Play()`. The viewer poses the bind pose (`pose_bind`), so an
  animated effect model is frozen. `model_playback.rs` already samples actions for
  scenery, so this is plumbing rather than new maths.
* `AlphaCmp` — `A3DSkinModel::EnableAlphaComp(bool)`. Nine elements. The
  comparison function and reference value live in that class, not in this file,
  so no threshold is invented here.
* `FacingDir` (≥ 99) — orientation from the frame's movement with 0.2/0.8
  smoothing. Not reachable from this pack.

**`Use3DCam` is dead in the client.** `IsForceUse3DCamera()` is
`return false/*m_bIsUse3DCamera*/;`, `SetProperty` has the assignment commented
out and `GetProperty` returns a literal `false`. The viewer parses the key for
completeness; nothing may act on it, and the doc comment on the field says so.

## 32. Element 160 is lit and fogged like the skin model it is

§31 left the effect model path unlit and unfogged: `EffectVertex` carried position
and UV only, so the normals `pose_bind` computes were discarded, and the pass had
no light or fog constants at all. A crystal rendered at full texture brightness
whatever the region's light, and stayed bright at any distance. Both are now done
with the client's own arithmetic rather than a new one.

**The vertex now carries what the skin shader reads.** `EffectVertex` gained
`normal`; `EffectMesh` gained the mesh's `MeshMaterial` (`ambient`, `diffuse`,
`emissive`) taken from `skin.data.materials[mesh.material]`. The placement
transforms the normal as a vector and renormalises it — the placement's scale is
the keypoint scale, which is uniform, so that is the client's
`matScale * GetTranMatrix(kp) * GetParentTM()` applied to a normal. A posed vertex
with no authored normal stays zero and keeps the flat colour.

**The colour law is `A3DGFXModel::UpdateModel`'s.** The element diffuse
(keypoint colour times the root's actual alpha) is spent on the *material*, not on
a vertex tint: `MTL_SCALED` multiplies each material colour by `diffuse.rgb/255`
with a clamp (`model_material::scale_rgba`), and an alpha below 255 becomes
`m_fTransparent = 1 - a/255`, which binds as `c8.a`. `model_material::bind` then
produces `c7` (ambient) and `c8` (directional diffuse), and the vertex shader
computes `min(c7 + c8 * lambert, 1)` with `lambert = max(dot(n, -L), 0)` — the
same `model_light::vertex_light` the repo already ported. `bPtLight` is forced
off, exactly as every `LIGHTINFO` `A3DGFXModel` builds sets it, so no model
element ever takes the dynamic point light.

**The light comes from the frame the models already use.** `gfx_effect_gpu::Draw::update`
takes an `EffectLight { lighting, direction, fog_range, fog_color }`, built by the
caller from the same `model_draw_gpu::Frame` the scenery skin models are drawn
with — `world_models_gpu` for the world and the walker pass for character add-on
effects. That is `AfxGetLightparam()` in the client: one directional light, the
device ambient, and the same fog.

**Fog is applied to meshes only.** `A3DSkinModel` is fogged like any model, so a
vertex belonging to a mesh sets the per-vertex `fog` flag and the fragment stage
computes the retained fraction `(end - depth) / span` on the reciprocal of `w`,
mixing towards `fog_color.rgb` — the formula `model_receiver_gpu` already uses. A
*quad* leaves the flag at zero: its slot carries `m_enumFogFlag`, which this pass
does not model, so it keeps the colour the element authored.

**What is still not implemented here:**

* Specular. The client calls `EnableSpecular(true)` and the skin shader writes a
  separate Blinn term (`model_light::directional_specular`, and
  `model_material::bind_specular` for `c9`). The scenery path models it through
  `model_receiver_gpu`; the batched effect pass has one colour stream, so adding
  specular means deciding how it combines with the authored blend pair, and the
  module that owns the maths still says "not yet wired to world model drawing".
  Left out rather than guessed.
* `AlphaCmp`. `A3DSkinModel::EnableAlphaComp(bool)` — nine shipped models. The
  comparison function and reference value live in `A3DSkinModel`, not in
  `A3DGFXModel.cpp`, so no threshold is invented.
* Action playback. `PlayActionByNameDC(m_strActName, m_nLoops, 0)` runs on the
  first tick; the viewer poses the bind pose. This is a structural change, not a
  formula: `collect_models` runs once per player and caches the posed meshes, so
  playing an action means re-posing per frame against each element's own clock,
  with a cache in front of the asset load. Recorded here with that plan rather
  than half-built.

## 33. Element-160 actions: the census, the loop rule, and the clock that was missing

§31 left `ModelActName` + `Loops` as the last unimplemented model feature, with a
note that it should only be built if the shipped models actually carry actions.
They do, and this is the count that decides it.

**Census of the 447 element-160 blocks in the retail `gfx.pck`** (every `^Key:
value$` line before `StartTime`, resolved against the `.SMD` each block names
under `gfx/models/`):

| measurement | count |
| --- | --- |
| blocks naming an action (`ModelActName` non-empty) | 245 |
| blocks leaving it empty | 202 |
| `Loops: 1` / `-1` / absent | 269 / 175 / 3 |
| model has **no** action list at all | 171 |
| model has actions, and the named one is among them | **234** |
| model has actions, but not the named one | 2 |
| model has actions, block names none | 9 |
| block has no `ModelPath` / the `.SMD` is not in the pack | 6 / 27 |

So 234 blocks draw a rig whose action really exists: an animated effect model is
the normal case, not an edge case, and the earlier "if most do not, downgrade the
item" test comes out the other way. The 171 action-less models are genuinely
static props.

This also **supersedes the `Loops` row of §31**, which listed `1`: 72, `-1`: 174,
absent: 201. The 201 there is the `WriteZ`-absent column; the real split is 269 / 175
/ 3, and 269 is consistent with the constructor default of 1 plus the authored
ones. §31's other rows re-measure identically (`WriteZ` 115/131/201, `AlphaCmp`
1: 9, `ModelActName` named 245 / empty 202).

**The client's rule, line by line.** `A3DGFXModel::UpdateModel` starts the action
exactly once per `Play()`:

* `A3DGFXModel.cpp:239-240` — `if (!m_strActName.IsEmpty())
  pSkinModel->PlayActionByNameDC(m_strActName, m_nLoops, 0);`, inside the
  `if (!m_bStart)` block that also resets `m_iMatMethod`, `m_MatScale` and
  `m_fTransparent`.
* `A3DGFXModel.cpp:314-330` — `A3DGFXModel::Play()` ends with `m_bStart = false;`,
  so a restarted root replays the action from its first frame.
* `A3DGFXModel.cpp:285` and `:346` — `pSkinModel->Update(m_dwTickTime)`, where
  `m_dwTickTime` is the element's own scaled tick delta.

The loop counter is `A3DSkinModelAction`'s:

* `A3DSkinModelAct.cpp:517` — `if (!pChannel || !iNumLoop || ...) return;`: a zero
  loop count plays nothing at all.
* `A3DSkinModelAct.cpp:532-533` — `m_iLoopTime = iNumLoop; m_iLoopCnt = iNumLoop;`.
* `A3DSkinModelAct.cpp:565-567` — `m_fDeltaTime += 1.0f * iDeltaTime;
  iDeltaTime = (int)m_fDeltaTime; m_fDeltaTime -= (float)iDeltaTime;`. The
  sub-millisecond remainder is kept across ticks, unlike `CECModel::Tick`.
* `A3DSkinModelAct.cpp:588-596` — each wrap decrements `m_iLoopCnt`, and when it
  reaches zero the action sets `m_bStopNextTick` and clamps
  `dwAbsTime = GetEndTime()`: the last frame is held, not looped again.
* `A3DSkinModelAct.cpp:602-606` — with `m_iLoopCnt < 0` a zero-length action
  breaks out immediately; `-1` never counts down.

**What the viewer does now.** `gfx_player::ModelInstance` owns one rig pointer,
the drawn topology (texture, triangle-soup order with bind-pose UVs, material),
the bind pose, and a `model_playback::Playback` built by the new
`Playback::from_action` with the element's `Loops`. `frame()` advances the clip by
the element's scaled delta with the client's fractional carry and re-poses only
when the clip moved; a model with no resolvable action keeps the bind pose and
never deforms. A rig that cannot be sampled falls back to the bind pose once with
a diagnostic, which is the client's `BindTrackSet()` failure path.

The structural pieces §32 predicted are all here: a per-element clock, per-frame
re-posing, and a cache in front of the asset load. `load_models` parses each
`.SMD`/`.BON`/`.SKI` once per root tree (113 distinct models behind 447 blocks),
and `ModelAsset::load_static_smd_with` keeps the `.SMD`'s action list and track
sets that `load_static_skins_with` still drops for `CECFace`. `Player::reset`
restarts every clip, mirroring `A3DGFXModel::Play`'s `m_bStart = false`.

The old justification in this file — "the skins are rigid, so the bind pose is the
only pose they need" — was wrong and is corrected in the code comment: a rigid
skin binds each vertex to 0 or 1 bones, and the *bones* still move. 234 blocks
show the difference.

## 34. `AlphaCmp` only toggles the device alpha test; its comparison is never written

`A3DSkinRender.cpp:1746` is the whole use of the flag:

```
if( pModel->GetAlphaCompFlag() )
    m_pA3DDevice->SetAlphaTestEnable(true);
```

`A3DDevice::SetAlphaTestEnable` (`A3DDevice.cpp:981-987`) writes
`D3DRS_ALPHATESTENABLE` and nothing else. The matching setters exist —
`SetAlphaFunction` at `A3DDevice.cpp:999-1002` (`D3DRS_ALPHAFUNC`) and
`SetAlphaRef` at `:1013-1018` (`D3DRS_ALPHAREF`) — but no caller on the model path
uses them, and `A3DSkinRenderWithEffect.cpp:1743` has even the enable commented
out. Nine shipped element-160 blocks author `AlphaCmp: 1`.

So the comparison function and reference are whatever the device defaults to, and
this corpus does not state them. Emitting a viewer-side `discard` threshold now
would be an invented constant, so the key stays parsed and unread. To implement
it later, the D3D8 defaults for `D3DRS_ALPHAFUNC` / `D3DRS_ALPHAREF` must be
established from a device reference rather than guessed.

## 35. Quad fog: `m_enumFogFlag` is recovered, and quads are fogged after all

§32 said a quad stays unfogged "because `m_enumFogFlag` is still unmodelled". The
flag is now read out of `A3DGFXRenderSlot.cpp`, and it says the opposite:

* `A3DGFXRenderSlot.cpp:341` — every new slot gets `m_enumFogFlag = GFX_FOG_CURRENT`.
* `A3DGFXRenderSlot.cpp:349-352` — only an *additive* slot is downgraded:
  `if (sh.DestBlend == A3DBLEND_ONE) { ... m_enumFogFlag = GFX_FOG_ZERO; }`.
* `A3DGFXRenderSlot.cpp:565-573` (`PreRender`, the quad path) and `:774-782`
  (`RenderSkinModel`) do the same thing with it: `SetFogEnable(m_bCurFogEnabled)`,
  and when fog is enabled `SetFogColor(m_clCurFog)` for `GFX_FOG_CURRENT` versus
  `SetFogColor(0)` for `GFX_FOG_ZERO`.

Fog is therefore **on for every slot whenever the device's current fog is on**;
the flag selects the colour it fades to, not whether it fades. An additive sheet
fades to black, everything else to the current fog colour. The viewer leaves the
per-vertex fog weight at zero for every quad, which is wrong in both directions.

This is not implemented in this commit, and the reason is verification rather than
the rule: on this machine the effect pass's raster tests return the cleared
backbuffer for every draw. Three of them (`gpu_effect_batches_honour_blend_mode_layer_and_depth_test`,
`gpu_effect_fogs_a_model_mesh_but_not_a_quad`,
`gpu_effect_rejects_non_unorm_targets_and_reports_missing_textures`) fail
identically at `64e947b` with the working tree stashed, so the harness, not this
change, is what is broken locally, and a shader edit could not be measured at all.
The plan once that harness renders again: give `Vertex::fog` a third state
(`2.0` = fade to black) chosen from the authored `DestBlend == 1`, keep `1.0` for
meshes and every other quad, and let the fragment stage `select` between
`fog_color.rgb` and zero. The §32 assertion that "a quad must ignore the model
fog" then has to be replaced by that law.

## 36. Quad fog implemented; §35's harness verdict was wrong

**Correction first.** §35 said the effect pass's raster tests "return the cleared
backbuffer for every draw" and that the harness was broken locally. That was
wrong, and the evidence was in the numbers: the failing pixel was
`[0.25098, 0.25098, 0.25098, 0.749]`, not the clear value. The draws were
rendering. Three assertions were stale:

* `gpu_effect_batches_honour_blend_mode_layer_and_depth_test` expected
  `1*0.5 + 0.25*0.5 = 0.625` for the alpha pair, but the fragment stage multiplies
  the sampled texel by the quad colour, so the source is the 64-grey map
  (0.25098) and the result is `0.25098*0.5 + 0.25*0.5 = 0.25049`.
* `gpu_effect_fogs_a_model_mesh_but_not_a_quad` asserted `quad[2] <= 0.02`, which
  a grey sheet can never satisfy: its blue channel is 0.25098 before any fog.
* `gpu_effect_rejects_non_unorm_targets_and_reports_missing_textures` expected the
  missing-texture group to vanish, while `Draw::update` deliberately keeps it on
  the fully transparent fallback ("Returning here instead would delete the
  bucket"), so the group and its six vertices are still there.

The lesson for the next session is the one that cost the most time here: read the
failing *value*, not the test's name. A clear-colour readback and a
texture-multiplied quad look like the same failure until the numbers are printed.

**The rule, implemented.** `A3DGFXRenderSlot.cpp:341` initialises every slot to
`m_enumFogFlag = GFX_FOG_CURRENT`; `:349-352` replaces it with `GFX_FOG_ZERO`
only when `sh.DestBlend == A3DBLEND_ONE`; and both consumers
(`:565-573` in `PreRender` for quads, `:774-782` in `RenderSkinModel`) call
`SetFogEnable(m_bCurFogEnabled)` first and then choose the fog *colour* —
`m_clCurFog` for `CURRENT`, `0` for `ZERO`. Fog is therefore on for every slot
while the device's fog is on, and the flag only decides what it fades towards.

The viewer now carries that as three states in `Vertex::fog` instead of a
boolean: `1` = current fog colour, `2` = black, and `0` reserved for unfogged.
`Draw::update` picks `2` when `quad.destination_blend == A3DBLEND_ONE` (code `2`,
`model_queue.rs:152` maps it to `Factor::One`) and `1` otherwise; every
element-160 mesh stays `1`, which is correct because `A3DGFXModel::Play`
registers its slot with a fixed SRCALPHA/INVSRCALPHA pair, never `ONE`. The
fragment stage selects `fog_color.rgb` or zero and mixes by the same
`(end - depth) / span` fraction §32 established.

This **supersedes the §32 sentence** "a quad leaves the flag at zero: its slot
carries `m_enumFogFlag`, which this pass does not model, so it keeps the colour
the element authored", and §35's claim that the law could not be verified.

The test that pinned the old behaviour is now
`gpu_effect_fogs_every_slot_towards_its_own_fog_colour`: the additive sheet must
land on `0.1255` in all three channels (half of 0.25098 towards black) and the
SrcAlpha/InvSrcAlpha sheet on `(0.625, 0.125)` towards the red fog. Both run on
this machine's adapter, so unlike §35's note the assertion is measured, not
merely compiled.

## 37. The transparency law was dead, the vertex alpha was clamped by the wrong term, and specular arrives

Three things came out of giving element-160 meshes the Blinn term, and the first
two were defects the compiler could not see.

**A comment had swallowed `m_fTransparent`.** In `gfx_effect_gpu::shade` the
assignment

```
lighting.transparency = if color[3] < 1. { 1. - color[3] } else { -1. };
```

sat at the end of the line that opens with `// ... anything else overrides
`c8.a`.` — inside the comment. It compiled, it looked right in the diff, and it
never ran: `transparency` kept the frame's value, so §32's law was inert and every
element-160 model drew fully opaque whatever its keypoint alpha said. The line is
its own statement again, and `gpu_effect_applies_the_element_alpha_as_material_transparency`
now pins a half-opacity model to 0.1255 (half of the 64-grey texel) so the
regression cannot come back silently.

**The vertex alpha must not take the Lambert term.** §32 wrote the shaded colour
as `min(c7 + c8 * lambert, 1)` for all four channels, which also scales `c8.a` by
the Lambert. The client does not: `model_vertex_light_rgba` computes
`a = ambient.a + dir_diff.a` when the point light is off — the directional alpha
is added whole — and only the point-light branch multiplies alpha by the Lambert
(`a = ambient.a + lambert*dir_diff.a + …`). The specular variant does the same,
`min(ambient.a + dir_diff.a, 1)`. `bPtLight` is always false here, so a model's
opacity is `c8.a` regardless of which way its facets face.

The effect shader now calls the ported function itself,
`model_specular_variant_diffuse(...)`, with the point light off, instead of
re-deriving the sum — so the alpha and the RGB clamp come from the same code the
scenery path uses. For a quad the normal is zero, the Lambert is zero and the
directional constant is zero, so the call still reduces to the element colour.

**Specular.** `A3DGFXModel::ChangeModel` calls
`A3DSkinModel::EnableSpecular(true)` on every effect model, so the specular shader
split is on for all of them and §32's omission was a real gap on crystals and ice:

* `c9` is `model_material::bind_specular` of the material's specular colour (with
  the same `MTL_SCALED` factor the other coefficients take) against the frame's
  `directional_specular`;
* the Blinn term is `model_light::directional_specular(normal, direction, eye,
  power, c9)`, whose eye input is the camera's forward direction, not a vector to
  the eye;
* the fragment stage combines them exactly as `model_receiver_gpu` does —
  `clamp(texel.rgb * diffuse.rgb + specular, 0, 1)` — and the opacity is the
  vertex alpha times the texture alpha.

`gfx_effect_gpu` composes `model_light::WGSL` into its shader source instead of
carrying a second copy of those functions. `gpu_effect_adds_the_model_blinn_term`
checks both ends: with a white material specular and a white specular light the
facet facing the half-vector saturates, and with the specular light off the same
facet is back to the diffuse-only 0.25098.

One detail of the client's fragment stage is still not modelled: `opacity =
v.diffuse.a * mix(1, t.a, material_alpha_flag)`. The pass always multiplies the
texture alpha, which is the flag-set case; a material that disables texture alpha
would be drawn more transparent here than in the client. Nothing in this corpus
was measured to distinguish the two, so it stays as it is and is recorded here.

**CI note.** The Linux job of PR #35 currently dies in "Install Linux build
dependencies" (`apt-get update && apt-get install libx11-dev libxi-dev
libxcursor-dev libxrandr-dev libxkbcommon-dev`) before any Rust step runs; the
same step passed at `64e947b` earlier the same day. The Windows x64 job builds the
release binary green on the same commit, so this is the runner's package mirror,
not the change. The unit and GPU tests above were not executed by CI in this run.

## 38. The CI that §37 could not reach: repaired, and both jobs are green

§37 ended with "the unit and GPU tests above were not executed by CI", because the
Linux job died in step 5 (*Install Linux build dependencies*) and every later step
was skipped. That is not a property of the change and it is now fixed, so the
evidence is finally real rather than local.

**Why the job died.** `sudo apt-get update && sudo apt-get install -y libx11-dev
…` is one step with no retry, and the runner's package mirror fails transiently:
`apt-get` exits 100 or stalls on the index fetch. Because every later step of the
job depends on it, one flaky mirror fetch skipped `cargo check`, both test steps
and `cargo fmt --check`, and left *Post formatting diff* failing as a curl-down
effect. `gh run rerun` is refused on a run whose workflow file has moved on, and
the REST rerun endpoint answers 403 for this token, so the only way out was to
make the step survive the flake:

* skip the install entirely when `dpkg -s` says the five headers are already on
  the image;
* otherwise bound each attempt with `timeout 240`, retry three times, and still
  fail the step if all three fail;
* repeat apt's own output as annotations on failure, because the raw step log is
  served from blob storage and is not reachable from every client (`gh run view
  --log` returns `EOF` here).

The same treatment was given to the `mesa-vulkan-drivers` install in the Vulkan
step, which failed for the same reason on the next run after step 5 was fixed.

**Two ordering fixes, both earned by a failure.** The formatting check was moved
ahead of the Vulkan installs: `cargo fmt --check` needs no GPU, and gating it
behind an environment-sensitive step meant a broken software driver hid every
rustfmt opinion behind "skipped". *Post formatting diff* now also refuses to post
an empty comment when an earlier failure left `/tmp/fmt.check` empty.

The sRGB window step now also encodes its failure in the exit code — `41` when no
lavapipe ICD is on the image (the probe fails and the test never ran), `42` when
the wgpu test itself failed — because check-run annotations are capped and the
step's own `::error::` lines were being dropped in favour of the runner's notices.
It also probes for any `*lvp*` driver description rather than one exact file name,
since Mesa has renamed that file more than once.

**What the first green run proves** (`43063516095`, both jobs):
`cargo check --locked --bins` type-checks the specular and transparency changes
of §37 — `Vertex.specular`, `Params.eye`, attribute location 6, the
`format!("{}\n{}", crate::model_light::WGSL, WGSL)` composition and both new
tests compile; the viewer unit tests and the GFX light audit pass; and the four
GPU steps (sRGB window, the 22 Phase 1 shader regressions, the terrain per-view
check) pass on lavapipe. The Windows job builds the portable viewer, the audio
and survey tools and stages the artifact green.

**rustfmt wanted the asserts split, and had said so before.** The first readable
formatting comment named three hunks in `gfx_effect_gpu.rs`, all of them mine:
`assert!((half[0] - 0.1255).abs() <= 0.01, "…")` and its two neighbours. They are
93 columns, well under the 100-column limit, but rustfmt wraps *call arguments*
at `fn_call_width` (60 by default), so a two-argument macro with a message string
goes to one argument per line whatever the total width. Commit `e7f2fcf` had
collapsed them onto single lines by guessing at the width instead of reading the
diff the comment carried; hand-formatting is not a substitute for the formatter's
own opinion, and the check is now ahead of the steps that can fail.

## 39. `Warp: 1` is a distortion source, not a card: 979 shipped elements were being drawn

The key had been parsed since the beginning (`gfx_file.rs:300`, read from version
13 on into element+0x…, `lookup.flag("Warp", false)`) and nothing read it. The
client does: `A3DDecalEx::Update` ends with

```cpp
if (m_bWarp) { AfxGetGFXExMan()->AddWarpEle(this); return false; }   // A3DDecalEx.cpp:1330-1336
```

so an element with the key is handed to the *distortion* pass and never enters the
normal render list. The viewer has no distortion pass, which means the client's own
answer is "draw no card here" — and it was drawing them.

**The census** (2019 pack, 5,459 roots): 979 elements in 708 roots set the key, by
element id `{123: 319, 100: 305, 101: 195, 124: 63, 120: 48, 121: 16, 125: 13,
110: 13, 210: 5, 140: 1, 200: 1}`. The decal classes are the bulk of it — 305
element-100 and 195 element-101 — and the `123`/`124`/`125`/`120`/`121` entries are
emitters and trails, which this viewer does not draw as cards in the first place,
or draws as particles (`120`-`125`) whose own warp handling is not yet touched.

**Implemented.** `Player::emit_sprite` now takes the frame's `Stats` and returns
before any geometry when `context.common.warp`, counting the element and logging it
on the decal channel with the `A3DDecalEx.cpp:1332` citation. The status line gained
`warp=` beside `dummies=` so a root that now draws nothing says why instead of
looking like a parse failure: `绿色传送门.gfx` is exactly that root — five elements,
three emitters, one trail, and a gate that is a warp element.

**What this retires.** Two of the three open reports were partly this key:

* the water gate `水门_瀑布.gfx` carries one warp card, a 0.85 x 0.85
  `normalmap\gas.dds` sheet with the 2/4 premultiplied blend, listed *third* of
  seven, sitting on top of the four real water cards. A grey `gas.dds` card over a
  scrolling curtain reads exactly as "a frozen white sheet" — §40 shows the curtain
  itself was scrolling the whole time;
* the portal `绿色传送门.gfx` reads as "the ring is missing, only particles" — and in
  retail there is no ring card either, because the gate is warp. The `123` element in
  that root, which an earlier note called the ring, reads `Warp: 0` on a per-element
  rescan and is `轨迹\风.dds`, a trail;

and in the new-race map set the warp elements are consistently *ripple* rings over
water rather than visible geometry — `水片长.gfx` is 6 scrolling `210` surface sheets
plus 7 warp `normalmap\水漩涡.dds` cards at 3–4 units, `水片圆.gfx` is 5 sheets plus 2.
Those rings were being painted over the water as opaque-ish grey discs.

## 40. The waterfall draws four scrolling cards and one warp veil, and the sheet's alpha was never the problem

`场景/新种族地图/水门_瀑布.gfx`, read out of the pack rather than assumed:

| # | id | texture | size | blend | tile | warp |
|---|----|---------|------|-------|------|------|
| 1 | 100 | `纹理\气连续03.tga` | 0.9 x 1.25 | 2/4 | 1, `TexRow 2 / TexCol 3`, `TexSpeed (0, -1.5)` | no |
| 2 | 100 | `纹理\流动（通道）.dds` | — | 2/4 | yes | no |
| 3 | 100 | `normalmap\gas.dds` | 0.85 x 0.85 | 2/4 | — | **yes** |
| 4 | 100 | `物体\humianbowen.dds` | — | 2/4 | — | no |
| 5,6 | 124 | `多个粒子\shuizhu01.dds`, `烟火\Effects_Textures_1184-1.bmp` | — | emitters | — | no |
| 7 | 100 | `烟火\沙.tga` | — | — | yes, V -1.8 | no |

The sheet the first card samples is `gfx/textures/纹理/气连续03.tga` in the same
archive: 223 x 512, TGA type 2, 32 bits per pixel, 8 alpha bits, **221 distinct
alpha values** spanning below 96 and above 160. It is a real alpha sheet with grey
RGB, and the `2/4` blend (One / OneMinusSrcColor) is the grey-premultiplied pair the
client authors for it. Nothing was dropped at load time; §15's "did the loader lose
the alpha" question is answered by the file, not by inference.

What the report saw was therefore (a) the warp `gas.dds` card of §39 and (b) the
fact that `TileMode` sheets scroll instead of stepping (§10.3) — and the scrolling is
now pinned against the shipped file, not against a fixture: V advances by
`TexSpeed.y * dt` = -0.75 per 500 ms, and the UV window is `TexCol x TexRow` = 3 x 2
texture widths.

## 41. The sun beams: the shipped shafts are axial, and the scene corpus has no billboards at all

`场景/树林光束.gfx` is eight element-100 cards of 0.5 x 10 under `DedaultScale: 0.7`,
every one `RotFromView: 1`, staggered by their own `StartTime`s (0, 500, 875, 1000,
1500, 1750, 2025, 2450 ms) and each holding **two keypoints with
`InterpolateMode: 0`**: the first tilted — `(-0.5, -0.5, -0.5, 0.5)`, a 120 degree turn
about -(1,1,1), for the first pair; `(-0.707, 0, 0, 0.707)`, a quarter turn about -X, for
the rest — and the second the identity. Both of those turns carry `_unit_z` onto **Y**,
so a shaft is the placement's Y column while its element is in the tilted keypoint and
its Z column once the identity takes over; which one it is depends on the element's own
`StartTime` and 875 ms span. The new corpus test builds both axes from the parsed
keypoints, checks they are exactly **two** distinct directions, then requires every
card's height (corner 2 minus corner 0) to be parallel to one of them and its width
square to it. A transposed parent — the bug "sun rays point in random directions"
turned out to be — puts both somewhere else and fails.

The test sweeps 3.2 s in 50 ms steps rather than sampling one instant, and that is
not caution for its own sake: ctrl 105 on every shaft is a `ColorDelta` of ±150 per
second on the keypoint alpha (`A3DGFXElement`'s colour offset, folded into the
rendered alpha), the eight `StartTime`s span 2.45 s and each element loops every
2.875 s, so *no* single frame has all eight cards with any margin — a first version
that asserted "eight quads at 2600 ms" was one fade rate away from being wrong. The
sweep checks the axis law on every card the root ever draws and proves all eight shafts
rendered at least once by their centres.

**The pending billboard fix is not on this path, and barely on any path.**
`FaceToViewer` is a version-39+ root key and only **24 of 5,459 roots** set it to 1;
none of them is a scene root — they are hit effects (`策划联入/怪物技能/击中/*`,
`09人鱼击中`) and status effects (`光芒之盾`, `冻结`, `魔气吞山河`). The whole
new-race map set (179 roots under `gfx/场景/新种族地图/`) sets it **zero** times, and
`树林光束`/`天煞光束` are versions 30 and 32, so their shafts never take the billboard
branch either. `QuadBasis::Billboard` staying screen-aligned while the client's
`Update_Billboard` builds an axial card is a real divergence, but it is a 24-root
divergence in combat effects — it is not the cause of any of the three scene reports,
and it is scheduled separately rather than mixed into this change.

## 42. City of Raging Tides: what the effect pack can and cannot enumerate

`怒潮` does not appear in a single archive path. `gfx.pck` holds effect roots and
their textures, not world placement: the map's per-object list lives in the world
data (`.ecwld` and the model/surface packs), which is not in this archive, so the
city cannot be enumerated object by object from here. What the pack does hold is the
map's effect set, and it is unambiguous: `gfx/场景/新种族地图/` with 179 roots,
including the water gate of §40, `树林光束`'s sibling `树林日光一道.gfx`, the
`能量喷泉{,3,4,5}` family, `主城光球大/小`, `完美塔*`, `云*`, `场景鱼群*`,
`塞亚岛传送门`, and the underwater families the city shares with the rest of
`gfx/场景/` (`瀑布水花*`, `海底*`, `泉水高亮*`, `水柱`, `海洋光束`, `龙宫入口`,
`喷泉{,水花2}`).

Counts over those 179 roots, which is the useful part — it says how much of the city
each of this task's fixes touches:

| class | roots |
|---|---|
| `FaceToViewer: 1` | 0 |
| `RotFromView: 1` on at least one element | 27 |
| `TileMode: 1` on at least one element | 41 |
| `Warp: 1` on at least one element (30 elements total) | 14 |

the 14 warp roots being `主城光球小` (4), `水片长` (8), `能量喷泉3` (3), `水片圆` (2),
`能量喷泉` (2), `能量喷泉4` (2), `能量喷泉5` (2), and one each in `塞亚岛传送门`,
`水门_瀑布`, `白兰树`, `赤水窄巷洞穴海螺岩_瀑布`, `远完美浮空城1_上边旋转`,
`远完美浮空城1_旋转涡轮`, `雨子效果`.

**One CI ordering point, in the opposite direction from §38.** §38 moved the
formatting check ahead of the Vulkan installs so a broken driver could not hide
rustfmt's opinion. That put it ahead of the *unit tests* too, and the first run of
this change proved the cost: `Check formatting` failed on four of the new asserts and
steps 10-17 were skipped, so a change whose tests had never compiled came back as a
formatting note. The check now sits after *Run viewer unit tests* and before the
Vulkan installs: still un-gated by the GPU, but no longer able to hide a compile or a
test failure behind a line break.

## 43. The corpus tests get a gate, so a skip stops looking like a pass

Sections 39-42 added four tests that read the retail archive: the waterfall sheet's
alpha, the water gate's scroll and its warp veil, the forest's beam axes, and the
portal gate. None of them has been executed. They cannot be: CI stages no
`gfx.pck`, and the archive is 97 MB, so it is not in the repository. Every run of
this suite therefore prints four `SKIPPED` lines and passes, which is
indistinguishable from four passing assertions — the exact failure mode the helper
in `gfx_player` already warned about in a comment ("a corpus test that silently skips
is indistinguishable from one that passes, and in this session that is exactly how
two wrong fixes came to look verified").

That has been true of seven tests in six modules for as long as the archives have
been out of the repository, and it is why this change is a mechanism rather than a
paragraph. `src/pck.rs` is now the single place where a corpus test decides it
cannot run:

```rust
pub const CORPUS_ENV: &str = "PWE_CORPUS";            // `require` is the gate
pub fn corpus_required() -> bool;                     // over the process env
pub fn corpus_required_with(Option<&str>) -> bool;    // the same rule, testable
pub fn corpus_absent(what: &str);                     // skip line, or panic
pub fn corpus_absent_with(required: bool, what: &str); // with the decision made
```

It lives beside the archive reader rather than in a module of its own for a
mechanical reason worth recording: those seven tests sit in `gfx_catalog`,
`gfx_decal`, `gfx_owner`, `gfx_player` and `scene`, and 28 of the 69 development
binaries include one of those files with `#[path = "../scene.rs"] mod scene;` — a
whole second source tree per binary, each with its own crate root. A new module would
have had to be declared in every one of those crates; `pck` is the one module that
26 of the 28 already include, so the gate needs two new declarations in total
(`grass-type-survey` and `scene-model-light-survey`, which include `scene` but not
`pck`). Every item in the gate is `#[cfg(test)]`, so no release build or
`cargo check --bins` carries any of it.

The seven skip sites were `gfx_catalog::presets_resolve_in_the_archive`,
`gfx_decal::authentic_static_weapon_decals_cover_the_audited_root_set`,
`gfx_owner::corpus_waterfall_plays_from_its_authored_placement`,
`scene::corpus_effect_records_agree_on_one_layout`, and three in `gfx_player`
(`corpus_root`, the potted crystal, and this task's own `open_pack` helper, which
carries the four new tests). All seven now call `crate::corpus::absent(...)` and so
all seven honour the switch. The module is `#[cfg(test)]`, so a release build carries
none of it.

**The switch is all-or-nothing on purpose.** `PWE_CORPUS=require cargo test --bin
perfect-world-explorer` fails on the first archive it cannot find and names it
(``PWE_CORPUS=require, but this machine has no `google_drive/gfx.pck` ``). A
per-archive opt-in would let a machine claim the gate while silently testing a
fraction of the corpus, which is the same confusion at a smaller scale. The value is
read case-insensitively and only `require` turns it on; anything else, including
nothing, keeps today's behaviour so CI stays green. Three unit tests cover the rule
itself: only `require` turns it on, the gate panics with the path in the message, and
the ordinary configuration prints and returns.

**What it is for.** The four assertions of §39-§41 rest on hand computation against
the archive — the alpha histogram of `气连续03.tga`, `TexSpeed`'s −0.75 per 500 ms,
the two keypoint axes of `树林光束`, the 2.5-unit warp gate of `绿色传送门` — and
one of them, "eight quads at 2600 ms", was already wrong when it was written and was
caught only because the fade had to be computed by hand. A machine that stages
`google_drive/` can now settle all four in one command instead of trusting the
prose; and a future change that breaks one of them fails there instead of discovering
it a session later.

**What is still not proven.** The gate proves the assertions run; it does not prove
the viewer *looks* like the client, and it cannot be run here — this sandbox has no
archive and no route to one. The numbers in §39-§41 remain hand-checked arithmetic
against a pack that was read, not a test run.

## 44. The decal cartel: three builders, one root frame, and the sunray's missing sign

**The element id picks the vertex builder, and nothing else does.** `A3DDecalEx::Update`
(A3DDecalEx.cpp:301-321) is a switch on `m_nEleType`:

```cpp
switch (m_nEleType) {
case ID_ELE_TYPE_DECAL_3D:        return Update_3D(kp, pView, pVerts, nMatrixIndex);
case ID_ELE_TYPE_DECAL_2D:        return Update_2D(kp, pView, pVerts, nMatrixIndex);
case ID_ELE_TYPE_DECAL_BILLBOARD: return Update_Billboard(kp, pView, pVerts, nMatrixIndex);
}
```

and all three ids construct the *same* class — `CreateEmptyElement`
(A3DGFXElement.cpp:605-612) maps `DECAL_3D`, `DECAL_2D` and `DECAL_BILLBOARD` to
`new A3DDecalEx(pGfx, nEleId)` — so the class cannot dispatch on anything but the id it
was handed. The three builders are three different pieces of maths, and each reads a
different set of keys: `RotFromView` is read in `Fill_Verts_3D` only, `ZOffset` in
`Fill_Verts_2D` only, and `OrgPt` is honoured by 3D and 2D but ignored by
`Update_Billboard`.

Against the staged pack (`google_drive/gfx.pck`, 97.7 MB, `0x20002`, 10,104 entries,
5,459 `.gfx` roots) the three ids split **3,542 / 5,244 / 83**, with the 83 billboard
records spread over 33 roots. Until this change `emit_sprite` sent every one of them
down the 3D builder and let `RotFromView` be the only branch, which is how the corpus's
larger half — the 5,244 element-101 cards, the ones every hit effect is made of — ended
up drawn as world-oriented quads.

**101 is drawn in screen space, at world size.** `Fill_Verts_2D`
(A3DDecalEx.cpp:640-760) projects the card's centre through `GetVPTM() * GetViewScale()`,
measures how many pixels one world unit of `camera->GetRight()` covers at that depth
(`fScale`), and emits four **pre-transformed** vertices:

```cpp
A3DVECTOR3 vCenterWorld = GetParentTM() * kp.m_vPos;
A3DVECTOR3 vZCenter = vCenterWorld - pView->GetCamera()->GetDir() * (m_fZOffset * m_pGfx->GetActualScale());
// project vZCenter, derive fScale from the projected camera right ...
verts[0] = A3DVECTOR3(-fWidth1,  fHeight2, vCenter.z);
verts[1] = A3DVECTOR3( fWidth2,  fHeight2, vCenter.z);
verts[2] = A3DVECTOR3(-fWidth1, -fHeight1, vCenter.z);
verts[3] = A3DVECTOR3( fWidth2, -fHeight1, vCenter.z);
```

with `fWidth1/2 = m_fWidth1/2 * kp.m_fScale` unless `NoWidthScale` (same for the
heights) and `m_pGfx->GetActualScale()` folded into `fScale` separately. Two things
follow that the viewer did not have: the card faces the camera **because it is built in
screen space**, not because a flag said so, and `ZOffset` is a push *toward the camera*
along `camera->GetDir()`, not an offset along the element's own Z. The board's 42
element-100 records that author a `ZOffset` get nothing from it — the client reads the
key in the 2D builder only — and the 694 element-101 records that author one get the
camera-space push.

**102 is two crossed quads, and this is where §41's "scheduled separately" divergence
was.** `Update_Billboard` (A3DDecalEx.cpp:1062-1170) is entered with `m_nRectNum = 2`
(set at :288) and writes eight vertices:

```cpp
float fWidth  = kp.m_fScale * m_fWidth;      // half sizes, no OrgPt split
float fHeight = kp.m_fScale * m_fHeight;
A3DMATRIX4 matWorld = GetTranMatrix(kp) * GetParentTM();
if (A3DCOLOR_GETALPHA(diffuse) < 5) return false;
pVerts[0..3] = (±fWidth, ±fHeight, 0);
pVerts[4..7] = (0, ±fHeight, ±fWidth);
```

one card in the element's XY plane and one in its ZY plane sharing the height axis, both
with the same UV rect and the same world matrix. The vertex order is
(top-left, top-right, bottom-left, bottom-right) for each card, which is the order
`push_quad` already takes. `FaceToViewer` and this id never meet: **0 of the 24**
`FaceToViewer` roots contain an element 102.

**`FaceToViewer` is the root's frame, rebuilt every tick — not a per-element billboard.**
`A3DGFXEx::Tick` (A3DGFXEx.cpp:1245-1258):

```cpp
A3DVECTOR3 vView = pCamera->GetPos() - GetPos();
m_fDistToCam = vView.Normalize();
if (m_bFaceToViewer) {
    m_matNoScale = _build_matrix(vView, m_vPos);
    m_qDir.ConvertFromMatrix(m_matNoScale);
    m_bTransDirty = true;
}
```

and `_build_matrix` (`a3dgfxkeypoint.h:396-416`) is a left-handed look-at with the world
up lane kept horizontal:

```cpp
if (fabs(vDir.y) > .999f) { vY = Normalize(vDir % _unit_x); vX = vY % vDir; }
else                     { vX = Normalize(_unit_y % vDir); vY = vDir % vX; }
Fill_TranMatrix(matTran, vX, vY, vDir, vPos);   // axes become matrix ROWS, pos row 3
```

so the effect's Z axis runs from the effect to the camera, its Y axis stays as close to
world up as the look-at allows, and `m_qDir` is re-derived from that matrix — the
authored orientation is **discarded every tick**, above every element of the root. That
is why the flag belongs in `advance_with_terrain`'s `base` (where it now rebuilds the
frame through `face_viewer_basis`) and not in the sprite path, where it used to select a
per-card billboard basis and conflated the root frame with element 101's shape.

The shipped census, over the same 5,459 roots: **24 roots set `FaceToViewer: 1`**,
carrying elements `{100: 23, 101: 198, 123: 11, 124: 74, 130: 4, 170: 10, 121: 3}`,
with **zero** `MatchGrnd`, `Warp` or `RotFromView` lines between them and **zero**
element 102. None of them is a scene root: they are the monster hit flashes
(`策划联入/怪物技能/击中/*`, `鳐鱼精*`, `深海纳加鱼人*`), `秋普攻_1`, the wizard rune
sub-effects, and three status effects (`光芒之盾`, `冻结`, `魔气吞山河`). Every
`gfx/场景/` beam root — `树林光束`, `天煞光束`, `树林日光一道/多道` — is version 30-73
with the flag at 0. **So none of the three scene reports of this task can have been the
billboard path**, and this change is scoped to say so rather than to claim a fix.

**The sunray corner order: §41's residual.** `Fill_Verts_3D`'s `m_bRotFromView` branch
(A3DDecalEx.cpp:357-413) is exact in the viewer's axis, but two details of the *extents*
were not:

```cpp
A3DVECTOR3 vView = vPos - pView->GetCamera()->GetPos();   // camera -> card
A3DVECTOR3 vUp   = CrossProduct(vView, vDir);
verts[0] = vPos - vDir * fHeight1 + vUp * fWidth2;   // (u, v) = (0, 1)
verts[1] = vPos + vDir * fHeight2 + vUp * fWidth2;   // (0, 0)
verts[2] = vPos - vDir * fHeight1 - vUp * fWidth1;   // (1, 1)
verts[3] = vPos + vDir * fHeight2 - vUp * fWidth1;   // (1, 0)
```

The width axis is `cross(world - camera, dir)`; the viewer used
`cross(camera - world, dir)`, the negation. And the `u = 0` edge takes `fWidth2` — the
**complement** of the `OrgPt` split — whereas the element-space branch twenty lines
earlier (A3DDecalEx.cpp:596-614) puts `-fWidth1` on the left. Reusing that split put
`fWidth1` where the client puts `fWidth2`: invisible on a centred card (the rectangle is
symmetric, so only the texture mirrored), and a real displacement on the 704 corpus
`OrgPt` lines whose pivot is off centre. `QuadBasis::Axial` now writes the four vertices
in the client's own terms with the client's sign, so both cases agree with the source
instead of approximately agreeing with it.

**What is pinned now.** Three unit tests join §41's corpus sweep:

* `a_billboard_decal_is_two_crossed_cards` — element 102 draws **two** quads of
  `2 * Width * scale * DedaultScale` by `2 * Height * …`, both centred on the element
  position, sharing the world Y as their height axis and perpendicular in X and Z.
* `a_two_d_decal_faces_the_camera_and_offsets_toward_it` — a 101 under a pitched camera
  puts its width on `view.right` and its height on `view.up`, keeps the world extents, and
  moves its centre by `-view.forward * ZOffset * actualScale`.
* `a_face_to_viewer_root_turns_its_own_frame` — a `FaceToViewer: 1` root placed with a
  quarter turn about X still draws its 100 card with the plane normal on the
  effect-to-camera vector and its height on world up, i.e. the authored placement
  orientation is gone.

The 树林光束 sweep of §41 is untouched by the sign fix because it asserts the *line* of
each shaft's height against the two keypoint axes, not the sign of the width.

**What is still unproven.** This sandbox has no Rust toolchain (`cargo` and `rustc` are
both absent), so none of this was compiled or run here: the verdict is CI's `cargo check
--bins`, the unit suite and `cargo fmt --check`. The four corpus assertions still need
an archive set this machine does not have beyond `gfx.pck` — and the arithmetic in
§39-§44 remains hand-checked against sources that were read, not a green run.

**The first run of this change caught one of its own errors, which is the argument for
writing the test first.** The 101 test failed with

```
the card is pushed toward the camera: Vec3(2.0, 6.0, 13.4641) vs Vec3(2.0, 6.0, 9.4641)
```

— four units of `Z` past where the camera-space push should have put it. `quad_corners`
was still being handed `(0, 0, ZOffset)` as the card's centre, so a 2D card took the
client's push **and** an invented element-space one, and the discrepancy was exactly
`ZOffset * actualScale` along the world axis. The centre is the element origin for every
builder now: no builder in the client pushes a card along the element's own Z. The first
run compiled and ran 1,085 other tests green, which also confirms the `FaceToViewer`
frame, the crossed cards and the axial corner order compile and behave; `48ad6ee` is the
run where both jobs — `Rust checks and tests (Linux)` and `Windows executable (x64)` —
finish green.

§41's closing paragraph said the billboard divergence was "scheduled separately". This
section is that change: it is closed against the same census, with the two mechanisms
kept apart — element 102 is `Update_Billboard`'s crossed pair, and root `FaceToViewer` is
`A3DGFXEx::Tick`'s rebuilt frame.

## 45. A key-by-key audit against the client's own serialisers, and three rules it turned up

**Method.** The three classes whose source is staged (`A3DGFXEx.cpp`, `A3DGFXElement.cpp`,
`A3DDecalEx.cpp`) each declare their `.gfx` line formats as `_format_*` / `_gfx_*` string
constants and read them back with `sscanf`. Extracting those literals gives the complete
key set each class can write, which is the only census that does not depend on the
corpus: **22 root keys, 58 element keys, 15 decal keys**. Diffing them against the 205
key literals `gfx_file.rs` looks up leaves **23 keys we never read** (and 146 literals we
read that these three serialisers do not write, which belong to the element classes whose
source is not staged — emitters, trails, models).

The 23 sort into four groups, and only one of them is a render rule:

| group | keys | why it is not a geometry divergence |
|---|---|---|
| audio | `AbsoluteVolume`, `Volume`, `VolMin`, `VolMax`, `SoundVer`, `SilentHeader`, `CanDoFadeOut`, `IsLoop`, `FixSpeed`, `PSInterval`, `PSLoopCount` | `A3DGFXSoundEx`'s job; the viewer plays no effect audio at all |
| shader | `ShaderFile`, `ShaderTex`, `PSConstCount`, `PSConstIndex`, `PSConstValue`, `PSFileVersion`, `PSTargetCount` | a custom pixel shader and its constants; no shader source is staged and the viewer draws with one fixed blend |
| editor / AABB | `IsAngelica3`, `Version`, `AccurateAABB` | editor metadata and the precise-AABB cull, which the viewer's frustum test already answers |
| camera shake / emitters | `ShakeDamp`, `ShakeDir`, `PitchMin`, `PitchMax` | the shake *type* is implemented (`GenerateDirection`, the attenuation and the two-frame flip of `A3DGFXEx.cpp:1427-1450`); damp and direction refine the amplitude and are open, as are the emitters' pitch limits |

`MatchSurface` and `SurfaceUseParentDir` deserve their own line because they are not a
card at all. `A3DDecalEx::Update` (A3DDecalEx.cpp:1215-1300) sends a `MatchSurface: 1`
element down `AfxGetGFXExMan()->GetSurfaceRender().Render(...)`, which stamps the texture
onto the **world geometry** with a radius clamped to 15 units — it needs the collision
surface, not the effect pack. The viewer draws such an element as a flat card, which is a
known divergence rather than an unread key.

**The rule that was parsed and unused: `YawEffect`.** It is read from version 61
(`gfx_file.rs` keeps it as `Sprite::yaw_effect`) and nothing consumed it. The ground
branch spells out what it does (A3DDecalEx.cpp:460-486):

```cpp
A3DQUATERNION quad = GetParentDir() * kp.m_vDir;
A3DVECTOR3 vNewNorm = quad * _unit_z;          // the sheet's own normal
float fAngle = DotProduct(vNewNorm, _unit_y);
if (fabs(fAngle) <= _grnd_delta) {
    AxisAngleToQuad(CrossProduct(vNewNorm, _unit_y), acosf(fAngle), quad2);
    quad_mix = quad2 * quad;                   // normal onto world up
} else quad_mix = quad;
quad_mix.Normalize(); quad_mix.ConvertToMatrix(matTran);
matTran.SetRow(3, vCenter);
...
A3DVECTOR3 vPos = matTran * A3DVECTOR3(fCol, fRow, 0);
vert.x = vPos.x; vert.z = vPos.z;
```

so with the key the lattice is the **element's own XY plane**, turned until its normal
points up, with only x and z read back — the sheet's yaw on the ground is the element's.
Without it the lattice is world aligned and the placement's rotation is thrown away.
`AxisAngleToQuad` (A3DQuaternion.cpp:168-181) is the shortest arc built by hand and does
**not** normalise its axis, which is why the branch is gated on the normal being off the Y
axis: a parallel normal gives a zero axis and `cos(0) = 1` (harmless), the antiparallel
one would normalise zero. `drape_sprite` now applies `Quat::from_rotation_arc(normal,
Y) * placement_rotation` and lays `(fCol, fRow, 0)` through it, which is the same rotation
wherever the client's quirk is numerically defined.

**`UReverse`, `VReverse` and `UVExchg` are endpoint swaps, not reflections.** All three
builders spell them out identically (A3DDecalEx.cpp:556-576, :1110-1130, and the 2D
branch's `fU`/`fV` arrays):

```cpp
if (m_bUReverse) { gfx_swap(fU[0], fU[1]); gfx_swap(fU[2], fU[3]); }
if (m_bVReverse) { gfx_swap(fV[0], fV[2]); gfx_swap(fV[1], fV[3]); }
if (m_bUVInterchange) { gfx_swap(fU[0], fV[0]); ... }
```

The viewer wrote `u = 1 - u`, which is the same thing **only when the window is the whole
texture**. On a flipbook the window is a cell, so the two disagree about which cell the
card is in: `TexCol: 2` at clock 0 has its left corners on `u = 0` and `u = 0.5`, and
`1 - u` sent them to `1` and `0.5` — the cell in the other half of the atlas, mirrored.
`apply_uv_flips` now swaps the corners instead, which also keeps the terrain drape's
interpolated lattice identical to the client's `nCol - j` index reversal.

**`IsCloseToGrnd()` is an absolute distance.** `CalcParentTM` (A3DGFXEx.cpp:1617-1622)
tests `fabs(m_vPos.y - m_fGroundHeight) < _match_grnd_max_delta` with the 0.2 at :254; the
viewer had `origin.y - surface < 0.2`, so an owner more than 0.2 *below* the terrain
counted as lying on it and its sheet was draped under the ground instead of keeping the
flat card. Both call sites (`advance_with_terrain`'s sampler and `ground_frames`) take the
absolute value now.

**Three tests pin the three rules**: `reverse_keys_swap_the_cell_they_are_on` (a 2x2
atlas, cell 0, with `UReverse`/`VReverse`), `a_yaw_effect_sheet_lies_in_the_elements_own_plane`
(a 4x1 sheet under a quarter turn about Y keeps world alignment without the key and turns
its long axis onto world Z with it), and the drape suite covers the near-ground change
through `match_ground_alone_needs_the_root_on_the_ground`.

**Verdict.** `26fafff` — the commit after one rustfmt hunk, which the check posted as a
PR comment — is green on both jobs: `Rust checks and tests (Linux)` and `Windows
executable (x64)`. The first run of this change failed only on rustfmt, so the three
tests did compile and pass on the run before it, and the near-ground change did not
break the existing drape suite.

**Still unproven**: the corpus counts quoted above (24 `FaceToViewer` roots, 42
element-100 `ZOffset`s, 83 billboards, the 906 `MatchGrnd` elements) come from
`gfx.pck` when it is staged and are re-derivable with `tools/pck_lib.py`; this session's
snapshot dropped the staging directory, so they are the numbers measured when it was
present rather than a fresh count.

## 46. `NoChangeDir` keeps the frame the root was built with, and the corpus agrees it is 81 roots

`NoChangeDir` was parsed from the first audit and never read. The client's rule is
three lines long and settles what the flag means:

* the constructor and `Reset` leave `m_matNoScale` at identity and
  `m_qDir` at `_unit_dir` (`A3DGFXEx.cpp:332`, `:26`);
* `SetParentTM` copies **only** the translation row and skips the three basis rows
  when the flag is set (`A3DGFXEx.cpp:1592-1598`);
* `SetDirAndUp` returns before touching the basis at all (`A3DGFXEx.cpp:1576`).

So a `NoChangeDir` root ignores the placement's rotation entirely and keeps the
world axes; only the position comes from the placement. `advance_with_terrain`
now builds that frame (`Mat4::from_cols_array_2d` with an identity basis and the
placement's translation) between the `FaceToViewer` branch and the normal
`normalize_basis` path, so the precedence matches the client: the parent frame is
built first, then a `FaceToViewer` element rotates against the camera, and a root
with both ends up facing the viewer either way. The status line reports the flag
next to `face_to_viewer`.

`a_no_change_dir_root_ignores_the_placement_rotation` pins it: a sprite placed
under a quarter turn about X keeps its card in the world XY plane, normal on Z.

**The count.** 81 of 5,456 roots author `NoChangeDir: 1`; `UseAABB` 3,490,
`ShakeCam` 241, `Raytrace` 109, `FadeByDist` 58, `FaceToViewer` 24,
`2DBackLayer` 12, `2DRender` 3, `PhysExist` 0. `FadeByDist`, `ShakeCam` and
`FaceToViewer` are read; the remaining two root flags are the 2D pass and neither
root count reaches five.

**Verdict.** Run
[`43112394747`](https://github.com/kasperfriend/Perfect-World-Explorer/actions/runs/43112394747)
is green on both jobs at `2a06d26`: `Rust checks and tests (Linux)` (1,088 tests,
the new one included) and `Windows executable (x64)`. The first run of the change
failed the new test on its own fixture rather than on the rule — the root reader
takes `NoChangeDir`, `2DRender` and `2DBackLayer` as one group after the
controllers and before the element count (`src/gfx_file.rs:1276-1280`), so writing
`NoChangeDir` beside `Raytrace` produced *"expected FadeByDist, found
NoChangeDir"*. The group now goes where the serialiser puts it.

**The estimate it sits in.** This pass also finished the corpus-wide coverage
census that the earlier sections kept quoting in pieces: see
`Gfx-effects-parity-estimate-2026-11-09.md`, which measures 5,456 non-empty roots
and **31,034 elements** (not the 76,278 that §45's parenthetical quotes — that
figure is the 76,519 controller records) and puts class coverage at 99.2% of
roots and visual-key coverage at 86.0%. Its head item, `DummyEle` on 399 roots,
is the next thing to read.

## 47. The grid scroll was counted twice, and the city's water is mostly refraction

Three reports are still open — sunbeams with arbitrary orientation, still
waterfalls, effects too large in the City of Raging Tides — and §41 closed the
third of them on reasoning rather than on the renderer. This section reopens all
three against the files. One real bug fell out; two of the reports now have a
precise answer about what is authored and what the viewer refuses to draw.

### 47.1 The 210/211 grid is drawn, and the census said it was not

`emit_clock` routes `Spec::Grid` straight to `emit_grid` (`src/gfx_player.rs:3379`),
and `emit_grid` has built one quad per lattice cell since the waterfall fix. The
census nevertheless listed 210/211 under `PARSED_ONLY`, and the estimate in
`Gfx-effects-parity-estimate-2026-11-09.md` deducted 43 roots and 500 elements
because of it. That set was stale: `tools/gfx-parity-census.py` now has 210/211 in
`RENDERED` and an empty `PARSED_ONLY`, and the re-run measures

| | roots | elements |
|---|---|---|
| class reaches a draw call | **5,456 (100%)** | **31,034 (100%)** |
| faithful: every visual key read | 2,565 | 10,710 |
| visual: everything that shows on screen | 4,716 | 26,459 |

so the class layer has no hole at all, and the only open rule that still moves
the needle is `DummyEle` (399 roots) in the greedy table.

### 47.2 What a grid actually authors

The lattice is `wNumber` x `hNumber`, not `TexRow` x `TexCol`. The first element
of `场景/新种族地图/水片长.gfx`, read out of `gfx.pck`:

```
GFXELEMENTID: 210     TexFile: 粒子\通道粒子2.tga
TexRow: 1  TexCol: 1  TileMode: 1  TexSpeed: 0.000000 0.000000
wNumber: 5  hNumber: 6
```

Five by six *vertices* — four by five cells — with a 1x1 atlas. Earlier sections
quoted this record as "`TexRow 5/6 x TexCol 2`"; the 5 and the 6 are the lattice
counts and the atlas is not tiled at all. `src/gfx_file.rs:2012-2013` reads
`wNumber`/`hNumber` into `Grid::columns`/`rows`, and `emit_grid` spreads one
texture rect across that lattice exactly as `A3DGridDecalEx::_setUVCoord` does.
`TexRow`/`TexCol` keep their tile meaning (`m_fTexWid = (float)m_nTexCol`), which
is why the sheet is a repeat count and not a frame grid.

### 47.3 The bug: `TexSpeed` was applied twice to every tiled grid

`flipbook_uv`'s tile branch is the owner of the scroll — it is
`A3DGFXElement::TexAnimation`'s `if (m_bTileMode)` arm, `m_fTexU += m_fTexSpeed *
dt` — and it returns the *already scrolled* window. `emit_grid` then added
`tex_speed * clock` to that window a second time, so every grid that both tiles
and scrolls ran at exactly **twice** its authored rate. The corpus is almost
entirely that case:

| `TileMode` | authors `TexSpeed` | grids |
|---|---|---|
| 0 | no | 49 |
| 1 | no | 4 |
| 1 | **yes** | **163** |

163 of 216 grids. That is every curtain, every water sheet and every `云` lattice
in the game — the city's `水片圆`/`水片长` sheets included — flowing double speed.
The second scroll is gone (`emit_grid` now uses `flipbook_uv`'s rect as it is),
and no grid loses motion by the change: the 49 `TileMode: 0` grids author no
`TexSpeed`, and the client's `else` arm moves cells by `TexInterval`, never by
`TexSpeed`.

The test that covered this was itself part of the problem. Its fixture copied
`瀑布.gfx`'s 2x4 curtain but dropped `TileMode`, so the sheet only moved because
of the duplicate scroll, and a parser test that never writes the flag can only
confirm the wrong law. `gfx/场景/瀑布.gfx` authors `TileMode: 1`, `TexRow: 1`,
`TexCol: 1`, `TexSpeed: 0 / -0.4`, and the fixture now says so and asserts the
rate: one second of clock is `V = -0.4`, not `-0.8`.

### 47.4 What the city is actually made of

Every `.gfx` under `gfx/场景/新种族地图/` — 179 roots, 1,293 elements:

| id | class | elements | | id | class | elements |
|---|---|---|---|---|---|---|
| 123 | particle | 235 | | 124 | particle | 110 |
| 100 | decal | 220 | | 200 | child | 95 |
| 150 | fractal | 146 | | 160 | model | 54 |
| 210 | **grid** | 126 | | 125 | particle | 49 |
| 101 | decal (2D) | 120 | | 140 | cylinder | 41 |
| 110 | ribbon | 33 | | 120 | particle | 22 |
| 121 | particle | 19 | | 152 | trail | 12 |
| 190 | paraboloid | 10 | | 130 | light | 1 |

Of the 126 grids, **116 author a scroll and 10 author `TexSpeed 0`** — those ten
are static by authorship, not by omission. 30 elements across the map carry
`Warp: 1`.

### 47.5 Why the water can still read as still

`水片长.gfx` is 13 elements: **7 of them are `Warp: 1`** (`normalmap\水漩涡.dds`,
`Width`/`Height` 3-4, one authored 90 degrees off the others) and 6 are grids.
`水片圆.gfx` is 7 elements, **2 of them warp** (`normalmap\水波.dds`, 4 and 5
units). `水门_瀑布.gfx` is four decal cards plus one warp veil.

A warp element is a refraction source, not a card: the client registers it with
`AddWarpEle` and the distortion pass consumes it (`A3DDecalEx.cpp:1332`), which is
why the player counts it in `stats.warp` and draws nothing
(`src/gfx_player.rs:3431-3441`). The viewer has no distortion pass, so on these
roots the surface you see is the *grid lattice alone* — a still, evenly lit sheet
stretched over the authored vertices, with no animated veil moving over it. That
is the honest reason the city's water looks frozen even once the grid scroll is
correct: the motion the artists put on the surface lives in the seven warp
elements per long sheet that no pass samples.

Closing it is a distortion pass — sample the warp sheet's normal map, offset the
scene colour behind it by the animated amount, keep the depth — not a draw call
for the card. Drawing the warp element as an ordinary quad would put a grey
normal map on screen, which is why §41's decision to drop it stands.

### 47.6 New evidence: the placement files are in the archive

The `.ecwld` files that hold the actual effect placements are downloadable from
the same Drive folder (`a01`..`a17`, one per region block), and they parse with
the record layout `src/scene.rs` already implements — 60 bytes of id, scale,
position, direction, up, speed, valid time, alpha and attenuation, then a
length-prefixed GBK path. `tools/ecwld-effects.py` scans one:

```
   $ tools/ecwld-effects.py google_drive/a16.ecwld
   579 scale = 4.230 pos = (-192.77, 266.42, -464.33)
       dir = (0.841, 0.000, 0.540) up = (0.000, 1.000, 0.000)  场景\天空之城光束.gfx
```

Two things this settles for the open reports:

* the shaft's axis is the placement's Z, and the map author wrote it **horizontal**
  for `天空之城光束` (`0.841, 0, 0.540`) with world up as `up`. A beam card built
  along that Z is what the client draws too — the orientation is authored, not
  randomised by the viewer. Where the sunbeams still look wrong, the next test is
  the *card* basis (`RotFromView` / `GetParentDir`) against a placement like this
  one, not the placement reader.
* the authored scales are large and deliberately so: 4.23 for that beam, 11.6 for
  `灯台火_火焰`, 9.6 for `灯台冰_火焰`, 0.5-1.9 for candles and poison glows. An
  effect that looks oversized is most likely wearing one of these; the scale law
  itself is `DedaultScale x placement scale`, applied once (`A3DGFXEx.h:194-211`),
  and it is not doubled anywhere in the path.

### Verdict

One code change: a tiled grid now scrolls at the rate it authors, which is every
water sheet, curtain and cloud lattice in the pack. One census correction: class
coverage is 100% of 5,456 roots and 31,034 elements, not 99.2%, because the grid
class has been drawn all along. The two remaining reports are now narrower than
they were: "static water" in the city is the missing distortion pass over 30 warp
elements plus 10 grids that author no speed, and "random sunray direction" needs a
placement-level comparison of the card basis, which the `.ecwld` files now make
possible without the shipped `world.ecwld`.

## 48. The placements are on disk: shaft directions and effect sizes are authored numbers

§47.6 left one question open — whether the beam orientation and the "too large"
effects come from the map or from the viewer. Seven region placement files
(`a01`, `a11`, `a12`, `a14`, `a15`, `a16`, `a17`, 731 effect records in total)
answer it.

### 48.1 Every beam in those files points where its record points

| file | root | scale | position | direction | up |
|---|---|---|---|---|---|
| a01 | `场景\蓝色光束.gfx` | 1.882 | (-284.1, 10.4, 376.0) | (-0.238, 0, 0.971) | (0, 1, 0) |
| a01 | `场景\蓝色光束.gfx` | 1.882 | (241.4, 10.4, 376.0) | (-0.238, 0, 0.971) | (0, 1, 0) |
| a01 | `场景\蓝色光束.gfx` | 1.882 | (-272.5, 10.4, -86.7) | (-0.238, 0, 0.971) | (0, 1, 0) |
| a01 | `场景\蓝色光束.gfx` | 1.882 | (241.4, 10.4, -86.7) | (-0.238, 0, 0.971) | (0, 1, 0) |
| a14 | `场景\蓝色光束.gfx` | 3.091 | (-268.4, 320.9, 215.7) | (0, 0, 1) | (0, 1, 0) |
| a16 | `场景\天空之城光束.gfx` | 4.230 | (-192.8, 266.4, -464.3) | (0.841, 0, 0.540) | (0, 1, 0) |

The four a01 beams are the same asset at the four corners of a courtyard, written
with one identical direction; a14's stands straight along +Z; a16's leans 33
degrees in the ground plane, because the sky-city artist wrote it that way. A
`RotFromView` shaft runs along the placement's Z axis carried through the
keypoint's quaternion, so these cards lean exactly as the map says — the viewer
is not scattering them.

That law is now checked against the client's own arithmetic rather than its
prose. `A3DDecalEx.cpp:357-380` builds the card from

```c
vDir  = RotateVec(GetParentDir() * kp.m_vDir, _unit_z);
vView = vPos - camera->GetPos();
vUp   = CrossProduct(vView, vDir).Normalize();
verts[0] = vPos - vDir * fHeight1 + vUp * fWidth2;
verts[1] = vPos + vDir * fHeight2 + vUp * fWidth2;
verts[2] = vPos - vDir * fHeight1 - vUp * fWidth1;
verts[3] = vPos + vDir * fHeight2 - vUp * fWidth1;
```

and `QuadBasis::Axial` (`src/gfx_player.rs:4017-4096`) reproduces it term for
term: `(world - view.position).cross(dir)` is `vUp`, `plus`/`minus` are
`fWidth2`/`fWidth1` from the `OrgPt` split, `top`/`bottom` are `fHeight2`/
`fHeight1`, and the four corners are `verts[1]`, `verts[3]`, `verts[0]`,
`verts[2]` in that order — the commission of which matters because the client's
`u` runs backwards along `vUp`. Nothing in the branch normalises `vDir` (a
rotation preserves length) and nothing flips the cross product.

### 48.2 The sizes are authored too, and they are large

| statistic | value |
|---|---|
| placements | 731 |
| scale, minimum / median / maximum | 0.164 / **2.358** / 17.359 |
| scale >= 2 | 460 (63%) |
| scale >= 3 | 232 (32%) |
| scale >= 5 | 90 (12%) |
| scale >= 10 | 14 (2%) |
| `speed` values in use | 0.1 … 1.0 in ten steps |
| `alpha` below 1 | 68 |
| `valid_time` values | 0, 1, 2 |

A median placement runs an effect at **2.36x** its authored geometry and one in
eight at more than 5x, so a water sheet or a flame that looks oversized is
usually wearing the number the map wrote — `灯台火_火焰` at 11.6, `灯台冰_火焰`
at 9.6, `天空之城光束` at 4.2. The scale law itself is `DedaultScale x placement
scale`, applied once (`A3DGFXEx.h:197-211`), and §47 confirmed nothing doubles it.
Where an effect is oversized relative to its *neighbours*, the next check is the
record's own `scale` field, not the renderer — which is why
`tools/ecwld-effects.py` prints it next to position, direction and `up`.

### Verdict

Both remaining reports are now decisions the data makes, not gaps in the code:
the beams point where the map points them, and the effects are as large as the
map sizes them. What is left open in the city is the distortion pass over its 30
warp elements (§47.5), which is the one visible difference the renderer still
knows it has.

## 49. The controllers never restarted, and that is where the city's lost effects went

The reports that came with this pass — effects piled up in the upper city, oversized
effects everywhere, sunbeams at odd angles — arrived with live logs from four maps
(`Sunrays-2.log`, `Waterfall-2.log`, `CityOfRagingTides-3.log`, and the older
`Archosaur`/`RagingTides` captures). Those logs name the roots, the element ids and
the world-space quads, which is what finally made the first of them falsifiable.

### 49.1 The symptom, in numbers

`CityOfRagingTides-3.log`, `effect-frames`:

```
主城光球大.gfx id=123 ... origin=(2641.8,253.9,-4073.5)
主城光球大.gfx id=101 ... origin=(2641.8,258.6,-4073.5)
主城光球大.gfx id=100 ... origin=(2641.8,264.0,-4073.5)
主城光球大.gfx id=200 ... origin=(2641.8,550.6,-4073.5)
主城光球大_闪电紫.gfx id=150 ... origin=(2641.2,550.6,-4073.7)
主城光球大_闪电蓝.gfx id=150 ... origin=(2642.0,509.6,-4073.7)
```

Same x and z as the orb, 292 units above it. `主城光球大.gfx` puts that child at
`Position: 0, -0.740011, 0` with `GfxPath: 场景\新种族地图\主城光球大_闪电紫.gfx`,
so it belongs 4.4 units *below* the orb's centre, not 292 above it.

The element's controllers explain the number:

```
GFXELEMENTID: 200 ... RepeatCount: -1 ... TimeSpan: 2000
CtrlType: 100   StartTime 0  EndTime -1   Dir: 0, 1, 0   Vel: 0.700000   Acc: 0
```

`CtrlType: 100` sweeps the element along +Y at 0.7 units per second inside a
two-second loop. The orb's root clock at capture was 70,960 ms; 0.7 x 70.96 = 49.7
local units, and the root's `DedaultScale` of 6.0 puts that at 298 world units —
the 292 measured. The sibling with `Vel: 0.6`-class sweep sits at the same kind of
height. Nothing was mis-placed: the sweep had simply been running for seventy
seconds without ever restarting.

### 49.2 The rule the player was missing

Controllers integrate against the *node age*, and the client restarts that age —
and every controller with it — on each `RepeatCount` loop. `A3DGFXKeyPointSet::
TickAnimation` reaches the end of its keypoint list and does
(`A3DGFXKeyPoint.cpp:1403-1410`)

```c
Resume(true);
m_pEle->ResumeLoop();
if (dwTotal) TickAnimation(dwSpan % dwTotal);
```

and `Resume` (`a3dgfxkeypoint.h:2085-2114`) clears exactly the state this player
was keeping:

```c
m_nActKeyPoint = -1;
m_ProcData.m_KPProcess = m_KeyPointArray[0]->m_KPOrg;
m_ProcData.m_vAxisOff.Clear();     // the accumulated offset
m_ProcData.m_fTimeSpan = 0;
m_ProcData.m_fScaleNoise = 0;
for (i...) m_KeyPointArray[i]->Resume();   // every controller's Resume()
```

`advance_element` seeded `CtrlState` only when the *keypoint index* changed. A
single-keypoint element whose `RepeatCount` is -1 or N therefore never reseeded,
and every integrator in `apply_controls` walked away from its authored value:

| controller | what accumulated |
|---|---|
| 100 motion | `state.offset` — the element drifts |
| 101 rotation | `state.rotation` — the element keeps spinning |
| 105 colour | `state.color_offset` — fades saturate and stay saturated |
| 106 path | `state.path_time` — the element leaves its path |
| 111 scale | `state.scale_extra` — the element grows to its clamp and stays |

**493 roots and 1,956 elements** hold a looping element with an integrating
controller (105: 1,346, 106: 1,236, 100: 915, 109: 435, 101: 368, 111: 132). Every
one of them had been drifting since the root spawned, which is what "misplaced
effects in the upper city" and "many oversized effects all over the world"
describe: the same root looks right for the first loop and wrong for every loop
after it, and how wrong depends only on how long the map has been loaded.

The fix tracks which *play* of the clock the state belongs to and reseeds on a
change, so a loop restarts its controllers exactly where the client's `Resume`
does. `a_looping_element_restarts_its_motion_controller` runs ten plays of a
half-second cycle and requires the sweep to stay inside one play's travel; before
the change the card walked 10 units off its start and kept going.

### 49.3 A second, smaller keypoint rule

The same read of `TickAnimation` settles how `InterpolateMode` gates controllers:

```c
if (m_KeyPointArray[i]->m_enumMode != A3DGFXKeyPoint::enumInterpNone) {
    ... Interpolate(dwSpan / m_KeyPointArray[i]->m_dwTimeSpan);
} else {
    m_KeyPointArray[i]->Process(m_ProcData, dwTickTime);   // runs m_CtrlLst
}
```

A keypoint's controllers are walked by `Process`, which only the `enumInterpNone`
(mode 0) branch calls; an interpolating keypoint goes through `Interpolate`, which
touches position, colour, scale, `Rad_2D` and — for mode 1 only — the direction via
`SLERPQuad`. The player ran `apply_controls` for every keypoint. The corpus writes
only modes 0 (36,456 keypoints) and 1 (141), never 2, and 39 of the mode-1
elements carry controllers, so the fix is small: controllers now run only where the
client runs them. The rest of `sample_keypoints` already matches this law, including
the fact that mode 2 would *not* slerp (that mode is unread by every shipped file,
so it stays as written).

### 49.4 The sunbeams: authored numbers, and why they look wrong

`树林光束.gfx` is eight cards of `Width: 0.500000` / `Height: 10.000000` half
extents under `DedaultScale: 0.700000`, each with a `RepeatCount: -1`, two
keypoints (`TimeSpan: 875` then `2000`) and `InterpolateMode: 0`. The two keypoint
directions are `(-0.5, -0.5, -0.5, 0.5)` and identity, so a beam spends 875 ms of
every 2,875 ms rotated by that quaternion and then snaps straight — a snapped
pose, not a turned one, because the authored mode is `enumInterpNone`. The log
shows the result: `basis dir=(0.00,1.94,2.90)` on one card and a different pose on
its neighbour, which reads as "random direction" when eight of them share a tree.

The sizes are the placement's: `Sunrays-2.log` measures that card at
`3.48 x 69.78` units with its corners printed, i.e. exactly `0.5 x 20` doubled by
the root's 3.49 (0.7 x 4.98 placement) — the scale law of §47.2, applied once.
Across the captured maps the same root renders at 22.6, 40.0, 69.8, 87.0, 91.5,
114.1, 123.0 and 147.4 units long, one per placement scale: the map, not the
renderer, chooses. What the viewer *did* get wrong on those beams is alpha: their
only controller is `CtrlType: 105 ColorDelta`, so §49.1's missing restart left the
fade saturated — `color=0x07ffffff` in the frame log is a beam whose alpha had
decayed to 7/255 and stuck there instead of fading back each loop.

### 49.5 The waterfall is not an effect at all

`Waterfall-2.log` loads 39 subjects around the waterfall and not one is the sheet
you see falling: the roots are `瀑布水花`, `瀑布水花中间`, `瀑布水花底`,
`瀑布水花湍急` (spray), plus `树林光束`, `彩虹`, `烟1`, `海底飘尘2`, `树叶雨` and
`花瓣分飞`. The white curtain in the capture is scenery, not a `gfx` root — its
motion in retail comes from the material on the model, and this viewer's model path
has no animated UVs at all (`src/model_material.rs` carries no scroll term). That is
a modelling-source question — `models.pck` and the material format — not an effect
one, and it is the honest reason the waterfall still looks like a still image after
the grid scroll was corrected in §47.

### Verdict

One rule, two changes: a looping element restarts its controller state (and its
keypoint binding) on every play, and controllers run only on the step keypoints the
client runs them on. That is 493 roots and 1,956 elements no longer drifting, plus
the fade/scale/offset saturation that made city effects look misplaced, oversized
and stuck. The remaining reports are now specific: the beams' angles and lengths are
the map's numbers, and the waterfall's motion lives in a model material this viewer
does not animate yet.

## 50. What the client source settles, and what it does not

This pass went back to the 1.5.2 sources that were used for §47–§49 with the six open
field reports in hand, and read each one to its own line of code. Four of the six now
have a definite answer from the sources or from the archive; two do not, and this
section says exactly which artefact would decide them rather than pretending
otherwise.

### 50.1 Loops reset the controllers — confirmed, not inferred

`A3DGFXKeyPointSet::TickAnimation` (`A3DGFXKeyPoint.cpp:1362-1446`) and
`Resume(bool)` (`a3dgfxkeypoint.h:2085-2114`) are now read in full:

```c
m_dwTimeSpan += dwTickTime;
if (m_dwTimeSpan < m_dwTimeStart) return;
DWORD dwSpan = m_dwTimeSpan - m_dwTimeStart, dwTotal = 0;
for (i = 0; i < m_KeyPointArray.size(); i++) {
    if (m_KeyPointArray[i]->IsInfinite()) break;          // never wraps
    DWORD dwCur = m_KeyPointArray[i]->m_dwTimeSpan;
    if (dwSpan < dwCur) break;
    dwSpan -= dwCur; dwTotal += dwCur;                     // spans are cumulative
}
if (i == m_KeyPointArray.size()) {
    Resume(true);                                          // resets the node
    m_pEle->ResumeLoop();
    if (dwTotal) TickAnimation(dwSpan % dwTotal);          // carry the remainder
    return;
}
```

and inside `Resume(true)` the five things that matter:

```c
m_nActKeyPoint = -1;
m_dwTimeSpan   = m_dwTimeStart;              // the clock restarts at TimeStart
m_ProcData.m_KPProcess = m_KeyPointArray[0]->m_KPOrg;   // position/scale/colour
m_ProcData.m_vAxisOff.Clear();               // the accumulated motion offset
for (i) m_KeyPointArray[i]->Resume();        // every controller's own reset
```

Three consequences worth stating plainly, because they are what the viewer now does:

- a **loop** restarts the element's motion offset, rotation, colour delta, path time
  and scale — §49's fix is the client's own behaviour, and the 292-unit drift in the
  upper city was simply seventy seconds of a sweep that never restarted;
- a keypoint with `TimeSpan: -1` (`IsInfinite`) **never** wraps, so its controllers
  integrate for the element's whole life. That is why `主城光球大` element 200 (a
  2 s loop) drifted and `树林光束` (infinite) does not;
- the loop length is the **sum** of the keypoint spans, plus `RepeatDelay` while
  `m_bInDelay` holds. 782 of 31,034 elements author a non-zero `RepeatDelay`
  (334 of them `1` ms, then 500, 1000, 300, 2000). `element_cycle` already adds it.

`A3DGFXCtrlMove::Process` is the trapezoid this player implements:

```c
if (!Tick(ProcData.m_fTimeSpan)) return;                 // StartTime/EndTime window
float fDist = CalcDist(m_fVelOrg, m_fAcc, ProcData.m_fTimeSpan, fTimeSpan);
ProcData.m_KPProcess.m_vPos += m_vMoveDir * fDist;
ProcData.m_vAxisOff         += m_vMoveDir * fDist;
```

with `CalcDist(v, a, t0, dt) = 0.5 * (v0 + v0 + a*dt) * dt`, `v0 = v + a*t0`
(`A3DGFXKeyPoint.cpp:48-53`). Nothing integrates "the element's age"; every controller
integrates the interval it was handed, which is why the reset is the whole story.

### 50.2 The card size was never the card size

`effect-cards` printed its height as `|p3 - p0|`. The four corners are written in the
client's order — top-left, top-right, bottom-left, bottom-right — so `p3 - p0` is a
**diagonal**, and every card in every log has read `sqrt(2)` too tall whenever it is
square. `彩虹.gfx` shows the arithmetic: authored `Width 17.5`, `Height 8.0`, root
scale 6.69 → 234.15 x 107.04 world units; the channel printed `234.13x257.43`, i.e.
the diagonal of that rectangle.

That single character is where "square city cards draw at 0.707 while 2:1 cards draw
at 0.447" came from in the 9 November notes. It is not an aspect defect: `A3DDecalEx::
Fill_Verts_3D` writes `(-W1, +H2, 0) (+W2, +H2, 0) (-W1, -H1, 0) (+W2, -H1, 0)` with
`W1 = OrgPt.x * 2W`, `W2 = 2W - W1`, `H1 = OrgPt.y * 2H`, `H2 = 2H - H1`, and
`quad_corners` reproduces that expression exactly, pivot included. The channel now
prints `|p2 - p0|`, the true height.

### 50.3 `Width`/`Height` are half extents, and `DedaultScale` is not the dog

`A3DDecalEx::SetSize`/`ID_GFXOP_DECAL_WIDTH` (`A3DDecalEx.h:167-177`, `:125`) double
the authored number, so a `0.5 x 10` shaft is `1 x 20` before any scale. Checked over
the archive: `DedaultScale` has median **1.000**, and **3,881 of 5,457 roots write
exactly 1.0** (99th percentile 3.0, 574 above 1, 1,002 below). So however the world
loader applies a placement — `SetDefScale(s)` replacing the file's default or
`SetScale(s)` multiplying it — the two readings can differ by at most the root's own
`DedaultScale`, which is 1.0 for seventy-one percent of the archive. **A wrong reading
there cannot produce "every effect in every map is too big"**; the only multiplier
that varies per placement is the `world.ecwld` scale itself.

### 50.4 Element 101 is a screen-space slot, and its size still agrees

`A3DDecalEx::Play` registers a `ID_ELE_TYPE_DECAL_2D` element with
`A3DFVF_A3DTLVERTEX` (`A3DDecalEx.cpp:1355-1370`) — a transformed-vertex slot, so a
101 is drawn in pixels, not metres. `Fill_Verts_2D` (`:714-760`) multiplies the
extents by `fScale`, the pixel height of one world unit at that depth:

```c
A3DVECTOR3 vExts = vCenterWorld + pCamera->GetRight();
vExts = matVPS * vExts;
float fScale = (vExts.x - vCenter.x) * m_pGfx->GetActualScale();
fWidth1 = m_fWidth1 * kp.m_fScale * fScale;
```

Because `fScale` cancels between the two representations, a 101 covers the **same
world footprint** as the 3D card of the same extents would. A 101 is not "a pixel
size read as metres": `白兰树.gfx`'s `Width 60 / Height 50` flare is a 120 x 100 unit
card in retail too. What is 2D-only about it is the depth handling — `ZOffset` moves
it `m_fZOffset * actualScale` along the camera's own direction before projecting, and
`rhw` carries the projected depth. `gfx_player` already does the first
(`gfx_player.rs:3697`); the second is what a world billboard gives for free.

`TileMode` is likewise a slot flag, not a flipbook:
`RegisterSlot(..., m_bTileMode, m_bTexNoDownSample, ...)`, and
`A3DGFXElement::CalcTexAnimation` sets `m_fTexWid = TexCol`, `m_fTexHei = TexRow`
for it, i.e. a wrap-addressed window of `TexCol x TexRow` repeats. 197 roots and 579
elements set it, and the effect pass samples with `AddressMode::Repeat`
(`gfx_effect_gpu.rs:252`), so the water-gate sheets do tile and scroll.

### 50.5 Still open, with the artefact that would close each

- **Sunbeam direction.** `树林光束.gfx` has `RotFromView: 1`, so the shaft axis is
  `RotateVec(GetParentDir() * kp.m_vDir, _unit_z)` — the placement's Z after the
  keypoint quaternion. The first keypoint's quaternion is `(-0.5, -0.5, -0.5, 0.5)`,
  a 120-degree turn, held for 875 ms of a 2,875 ms loop, and `InterpolateMode: 0`
  means it *snaps* rather than turning. The `Shaft` selector in `quad_corners` is a
  debug knob over that one line and must be read only in the `EffectShaft::Client`
  setting. What is *not* yet established is whether the map's `direction` reaches the
  client as `GetParentDir()` unchanged or is re-orthogonalised against `up` first;
  `A3DGFXEx.h` (`SetDefScale`/`SetDir`) plus the scene loader's call site would
  decide it, and neither is in the supplied sources.
- **Effect scale.** Same artefact, same question: whether the ecwld record's scale
  arrives through `SetScale` or `SetDefScale`. The archive cannot decide it, because
  the two differ only where a root writes a `DedaultScale` other than 1 (`50.3`).
- **The waterfall sheet.** `瀑布.gfx` and `水门_瀑布.gfx` are byte-identical 8,034-byte
  files: five element-100 cards of `0.9 x 1.25` and `0.85 x 1.25` half extents with
  `TexRow 2`/`TexCol 3`, `TileMode: 1` and `TexSpeed` V of -1.0 to -2.0, plus two
  element-124 droplet emitters. They are 1.8-2.5 units wide, so they are the gate's
  water, not the curtain in the report. The curtain is scenery — `瀑布.smd`, whose
  `SKI` material carries no UV speed and whose ECM has no animation — so a static
  sheet is what the *files* describe. Whether retail scrolls it in a pixel shader is
  the one question here that needs a shader dump rather than a data file.

## 51. The placement contract is closed: the client's own basis builder, record and setters

§50 left two questions open because the artefacts behind them were not in the supplied
tree — whether the ecwld `direction` reaches `GetParentDir()` unchanged, and whether the
record's scale arrives through `SetScale` or `SetDefScale`. Both are now answered from the
client's own code, and the answer closes the *whole* placement pipeline, not just those two
lines. Four files were needed: `A3DFuncs.h`/`A3DFuncs.cpp` for the basis builder,
`A3DMatrix.cpp` for the multiply convention, and `EC_WorldFile.h` for the record. Each is
quoted verbatim below. (`A3DGFXElement.h` was fetched for `SetSize` as well and does not
have it — see 51.6.)

### 51.1 `a3d_TransformMatrix` builds the frame, and the frame is `(up x dir, up, dir)`

`A3DFuncs.h:61` declares it; the body is `A3DFuncs.cpp:299-327`:

```c
A3DMATRIX4 a3d_TransformMatrix(const A3DVECTOR3& vecDir, const A3DVECTOR3& vecUp, const A3DVECTOR3& vecPos)
{
    vecZAxis = Normalize(vecDir);
    vecYAxis = Normalize(vecUp);
    vecXAxis = Normalize(CrossProduct(vecYAxis, vecZAxis));
    ...
    mat.m[0][0..2] = vecXAxis;      mat.m[1][0..2] = vecYAxis;
    mat.m[2][0..2] = vecZAxis;      mat.m[3][0..2] = vecPos;   mat.m[3][3] = 1;
}
```

That is `src/gfx_owner.rs::placement_transform` line for line, including the detail that
`vecUp` is **normalized and not orthogonalised against `dir`**: a record whose `up` is not
perpendicular to its `direction` produces a sheared, non-orthonormal basis, and the viewer
reproduces that reading rather than inventing a Gram-Schmidt step. The shipped corpus never
exercises the shear — `scene.rs`'s layout test finds the two vectors unit length *and*
perpendicular on 2,275 of 2,275 records of `world.ecwld`, which is also the strongest
independent confirmation that the 60-byte layout in 51.2 is the right one.

The rows are the axes and the third row is the position, which is only true under the
row-vector convention. `A3DMatrix.cpp:68-80` settles that too: **both** `v * mat` and
`mat * v` for `A3DMATRIX3` are the same expression, and both `A3DMATRIX4` operators
(`:121-146`) delegate to `D3DXVec3TransformCoord`, whose row-vector form is
`out = v.x*row0 + v.y*row1 + v.z*row2 + row3`. So `world = local * M` with the axes on the
rows — which is exactly a column-major `Mat4` whose columns are `(right, up, forward)` and
whose `w` column is the position, i.e. `placement.transform_point3(local)`.

Consequence for §50.5's first bullet: `SetParentTM` reads rows 0/1/2, normalizes each and
converts the result to `m_qDir` (`A3DGFXEx.cpp:1587-1602`), and no code in that path ever
re-derives `up` from a world up vector. **The ecwld `direction` reaches `GetParentDir()`
unchanged**, and the 120-degree keypoint in `树林光束.gfx` is applied to it as authored.

### 51.2 The record, verbatim, and the two setters that read it

`EC_WorldFile.h:238-249` (the `>= 12` layout every shipped world uses):

```c
struct ECWDFILEEFFECT12 {
    DWORD dwExportID;      // 0
    float fScale;          // 4
    float vPos[3];         // 8
    float vDir[3];         // 20
    float vUp[3];          // 32
    float fSpeed;          // 44
    int   iValidTime;      // 48  0 daylight, 1 night, 2 both
    float fAlpha;          // 52
    bool  bAttenuation;    // 56  -> padded to 60
};
```

`src/scene.rs:100` reads `valid_time` at 48 and alpha at 52 off a 60-byte record: correct
for this layout. `EC_ManOrnament.cpp:844-960` then does, in this order,
`pNode->fScale = Data.fScale` → `pGFX->SetScale(pNode->fScale)`,
`pNode->fSpeed = Data.fSpeed` → `pGFX->SetPlaySpeed(...)`,
`a3d_TransformMatrix(pNode->vDir, pNode->vUp, pNode->vPos)` → `pGFX->SetParentTM(mat)`.
With `CalcActualScale` (`A3DGFXEx.h:197-211`) the card therefore carries
**`Width x Height x kpScale x DedaultScale x record fScale`** — a plain product, with no
replacement path anywhere, which is what `gfx_decal.rs:268` already computes
(`actual_scale = default_scale * event_scale`).

### 51.3 The arithmetic agrees on every axis of every field report

The log's `w`/`h` are `|p1-p0|` and `|p2-p0|`, i.e. full extents, and the card is built from
half extents split at `OrgPt` (all three reported roots sit at the default 0.5), so
`extent = authored x DedaultScale x fScale x 2`. Solving for one unknown per root — the
map's `fScale` — from **both** axes independently:

| root | authored W x H | DedaultScale | drawn w x h | fScale from w | fScale from h |
|---|---|---|---|---|---|
| `场景/彩虹.gfx` | 17.5 x 8.0 | 3.0 | 127.67 x 58.4 | 1.2159 | 1.2167 |
| `场景/树林光束.gfx` | 0.5 x 10.0 | 0.7 | 3.48 x 69.7 | 4.971 | 4.979 |
| `新种族地图/白兰树.gfx` (101) | 60 x 50 | 1.0 | 93.34 x 77.7 | 0.778 | 0.777 |

Six axes, three roots, one scale each, agreeing to better than 0.2%. The composition
`Width x DedaultScale x fScale` therefore reproduces the geometry the viewer is drawing,
and **the "7.3x oversized" figure in §50.6 is not a viewer factor at all**: for the rainbow
it is `DedaultScale 3.0` times a map that placed the root at `fScale ~ 2.43`, and for the
forest shaft it is a map that placed a `0.5 x 10` sheet at `fScale ~ 5.0`. Those numbers are
what the `.ecwld` says, and retail multiplies them the same way.

### 51.4 Alpha and speed, in the same pass

`DefaultAlpha` is a multiplier, not a target: `gfx_decal.rs:269`'s
`actual_alpha = default_alpha * event_alpha`, matching the record's `fAlpha` reaching
`SetAlphaTransition(max_alpha * fAlpha)` with `max_alpha` forced to `1.0f`
(`EC_ManOrnament.cpp:957-966`). `树林光束.gfx` writes `DefaultAlpha: 0.5` — the forest
shafts are half-alpha additive cards, and the rainbow writes `PlaySpeed: 0.5`, which is the
file's default that `SetPlaySpeed(Data.fSpeed)` overrides per placement.

### 51.5 The one structural thing the client does that the viewer does not: it draws far less

The flag is already parsed — `Effect` carries `attenuation: bool` off `bytes[56]`
(`scene.rs:55`) — but nothing reads it, and `gfx_owner.rs:29` names the omission:
"the visual effect's alpha/attenuation fade over distance". `bAttenuation` is not cosmetic (`EC_ManOrnament.cpp:250-300`): each frame, a node whose
`bAttenuation` is set and whose distance to the **host player** exceeds
`CECSceneBlock::GetResLoadDists(SCNRES_EFFECT)` is faded out (`FadeGFX(node, false)`, a
3,000 ms alpha transition) and faded back in when the player returns. The distances are
`EC_SceneBlock.cpp:1109` — **60 units** in scheme 1 ("in game - near") and `:1133`
**120 units** in scheme 2. `ECWDFILEEFFECT`'s default for the flag is `true`
(`EC_ManOrnament.cpp:860`), and the same loop loads at most **one gfx per frame**
(`break` at `:275`), so a teleport streams effects in one per frame, each cross-fading.

The viewer draws every one of the map's roots at every distance. That is the largest
remaining difference in *what appears* (as opposed to where or how large): a grove that
retail shows as the two or three shafts standing within 60 units of the player is, in the
viewer, twenty-two shafts at once — including the ones the map placed at `fScale ~ 5` for
the far side of the valley. It does not change any single card's size, but it changes how
the field reads.

### 51.6 Still open

- **`SetSize`, and with it the absolute factor of two.** `m_fWidth1/m_fWidth2` are never
  computed in the client's `A3DDecalEx.cpp` — they are copied in the copy constructor and read
  in the vertex builders, and the only derivation is `SetSize(m_fWidth, m_fHeight)` in the
  constructor, a member of `A3DDecalEx` declared in `A3DDecalEx.h`, which is not in the
  archive. `Fill_Verts_3D` places the corners at `-fWidth1` / `+fWidth2`, so the card's
  total extent is `m_fWidth1 + m_fWidth2`; whether that sum is `Width` or `2 x Width`
  decides a uniform factor of two on every card. The viewer doubles, on the strength of the
  static-decal path (`gfx_decal.rs`, verified natively, `width = [w0, 2w - w0]`), and the
  table in 51.3 is consistent either way because the factor cancels. **The artefact that
  would close it is `A3DDecalEx.h`** — found, and it closes it: see §52.1.
- **Day/night and distance are modelled but not paired.** `iValidTime` reaches
  `effect_visible` (`scene.rs:114`) and the viewer has a time of day, but the 60/120 unit
  `bAttenuation` radius is not applied at all, and there is no `FadeGFX` cross-fade.
- **The `RotFromView` width axis is already the client's.** `Fill_Verts_3D`'s `m_bRotFromView`
  branch (`A3DDecalEx.cpp:357-413`) is `vDir = RotateVec(GetParentDir() * kp.m_vDir, _unit_z)`,
  `vUp = normalize(CrossProduct(vPos - camera.Pos, vDir))`, height along `vDir`,
  `verts[0..3] = vPos -/+ vDir*fHeight +- vUp*fWidth` — the `QuadBasis::Axial` arm of
  `quad_corners` (`gfx_player.rs:4068`) computes exactly these four corners from exactly
  those two axes, so a shaft's width never foreshortens and its length does. The vertical
  beam left in Archosaur is therefore a keypoint direction, not a basis error: see §50.5.

## 52. `SetSize` closes the last arithmetic question, and the distance fade is now drawn

### 52.1 The factor of two is the client's

51.6 ended on the one thing the archive could not settle: whether
`m_fWidth1 + m_fWidth2` is `Width` or `2 x Width`. `A3DDecalEx.h` was found and it settles
it in six lines (`A3DDecalEx.h:167-177`):

```c
void SetSize(float fWidth, float fHeight)
{
    m_fWidth = fWidth;  m_fHeight = fHeight;
    fWidth  *= 2.0f;    fHeight  *= 2.0f;
    m_fWidth1  = m_fOrgPtWidth  * fWidth;   m_fWidth2  = fWidth  - m_fWidth1;
    m_fHeight1 = m_fOrgPtHeight * fHeight;  m_fHeight2 = fHeight - m_fHeight1;
}
```

The `2.0f` is the client's, the `OrgPt` fraction splits the doubled extent, and
`GetProperty(ID_GFXOP_DECAL_WIDTH)` hands the editor `m_fWidth * 2.0f` again (`:124-125`).
So a card's world extent really is `2 x Width x kpScale x DedaultScale x fScale`, which is
what `quad_corners` builds (`gfx_player.rs:4026-4029`) and what the table in 51.3 assumed.

**The size pipeline is therefore closed end to end**: authored `Width`/`Height` halved into
the file's `DedaultScale`, multiplied by the map's `fScale`, doubled here. Every value in
the three rows of 51.3 is now derived rather than fitted, and there is no remaining
candidate for the field reports' "oversized" that is not the numbers those files carry.

### 52.2 `bAttenuation` is implemented, off by default

`CECOrnamentMan::Update` is the only consumer of the flag, and it is a fade, not a cull
(`EC_ManOrnament.cpp:274-292`): inside `SCNRES_EFFECT` → `FadeGFX(node, true)`, outside →
`FadeGFX(node, false)`, each a 3,000 ms `SetAlphaTransition` to `max_alpha * fAlpha` with
`max_alpha` pinned to `1.0f` (`:957`, `:966`). The node is **not** released while faded out;
only the time-of-day branch releases.

The viewer now carries that law:

- `world_models_gpu::Producer` keeps the record's flag and a `fade` value; a fresh root
  starts at 0 when it attenuates, which is where `LoadGFXFromFile` + `FadeGFX(true)` leave
  it, and at 1 otherwise.
- `apply_attenuation` walks the producers every tick, measures a **full 3D** distance from
  the eye (the client measures `a3d_Magnitude(vPos - vHostPos)`, not a ground distance),
  and steps `fade` toward 0 or 1 linearly at `delta_ms / 3000`, which is what
  `SetAlphaTransition` interpolates.
- `gfx_owner::Manager::set_attenuation` clamps and stores the number, and the root alpha
  becomes `record fAlpha * fade` at emit time. The owner stays loaded throughout.
- `View settings → World → Effect fade distance` (`effect_distance=`, 0..120 m, default
  **0 = off**) sets the radius. 60 is the client's near scheme
  (`EC_SceneBlock.cpp:1109`), 120 its far one (`:1133`).

The default is off on purpose: the client measures from a player standing in the map, and a
camera that flies over a whole zone is not that player — at 60 units, the ground under a
high camera is outside the radius and the map would look empty. With the slider at 60 the
picture is the one retail draws from a player's eye, including the 3-second fade as effects
enter and leave the radius.

### 52.3 What this changes for the six reports

Nothing moves, resizes or re-orients because of 52.1-52.2 — that is the point. The remaining
reports are about *which* roots a place draws and *how a class draws them*, not about the
placement contract:

| report | where the answer now sits |
|---|---|
| forest sunbeams | 8 staggered elements, one holding a 120-degree keypoint for 875 ms of a 2,875 ms loop (§50.5); sizes are the map's `fScale ~ 5` |
| Archosaur's vertical beam | the same keypoint rule; the horizontal pair is the elements whose keypoint leaves the shaft along `direction` |
| Wraithgate cyan fires | the `CtrlState` reseed skip when `repeat_count == 1` (§49) |
| Raging Tides kelp, white blobs | per-class drawing and blend, not placement |
| waterfalls | authored static sheet (§50.5); needs a shader dump |
| Nightfire craters | glow element size and blend |

## 53. The six field reports, one at a time, against the client's own drawing code

This section answers the six named issues in the order they were given. For each: the file
or files the issue is about, the rule the client applies (with the line it lives on), what
the viewer does, and the verdict — ours, the file's, or still unread.

Five new source facts came out of this pass and they decide more than one report:

| fact | source | consequence |
|---|---|---|
| a `MatchSurface` element is **never a card**: `Update` hands it to `AfxGetGFXExMan()->GetSurfaceRender().Render(...)` and returns `false` | `A3DDecalEx.cpp:1242-1300`, `:1329-1334` | the 16 corpus `MatchSurface:1` elements must be projected, not billboarded; none of them is a world placement |
| a grid decal scrolls by `TexSpeed`/`TileMode` and animates only from a serialised `m_pGridAnimation` block that starts at **version 99** | `A3DGridDecalEx.cpp:176-179`, `:360-364` | **every one of the 216 grid elements in `gfx.pck` sits in a root older than 99**, so retail animates none of them either |
| `AffByScl` exists only from version 100 and defaults false; it scales the whole grid by `kp.m_fScale` when set | `A3DGridDecalEx.cpp:183-195`, `:425` | all 216 grids in the archive have no such key, so no grid takes the keypoint scale — which is what the viewer already does |
| quaternion product order is Hamilton (`P * Q` applies `Q` first), shown by `RotateQuat` and `GetMatchGrndQuat` pre-multiplying world-space corrections | `a3dgfxkeypoint.h:257-286` | the viewer's `placement * keypoint` is the client's `GetParentDir() * kp.m_vDir` |
| the alpha cutoff is `< 5` of 255 before anything is built | `A3DDecalEx.cpp:1063`, `:1066` | the viewer's `ALPHA_CUTOFF = 5/255` is the same number (`gfx_player.rs:128`) |

### 53.1 Sunbeams in the forest — `场景/树林光束.gfx`

**The file.** Eight element-100 cards, all `Width 0.5 x Height 10`, `RotFromView: 1`,
`SrcBlend 5 / DestBlend 2` (source-alpha, one: additive), `DedaultScale 0.7`,
`DefaultAlpha 0.5`. `StartTime` is staggered across the loop: 0, 500, 875, 1000, 1500,
1750, 2025, 2450 ms. Keypoint 0 holds for `TimeSpan 875` and carries the 120-degree
quaternion `(-0.5, -0.5, -0.5, 0.5)` at `Position (0, 4.70, 0.24)`; keypoint 1 is the
identity at the origin for 2000 ms. Both are `InterpolateMode: 0`, so the client **snaps**
between them rather than turning. Total cycle 2,875 ms, which is why the eight starts are
spread over 2,875.

**The rule.** `Fill_Verts_3D`'s `m_bRotFromView` branch (`A3DDecalEx.cpp:357-413`): shaft
along `RotateVec(GetParentDir() * kp.m_vDir, _unit_z)`, width along
`normalize(cross(vPos - eye, shaft))`, half extents `m_fWidth1/2` and `m_fHeight1/2`
(`SetSize` doubles and splits them, `A3DDecalEx.h:167-177`). A 120-degree keypoint maps
local +Z to local +Y, so for 875 ms of every 2,875 the shaft stands along the placement's
*up* instead of its *direction* — a 90-degree snap, authored.

**Verdict: the file's, not ours.** With `fScale ~ 4.97` the sheet is 3.5 wide and 70 long,
which is what §51.3's arithmetic derives from the map's own numbers. The viewer applies the
snap, the stagger, the additive blend and the half alpha exactly as listed. Even the degenerate case
agrees: `A3DVECTOR3::Normalize()` clears a vector whose magnitude is under 1e-6 and returns
0 (`A3DVector.h:133-141`), so an eye looking straight down a shaft gives the client a zero
`vUp`, four vertices collapsed onto the shaft line, and nothing to see — which is exactly
what `quad_corners` does when it bails on a zero `right` (`gfx_player.rs:4121-4123`).

**What would end it:** a retail capture of the same grove at the same camera. The decal
channel already prints `shaft=client` per card, so the comparison is one log away.

### 53.2 Raging Tides — seaweed, white blobs, misplaced effects

Three separate mechanisms, and only one of them is a placement question.

- **Ground sheets (`MatchGrnd`).** `Fill_Verts_3D`'s ground branch (`A3DDecalEx.cpp:441-542`)
  replaces the card with a grid laid out in the **world XZ plane**, two-unit cells, at most
  4x4, each vertex taking its own terrain height plus a 0.2 lift, and only when
  `m_bGroundNormal && !m_bGrndNormOnly && IsCloseToGrnd() && MatchGrnd()`. The viewer
  implements that branch (`drape_sprite`, `gfx_player.rs:3735`) with the same cell size,
  the same clamp and the same lift, and falls back to a flat card when the surface is
  missed so half a sheet cannot tear.
- **`MatchSurface` sheets.** These are *not* cards: `Update` calls
  `GetSurfaceRender().Render(pView, verts, indices, vCenter, fWidth, fHeight, fRadius, …)`
  and returns false (`A3DDecalEx.cpp:1242-1305`), with `fRadius = max(W,H) * 1.5` clamped to
  `0.1..15` and the patch's yaw taken from the parent direction only when
  `SurfaceUseParentDir` is set. The viewer draws them as cards, which is wrong in principle.
  It affects 16 elements in 8 roots and **not one of them is a world placement** — they are
  all skill effects (`巫师/技能抑制_闪电`, `宠物技能/彩虹祝福击中`, `巫师_土崩_击中`, …), so
  they cannot be the world sheet in the report. Recorded as an open item rather than
  guessed at.
- **Particles.** The six classes `A3DGFXParticle*` (ids 120-125) are still unread at the
  source level: `AlphaSeg`, `BetaSeg`, `IsAvgGen`, `AvgGenSeg`, `IsAvgGenSeq` and
  `ScaleNoOff` are parsed but nothing consumes them, and the census counted them as unread.
  A "white blob" is what an additive particle looks like when its `AlphaSeg`/`BetaSeg` fade
  curve is ignored, and a blob that is too big is what `ScaleNoOff` would fix. **This is the
  one class in this report where the client source is genuinely missing**, and it is the
  next file to fetch (`A3DGFXParticle.cpp` / `A3DGFXElement.h`'s particle section).

### 53.3 Waterfalls — a sheet that should move

**The file.** `瀑布.gfx` and `水门_瀑布.gfx` are byte-identical: five element-100 cards of
`0.9 x 1.25` and `0.85 x 1.25` with `TexRow 2 / TexCol 3`, `TileMode: 1`, `TexSpeed` V of
-1.0 to -2.0, plus two element-124 droplet emitters. That is the gate's water, 1.8-2.5
units across, not the curtain in the report.

**The rule for grids.** A `A3DGridDecalEx` (ids 210/211) builds its UVs once
(`_setUVCoord`, `A3DGridDecalEx.cpp:22-52`) and moves them by `TexSpeed` through the shared
`TexAnimation`; its *vertices* move only if `TickAnimation` gets a value from
`m_pGridAnimation`, which is loaded only for `dwVersion >= 99`
(`A3DGridDecalEx.cpp:176-179`).

**Verdict: proven static in retail for every grid in this archive.** No root in `gfx.pck`
that carries a 210/211 element is version 99 or newer — 0 of 43 roots. So the client has no
vertex animation for any of them, and the only motion a grid can show is the `TexSpeed`
scroll, which the viewer already applies (and only on `TileMode: 1`, which is the client's
own `if (m_bTileMode)` arm). For the big curtain the answer stays what §50.5 said: it is
`瀑布.smd` scenery with a `SKI` material that carries no UV speed, and a shader dump is the
only thing left that could overturn it.

### 53.4 Nightfire — glow craters too large, crater surface too bright

The two halves have different owners and only the first is a size question.

- **The glow cards** are element 100/101 decals, so their extent is
  `2 x Width x kpScale x DedaultScale x fScale` (§52.1). Every factor is read and printed:
  the culling channel writes `record scale {:.3}` and the frame channel writes
  `scale={:.3}` for the same card, so the two factors are comparable in one capture.
- **Brightness** is `kp.MultiplyAlpha(GetActualAlpha())` per element
  (`A3DDecalEx.cpp:1061`) with a hard `< 5` cutoff, then `DefaultAlpha` x record `fAlpha` at
  the root. The viewer folds the same three numbers (`gfx_decal.rs:268-269`,
  `scene.rs:52-56`). The client's other per-element colour step is the outer-colour
  multiply, `kp.m_color * GetOuterColor()` (`A3DDecalEx.cpp:303-305` for every decal element,
  `:1267-1268` in the surface branch, `A3DGridDecalEx.cpp:394-395` for grids). **That is
  closed for world effects rather than open**: `m_bUseOuterColor` starts false
  (`A3DGFXEx.cpp:307`), `Reset` sets it false again (`:1137`), and no recovered source calls
  `SetOuterColor` at all — the definition at `A3DGFXEx.h:235` has no caller. A world
  placement is therefore never tinted by an outer colour, and the viewer's white-with-alpha
  value is the same thing.
- **The blue surface** is not an effect at all if it is the crater's water or ice; it goes
  through the water/composite passes, not the effect pass.

### 53.5 Wraithgate — oversized green effect, drifting cyan fires

**The warp element is already excluded, and correctly.** `绿色传送门.gfx`'s gate is a
`Warp: 1` element-101 of `normalmap\gas.dds`, and `A3DDecalEx::Update` ends with
`if (m_bWarp) { AfxGetGFXExMan()->AddWarpEle(this); return false; }`
(`A3DDecalEx.cpp:1329-1334`): the element never enters the normal render list, so drawing it
as a card was the bug §45 fixed. The green the report sees comes from the other elements of
that root, whose sizes follow 52.1 like every other card.

**The drifting fires are the controller clock, and that mechanism is implemented.** The
client restarts an element's node age on every play — `A3DGFXEx::TickAnimation`'s overrun
path calls `Resume(true); ResumeLoop();` and re-enters — and every controller integrates
against that age (`A3DGFXCtrlMotion::Process`, read at `0x6eaf60`). The viewer clears
`seeded`, `play` and the whole `CtrlState` for every element in `Player::reset`
(`gfx_player.rs:1697-1712`), which runs on each root loop, and again on a keypoint change.
`RepeatCount: 1` elements are covered by that reset — the `!= 1` test at
`gfx_player.rs:2117` only decides whether the *element-level* loop counter can advance,
which for one play it cannot.

**What is left is evidence, not code:** the cyan fires' root name and one EffectFrames capture
would say whether the drift is a controller (frame lines move while `key=` stays) or a
`CtrlMethodCount` we do not run (no controllers at all).

### 53.6 Archosaur — one vertical beam left of three

`圣城之光.gfx` carries 13 elements: three `circle0333.dds` shafts at `1.0 x 35/25`,
`kpScale 0.4`, each with its own keypoint quaternion —
`(-0.4056, 0.5792, 0.5792, 0.4056)`, `(-0.7011, -0.0923, -0.0923, 0.7011)` and
`(-0.5610, -0.4305, -0.4305, 0.5610)`. Those three quaternions are three different
orientations of the same sheet, authored per element; the placement's own `direction` is
common to all three. So one shaft standing vertical and two lying flat is the file saying so,
not a basis error — and the basis is now proven three ways (§51.1, §51.2, §52.1).

**Verdict: authored.** The one test that would overturn it is retail footage; the decal
channel's `dir=` field (the parent axis) plus the frame channel's `key=` (the keypoint axis)
per card is enough to check any single beam against the file.

### 53.7 The code items this pass produced

1. **Outer colour — no work needed for the world.** `SetOuterColor` (`A3DGFXEx.h:235`) has
   no caller in any recovered source and its flag defaults false, so no world root is ever
   tinted and the viewer's white-with-alpha value is equivalent. Worth re-opening only if a
   skill-side caller turns up.
2. **`MatchSurface`.** Route the 16 elements through the surface path (or at least through
   `drape_sprite`) instead of drawing them as cards. No world placement is affected, so it
   cannot change any of these six reports; it is correctness for the skill roots.
3. **Particles.** Fetch `A3DGFXParticle.cpp`; `AlphaSeg`/`BetaSeg` (the fade curve) and
   `ScaleNoOff` (the size) are the two keys that decide a "white blob", and both are parsed
   and unused today.

## 54. The city that lost its effects, and where the waterfall mesh actually is

### 54.1 The zone, located

`world.ecwld` is a version-12 ECWLD: 8192 x 11264 world units, 64-unit blocks,
176 x 128 blocks, 22,528 block records. Every block carries its own effect
records. Scanning the whole file for the 60-byte record plus its length-prefixed
GBK path (the layout `src/scene.rs` reads) gives **2,346 placements** whose
`iValidTime` is 0, 1 or 2, in the box `x -3875..3790`, `z -5462..5232`.

The **City of Raging Tides and its neighbours** are the densest cluster in the
file: a box `x 1584..3378`, `z -5274..-3821` holds **295 placements**, and
**all 295 carry `iValidTime = 2`** — 24-hour, so no time of day hides them:

| placements | root |
| --- | --- |
| 64 | `场景/新种族地图/光.gfx` |
| 34 | `场景/新种族地图/蓝水晶光晕.gfx` |
| 22 | `场景/新种族地图/主城光球小.gfx` |
| 14 | `场景/新种族地图/路灯.gfx` |
| 14 | `场景/新种族地图/主城光球大.gfx` |
| 8 | `血鲷封印台发光体`, 8 `水族箱玻璃` |
| 6 each | `沙尘`, `大坑_液体`, `帐篷火盆`, `火苗`, `蓝火`, `篝火`, `光运` |
| 4 | `新种族地图/水门_瀑布.gfx` (the water gate) at `(2619,239,-4120)` |

The names settle what this is: `新种族地图` is the new-race map family,
`主城光球` the capital's light orbs, `血鲷封印台` the seal platform,
`水族箱` the aquarium glass, `赛亚岛传送门` a teleport gate. Nothing else in
the world is authored this densely. So the report is not a zone with no
authored effects: it is a zone with 295 of them, all of them 24-hour.

The records themselves are ordinary: `fAlpha` is 1.0 for 184 of the 295 (2.0,
5.0, 0.3, 0.5 among the rest), `fScale` runs 0.2..7.2, `fSpeed` is non-zero for
every one, and no position, direction or up vector is degenerate. There is no
data-side reason for the zone to be empty, which leaves the gates between the
record and the draw.

### 54.2 The gate that could hold 295 roots at zero alpha

The gates, in order: activation (`apply_activation`), the day/night window here
(none — all 24-hour), the build queue (one root per frame), and the distance
fade (`apply_attenuation`).

The fade had a defect. A fresh attenuating root is installed at `fade = 0`
(`world_models_gpu.rs`, the `Producer` literal) and reaches full alpha through
`apply_attenuation`; the owner's alpha is `outer[3] * owner.attenuation`
(`gfx_owner.rs:908`). `apply_attenuation` returned early whenever
`effect_distance <= 0`:

```rust
if self.effect_distance <= 0. {
    return;
}
```

`0` is the default and the "off" end of the slider. So the early return was not
"draw every root at the alpha the map authored" — it was "never write the
owner's alpha at all". A root that had been faded out (slider above zero, camera
further than that distance) kept `owner.attenuation = 0` after the slider
returned to `0`, and nothing else in the frame writes that value, so **the root
stayed invisible for the rest of the session**. Roots only ever get their alpha
back through a rebuild.

That is a state that fits the report exactly: move the slider once to compare, fly
on, and everything the fade had dimmed at that moment is gone — the effect of a
radius measured from a camera that has since flown 4,000 units away.

Two changes:

* `0` now restores: every attenuating producer whose `fade` is not 1 is set to 1
  and pushed to its owner, so switching the fade off undoes it.
* The distance is horizontal. `CECOrnamentMan::Update` measures
  `a3d_Magnitude(vPos - vHostPos)`, and both are ground positions — the
  character's feet, not its eyes. The explorer's camera stands in for the
  character, so only its X and Z are used. Measuring to the camera's height meant
  a camera 500 units above a city was "outside" a 60-unit fade of every light in
  it, which is not what the client does to a player standing in that city.

Unit test `the_fade_is_horizontal_and_zero_restores_full_alpha` pins both.

### 54.3 Why no model waterfall can be drawn at the main map's waterfalls

The report says the waterfall is a model and asks for it to be drawn from the
model archive. The shipped map says otherwise, and the difference matters
because drawing a mesh where the client draws none is invention.

* The archive entries exist: `瀑布.ecm` is **116 bytes**, `瀑布.smd` **151**,
  `瀑布.bon` **376**, `瀑布.ski` **933**, and the sheet `瀑布.sdr.tga` 131,116.
  That is one small static plane (a `.smd` descriptor naming the `.bon` mesh and
  the `.ski` material), not an animated mesh — which is what §5.1 measured.
* **No ECWLD in the main world places it.** Every length-prefixed path string in
  `world.ecwld` — 2,348 of them, 177 distinct — was decoded and classified:
  **zero end in `.smd`**, and **no record names `瀑布.ecm`**. The `.ecm` records
  that do appear are `马`, `大风车动画`, `小风车动画`, `完美吐泡植物花`,
  `完美主城花园门水晶(1)`, `藏獒`, `浑天仪`, `海马旗子02左/右`, `远征地主建筑`,
  `完美漂浮水晶`, `沉船破旗1`, `新手村中心台子水晶`.
* What the map does place at a waterfall is spray and sheets: 31 records name a
  `瀑布` file — 28 splash emitters (`瀑布水花`, `瀑布水花底`, `瀑布水花中间`,
  `瀑布水花中间2`, `瀑布水花湍急`, scale 1.0..10.4) plus four
  `新种族地图/水门_瀑布.gfx` tiles at the City of Raging Tides water gate.
  Nothing places `瀑布.gfx`, `瀑布大/小/1..4/子/小慢.gfx`, though 22 archive
  entries match `瀑布`; the client does not draw those in this map either.
* The curtain the camera points at is therefore terrain-and-scenery geometry —
  a `.bmd` ornament mesh — with the splash emitters in front of it, exactly as
  the client has it. The viewer already streams and draws ornaments; a `.smd`
  waterfall would be drawn too if a map placed one, because `Scene::block.models`
  is loaded through `world_models::Assets` regardless of extension.

So there is no code change here that would add a model the data does not place.
What the water gate *does* have is four tile-mode sheets with a negative V
`TexSpeed`, and those are drawn by the effect pass whose sampler is `Repeat`
(`gfx_effect_gpu.rs:252-253`), so their scroll wraps rather than clamping — and
they are inside the 295 placements that §54.2 is about.

### 54.4 What is still unknown

Whether the report's zone was empty because of §54.2 or because the build queue
never admitted those roots. The status line distinguishes them: it prints
`world effects: N roots playing, M quads, K would not build, F beyond the
radius, R outside the resident window, T off by the day/night window`, and the
FX line's tooltip lists the failures.

The last three counts are new. `World::apply_activation` already decided each
producer's fate every frame from three gates — the activation radius around the
camera, `cutoff_residency::contains` against the resident terrain window, and
`scene::effect_visible(mode, night)` against the record's `iValidTime` — but it
reduced them to one boolean and threw the reasons away. It now counts them, and
`World::activation_census` hands the three numbers to that line. The counts overlap
(they are not a partition), and they are what a zone that still draws nothing
should be read against: 295 for the city in §54.1 means the radius, not the
loader.

## 55. A headless preview, so an effect can be looked at without the viewer

Every verdict in this file until now was read out of the code or out of a data
census. That is how a wrong verdict survived review: §54 said the thing that
renders as falling water is the static BMD mesh, and the report was right that
the client draws a waterfall. Neither claim can be settled by reading, so the
effect pass now renders offscreen and writes PNGs.

`src/effect_preview.rs` draws nothing of its own. It builds a `World` exactly
as the viewer does, installs the placement the map authors, ticks the real
frame loop and renders the real pass into an RGBA texture:

```text
cargo test --release --bin perfect-world-explorer -- --ignored --nocapture effect_previews
```

| variable | meaning |
| --- | --- |
| `PWE_PREVIEW_ELEMENT` | client `element` directory holding `gfx.pck` |
| `PWE_PREVIEW_WORLD` | an `.ecwld` to take placements from |
| `PWE_PREVIEW_ROOTS` | `;`-separated path substrings to keep (empty: every placement) |
| `PWE_PREVIEW_FRAMES` | `,`-separated shot times in milliseconds |
| `PWE_PREVIEW_MAX` | how many placements to render |
| `PWE_PREVIEW_SIZE` | PNG edge in pixels |
| `PWE_PREVIEW_FADE` | the distance fade to shoot with, 0 for off |
| `PWE_PREVIEW_OUT` | output directory |

Each shot prints the one number that decides whether an effect is the size the
map authored — the world-space box of the quads that were submitted and that
box's diagonal:

```text
preview: 场景\新种族地图\水门_瀑布.gfx at (…) scale=2.74 alpha=1.00 t=900ms \
  quads=7 pixels=… box=(…)..(…) diagonal=…
```

A sheet drawn at twice its authored size says so in that line whatever it looks
like on screen, and it does so for every placement the map authors without any
of them being opened by hand.

### 55.1 The smoke case proves the harness, and it already caught one thing

`effect_preview_smoke` needs no client file: it authors a version-37 root with
one sprite card, serves it from a text reader, and asserts the card reaches the
pass and covers pixels. It runs in the GPU job of `pr-checks.yml`, which is the
only place with a Vulkan ICD (`mesa-vulkan-drivers`, lavapipe); on a machine
with no adapter it reports `SKIPPED:` through the same gate §43 gave the corpus
tests, because a test that needs a GPU must not redden the plain unit-test job.

It failed twice before it passed, and both failures are evidence:

1. `quads=1 pixels=0`. The card reached the pass and painted nothing, because a
   bucket whose map will not load is drawn with a **transparent** stand-in
   (`gfx_effect_gpu::Draw`'s fallback), not with white. The first version of the
   smoke test had no texture archive and so asserted exactly the wrong thing.
   The archive now answers every name with a 4x4 white TGA built in the test.
   The same fallback is why an effect whose `TexFile` resolves to the wrong pack
   is invisible rather than grey — §"the texture archive mismatch" in
   `Gfx-effect-texture-archive-mismatch-2026-11-07.md`.
2. `panicked at … an adapter` in the unit-test job: the module compiled, all
   1,094 other tests passed, and the only failure was the missing Vulkan ICD.

The frames themselves are a manual run: `.github/workflows/effect-preview.yml`
fetches `gfx.pck` and `world.ecwld` on a runner, runs the ignored corpus test and
commits the PNGs and `render.log` to `Findings/effect-preview/`, so the evidence
sits next to the code instead of in an artifact nobody can read.

## 56. The lab: one effect, in isolation, with a ruler

§55's previews answered "did anything draw". They could not answer "how big is it",
because the camera framed the box: a 12-unit card and a 2,800-unit shaft look the
same once the distance follows the size. This section is the lab that fixes that,
and what it says about the three effects the reports are about.

Each shot is the real pass, one authored placement, and a yardstick drawn
underneath it by the harness and by nothing else: a ground grid at one spacing,
a tick ladder up the effect's own height, and the two ground axes in colour. The
report line carries the numbers that decide a report:

```text
preview: 场景/圣城之光.gfx at (-2770.6, 605.4, 3093.6) dir (0.00, 0.00, 1.00)
  up (0.00, 1.00, 0.00) night 2 scale 89.79 alpha 1.00, clock 792 ms (shot 0),
  view az 35/el 20 zoom 1.00, grid 500.00 u, 8.888 m/px:
  quads=82 pixels=5866 size=557.30x2839.10x567.54 card=71.83x2515.07
```

`size` is the world-space box of the quads that were submitted, `card` the
**longest submitted card's own width and length**, `m/px` the world units one
pixel covers, `clock` the root's own age (a warm-up pass admits the root, so a
shot asked for at 900 ms is taken at 1,716 ms and the report says so).
`PWE_PREVIEW_ZOOMS` and `PWE_PREVIEW_ANGLES` give the shape, `PWE_PREVIEW_QUADS`
prints every card's own numbers, and `PWE_PREVIEW_CENSUS` prints the placement
scale distribution the shots should be read against.

### 56.1 Waterfalls: the gate's water is an effect, it draws, and it moves

`场景/新种族地图/水门_瀑布.gfx`, placed four times around (2619, 239, −4120) at
scale **2.736**:

| | measured |
| --- | --- |
| cards | 79 at clock 792 ms, 77 at 1,716 ms |
| box | 5.89 x 8.74 x 7.05 units (diagonal 12.68) |
| longest card | 5.47 x 8.76 |
| the three sheets | `纹理\气连续03.tga` 4.92 x 8.43, `纹理\流动（通道）.dds` 4.65 x 8.27, `物体\humianbowen.dds` 5.47 x 8.76, all centred at y 242.8 |
| droplets | `多个粒子\shuizhu01.dds`, 1.26 x 1.79 and up, at y 238.7 |

`2 x 0.9 x 2.736 = 4.92` and `2 x 1.25 x 2.736 = 6.84`: the sheets are the file's
own numbers (§52.1's law), and the picture is a curtain of tiled water with a
splash under it.

**It moves.** Between clock 792 ms and 1,716 ms, 22 % of the frame's pixels change
by more than 8/255, and the vertical lag that best explains the second frame as a
shifted first is **74 px**. The sheet is 8.76 units tall, which is 230 px at
`0.038 m/px`, and its tiles are half that (90 px) because `TexRow 2`; the
elements scroll at `TexSpeed` V −1.0, −1.5 and −2.0 per second, so 0.924 s of
clock moves them 83, 125 and 166 px. The measured 74 px is the −1.0 element's
tile — the sheet scrolls at the authored speed.

**This supersedes §53.3.** That section concluded the gate's water was
`瀑布.smd` scenery and that the big curtain was a mesh. The mesh is real and is
placed nowhere in this map (no record names `瀑布.ecm`, §54.4); what draws the
water gate is this effect, and it draws it as falling water.

### 56.2 Forest sunbeams: the size is the file's, and §53.1 was reading a different placement

`场景/树林光束.gfx` at scale **9.36**:

| clock | cards | box | longest card |
| --- | --- | --- | --- |
| 792 ms | 2 | 95.55 x 100.16 x 8.27 | (the 5 cards that have started) |
| 1,716 ms | 5 | 96.00 x 101.82 x 130.98 | 6.55 x 131.04 |

The card is `2 x Height(10) x kpScale(1) x DedaultScale(0.7) x fScale(9.36) = 131.04`
long and `2 x Width(0.5) x 0.7 x 9.36 = 6.55` wide — the measured 131.04 and 6.55
to two decimals. The card count follows the authored `StartTime` list exactly:
0 and 500 ms have started at a clock of 792, and 0/500/875/1,000/1,500 at 1,716
(the 875, 1,000 and 1,500 elements begin inside the second shot's window).

So the forest's shafts are 131 units long in the client too, at this grove's
scale of 9.36. §53.1 quoted 70 units for the same root, which is the same file at
**4.97** — both scales are in the map. Nothing here is doubled, dropped or
re-scaled; the two numbers differ because the placements do.

### 56.3 Archosaur: a 2.8 km pillar, and the map authors that size

`场景/圣城之光.gfx`, 13 elements, two placements, both `night 2`, `dir (0,0,1)`,
`up (0,1,0)`:

| placement | scale | cards | box | longest card |
| --- | --- | --- | --- | --- |
| (−2770.6, 605.4, 3093.6) | 89.79 | 82 (106 at 1,716 ms) | 557.30 x **2839.10** x 567.54 | 71.83 x 2515.07 |
| (1529.5, 311.4, 1032.3) | 17.70 | 87 (116 at 1,716 ms) | 112.29 x **559.71** x 113.66 | 14.16 x 495.83 |

The shafts are the file's `1.0 x 35/25` cards with `kpScale 0.4`:
`2 x 35 x 0.4 x 89.79 = 2514.1` and `2 x 1 x 0.4 x 89.79 = 71.83`, and at the
second placement `2 x 35 x 0.4 x 17.70 = 495.6` and `2 x 1 x 0.4 x 17.70 = 14.16`.
Both pairs match the measurement to a tenth of a unit, and the two placements'
sizes are in exactly the ratio of their scales (5.07).

**The scale is the map's own number, not a field read wrong.** A census of all
**2,275** effect placements in this `world.ecwld` gives a scale distribution of

```text
min 0.164   p10 0.812   median 1.799   p90 6.816   max 380.953
```

and the two `圣城之光` rows occupy 17.70 and 89.79 inside it. §48's table stopped
at 17.359 because it counted one zone's placements, not the map.

So what a player sees at Archosaur is a light pillar **2,839 units tall and 71.8
units wide**, and a second one 560 units tall, drawn from the file's own
arithmetic at the map's own scales. If retail looks smaller than that, the
difference is in the map's numbers or in an element rule the archived root does
not show — it is not in the build, the doubling, the keypoint scale or the
placement scale, because each of those is now measured against the authored
value at two placements and matches.

The cards themselves, largest first, from the per-quad dump:

| card | centre | axis | texture |
| --- | --- | --- | --- |
| 71.83 x 2515.07 | (−2782.5, 1275.3, 3097.1) | (−0.02, **−1.00**, 0.02) | `圆环光\circle0333.dds` |
| 71.83 x 2515.07 | (−2770.6, 1275.3, 3091.7) | (−0.02, **−1.00**, 0.02) | `圆环光\circle0333.dds` |
| 71.83 x 1797.18 | (−2756.3, 591.1, 3098.2) | (−0.03, **−1.00**, 0.02) | `圆环光\circle0333.dds` |
| 71.83 x 1797.18 | (−2770.6, 1041.8, 3093.6) | (−0.03, **−1.00**, 0.02) | `闪电\lightning007.dds` |
| 404.04 x 571.40 | (−2770.6, 605.4, 3093.6) | (0.71, **0.00**, 0.71) | `图案\greenring.dds` |
| 206.51 x 292.05 | (−2770.6, 605.4, 3093.6) | (0.85, **0.00**, 0.53) | `图案\火环2.dds` |

`2 x 25 x 0.4 x 89.79 = 1795.8` is the pair of shorter shafts to a tenth of a
unit, so the root's `1.0 x 35` and `1.0 x 25` cards are both drawn at the
authored size. **Every shaft card's axis is vertical** (the three authored
keypoint quaternions each map +Z to +Y), and the two rings are flat, as a ring
should be.

**This corrects §53.6.** That section read the report as "one vertical beam left
of three", i.e. two shafts lying flat. Measured, all of them stand up; the
pillar reads as one because the shafts coincide at the placement's centre, not
because two are missing. The report's other half — the size — is the map's own
number (§56.3 above), so what is left of the Archosaur item is the texture's own
alpha profile, not the build.

### 56.4 What the lab still cannot answer

Why retail's pillars read as shafts rather than hairs: that is a question about
the shaft texture's own alpha profile, and the answer is in `circle0333.dds`, not
in the geometry. The lane is open and the tool for it now exists: the same shot
with `PWE_PREVIEW_QUADS=1` names the map each card uses, and a decoded texture
next to it would end the argument.

## 57. The card is not its diagonal, the sheets scroll at exactly `TexSpeed`, and the rings are flat

The lab gained three things in one pass, and each of them settles a question the
last report could only guess at: the printed card is the card rather than its
diagonal, every shot quotes the clock the *player* reached and the speed the
placement put on it, and every card line carries its UV window. With those, the
waterfall, the rings and the sunbeams are measurable from `metrics.md` alone.

### 57.1 `card=` was the diagonal, and so was the viewer's own census

`EffectQuad::positions` is in the client's corner order — top-left, top-right,
bottom-left, bottom-right — so the height is corner **2** minus corner **0**.
The lab took corner **3** (bottom-right), which is the *diagonal*, and the viewer's
`effect-census.txt` did the same. A square card is the clearest case: its diagonal
is exactly `sqrt 2` times its side, and every ring in the corpus reported one.

| root | card before | card now | what it is |
| --- | --- | --- | --- |
| `图案/greenring.dds` (scale 89.79) | 404.04 x 571.40 | **404.04 x 404.04** | a square, `571.40 / 404.04 = 1.4142` |
| `图案/火环2.dds` | 206.51 x 292.05 | **206.51 x 206.51** | `292.05 / 206.51 = 1.4143` |
| `纹理/气连续03.tga` (waterfall) | 4.92 x 8.43 | **4.92 x 6.84** | `2 x 0.9 x 2.736` by `2 x 1.25 x 2.736` |
| `圆环光/circle0333.dds` (shaft) | 71.83 x 2515.07 | **71.83 x 2514.04** | `2 x 0.4 x 89.79` by `70 x 0.4 x 89.79` |

The shafts hid the bug — 2515.07 against 2514.04 is a tenth of a percent — which
is why §56.3's table looked healthy while the rings did not. Both the lab and
`gfx_owner::census_row` now read corner 2.

### 57.2 The waterfall's sheets scroll at exactly the authored `TexSpeed`

`水门_瀑布.gfx` is `TileMode 1`, so `A3DGFXElement::TexAnimation` adds
`m_fUOffsetSpeed * dt` to the window instead of stepping an atlas cell. Three
shots of the same placement, one world each, `PWE_PREVIEW_PER_ROOT=3`:

| clock | `气连续03` (speed −1.5) v | `流动（通道）` (−1.8) v | `humianbowen` (−1.0) v |
| --- | --- | --- | --- |
| 792 ms | −1.188 | −1.426 | −0.792 |
| 1,716 ms | −2.574 | −3.089 | −1.716 |
| 3,795 ms | −5.693 | −6.831 | −3.795 |

Every entry is `speed x clock / 1000` to three decimals, and the window stays
`TexCol x TexRow = 3 x 2` at every clock — it never leaves its sheet. The played
clock is the player's own, printed as `player clock 792 ms play 1.000 (file
1.000)`, and it equals the requested time because this record and this file both
run at 1.0.

The element table says the same thing from the asset's side
(`Findings/effect-preview/effect-elements.log`): `version=79`, `play_speed=1.000`,
seven elements, and the four drawn sheets are `tile=1` with
`speed=(0,-1.5)/(0,-1.8)/(0,-2.0)/(0,-1.0)` and `grid=3x2/2x2/1x1/1x1`. The
`normalmap\gas.dds` card is a warp source and draws no card at all — the decal
channel says so in the native terms (`AddWarpEle, no normal card,
A3DDecalEx.cpp:1332`).

The PNGs agree that something moves — 21.2 % of the pixels differ by more than
8/255 between 792 ms and 1,716 ms, 18.1 % between 792 and 3,795 — but not with a
coherent vertical shift (best 1 px), and that is the sheet's own nature: `气连续03`
is a smoke sequence whose rows are near-copies, so a 1.4-texture-height scroll
changes the picture without translating it. The UV column, not a shift search, is
the measurement.

### 57.3 The `图案` rings are flat, square and on the ground

| card | size | axis | reading |
| --- | --- | --- | --- |
| `图案/greenring.dds` | 404.04 x 404.04 | (0.00, 0.00, 1.00) | flat: the height runs along Z, `Y = 0` |
| `图案/火环2.dds` | 206.51 x 206.51 | (0.22, 0.00, 0.98) | flat, turned 12.7 degrees |
| `图案/火环2.dds` | 206.51 x 206.51 | (−0.22, 0.00, 0.98) | flat, turned the other way |
| `图案/yuanhuanhuang.dds` | 157.69 x 163.13 .. 539.20 x 557.79 | (0.00, −1.00, 0.00) | **vertical**, 99 units above the two flat rings |

So the two rings at the placement's own height are lying in the ground plane, at
the placement origin (the same point the map puts the effect), and the "vertical
rings" in the frame are the five `yuanhuanhuang` cards a hundred units above them
— a second set, standing up, which is what a picture-read of §53.6 mistook for the
rings themselves. §56.3's `(0.71, 0.00, 0.71)` was the *diagonal* of the flat
square, not a lean: a card lying in the ground plane and turned 45 degrees reports
exactly that.

The decal channel names the builder for each of them, and it settles the last
doubt: every `图案` card — flat or standing — takes the **element** branch, and
none of them is a billboard:

```
gfx/场景/圣城之光.gfx id=100 basis=element  rot_from_view=false 图案/greenring.dds
gfx/场景/圣城之光.gfx id=100 basis=element  rot_from_view=false 图案/yuanhuanhuang.dds
gfx/场景/圣城之光.gfx id=100 basis=axial    rot_from_view=true  圆环光/circle0333.dds
gfx/场景/圣城之光.gfx id=100 basis=axial    rot_from_view=true  闪电/lightning007.dds
```

The two textures that end up in different planes therefore differ in the asset's
own keypoint `Direction`, which is folded into the element frame before the card
is built, and not in the rule that builds them. A standing `yuanhuanhuang` card
is what the file draws; if it should lie down, the file is where to look.

### 57.4 `圣城之光` is placed at `fSpeed` 0.8, not at the file's 1.0

`player clock 634 ms play 0.800 (file 1.000)` for a shot that asked for 792 ms:
the record's `fSpeed` is 0.8 and the file's `PlaySpeed` is 1.0. `CECOrnamentMan`
does not multiply the two — it *sets* the play speed from the record
(`pGFX->SetPlaySpeed(pNode->fSpeed)`, `EC_ManOrnament.cpp:950`), so the file's
value is replaced. The viewer used to scale its delta by `fSpeed` and then let
`A3DGFXEx` multiply by `PlaySpeed` as well; `Player::set_play_speed` now carries
the placement's value and the pass hands the player the raw frame delta. A root
whose file authors `PlaySpeed: 0` used to play at `0 x fSpeed` however fast the
frame ran, which is one way to get a frozen sheet with live geometry.

### 57.5 The forest sunbeams are the authored size, in two different planes

`场景/树林光束.gfx` at scale 9.36 gives cards of **6.55 x 130.98**, i.e.
`2 x 0.5 x 0.7 x 9.36` by `2 x 10 x 0.7 x 9.36`, and the number of them follows
the element schedule (2 at 792 ms, 5 at 1,716, 7 at 3,795, out of eight elements
at `StartTime` 0, 500, 875, 1,000, 1,500). Two axes appear:

* `(0.69, −0.72, 0.00)` — the placement's `up`, i.e. the element-plane card, a
  44-degree slant. These are the streaks the PNG shows.
* `(0.00, 0.00, −1.00)` — the placement's `dir`, i.e. the `RotFromView` axial
  card, whose height sits on the record's `Direction: (0,0,1)`. In this
  placement that direction is horizontal, so those cards lie in a horizontal
  plane at the placement origin.

Every one of the eight elements is `id=100` with `RotFromView: 1` (`basis=axial`),
so the two axes are not two rules: each element carries its own keypoint
`Direction`, that quaternion is part of the element frame, and the axial height
runs along that frame's Z. The element table confirms the rest of the card:
`size=0.500x10.000`, `grid=1x1`, `tile=0`, `speed=(0.000,0.000)`,
`default_scale=0.700`, `default_alpha=0.500`.

### 57.6 What is still open: the viewer's own clock

Everything above is measured inside one process that builds the world, ticks it
and draws it. The field report that started this — the waterfall standing still in
the *viewer* — cannot be reproduced here, because the lab's clock advances at every
tick and the sheets scroll by exactly `TexSpeed`. Two readouts now exist for the
viewer's side of it, neither of which needs a debugger:

* the World tab prints **`World effect clocks: <low>..<high> ms across N playing
  roots`**, read from the players themselves. A number that keeps rising means the
  delta reaches the effects and a motionless sheet is a texture/blend problem; a
  number that stands still means no delta reaches them.
* `PWE_EFFECT_CENSUS=1` writes `effect-census.txt` next to the executable every
  two seconds, and every row now carries `clock=` and `dclock=` — the second one
  is the change since the previous report, so `dclock=+2001ms` is a running root
  and `dclock=+0ms` is a frozen one, with no arithmetic for the reader.

### 57.7 `PWE_LOG` is real now, and the logs are published with the frames

The docs promised `PWE_LOG` and nothing read it. It does now: `diagnostics::apply_env`
takes the channel keys the Logging tab prints — in any spelling, `effect-elements`,
`EffectElements` or `effectelements` — and turns them on, on top of whatever the
settings file had. The preview test never runs `main`, so this is also the only way
a headless run can name a channel; the workflow sets
`PWE_LOG=effect-elements,effect-decals` and copies the resulting files into
`Findings/effect-preview/` with `git add -f`, because `*.log` is ignored in this
repository and the first attempt published nothing at all.

Those two files are what every claim in §57.3 and §57.5 rests on: the element table
is the asset as authored, and the decal channel is the branch each card was built
by, printed next to the card's texture, its `rot_from_view`, its origin and the
clock at which it was drawn.

### 57.8 The 1.5.2 client, read: what a tiled card really samples

The engine, not a decompilation this time — `PWClient152/Common/GfxCommon2` from
`coussiraty/PWClient-152` (`A3DGFXEx.cpp`, `A3DGFXElement.{h,cpp}`,
`A3DDecalEx.{h,cpp}`, `A3DGFXKeyPoint.h`). Three of the questions above stop being
questions:

**The placement chain is ours exactly.** `CECOrnamentMan::LoadEffect`
(`ElementCore/ElementClient/EC_ManOrnament.cpp:847`) reads `ECWDFILEEFFECT12` —
id, `fScale`, pos, dir, up, `fSpeed`, `iValidTime`, `fAlpha`, `bAttenuation` — then
`LoadGFXFromFile` does `pGFX->SetScale(fScale); pGFX->SetPlaySpeed(fSpeed);
pGFX->SetParentTM(a3d_TransformMatrix(vDir, vUp, vPos));`. So the record layout of
`scene.rs` is the shipped one, field for field, and the two numbers our
`effect-clock` channel prints are the two the client applies.

**The card size is `2 x (file Width) x (DefScale x record scale)`.** `A3DDecalEx`
keeps `m_fWidth` as a *half* extent (`A3DDecalEx.h:33`), `Load` reads the file's
`Width:` straight into it, and `Save` writes it back unchanged — the `x2` lives
only in the editor property (`GetProperty` returns `m_fWidth*2`). `Update_3D`
builds the quad from `m_fWidth1/2 * kp.m_fScale` and transforms it by
`GetTranMatrix(kp) * GetParentTM()`, and `CalcParentTM` is
`m_matParent = m_matScale * m_matNoScale` with `m_fActualScale = m_fDefScale *
m_fScale` (`A3DGFXEx.h:199`). `GetTranMatrix` carries rotation and translation
only. The `RotFromView` and ground branches multiply by `GetActualScale()`
themselves and return local=false, so the scale is applied once in every path.
Our 4.92 x 6.84 waterfall card is 2 x (0.9, 1.25) x (1.000 x 2.733) and is what
the client draws. **The sizes were never the bug.**

**A tiled card shows one sheet, not `TexCol x TexRow` of them.** This is the
stumbling block. `A3DGFXElement::GetTexTrans` hands the pixel shader
`rw = 1/m_fTexWid`, `u0 = rw`, `u1 = -m_fTexU * rw` and the shader samples
`u0*uv + u1`, while `m_fTexWid` is `TexCol` in tile mode (`CalcTexAnimation`) and
the vertex UV spans `m_fTexWid` texture widths. The two cancel the grid out of the
picture: what reaches the sampler is a window exactly one sheet wide, sliding by
`TexSpeed / TexCol` per second. Writing the raw `TexCol x TexRow` window into the
vertices (what `flipbook_uv` did) laid 3 x 2 copies of the sheet over every
waterfall card. Six copies of a soft water cutout fill the gaps the single sheet
leaves, so the alpha goes uniform, the silhouette flattens into a hard band, and
the texture is fine enough to read as still — a static bright image over a clock
and a UV that were both advancing (`clock=122874ms`, `V=-184.311` in
`CityOfRagingTides.log`).

### 57.9 The render test: six disputed roots, measured against the client's own formula

The 13 November preview run (`Findings/effect-preview/render.log.txt`) rendered
one placement of each root the reports name, at 0 / 500 / 3000 ms, from two
cameras, with `PWE_LOG=effect-elements,effect-cards,effect-uv,effect-clock`. Every
card it drew can be checked against `2 x (file Width) x (DefScale x record scale)`,
which is the client's `Update_3D` + `CalcParentTM`:

| root | DefScale | record scale | file `Width x Height` | rendered card | ratio |
|---|---|---|---|---|---|
| `树林光束` | 0.700 | 9.36 | 0.5 x 10 | 6.55 x 130.98 | **1.000 / 1.000** |
| `圣城之光` `greenring` | 1.000 | 89.79 | 2.25 x 2.25 | 404.04 x 404.04 | **1.000 / 1.000** |
| `天煞光束` | 1.000 | 6.54 | 2.0 x 50 | 26.17 x 654.33 | **1.000 / 1.000** |
| `水门_瀑布` `气连续03` | 1.000 | 2.74 | 0.9 x 1.25 | 4.92 x 6.84 | **1.000 / 1.000** |

So the sizes the reports call oversized are the sizes the client computes, on all
four roots, to three decimals. A size error is not what this is.

Two things in the same run *were* wrong:

1. **The tiled UV window** (§57.8). Every `tile=1` element now samples one sheet
   and slides it: `水门_瀑布` `grid=3x2 speed=(0,-1.5)` reads
   `uv=[(0,-0.025),(1,-0.025),(0,0.975),(1,0.975)]` at 33 ms, i.e. a 1 x 1 window
   moved by `1.5 / 2 x 0.033`. Before the fix the same line read a 3 x 2 window.
2. **The file's own `PlaySpeed` was thrown away** when a placement set one.
   `场景/天煞光束.gfx` authors `PlaySpeed: 0.4` and its placements author
   `fSpeed: 1`, so the client runs it at 0.4 while the player ran it at 1 — the
   only root of the six whose clock did not match its shot (`player clock 792 ms`
   for `asked 792 ms`; 317 was correct). `Player::play_speed` now returns
   `root.play_speed * override`, which is `m_fDefPlaySpeed * m_fPlaySpeed`.

The frames show the difference: the waterfall PNG is a soft, see-through sheet
with the flow pattern readable at its authored scale, where the pre-fix frame was
a finer, busier version of the same surface.

### 57.10 Every card of six roots, element by element: the sizes are the client's, and the frame is why they *look* oversized

§57.9 checked one card per root. The same run's two channels can be joined
properly, and this is the whole table. `effect-elements.log.txt` prints each
element's own `Width x Height` and `grid`; `effect-cards.log.txt` prints each
card the decal channel actually submitted, with its texture, `a=` and pixel
extents. Joining them *by texture and by size* — not by the `#n` index, which is
the quad's index in the frame, not the element's — every non-particle card
resolves to one element and one keypoint scale:

| root | DefScale | record scale | file `Width x Height` | rendered card | keypoint scale |
|---|---|---|---|---|---|
| `树林光束` (`circle019/013/004`) | 0.700 | 9.36 | 0.50 x 10.00 | 6.55 x 130.98 | 1.000 |
| `圣城之光` `greenring` | 1.000 | 89.79 | 2.25 x 2.25 | 404.04 x 404.04 | 1.000 |
| `圣城之光` `火环2` | 1.000 | 89.79 | 1.15 x 1.15 | 206.51 x 206.51 | 1.000 |
| `圣城之光` `yuanhuanhuang` | 1.000 | 89.79 | 0.75 x 0.725 | 156.01 x 150.81 | 1.158 |
| `圣城之光` `circle0333` (35 tall) | 1.000 | 89.79 | 1.00 x 35.00 | 71.83 x 2514.04 | **0.400** |
| `圣城之光` `circle0333` (25 tall) | 1.000 | 89.79 | 1.00 x 25.00 | 71.83 x 1795.75 | **0.400** |
| `天煞光束` `circle032` | 1.000 | 6.54 | 2.00 x 50.00 | 26.17 x 654.33 | 1.000 |
| `天煞光束` `circle032` (narrow) | 1.000 | 6.54 | 1.00 x 50.00 | 13.09 x 654.33 | 1.000 |
| `水门_瀑布` `气连续03` | 1.000 | 2.74 | 0.90 x 1.25 | 4.92 x 6.84 | 1.000 |
| `水门_瀑布` `流动(通道)` | 1.000 | 2.74 | 0.85 x 1.25 | 4.65 x 6.84 | 1.000 |
| `水门_瀑布` `humianbowen` | 1.000 | 2.74 | 1.00 x 1.25 | 5.47 x 6.84 | 1.000 |
| `绿鬼火3` `星星6` | 2.000 | 8.95 | 0.125 x 0.125 | 4.48 x 4.48 | **6.400** |
| `骷髅光柱` `circle038` | 3.000 | 1.00 | 0.00 x 0.00 | 2.09 x 90.02 | emitter |

Every entry is `2 x Width x DefScale x recordScale x keypointScale`, and the
keypoint column is the file's own `Keys[...].Scale` at the sampled time, not a
fitted fudge: `圣城之光` authors its four shafts at `Scale 0.4`, and
`2 x 1 x 35 x 89.79 x 0.4 = 2514.04` exactly. The `size=0` rows are particles:
their `Width/Height` are zero in the file and the card takes its size from the
emitter (`ScaleMin/ScaleMax` plus `ScaleChage`), which is the `extra` term in
`quad_corners` — so they cannot be checked this way, and none of them is
oversized by a factor: `绿鬼火3`'s flames are 28.65 units across at record scale
8.95, i.e. 3.2 units of authored flame.

**So why does the picture read as oversized, and does it still?** Two answers,
and neither is a scale line:

1. *The lab cannot show relative size.* `framing_shot_at` puts the camera at
   `radius / tan(22.5°) x 1.15`, so the effect's own bounding box always covers
   about 87 % of the frame: a 0.11-unit smoke puff and a 2514-unit shaft produce
   the same picture. The size is in the log line (`card=` and `m/px`), never in
   the image. Reading "too big" off a fitted frame is a property of the harness,
   which is exactly the preview/client misunderstanding the report suspected —
   but it is in the *framing*, not in the geometry.
2. *What was actually wrong in the pipeline* is the pair in §57.9: the tiled UV
   window and the discarded `PlaySpeed`. Both are shared-parameter bugs — one
   line of UV arithmetic, one line of speed composition — and both are fixed and
   tested. Nothing about them is per-effect.

In the world the same cards land at the same world size, because the placement
matrix carries `DefScale x fScale` once (`EC_ManOrnament.cpp:953`
`pGFX->SetScale(pNode->fScale)` into `A3DGFXEx::m_fDefScale`) and the census rows
in §57.5 reproduce the same numbers outside the lab. A 654-unit pillar of light
and a 2514-unit sky shaft are large because the maps author them that way, and
the remaining reason they dominate a lit scene — `blend=2/4` (`ONE +
INVSRCCOLOR`) over bright ground — is §57.6's open item, not a size.

### 57.11 The fix, seen on a real placement: `play 0.400 (file 0.400)`, and the sampler the client actually sets

The preview run on `e2fd50c` (frames committed as `dcb06d4`) renders the same
placements as §57.9 with the fixed player, and the summary lines now read:

```
场景/天煞光束.gfx at (1528.0, 602.5, 1032.3) night 2 scale 6.54 alpha 0.60,
  asked 792 ms (shot 0) … player clock 317 ms play 0.400 (file 0.400)
  asked 3795 ms (shot 3000) … player clock 1518 ms play 0.400 (file 0.400)
场景/新种族地图/水门_瀑布.gfx at (2618.9, 239.3, -4119.7) … scale 2.74 …
  asked 3795 ms (shot 3000) … player clock 3795 ms play 1.000
```

317 = 792 x 0.4 and 1518 = 3795 x 0.4, so the root now runs at the file's rate
where the same lines read `player clock 792 ms` and `play 1.000 (file 0.400)`
before. The waterfall, whose file is `PlaySpeed 1.0`, is unchanged at 1.0 —
the fix moves only the roots that disagreed.

**The sampler question is settled by the client's default.** `A3DDecalEx` writes
unbounded `m_fTexU/m_fTexV` and the pixel shader turns them into
`u0 = 1/TexWid, u1 = -TexU * 1/TexWid` (`A3DGFXElement.cpp:1385`,
`GetTexTrans`), i.e. a window `1/TexWid` wide that slides forever. That can only
be right with wrapping, and `A3DHLPixelShader.h:110-118` constructs every
sampler as `addrs[i] = A3DTADDR_WRAP` unless the effect's own `.psh` says
otherwise. So `gfx_effect_gpu.rs:252` (`AddressMode::Repeat`, with the comment
about a clamped scroll freezing the sheet) is the client's behaviour, not a
choice — and the `ClampToEdge` in `gfx_decal_gpu.rs:92` is a different renderer
for single-sheet static decals, with no `TexSpeed` and no `tile=1` element to
scroll. Neither line is the bug the reports described.

The published frames agree with the log: at 0 and 3000 ms the waterfall sheet is
the same authored pattern in two positions (the base foam mounding up and the
mottling shifted), not a finer or double-tiled version of it.
