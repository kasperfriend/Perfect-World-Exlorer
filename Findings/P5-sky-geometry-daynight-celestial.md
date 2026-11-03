# P5.01–P5.03 — Sky geometry, day/night switch, sun and moon

Agent 5-A. Evidence: `elementclient.exe` (Drive id `19h1s6U7JR0uvVccqeL8ZTAVpVeal1X1X`,
6,000,640 bytes, image base `0x400000`) plus the `textures.pck` asset names in the
client config. Every address below is a virtual address in that image.

Nothing in this report is inferred from the look of the artwork: geometry comes
from executing the original builders under Unicorn, and the time rule comes from
the original update functions.

The client's own `configs.pck` (834,575 bytes, 67 entries) and `shaders.pck`
(8,225,973 bytes, 371 entries) were also decoded; section 0 records the container
format and `tools/extract-angelic-pck.py` reproduces the extraction. That closed
the three questions this report previously listed as blocked on them: which
shader (if any) covers the empty band, which `Textures\Sky` folders the day and
night sets name, and what the celestial material states and flare pipeline are.

---

## 0. The shipped config and shader archives

`configs.pck` and `shaders.pck` are **Angelica File Package** (`0x00020002`) files,
not the retail v2 PCK that `src/pck.rs` reads. They share the 276-byte record
layout (name, `u32` offset, `u32` size, `u32` compressed size at `+260/+264/+268`)
but not the trailer:

| Offset | Content |
| --- | --- |
| `0x000` | `u32 0x4dca23ef`, file size, `u32 0x56a089b7` |
| `0x00c` | file payloads; a stored file is raw bytes, a compressed one a bare zlib stream |
| — | directory: per entry an 8-byte header and a zlib stream of one 276-byte record |
| `-280` | `u32 0xa508bdc4`, `u32 version`, `u32 unknown` (configs `0x21cfb4f6`, shaders `0x21be2dbf`), ASCII `"Angelica File Package, Perfect World..."`, `u32 0x853567c8` at `+268`, entry count at `+272`, version again at `+276` |

`tools/extract-angelic-pck.py` validates the trailer, finds each directory record
by the zlib stream that ends exactly at the running position, and reads entries by
name — `--list`, `--cat <name>`, `--out <dir>`. Both archives decode completely
(67/67 and 371/371). `src/pck.rs` is unchanged: it still targets the retail
archive the viewer loads `textures.pck` from.

---

## 1. Geometry and projection (P5.01)

### 1.1 Addresses

| Item | Address | Notes |
| --- | --- | --- |
| `A3DSkySphere` constructor | `0x799610` | stores `+0x4c0` = 6 bands, `+0x4c4` = `0x3e9c61aa` (0.30543262 rad), `+0x4c8` = 50.0 |
| Horizon curtain builder | `0x79a330` | writes stream `this+0x584`; index stream `this+0x5a0`; counts `+0x58c`/`+0x590` |
| Polar cap builder | `0x79a810` | writes stream `this+0x588` |
| Cap draw | `0x799a40` | binds the `+0x504` or `+0x544` transform, cap texture `+0x54` |
| Curtain draw | `0x799b40(textA, textB)` | two passes over the whole buffer, second pass under a `z`-mirrored world matrix |
| Sky tick / commit | `0x79a040`, `0x79a0e0`, `0x79b150`, `0x79b180` | timer and deferred set swap (section 2) |
| Texture paths | `0x79ac6c`, `0x79ac80`, `0x79ac97`, `0x79aee8`, `0x79af06`, `0x79af1d` | `Textures\Sky\%s` |
| Texture creation | `0x7997a0` | `+0x5ac`, `+0x5a4`, `+0x5a8`, `+0x54`; early-out if `ds:0x988334 == 4` |

Assets: `A3DSkySphere::TransSky(this, roof, 01, 02, 0)` (`0x79ac30`) loads exactly
the three names the caller supplies through the format string `Textures\Sky\%s`.
The default environment in `element_client.cfg` (`[DefEnv]`) supplies
`Sky\clear\Roof.bmp`, `Sky\clear\01.bmp`, `Sky\clear\02.bmp`, which resolve to
`textures/sky/clear/{roof,01,02}.bmp` — the folders present in the packed
`textures.pck`. `autofamilyconfigs/sky.lst` holds the same triple for each named
set (section 2.3). The cap samples `roof.bmp`, the two curtain islands sample
`01.bmp` and `02.bmp`.

