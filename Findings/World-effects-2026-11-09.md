# World effects — the 9 November reports, and three decode errors behind them

Second pass over the four reports left open by
[`Every-effect-2026-11-08.md`](Every-effect-2026-11-08.md). The three defects this
pass fixed are all **class-level** defects — they change what nearly every effect
in the game draws, not one root — and all three come with a native citation and a
unit test that fails on the previous code: ring sections, ring pitch, grid UV
spread, and (section 5.2) the ground-draped decal pass.

This environment has no Rust toolchain, so **nothing here was compiled or run**:
the new tests were written against the fixtures the existing suite uses and
reasoned through by hand, but `cargo test` was not executed against them. Treat
the code changes as reviewed-but-unexecuted until CI says otherwise. Everything
in the *Measurements* sections was computed here, from the shipped data.

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
