# Sun/moon size, flare quads, clouds, weather and sky rotation — 2026-11-04

Follow-up to `Sky-parity-checkup-2026-11-04.md`. Everything below was read out of
the real client in this session: `elementclient.exe` (6,000,640 B, image base
`0x400000`, Drive id `19h1s6U7JR0uvVccqeL8ZTAVpVeal1X1X`) together with the
`textures.pck` (117,047,648 B, `e6d48bb269d5f7b5ff576fe1045fdca9`) and
`configs.pck` (588,068 B) of the same `pw142` install. Nothing here is inferred
from the look of the art.

**The headline: the previous report's "native billboard is 1.146 degrees" is
wrong.** `CECSunMoon::Init` fills three template vertex streams whose numbers are
world units in a plane 100 units from the camera, and the templates are 20 × 20,
120 × 40 and 40 × 40 units — not the ±1 unit quad the earlier pass assumed and
not the implied 1.146°/0.63°/0.86° disc. The sun quad is **11.42°** and the moon
quad **22.62°**, and the sun grows to **3x** that at dusk.

---

## 1. Sun and moon: the streams carry the size, not a scale

`CECSunMoon::Init` (`0x4b2c00`–`0x4b3560`) creates three vertex buffers and
locks each one to fill an explicit quad template. A vertex is 28 bytes —
`{x, y, z, pad, diffuse, u, v}` — and the fills are, in stream order:

| stream | create | lock/fill | template (`x`, `y` extents) | size |
| --- | --- | --- | --- | --- |
| sun | `+0x30` | `0x4b2fde`–`0x4b30f9` | `x -10..10`, `y 0..-20` | 20 × 20 units |
| sun shine | `+0x34` (8 verts, `0x4b2ee2`) | `0x4b3130`–`0x4b3337` | `x -60..60`, `y 10..-30`, 4 columns, `u` = 0/⅓/⅔/1 | 120 × 40 units |
| moon | `+0x38` | `0x4b3425`–`0x4b3517` | `x -20..20`, `y 0..-40` | 40 × 40 units |

The fills are literal stores: `0x4b2fde` writes `(-10, 0, 0, …, u 0, v 0)`,
`0x4b301d` `(10, 0, …, u 1, v 0)`, `0x4b305a` `(-10, -20, …, u 0, v 1)`,
`0x4b30a1` `(10, -20, …, u 1, v 1)` — one quad, 20 × 20 units, UV 0..1. The
moon stream is the same pattern at `±20` and `0..-40`. An index buffer
(`0x785700`, `0x4b3384`) turns the 8-vertex shine strip into triangles.

Two consequences:

* The native angular size is `2 * atan(20 / 2 / 100)` = **11.421°** for the sun
  and `2 * atan(40 / 2 / 100)` = **22.619°** for the moon. The old ±1 figure is
  the `ds` pair `0x8d6a94` = -0.5 / `0x8d6a98` = 2.0, and **neither address is
  referenced anywhere in `0x4b3a20`–`0x4b4290`**: their only users are
  `0x429610`, `0x429e45`, `0x42ab72`, `0x4747f8` and `0x47c2dc`, all unrelated.
  The number in the earlier report was an inference from two unrelated
  constants, and it was wrong by 10x (sun) and 20x (moon).
* The transform is orthonormal, so the template numbers survive to the screen:
  `0x788960` normalises both basis vectors it is handed (length test against
  `0x8f07a8`/`0x8f07a0`, divide through `0x408020`), `0x788ff0` is a pure
  uniform-scale matrix, and `0x779ca0` is `SetTransform(WORLD, …)`.

### 1.1 What the textures put inside those quads

Measured from the packed art (`google_drive/extracted/`, ignored):

| texture | quad | content |
| --- | --- | --- |
| `textures/nature/sun.bmp` 256×256 | 20 × 20 units, 11.42° | solid core to 40% of the frame (4.6°), 50%-of-peak contour at 60.4% (6.9°), glow to 81% (9.2°) |
| `textures/nature/moon.tga` 512×512 | 40 × 40 units, 22.62° | alpha bbox 428 × 425 px = 83.6% of the frame (**18.9°** disc), solid core `alpha > 200` 335 px = 65% (14.9°) |
| `textures/nature/sunshine.bmp` 256×128 | 120 × 40 units, 61.93° × 22.62° | anamorphic streak; vertical profile peaks at row ~70/128, horizontal at the centre column |
| `textures/nature/sunflare.bmp` 128×128 | flare sprite | 50%-peak radius 15% of the half-frame, 10%-peak 57% |
| `textures/nature/flares.bmp` 256×256 | ring sprite | ring radius 118% of the half-frame (clipped by the sprite edge) |
| `textures/nature/moonflare.bmp` 256×256 | moon halo | byte-identical to `sun.bmp` (`627444c5…`) |