### 1.2 Curtain — 70 vertices, 288 indices, 96 triangles

Rows are `39.27 * [1, 0.5, 0.167, 0, -0.27]`; each row holds two 7-column islands
(azimuth step 15 degrees), `u = 1 - column/6`, position `50 * (cos a, 0, sin a)`.

| row | y | front island `v` (az 90…180) | back island `v` (az 0…90) | diffuse |
| --- | --- | --- | --- | --- |
| 0 (rim) | 39.2700005 | 0.05 | 0.95 | `0x00ffffff` (alpha 0) |
| 1 | 19.6350002 | 0.275 | 0.725 | `0xffffffff` |
| 2 | 6.5580897 | 0.42485 | 0.57515 | `0xffffffff` |
| 3 | 0.0 | 0.5 | 0.5 | `0xffffffff` |
| 4 | -10.6029005 | 0.5 | 0.5 | `0xffffffff` |

Each row's two island runs sum to one texture height (`v` and `1 - v`); the two below-horizon
rows both sit on the midline `v = 0.5`, so the image's lower half is never sampled there.

Elevation coverage, as seen from the sphere centre: the curtain is a cylinder of radius 50, so a ray meets it at `50 * tan(elevation)`; the rows span **-11.98° to +38.16°**. Quads are
`(v0, v0+14, v0+15), (v0, v0+15, v0+1)`; all 96 triangles have
`normal · centroid < 0`, i.e. they face the camera inside the sphere.

### 1.3 Cap — 145 vertices, 792 indices, 264 triangles

Pole `(0, 50, 0)` at `uv (0.5, 0.5)`, then 6 rings of 24 sectors:
`theta = 0.30543262 * ring/6`, `phi = 2*pi*sector/24`,
`(50 sin theta cos phi, 50 cos theta, 50 sin theta sin phi)` and the planar disc
mapping `u = 0.5 + x / (2*50*sin(step))`, `v = 0.5 + z / (2*50*sin(step))`.

| ring | y | ring radius |
| --- | --- | --- |
| 1 | 49.93523 | 2.54417 |
| 2 | 49.74109 | 5.08175 |
| 3 | 49.41809 | 7.60616 |
| 4 | 48.96709 | 10.11176 |
| 5 | 48.38912 | 12.58944 |
| 6 | 47.68585 | 15.03530 |

Elevation coverage: **72.5° to 90°**. Indices: pole fan `(0, i, i+1)`, then
`(a, c, d), (a, d, b)` per sector; all 264 triangles have a positive `x/z`
signed area.

### 1.4 Joins

* **Island join at azimuth 90°** — the same point is stored twice: front island
  column 0 has `u = 1`, back island column 6 has `u = 0`. The rasteriser never
  interpolates across the join, so the texture run reverses exactly at the
  meridian. Verified in `island_join_is_a_duplicated_column_with_a_reversed_texture_run`.
* **Meridian at azimuth 0°/180°** — no duplicate column exists. The second draw
  feeds the identical vertex with `z` mirrored, so the ring closes with the same
  `u` on both sides. Verified in `meridian_join_reuses_one_vertex_for_both_draws`.
* There is **no crossfade in the client anywhere**. The seam handling is
  "separate texture per half space, exact vertex reuse at the mirror plane". The
  previous `u < 0.08` / `u > 0.92` crossfade in `src/sky_gpu.rs` was removed with
  the approximation it propped up.
* **Viewer addition after the second test round:** the ray projector now samples
  both texture runs and crossfades them over 3.4° of azimuth around the 90°
  island join (`1 - smoothstep(0, 0.06, |az - pi/2|)`, 50/50 exactly at the
  join). The native hard switch left a visible cloud seam in the packed art, so
  the join is the one place where the viewer deliberately blends.

### 1.5 The band the native buffers leave open

The curtain rim is at 38.16° and the cap rim at 72.5°, so **38.16°–72.5° has no
geometry in either stream**. The native rim vertex carries alpha 0, so the
client shows the pass background there. The renderer reproduces exactly that:
`sky_geometry::project` returns
`Region::CapEdgeGap` with `alpha = 0` for the
band and the shader mixes towards the opaque-pass clear colour
(`0.18, 0.32, 0.48`, the value `draw_opaque` uses when it draws the sky).

