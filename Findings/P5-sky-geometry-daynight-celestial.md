# P5.01–P5.03 — Sky geometry, day/night switch, sun and moon

Agent 5-A. Evidence: `elementclient.exe` (Drive id `19h1s6U7JR0uvVccqeL8ZTAVpVeal1X1X`,
6,000,640 bytes, image base `0x400000`) plus the `textures.pck` asset names in the
client config. Every address below is a virtual address in that image.

Nothing in this report is inferred from the look of the artwork: geometry comes
from executing the original builders under Unicorn, and the time rule comes from
the original update functions.

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

Assets (config `sky_image01..03`): `5\Roof.bmp`, `5\01.bmp`, `5\02.bmp`. Folders
present in `textures.pck`: `textures/sky/clear|evening|night2/{01.bmp,02.bmp,roof.bmp}`.
The cap samples `roof.bmp`, the two curtain islands sample `01.bmp` and `02.bmp`.

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
* There is **no crossfade anywhere**. The seam handling is "separate texture per
  half space, exact vertex reuse at the mirror plane". The previous
  `u < 0.08` / `u > 0.92` crossfade in `src/sky_gpu.rs` was removed with the
  approximation it propped up.

### 1.5 The band the native buffers leave open

The curtain rim is at 38.16° and the cap rim at 72.5°, so **38.16°–72.5° has no
geometry in either stream**. The native rim vertex carries alpha 0, so the
client shows the pass background there. The renderer reproduces exactly that:
`sky_geometry::project` returns `Region::CapEdgeGap` with `alpha = 0` for the
band and the shader mixes towards the opaque-pass clear colour
(`0.18, 0.32, 0.48`, the value `draw_opaque` uses when it draws the sky). This is
a reported gap, not a hidden blend; see section 6 item 1.

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

Angular size: `2 * atan(1/100)` = **1.1459°** half-angle 0.01 rad. The previous
experimental discs used half-angles 0.028/0.035 rad (3.2°/4°) and were gated on
`3.5 <= hour < 21`; both are replaced.

Two native quirks are reproduced rather than "fixed":

* both objects share the single recovered azimuth `+0x60`, so the moon is not
  placed opposite the sun;
* the disc placement uses the elevation ramps, **not** the stored
  `+0x54/+0x58/+0x5c` direction (`client_sun::sky_object_direction`, which negates
  the horizontal components and uses the day fraction as an angle). The old
  shader used that curve, which is why the discs sat in the wrong place.

Unsupported states are explicit: `sky_celestial::disc` returns
`BelowHorizon { elevation_degrees }`, `Hidden { fade }`, `MissingAsset(path)` or
`NotFinite(..)`, and the World settings tab prints the outcome of both discs via
`Renderer::celestial_status`.

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

1. **Sky vertex-shader projection.** The recovered buffers leave 38.16°–72.5°
   empty (curtain seen as a cylinder, cap as a sphere). Whether the client's per-vertex program (`W_TRIANGLE`, or the `W_SIN`
   used with `solarsky.txt`) stretches one of the two meshes into that band
   cannot be decided without the shader text: `shaders.pck` (8,225,973 bytes,
   371 entries) uses a footer key this repository cannot decode —
   `[end-280] = 0xa508bdc4` (guard expected `0xfdfdfeee`), `[end-268] = 0x853567c8`
   (expected `0xf00dbeef`), version `0x20002`, no zlib stream anywhere in the file.
   Until then the band is rendered as the pass background at the native rim alpha,
   and `Region::CapEdgeGap` reports it.
2. **Sky set identity.** `Sky/sky_id_day` / `Sky/sky_id_night` (auto-scene keys at
   `0x97b600`) select which `Textures\Sky\<name>` folders the day and night sets
   come from. That mapping lives in `configs.pck` (834,575 bytes, 67 entries, same
   undecoded footer). The viewer therefore uses `clear` for the day set and
   `night2` for the night set, and the `evening` set has no recovered consumer.
   The same package also fixes the two-way assignment of `+0x5a4` / `+0x5a8` to
   `5\01.bmp` / `5\02.bmp` (texture creation order in `0x7997a0`), which decides
   which panorama half covers the `+z` side; the viewer assumes `+0x5a4` = `01.bmp`
   following the config order.
3. **Sky rotation.** `0x79a0e0` advances an angle at `[+0x504+0xc] * (1/30)` per
   frame and rebuilds the texture transform through `0x789ec0`/`0x789f20`
   (half-angle 0.5). The rate is set from a config/region value that is not yet
   identified, so the viewer holds rotation at 0 instead of inventing a drift.
4. **Disc blending and halos.** The client's material states for the two discs and
   the flare effects (`solarflare_14.txt`, `sunflare.bmp`, `moonflare.bmp`) are
   pixel-shader work held in the same unreadable `shaders.pck`. The viewer adds
   `sun.bmp` and alpha-blends `moon.tga`, matching the two shipped asset formats,
   and says so in the settings tooltip.
5. **Night strength consumer.** `+0x70` (up to 1.75 before 03:00) feeds the
   lighting record path (`0x79aeb0`), not the texture weights. The viewer's
   lighting is owned elsewhere and is not wired to it yet.
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