For comparison the real sun and moon subtend 0.53°: the client draws a sun whose
glow is 18x the real disc and a moon nearly 36x. That is the retail look, and it
is what the viewer now draws.

### 1.2 The sun swells, the moon does not

In the render pass, `[this+0x70]` (the recovered night strength) drives the
scale:

* `0x4b3d1d`: `fld [+0x70]` / `fadd st, st` / `fadd 1.0` → `1 + 2*n`, pushed
  three times into `0x788ff0` (scale) before the sun's disc draw `0x4b3d7c`;
* `0x4b3fc6`: the same `1 + 2*n` before the shine-strip draw `0x4b4026`;
* `0x4b4161`–`0x4b41d0` (moon): `0x788960` only, and `0x4b5c43` pushes literal
  `1.0` three times — the moon is never scaled.

`n` saturates at 1 while a disc is visible, so the sun runs 11.42° → 33.40°
across dusk (half extent 10 → 30) and the shine strip grows from 61.93° × 22.62°
to 121.9° × 61.9° (half extents 60/20 → 180/60), which is why retail sunsets have
a glare that reaches the screen edges.

### 1.3 The shine strip is a dusk effect, not a midday one

`0x4b3d8a` skips the strip when `[+0x70] == 0` and `0x4b3d9e` skips it when
`[+0x74] == 1.0` (the night set). Inside that window the strength is
(`0x4b3dc1`–`0x4b3e22`):

| night strength | shine |
| --- | --- |
| `n <= 0.3` | 0 |
| `0.3 < n < 0.6` | `(n - 0.3) * 3.3333` |
| `0.6 <= n < 0.8` | 1.0 |
| `n >= 0.8` | `(1 - n) * 5.0` → 0 at `n = 1` |

The previous viewer drew the streak whenever `(1 - n) * 5 > 0`, i.e. all day and
never at the sunset it belongs to. It is now the recovered window
(`sky_celestial::shine_ramp`).

### 1.4 The flare chain: what is recovered and what is not

`CECSunMoon::Init` creates, in this order (`0x4b37fd`, `0x4b380f`, `0x4b3877`,
`0x4b3880`, `0x4b38e1`, `0x4b38ec`):

| object | create call | count | purpose |
| --- | --- | --- | --- |
| sun point stream | `0x784dd0`, `push 0x36` | 54 vertices | occlusion samples, drawn as `D3DPT_POINTLIST` (`0x4b5295`: `push 0x36`, `push 0`, `push 1` → `0x77a690(1, 0, 54)`) |
| flare alpha stream | `0x784f10`, `push 0x44`, `push 0x644` | 68 | index buffer for the alpha pass |
| flares stream | `0x784dd0`, `push 0x5a` | 90 vertices | the flare sprites, drawn as a triangle strip (`0x4b5b74`: `push 0x1e`, `push 0`, `push 4`) |
| flare alpha target | `0x786840`, descriptor `1,1,1,0x15,0x4d` | — | off-screen accumulation surface |

The sprite pass is driven by `ps/solarflare_14.txt`: six flare-alpha samples,
each weighted `0.4`, summed, saturated and multiplied into `c0.rgb`.

**Not recovered:** the per-flare placement and size of the 90-vertex flares
stream. That buffer is filled from the sun's screen position and the 54-point
visibility mask, and both the offsets and the sizes are computed at run time from
the light direction rather than read from a table — the function that fills it
(`0x4b4290`–`0x4b5ef4`, 7 KB) is not yet reduced to an equation. What *is*
recovered about it: the stream format is the same 28-byte vertex, the vertices
carry `0xbf000000` (-0.5) and `0x3f800000` (1.0) as their half-extents (the 36
references to `ds:0x8d5894` = 0.5 in that function), and two of the sprites are
anchored at `(0, -10, 0)` and `(0, -20, 0)` world units from the sun point
(`0x4b44f6`, `0x4b45e2`).

An earlier viewer revision anchored the six samples to the recovered sun and
moon quads inside the disc plane. That is not what the client does: the sprite
stream is placed from the sun's **screen** position, and its sprites are lens
ghosts, not sky-space billboards. The chain now hangs in screen space (§6.2).