**Nothing in the client fills the band.** The two draw paths
(`0x799a40` cap, `0x799b40` curtain) bind the transformed stream and set
fixed-function state; neither selects a vertex program, and the client's only
shader archive contains no sky program at all — `shaders.pck` holds 371 entries
(`moon.sdr`, `sun.sdr`, `sunshine.sdr`, `ps/solarflare_14.txt` and effects), and
`A3DSky`/`A3DSkySphere` never appear in it. The band is therefore a property of
the shipped buffers, not of a program this repository cannot read. What the
retail game may have drawn *over* it (clouds, fog) is not part of these two
objects and is not established here.

Two test rounds reported this band as a blue ring around the roof disc, so the
viewer now fills it instead of leaving it to one fixed colour. The band is drawn
as a gradient from the pass background at the curtain rim to the **cap's own rim
colour** at 72.5 degrees:

* the background is the frame's atmospheric fog colour (`camera.air_fog`), not a
  hard-coded `0.18, 0.32, 0.48`, so the lower end follows the region and hour;
* the upper end samples the roof texture on exactly the circle the cap mesh ends
  on (`0.5 + 0.5 * d.xz / |d.xz|`), so the gradient and the opaque cap meet
  pixel-for-pixel and no disc edge is visible;
* the curtain's own rim alpha (zero on the top row) still blends the skirt into
  the same fog colour, so the lower end joins too.

The mixer is `smoothstep(sin 38.16 deg, sin 72.5 deg, d.y)` in the WGSL branch
and `Region::CapEdgeGap` still reports the band, carrying the rim lookup. This is
a viewer-only composition: retail covers the band with an object this build does
not draw (`0x799f88` renders something taken from `[A3DSky+0x54]` through vtable
`+0x1c`, between the cap and the curtain draws) and that object is not
identified yet.

---

## 2. Day/night rule (P5.02)

### 2.1 Addresses

| Item | Address | Content |
| --- | --- | --- |
| Clock | `0x4b5f10` | `+0x68 += ms * ds:0x8db108` (double `6.944444e-8` = 1/14,400,000), wrapped at `ds:0x8d6138` = 1.0 |
| Authored time | `0x4b6030` | `+0x68 = wrap(time)`, then update |
| Update | `0x4b60e0` | `+0x4c` sun elevation, `+0x50` moon elevation, `+0x60` azimuth, `+0x70` night strength, `+0x74` night flag, `+0x54/+0x58/+0x5c` direction |
| Sun direction | `0x4b60b0` | `+0x60 = atan2(-z, -x)` from the region `sun_dir` |
| Set selection | `0x4203e0` | 8-way branch on `BYTE[esp+0x19c]`, `[sunmoon+0x74]` vs `0.0/1.0/0.8`, texture-coord vs `0.0/0.5`; each arm `0x79ac30(this, a, b, c, 0)` (texture set) and then `0x79aeb0(this, a, b, c, time)` (lighting record) |
| Switch length | `0x4b6250` | returns `0x249f00` (2,400,000) if `[+0x74] != 0`, else `0x1b7740` (1,800,000); caller halves it |
| Timer start | `0x79b150(this, n)` | `+0x5b0 = n`, `+0x5b4 = n` (clamped >= 1), `+0x5b8 = 1` |
| Timer step | `0x79a040` | `+0x5b4 -= 0x21` while `+0x5b8`; interpolates `(+0x504, +0x4cc)` and `(+0x544, +0x4e8)` through `0x79a0e0` with the same weight; commits through `0x79b180` at zero |
| Commit | `0x79b180` | releases `+0x5ac`, `+0x5a4`, `+0x5a8`, repopulates from `+0x8c8`, `+0x8cc`, `+0x8d0`, copies `+0x4e8 → +0x4cc` and `+0x544 → +0x504` |

### 2.2 Recovered schedule (`t = hour/24`)

| hours | sun elevation `+0x4c` | moon elevation `+0x50` | night flag `+0x74` | night weight |
| --- | --- | --- | --- | --- |
| 00:00–03:30 | 0 | `(1 - 6.8571429 t) * 60°` (60° → 0°) | 0 | `1 - t/0.0625` (1 → 0) |
| 03:30–12:00 | `(t - 0.14583333) * 197.6°` (0 → 70°) | 0 | 0 | 0 |
| 12:00–21:00 | `(1 - (t - 0.5) * 2.6666667) * 70°` (70° → 0°) | 0 | 1 from 18:00 | `(t - 0.75)/0.0833` from 18:00 (0 → 1) |
| 21:00–24:00 | 0 | `(t - 0.875) * 480°` (0 → 60°) | 1 | 1 |

`+0x70` night strength: 1.75 at midnight, 0 at 03:30, 0 through the day, 0 → 1
from 18:00 to 21:00, 1 to midnight. It is deliberately discontinuous at midnight
(1.0 → 1.75); `elevation_ramps_are_continuous_apart_from_the_midnight_strength_step`
pins both facts.

Switch lengths: night 2,400,000/2 = **1,200,000 clock-ms = 2 h** (18:00 → 20:00),
day 1,800,000/2 = **900,000 clock-ms = 1.5 h** (00:00 → 01:30). Because the timer
is proportional to the clock, the blend weight is a pure function of the hour:

```
night_weight(hour) = 1 - t/0.0625            t < 0.0625
                   = 0                       0.0625 <= t < 0.75
                   = (t - 0.75)/0.08333     0.75 <= t < 0.83333
                   = 1                       t >= 0.83333
```

This replaces the previous `smoothstep(5,8,hour)` / `smoothstep(17,20,hour)`
schedule. The shader no longer contains a day/night curve at all: `SkyFrame` is
filled from `sky_time::night_weight`, and the shader mixes the two recovered
texture sets with it. The set itself is picked by `+0x74` (18:00 today,
midnight back), which `night_flag_is_the_texture_set_selector` pins.

### 2.3 Where the texture names come from

`0x4203e0` calls `0x79ac30(this, roof, first, second, 0)` — `TransSky` — with the
three paths of the set it wants; `TransSky` then loads `Textures\Sky\%s`
(`0x79ac6c`, error string `A3DSkySphere::TransSky(), Can't load texture [%s]`) and
`0x79b180` commits the new set at the end of the timer. The paths themselves are
configuration data:

* `element_client.cfg`, `[DefEnv]` — the default environment:
  `sky_image01 = Sky\clear\Roof.bmp`, `sky_image02 = Sky\clear\01.bmp`,
  `sky_image03 = Sky\clear\02.bmp`.
* `autofamilyconfigs/sky.lst` — the named sets, 3 rows of `index, name, roof, 01, 02`:

| index | name | roof | first island | second island |
| --- | --- | --- | --- | --- |
| 0 | 晴天 (clear) | `clear\roof.bmp` | `clear\01.bmp` | `clear\02.bmp` |
| 1 | 阴天 (overcast) | `阴天01\roof.bmp` | `阴天01\01.bmp` | `阴天01\02.bmp` |
| 2 | 夕阳 (sunset) | `夕阳\wwwww.bmp` | `夕阳\01.bmp` | `夕阳\02.bmp` |

So a set is three configured paths, `sky_id_day`/`sky_id_night` select which set a
region asks for, and in this build's default environment both day and night name
the `clear` folder: what changes at dusk is the lighting (`+0x70` strength, the
terrain and model day/night switch) and the rim alpha, not the sky image.

**Deviation removed after the first test round.** `sky_gpu` used to draw `clear`
by day and the packed `night2` folder by night, because the night-strength
lighting consumer is missing (section 6 item 5). That substitution put a painted,
motionless moon into the night sky on top of the recovered billboard, so the
second round dropped it: one authored set now covers both phases, exactly as
`element_client.cfg` names `Sky\clear` for all three `sky_image` slots, and the
night look is the recovered regional light colour modulating that set (uniform
`tint`, filled from `RegionalLight::diffuse`). The switch timer is unchanged; only
the second art set is gone.

---

## 3. Sun and moon (P5.03)

| Item | Address | Content |
| --- | --- | --- |
| Render | `0x4b3e00`–`0x4b41ee` | one 4-vertex, 6-index billboard per object (`push 0x6` at `0x4b401a`) |
| Gate | `0x4b4035` | `fcomp ds:0x8d58a0` (0.0) on the object's own elevation |
| Placement | `0x4b3f69`–`0x4b3fb6` | `direction * ds:0x8d6a9c` = **100.0** |
| Extent | `0x8d6a94` = -0.5, `0x8d6a98` = 2.0 | corners at `±1.0` world units |
| Direction | `0x4b4085`–`0x4b40b1` | `(cos az cos el, sin el, sin az cos el)` |
| Alpha | `0x4b405f`–`0x4b4080` | packed diffuse `([+0x48] & 0xfefefe | 0x01010100) >> 1 | fade << 24` |
| Fades | `0x4b5f63`–`0x4b6021` | `+0x7c` sun, `+0x80` moon, `± ms * ds:0x8db100` (0.0003), clamped to [0, 1]; targets `+0x84`/`+0x85` |
| Textures | — | `textures/nature/sun.bmp`, `textures/nature/moon.tga` |