Two more things about the pass, recovered and used by the viewer:

* `ps/solarflare_14` reads an alpha texture (`ps/solarflare_14.txt`) and samples
  it six times; the samples are summed, saturated and multiplied into `c0.rgb`.
  The weight per sample is `0.4`.
* The flare alpha target (`0x786840`) is a 1×1-format off-screen surface, which
  is how the client gates the whole chain: nothing is drawn over a disc that the
  54-point point-list says is hidden.

### 1.5 What changed in the viewer

| file | change |
| --- | --- |
| `src/sky_celestial.rs` | `HALF_EXTENT = 1.0` replaced by `SUN_HALF_EXTENT = 10.0`, `MOON_HALF_EXTENT = 20.0`, `SUNSHINE_HALF_WIDTH/HEIGHT = 60/20`, `SUN_SWELL = 2.0`; new `sun_half_extent(n)` and `shine_ramp(n, night_set)`; `disc()`/`discs()` take the night strength; tests assert 11.421°/22.619° and the swell |
| `src/sky_gpu.rs` | shader draws the recovered rectangles (20-unit sun, 120×40 strip concentric with the sun, 40-unit moon, flare ghosts in the same unit system), the `1 + 2*n` swell, the recovered shine window, and passes the night strength in `sets.w`; `DISC_SCALE`/`GLOW_SCALE` retired |
| `src/gpu.rs` | status line reports the client's quad sizes and the recovered shine strength instead of "viewer size"; the sun/moon status and the settings tooltip say 11.42°/22.62° |
| `src/sky_transition.rs`, `src/bin/phase5-sky-seam-check.rs` | tests and the seam check now expect the night set at index **1** (`sky_id_night = 1`); they still asserted the old index 2 |
| `src/bin/phase5-sky-transition-gpu-check.rs`, `src/bin/model-draw-gpu-check.rs` | build the modules `sky_gpu` now reads (`angelica_pck`, `sky_config`); the GPU check's texture array grew to 15 layers and enables the shine window through `sets.w` |

The strip is **concentric** with the sun, not offset below it: the client hangs
both quads from the emitted point (sun `y 0..-20`, strip `y 10..-30`), so their
centres coincide at `y = -10` and only the art's own bright core needs aligning
(`glow_center.xy`).

### 1.6 What the failing checks actually were

Both CI jobs are green on this branch (`Rust checks and tests (Linux)` 2m36s,
`Windows executable (x64)` 6m16s). The failures were **not** in the celestial
geometry work; every one of them was a stale expectation or an unbuildable
check target left by the day/night-set change:

| failure | cause | fix |
| --- | --- | --- |
| `rustc --test src/sky_transition.rs`, `phase5-sky-seam-check` | the night set moved from index 2 to 1 (`sky_id_night = 1`), but four `frame_sets` expectations and the seam check still expected 2 | expectations updated; the two tests that *started* on set 1 now start on set 2 so their swaps stay real changes |
| `cargo check --bins` (3 bins) | `sky_gpu`/`environment` now read `angelica_pck` and `sky_config`, which the standalone check binaries did not build | the five affected bins declare both modules |
| `cargo check --bins` | `sky_gpu` imported `sky_celestial`, which those binaries also do not build | `sky_gpu` keeps its own half-extent literals and says so |
| viewer unit tests (2) | `assert_eq!(entries[0].size, 3)` on a 6-byte fixture; `angular_radius(half) * 2 * DISTANCE == 2 * half` is an arc-length identity that only held at the old 1-unit quad | fixture compares against the payload length; the identity is now `tan(half) * DISTANCE == half` (the 0.07-unit difference at 10 units is why it broke) |
| viewer unit tests | `flate2::Flush` does not exist in flate2 1.x | `FlushDecompress::Finish` |
| Angelica directory scan | a decoy zlib header aborted the scan instead of being skipped, so `configs.pck` would have failed to read at run time | bad candidates `continue` |
| `cargo fmt --check` | new modules and one call site were not in rustfmt order/layout | reordered and reformatted |

One behaviour change came out of this: the GPU sky check
(`phase5-sky-transition-gpu-check`) now builds the 15-layer atlas the shader
samples and drives the shine through `sets.w` (night strength 0.45) instead of
the retired `DISC_SCALE` lane, so the sun/glow core alignment is still asserted
on the production shader.

---

## 2. Clouds: exactly where they stand

### 2.1 The client