Angular size: `2 * atan(1/100)` = **1.1459°** half-angle 0.01 rad. This is the
whole recovered quad, so the second test round's "sun and moon are tiny" is the
native size: what makes the retail sun read larger is the `sunshine.bmp` /
flare pass of section 3.2, which is still unimplemented and is not faked by
enlarging the billboard. The previous
experimental discs used half-angles 0.028/0.035 rad (3.2°/4°) and were gated on
`3.5 <= hour < 21`; both are replaced.

Two native quirks are reproduced rather than "fixed":

* both objects share the single recovered azimuth `+0x60`, so the moon is not
  placed opposite the sun;
* the disc placement uses the elevation ramps, **not** the stored
  `+0x54/+0x58/+0x5c` direction (`client_sun::sky_object_direction`, which negates
  the horizontal components and uses the day fraction as an angle). The old
  shader used that curve, which is why the discs sat in the wrong place.

The first test round reported the status line claiming the sun was switched off
at every hour, and no moon at night. Both had one cause: `Environments::endpoints`
(the non-streamed selector) took the flag word and the authored direction from
the same box it picked for fog — the nearest containing box, which is a *local*
one whenever the player stands inside one. `0x420992` reads that word from the
independently selected **global** box instead (`+0x80`; the loader maps record
`+4` to area `+0x80` at `0x48a983..0x48a98e`), so a local box carrying 0 zeroed
both fades and no disc was ever submitted. The selector now keeps the two
selections apart exactly like `RegionStreaming::endpoints`, and a world with no
containing global box keeps the recovered constructor default `0x18` rather than
borrowing a local word. `Renderer::celestial_status` also prints the latched flag
word so the state is visible in the settings tab.

Unsupported states are explicit: `sky_celestial::disc` returns
`BelowHorizon { elevation_degrees }`, `Hidden { fade }`, `MissingAsset(path)` or
`NotFinite(..)`, and the World settings tab prints the outcome of both discs via
`Renderer::celestial_status`.

### 3.1 Material states (`shaders.pck`, `sun.sdr` / `moon.sdr` / `sunshine.sdr`)

All three are `version 5` Angelica effect files with the same two stages and the
same default `texturefactor 1 1 1 1`:

| Stage | Color | Alpha |
| --- | --- | --- |
| 0 | `TOP_MODULATE`, `TA_TEXTURE` × `TA_DIFFUSE` | `TOP_MODULATE`, `TA_TEXTURE` × `TA_DIFFUSE` |
| 1 | `TOP_MODULATE`, `TA_TFACTOR` × `TA_CURRENT` | `TOP_MODULATE`, `TA_TFACTOR` × `TA_CURRENT` |

with `texture TEX_NORMAL 0 Textures\nature\sun.bmp`, `...\moon.tga` and
`...\sunshine.bmp`. So each quad is texture × vertex-diffuse × texture-factor,
exactly the `fade << 24` alpha the render code packs: the disc's coverage comes
from the texture's alpha times the fade, and the texture factor provides the
per-state tint. The glow is not a separate size: `0x4b3e00` builds the sun's stage-1 factor from
the night strength, `factor = (1.0 - [+0x70]) * 5.0` (`ds:0x8d5c28` = 1.0,
`ds:0x8db0f8` = 5.0), times `0.8` (`ds:0x8d62dc`) while `[+0x74]` selects the
night set, and multiplies the `+0x44` light colour by it before submission; a
non-finite strength takes the literal `1.0` branch at `0x4b3e09`. The moon's
block (`0x4b4049`) instead halves its `+0x48` colour and scales nothing. That
rule is reproduced by `sky_celestial::glow_texture_factor`, which is tested but
not yet consumed by the shader.

No render-state call exists in the celestial render: `0x77bd00` is the only
`SetRenderState` wrapper (`vtable+0xc8`) and nothing in `0x4b3e00`–`0x4b41ee`
calls it, so the discs inherit the enclosing pass's blend state. Additive versus
alpha for `sun.bmp` therefore still rests on the asset formats, not on recovered
state.

### 3.2 The solar-flare pipeline (`CECSunMoon::Init`) — not implemented

The Sun/Moon object owns far more than two quads. Its init strings (`0x95a900`–
`0x95ac94`) enumerate: a *sun shine* stream and texture (`sunshine.bmp`), a
*flares* stream, a *flare alpha* stream, a *sun point* stream, an off-screen
*flare alpha accumulation target*, the pixel shader `shaders\ps\solarflare_14.txt`
and the textures `flares.bmp`, `sunflare.bmp`, `moonflare.bmp`.

`shaders.pck` holds that program (`ps/solarflare_14.txt`, 467 bytes): it samples
six flare-alpha textures `t0..t5`, adds their alphas with weight `0.4` each,
`saturate`s, multiplies the sum into `c0.rgb` and passes `c0.a` through — the fade
constant the render code supplies. Init also builds the streams the pass feeds
(`0x4b37d1`/`0x4b383b`/`0x4b38ac` create buffers with `0x36`, `0x5a` and `0x44`
element counts, the last through the index-buffer helper `0x784f10`), and creates
the accumulation target at `0x4b374b`–`0x4b3777` (`0x786960` on a `0x786840`
object; the descriptor carries `1, 1, 1, 0x15, 0x4d`). The semantics of the
individual create arguments are not yet pinned down, so the pass is documented
rather than approximated: combined with `flares.bmp`/`sunflare.bmp`/
`moonflare.bmp` this is a screen-space bloom-flare and it is entirely absent from
the viewer. Section 6 item 4 lists it as the remaining P5.03 work.

---

## 4. Commands and results

```
pip install --break-system-packages capstone pefile unicorn      # 5.0.7 / 2024.8.26 / 2.1.4
objdump -d -M intel google_drive/elementclient.exe > /tmp/dis/text.asm
python3 tools/emulate-sky-sphere.py            # Unicorn run of 0x79a330 and 0x79a810
python3 tools/verify-sky-geometry.py
```

```
surround   verts  70/ 70 indices 288/288 worst component delta 7.629e-06
roof       verts 145/145 indices 792/792 worst component delta 6.676e-06

both meshes match the emulated native buffers
```

(`surround`/`roof` are the harness names for the curtain and the cap; the residual
delta is x87 extended-precision sine/cosine versus f64.)

No Rust toolchain exists in this sandbox (`cargo`/`rustc` absent, no crates.io
access), so `cargo test` was **not** executed here. The Rust unit tests encode the
same golden numbers as the Python verification above and are the first thing to run
in CI.

---

## 5. Regressions added

`src/sky_geometry.rs` — `curtain_matches_the_emulated_native_buffer`,
`cap_matches_the_emulated_native_buffer`,
`combination_covers_both_rims_without_shared_vertices`,
`island_join_is_a_duplicated_column_with_a_reversed_texture_run`,
`meridian_join_reuses_one_vertex_for_both_draws`,
`projection_agrees_with_the_vertex_buffer_inside_the_curtain`,
`projection_agrees_with_the_vertex_buffer_inside_the_cap`,
`opposite_halves_use_the_front_and_back_textures`,
`cap_edge_gap_is_reported_and_invisible`,
`curtain_alpha_fades_to_zero_at_the_recovered_rim`,
`curtain_faces_the_camera_inside_the_sphere`,
`non_finite_directions_are_rejected`.

`src/sky_time.rs` — `clock_wraps_at_four_real_hours_per_day`,
`elevations_follow_the_recovered_piecewise_curve`,
`elevation_ramps_are_continuous_apart_from_the_midnight_strength_step`,
`night_flag_is_the_texture_set_selector`,
`night_weight_is_the_linear_switch_timer`,
`frame_timer_reaches_full_weight_at_the_commit`,
`frame_timer_weight_matches_the_hour_based_weight`,
`direction_is_unit_length_and_uses_the_authored_azimuth`.

`src/sky_celestial.rs` — `disc_size_matches_the_native_quad_at_distance_100`,
`sun_and_moon_are_placed_by_their_own_elevation`,
`quad_is_a_flat_two_triangle_billboard_of_the_recovered_extent`,
`visibility_fade_is_uploaded_as_a_truncated_alpha_byte`,
`unsupported_states_fail_explicitly`.