* `CECWorld::LoadWorld` reads `world.cld`, an **MOXB** grid; `0x645150` parses a
  cell as `n` × 36-byte vertices plus `m` groups, each group a 20-byte header
  (four floats + index count) and that many 32-bit indices.
* `0x646450` locks an index buffer (`0x7855c0`, `0x2000`, stride `0x20`), walks
  the cell list and submits each group as a triangle list. The parent pass binds
  `textures/cumulus01.tga` (`0x645c5b`) and alpha blend states (`0x77a0e0`)
  before drawing.
* `0x646720` picks the atlas cell from the first vertex tag: subtract one, wrap
  into the 4 × 4 grid, quarter-texture UV interval.
* `textures/cumulus01.tga` and `textures/sky/cloud.tga` are **byte-identical**
  (`4bd980bc…`): 1024 × 1024 RGBA, 16 cells of 256 × 256, white RGB with the
  cloud shape in alpha.
* No time, wind or drift input exists in the cloud class: the surface is static
  geometry. `CloudDetail` (`0x94eb40`) is a UI slider label, not a config key
  read by this class; the slider only appears in the settings dialog code.

### 2.2 The viewer

`cloud_surface.rs` reads MOXB faithfully (bounds-checked 36-byte vertices,
20-byte group headers, indices). `cloud_gpu.rs` renders **one camera-facing
billboard per indexed vertex**, size `group_parameters[3]` quantised to
{20, 50, 70} × 0.5, day/night vertex colours, the recovered 4 × 4 atlas mapping
with a one-texel inset, alpha blended, fogged and depth-tested, behind the
`cloud_detail` toggle.

**State, explicitly:** the data reader and the atlas mapping are the client's;
the *geometry* is not. Retail submits the cell's own triangle lists with the
group's own four parameters, so cloud silhouettes, layering and the group scale
are still viewer approximations. Nobody has yet decoded the four group floats
(`CloudGroup::parameters`), which is the next concrete step — they are the only
thing standing between the current billboards and the native mesh.

---

## 3. Weather: exactly where it stands

### 3.1 The client

* `CECWorld::InitNatureObjects` (`0x428ff9`, `0x429174`) creates two
  `A3DParticleSystem` (the dumper labels it `A3DSuperSpray`) objects and loads
  `textures/rain.tga` (32 × 32 RGBA) and `textures/snow.bmp` (32 × 32 grey).
* Their tuned parameters are plain stores in the constructors, and the client's
  own debug dumper (`0x79e270`) names the fields they belong to:

| field | rain (`0x42901c`–`0x429067`) | snow (`0x42919f`–`0x4291eb`) | dumper label |
| --- | --- | --- | --- |
| `+0x1600` | 0.5 | 0.5 | Speed |
| `+0x1604` | 50.0 | — | `SpeedVariation: %f%%` |
| `+0x160c` | 0.04 | 0.3 | `Size: %f` |
| `+0x1610` | 30.0 | — | `SizeVariation: %f%%` |
| `+0x1790` | — | 40.0 | (integer field) |
| `+0x1794` | 8.0 | — | (integer field) |

* The area flag word (`world.ecbsd` record `+0x04`) selects rain (bit 1) and snow
  (bit 2) independently at `0x421520`.
* `0x429378` advances both objects through `vtable+0x48` on a strict 33 ms
  accumulator.

### 3.2 The viewer

`client_weather.rs` reproduces the clock (strict `> 33` ms, catch-up, u32 wrap)
and the flag selection, both unit tested. `weather_gpu.rs` draws depth-tested,
camera-centred precipitation using the client's own `rain.tga`/`snow.bmp`, the
native 33 ms tick count and the parsed water-area masks for the submerged view.

**State, explicitly:** textures, flags and clock are the client's, and since the
2026-11-04 follow-up the two objects' own `Speed`/`SpeedVariation`/`Size`/
`SizeVariation` stores feed the shader through two uniform lanes
(`client_weather::RAIN`, `client_weather::SNOW`, `weather_gpu::profile_lanes`):

| lane | rain | snow |
| --- | --- | --- |
| quad width (client `Size`) | 0.04 | 0.3 |
| width spread (client `SizeVariation`, read literally as ± that fraction) | ±30 % | none stored |
| descent (`Speed`) | 0.5 | 0.5 × `SNOW_SPEED_SCALE` = 0.03 |
| speed spread (`SpeedVariation`) | ±50 % | none stored |