`src/sky_gpu.rs` — the two reflection tests plus
`sky_frame_defaults_to_the_recovered_day_state`.

Time boundaries covered: 00:00, 01:30, 03:30, 12:00, 18:00, 20:00, 21:00, 24:00.

---

## 6. Remaining gaps

1. **Sky coverage above 38.16°.** Resolved as far as the client is concerned: no
   vertex program exists for the sky (`shaders.pck` has none) and neither draw
   path selects one, so the two recovered buffers are the whole sky object and
   the band is empty in the client too. What the retail scene layers over that
   band — clouds or fog from other objects — is outside these two classes and is
   not established here. The viewer renders the same coverage, reports the band
   as `Region::CapEdgeGap`, and fills it with a fog-to-cap-rim gradient
   (section 1.5) until the object retail draws there is identified.
2. **Sky set identity.** Resolved: `TransSky` (`0x79ac30`) receives the three
   paths from configuration and prefixes `Textures\Sky\`; the default
   environment in `element_client.cfg` names `clear` for all three images and
   `autofamilyconfigs/sky.lst` lists the named sets (section 2.3). The viewer now
   uses that one authored set for both phases (section 2.3); the `evening` and
   `night2` folders stay loaded but are never sampled.
3. **Sky rotation.** `0x79a0e0` advances an angle at `[+0x504+0xc] * (1/30)` per
   frame and rebuilds the texture transform through `0x789ec0`/`0x789f20`
   (half-angle 0.5). The rate is set from a config/region value that is not yet
   identified, so the viewer holds rotation at 0 instead of inventing a drift.
4. **Disc blending and the flare pipeline.** `sun.sdr` / `moon.sdr` /
   `sunshine.sdr` are recovered (section 3.1) and match the viewer's texture ×
   fade alpha; the glow factor expression is reproduced and tested, but the
   shader does not consume it yet. The celestial render issues no
   `SetRenderState` call at all (`0x77bd00` is the only wrapper and it is not
   reached from `0x4b3e00`–`0x4b41ee`), so the blend state is inherited and
   additive-versus-alpha for `sun.bmp` is still unproven. The whole flare
   pipeline of section 3.2 — three buffers, an off-screen alpha accumulation
   target, three flare textures and the `ps.1.4` program — is documented but not
   implemented. This is the remaining P5.03 work.
5. **Night strength consumer.** `+0x70` (up to 1.75 before 03:00) feeds the
   lighting record path (`0x79aeb0`), not the texture weights. The viewer's
   lighting is owned elsewhere and is not wired to it yet; this is also why the
   night sky set deviates (item 2).
6. **Timer units.** The 33-per-frame step is treated as 33 clock-milliseconds
   (consistent with the `1/30` fixed step at `0x8f14bc`), which makes the switch
   lengths pure functions of the hour. No x87 accumulation site proving "ms" over
   "frames" was found.
7. **Texture transform lerp.** `0x79a0e0` also interpolates the two texture
   transforms (`+0x504` → `+0x544`); the viewer has no per-set transform to
   interpolate, so only the colour/UV mapping and the set weight are applied.

---

## 7. Files

* `src/sky_geometry.rs` (new) — recovered meshes, projection, joins, coverage.
* `src/sky_time.rs` (new) — clock, elevations, night flag/strength, switch timer.
* `src/sky_celestial.rs` (new) — native disc billboard, fades, explicit failures.
* `src/sky_gpu.rs` — shader now samples the recovered projection and mixes the two
  sets with `night_weight`; crossfade and smoothstep removed; `SkyFrame` input.
* `src/client_sun.rs` — added `Visibility::fade(sun)` for per-object fades.
* `src/gpu.rs` — feeds `SkyFrame`, added `Renderer::celestial_status`, replaced the
  experimental disc label.
* `src/main.rs` — module declarations.
* `tools/emulate-sky-sphere.py`, `tools/verify-sky-geometry.py` — Unicorn harness and
  the formula cross-check quoted above.
* `tools/extract-angelic-pck.py` — reads the `configs.pck`/`shaders.pck` container
  (section 0) so the sky texture table, the celestial effect files and the flare
  shader can be re-read from the same assets.