The particle *population* is still not the client's: count, spawn box (a
camera-relative 56 × 72 units slab), fall range, streak length, alpha and the
flake descent scale are viewer values, and no rain→snow physics difference is
claimed beyond the numbers above. `SNOW_SPEED_SCALE` exists because both objects
store `Speed 0.5` yet the client's flakes visibly drift while its drops fall; the
two integer fields that presumably separate them (`+0x1790 = 40` for snow,
`+0x1794 = 8` for rain) are not decoded, and pretending one `Speed` explains both
would have made the flakes fall like drops.

---

## 4. Sky rotation: the client has the mechanism and the speed is zero

`A3DSkySphere::Tick` (`0x79a040`) calls `0x79a0e0` twice — once for the cap
`(matrix +0x504, state +0x4cc)` and once for the curtain
`(matrix +0x544, state +0x4e8)`:

```
0x79a0e0:
  fld  [obj + 0xc]                 ; rotation speed, radians per 1/30 s
  fcomp ds:0x8effc4                ; == 0.0 (or unordered) -> skip entirely
  ...
  fld  [obj + 0xc]
  fmul ds:0x8f14bc                 ; 0.0333333 = 1/30
  fadd [obj + 0x18]                ; accumulated angle
  fst  [obj + 0x18]
  call 0x789ec0 / 0x789f20         ; rebuild the texture rotation, half-angle 0.5
```

The two state blocks are members of the sphere (`+0x4cc` and `+0x4e8`, 28 bytes
each) and their speed fields (`+0x4d8` and `+0x4f4`) are **written exactly once
in the whole image** — by the sphere's own constructors (`0x42d1ec`, `0x42d21a`,
`0x42d4cc`, `0x42d48c`), with a register that is zero. There is no setter, no
config read and no region field that reaches them: the only other writes to
`+0x4d8`/`+0x4f4` in the image belong to an unrelated container class
(`0x42ebdf`, `0x42eebf`, `0x42f06a`, where `+0x4d8` is an item count and
`+0x4d4` an array).

**So the retail sky does not rotate.** It has a per-band texture rotation about
the vertical axis, rebuilt through the half-angle-0.5 matrices, and the shipped
client leaves its rate at zero. The viewer holding rotation at 0 is not a
limitation to close: it is the client's own behaviour, and the earlier note that
"the rate source is not identified" can be closed as *there is no rate source*.

The only animated part of the dome is the set switch: `+0x5b4` advances by 33
every frame to `+0x5b0` and then swaps (`0x79b180`), which the viewer already
models in `sky_transition`.

---

## 5. Residual list after this pass

1. **Flare sprite placement and size** (the 90-vertex stream) — still the one
   piece of the sun/moon chain generated at run time. The viewer now draws the
   same six samples in screen space (§6.2) with its own ghost ladder and size
   ladder, because the fill function is not reduced; the client's 54-point
   occlusion mask is likewise still missing, so a disc behind a mountain flares.
2. **The template's vertical anchor.** All three templates hang from their
   anchor (`y` runs `0..-height`), so the texture centre sits `height/2` below
   the emitted position. The viewer keeps the disc centred on the emitted
   direction; whether retail really draws the sun 5.7° below its own elevation is
   unproven, and the elevation curve itself is a recovered approximation.
3. **Cloud group parameters** — four floats per group, unassigned; decoding them
   is what turns the billboards back into the native mesh.
4. **Weather particle population** — sizes/speeds recovered and now consumed;
   spawn model, count and lifetimes still the viewer's.
5. The band `38.16°`–`72.5°` and the `0x799f88` object remain as documented in
   `P5-sky-geometry-daynight-celestial.md` §1.5 / §6.

---

## 6. Follow-up pass, 2026-11-04: overrides, flare sprite, weather constants

Scope: the World-tab overrides, the flare sprite, and the one weather item that
was recovered-but-unused. Nothing else was touched.

### 6.1 World-tab overrides

| control | field | what `Auto` is | what a fixed choice does |
| --- | --- | --- | --- |
| **Sky set** | `ViewSettings::sky_override` → `sky_transition::SkyOverride` | `0x4203f4` drives `sky_transition` from `autoscene.cfg`'s `sky_id_day`/`sky_id_night` through `sky.lst`, including the counted fades | publishes one pinned row `(row, row, 0.0)`, so no fade is ever drawn; the swap machine keeps running underneath so returning to `Auto` resumes the clock's own set instead of snapping to a stale one |
| **Weather** | `ViewSettings::weather_override` → `client_weather::WeatherOverride` | `0x421520`: EC-BSD `world.ecbsd+0x04` bit 1 = rain, bit 2 = snow, independent (a zone may ask for both) | forces none / rain / snow for every view (main, reflection, above-water) before the selections are distributed |

Both are persisted (`weather_override=`, `sky_override=`), default to `Auto`, and
a file that predates them keeps both on `Auto`. The sky rows are the shipped
`sky.lst` order: `0 晴天`, `1 阴天01`, `2 夕阳`. Note again that the shipped
night row is *overcast*, not a painted night panorama.

### 6.2 The flare sprite and the disc colour, as implemented

**The discs were twice too bright.** `0x4b405f`–`0x4b4080` packs each disc's
vertex diffuse as `([+0x48] & 0xfefefe | 0x01010100) >> 1 | fade << 24`: the
object colour is halved and floor-ed at 1/255 per channel.
`sun.sdr`/`moon.sdr` stage 0 is `TOP_MODULATE(TEXTURE, DIFFUSE)`, so retail
draws both discs at *half* their object colour. The viewer multiplied by the
texture alone, i.e. at twice that strength — this is the reported "sun too
white", and the shader now applies `HALFCOLOUR = 127/255` to both discs.

Honest scope: what is recovered is the *operation* (half, floored), not the
value of `+0x48`. Nothing in the record writes it in the disassembly that was
read; the `| 0x01010100` floor only makes sense for an opaque white, and white is
also the value that would leave this a pure 0.5. So the viewer applies the
recovered operation to the white default. If `+0x48` turns out to be a per-region
light colour, the multiply belongs there instead and the factor should be
`colour * 0.5`.

**The chain is one placeable sprite, not a ghost ladder.** Reading
`ps/solarflare_14` again: the six `t0..t5` samples are *one sprite's* coverage —
their alphas are summed at weight `0.4`, saturated, and multiplied into the
sprite colour `c0.rgb`. They are not six sprites. The sprites themselves come
from the 90-vertex strip, whose fill (`0x4b4290`-`0x4b5ef4`) computes their
positions and sizes from the sun's screen position and the 54-point occlusion
mask; that fill is still not reduced. The previous revision invented a ghost
ladder along the screen diagonal, which is not in the client, so it is gone.
What remains is:

* the **`flares.bmp` ring**, concentric with the sun at twice the sun's own quad,
  which is the one flare feature whose position is not in doubt and the only
  sprite whose own centre is empty — `sunflare.bmp` would add its bright middle
  on top of the disc that is already there, which is the opposite of what the
  "too white" report asks for;
* multiplied by the **recovered cover** — six flare-alpha samples
  (`sunflare.bmp` and `flares.bmp` at three scales each) at weight `0.4`,
  saturated;
* multiplied by the **recovered half colour** and the disc's visibility fade.

The chain is drawn in screen space: `sky_gpu::flare_lanes` projects the sun
direction with the same matrices the sky pass uses and returns its NDC position
(`glow_center.zw`) plus the weight (`celestial.w`); the screen aspect rides in
`tint.w` and the sun's own NDC half extent (0.143 at the pass's 70° field of
view) in `background.w`. The weight is zero for a reflected view and — the
second bug report — for **any disc at or below the horizon**, which is the same
`0x4b4035` test that stops the disc itself. A sun that has set submitted nothing
but its visibility fade, and the chain followed it below the horizon; it no
longer does.

**Still viewer-chosen:** the 2x ring scale. **Still missing:** the sprite
placement and the occlusion mask, so a disc behind a mountain still flares and
the retail multi-sprite chain is not reproduced.

### 6.3 What is still not implemented, sky-side

1. Flare sprite placement and size (above), and its occlusion mask. The chain is
   one concentric ring because that is the only position the recovered evidence
   pins; the client's generated strip (a 90-vertex buffer drawn as 30 triangles)
   is not reproduced.
2. The template's vertical anchor — the discs are still centred on the emitted
   direction rather than hanging `height/2` below it (§5.2). Unchanged on
   purpose: it would move the sun 5.7° and the elevation curve is itself an
   approximation.
3. Cloud group floats: four per group, still unassigned; the geometry is a
   billboard approximation (`cumulus01.tga` atlas + the 4×4 MOXB mapping are the
   client's).
4. The `38.16°`–`72.5°` band and the `0x799f88` object (§5.5).
5. Weather population: count, spawn box, fall range, streak length, lifetime.
6. Sky-dome rotation: recovered mechanism, **zero rate** in retail — nothing to
   implement (§4).
