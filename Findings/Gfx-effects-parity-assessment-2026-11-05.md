# GFX effects parity assessment: the emitter classes, measured against the retail corpus

This is a follow-up to [the container/runtime write-up](Gfx-effects-reverse-engineering-2026-11-05.md). That document ended with one native behaviour explicitly not copied — "the emitter shape classes were never located in the binary, so box and sphere spawns distribute uniformly inside the authored `AreaSize`/`Radius`". They are located now. This assessment records how, what the client actually does at particle birth, what the retail corpus says about how much that matters, the three places where the viewer was measurably wrong and is now corrected, and the five native behaviours that are measured, documented and still deliberately not reproduced.

Everything below was read out of the inspected `elementclient.exe` (image base `0x400000`, `.text` at RVA `0x1000`, `.rdata` at `0x4d5000`, `.data` at `0x54d000`) and measured over every `.gfx` member of the retail `gfx.pck`: 5,459 roots, 31,019 elements, 13,665 of them particle emitters. Neither the archive nor the executable is committed; the counts and addresses are the evidence trail.

## How the emitter classes were found

The `.gfx` keys do not exist as strings in the binary. Searching for `AreaSize` finds nothing, because the parser never compares key names: the *writer* prints them. What the executable holds is one `"Key: %fmt"` printf format string per key, collected into per-class pointer tables in `.data`:

| Table | Class | Entries |
| --- | --- | --- |
| `0x97d4a8` | keys every element shares | 33 |
| `0x97f1d4` | sprite (100/101/102) | 13 |
| `0x97f284` | particle block A | 20 |
| `0x97f2ac` | particle block B (`ParIniDir` format at `0x97f2cc`) | 10 |
| `0x97f3ec` | average generation (`IsAvgGen`, `AlphaSeg`, `BetaSeg`) | 3 |
| `0x97f130`, `0x97f180`, `0x97f018`, `0x97f074`, `0x97f0b0`, `0x97f428` | misc, ribbon 110, grid 210, child 200, model 160, ref model | — |

From a format string the reader is one xref away, and from the reader the object layout follows: each `sscanf` result is stored at a fixed offset, so the tables give an ordered key→offset map per class. That map is what turned the emitter sub-object from a guess into a known struct.

The classes themselves came from the constructors. Eighty-two vtable-write sites sit between `0x6e0000` and `0x740000`; the one that matters is the emitter factory `0x723ab0`, which dispatches `id − 100` through a six-case jump table at `0x723de4` into a shared base constructor `0x723250` (vtable `0x8edf24`) plus a per-class tail. Every particle element therefore owns an emitter object of ids 100…105 (editor side) and 120…125 (runtime side), allocated at a per-class size with a per-class vtable:

| Case | Ids | Size | Vtable | Constructor tail |
| --- | --- | --- | --- | --- |
| 0 | 100/120 | 0xb4 | `0x8ee0bc` | — |
| 1 | 101/121 | 0xe4 | `0x8ee078` | `+0xb4/+0xb8/+0xbc = 1.0` (the `AreaSize` default the corpus writes 1,361 times) |
| 2 | 102/122 | 0xd8 | `0x8ee034` | — |
| 3 | 103/123 | 0xf8 | `0x8edff0` | `+0xe4 = 0` (`IsAvgGen`), `+0xe8 = 10` (`AlphaSeg`), `+0xec = 10` (`BetaSeg`) |
| 4 | 104/124 | 0xe4 | `0x8edfac` | — |
| 5 | 105/125 | 0xd4 | `0x8edf68` | `+0xc4 = 1.0`, `+0xcc = 0` (`AvgGenSeg`), `+0xd0 = 1` (`IsAvgGenSeq`), plus a sub-object `0x70a600` at `+0x90` |

All six share the same vtable slot meanings, which is what makes the emission maths readable once: slot 1 samples the direction (`0x723440`), slot 2 the lifetime (`0x7237e0`), slot 3 the colour (`0x7237f0`), slot 4 the phase (`0x723a80`), slot 5 the scale (`0x723a20`), slot 6 the rotation (`0x723a50`), slot 10 is shared (`0x724b20`), and slots 7, 8, 9, 11…16 are per class — slot 8 being the birth routine that places a new particle:

| Id | Slot 7 (count) | Slot 8 (birth) | Other per-class slots |
| --- | --- | --- | --- |
| base | `0x892f56` | `0x724ab0` | read `0x723e00`, write `0x724460` |
| 120 | `0x726530` | `0x727230` | — |
| 121 | — | `0x727760` | 9 `0x729670`, 14 `0x7299a0` |
| 122 | `0x7288e0` | `0x7288f0` | 14 `0x7291d0` |
| 123 | — | `0x727e40` | 9 `0x729670`, 11 `0x727c20`, 12 `0x725c90`, 13 `0x725da0`, 14 `0x727c70`, 15 `0x727d70`, 16 `0x7288a0` |
| 124 | — | `0x729b40` | 9 `0x729670` |
| 125 | `0x7256b0` | `0x726570` | 9 `0x727220` (property dispatch `0x7256d0`, jump table `0x725aac`, key index `arg − 0xdb`), 11 `0x726ff0` |

The particle element calls those slots from its own emit routine `0x715b50`, takes the new node from the free list at `+0x18c/+0x194/+0x198`, and links it with the element back-pointers at node`+0x64/+0x68`. The birth routine's output buffer *is* the particle node — the offsets the earlier write-up attributed to a "temp slot" (`+0x00` position, `+0x0c` colour, `+0x10` scale, `+0x24` rotation, `+0x28` velocity, `+0x34` age) belong to the affector loop's 0x3c-byte copy, which is a different struct. On the node, `+0x00` is the age, `+0x04` the TTL, `+0x08` the rotation, `+0x0c` the packed colour, `+0x10…+0x1c` the position in millimetres, `+0x28` the drag term, `+0x44` the unit direction, `+0x50` the mirrored displacement, `+0x60` the speed, `+0x6c` the scale and `+0x74` the bind target.

## What the client does at birth

Element 123 — 9,869 of the corpus's 13,665 particle elements, so the dominant case by a factor of seven — branches three ways at `0x727e40`: `IsAvgGen` (emitter`+0xe4`) to `0x728420`, `IsSurface` (emitter`+0x84`) to `0x727f00`, and otherwise to the ordinary shape routine `0x728270`. That routine is the one the viewer has to match:

```
728 270:  three draws of rand() * (2/32767) - 1     ; 0x8e6efc = 2/32767
          retry while x*x + y*y + z*z > 1            ; rejection sampling
          out+0x10/0x14/0x18 = M * (x, y, z)         ; M at emitter+0xc0..+0xe0
          if (!IsBind) out += emitter+0x68           ; the emitter position
          if (IsDrag)  out+0x28 = emitter+0x68       ; 0x86, DragPow at +0x88
          out+0x34 = emitter+0xa0                    ; the emitter colour quaternion
          vtable[1](out+0x44)                        ; slot 1 = the direction
```

The matrix `M` is built by slot 9, `0x729670`, which ids 121, 123 and 124 share:

```
729 670:  if (emitter+0x85)                          ; IsBind
            row0 (+0xc0..+0xc8) = global(0x9c0358) * (AreaSize.x * 0.5)
            row1 (+0xcc..+0xd4) = global(0x9c0348) * (AreaSize.y * 0.5)
            row2 (+0xd8..+0xe0) = global(0x9c0338) * (AreaSize.z * 0.5)
          else
            the same three rows scaled by the basis handed in by the caller
```

`AreaSize` lives at emitter`+0xb4/+0xb8/+0xbc` (the constructor default of `1.0, 1.0, 1.0` for id 121 is the corpus's second most common value), the 0.5 comes from `0x8d5894`, and the three globals at `0x9c0338/0x9c0348/0x9c0358` are the world axes — initialized at run time, so their values are not in the file, but their role is fixed by the ordering. **The emission volume is therefore an ellipsoid with semi-axes `AreaSize/2`, sampled uniformly by volume, not a box of half-extents `AreaSize/2`.** Id 125 is the only class with a `Radius`, and its property dispatch writes a scalar at `+0xc0` rather than a matrix.

The direction sampler, slot 1 at `0x723440`, is shared by all six classes:

```
723 440:  basis = IsBind ? global(0x9c0338) : f(quaternion at +0x74..+0x80)
          if (Angle at +0x10 == 0.0 at 0x8d58a0) return basis
          alpha = rand() * (1/32767) * Angle         ; 0x8e6f10, no clamp
          beta  = rand() * (2*pi/32767)              ; 0x8e6f14
          two successive quaternion rotations through 0x7b6fd0
```

So the polar angle is uniform in `[0, Angle]` and the azimuth uniform in `[0, 2π)`. The lifetime, colour, scale and rotation slots are equally plain: `0x7237e0` draws the TTL, `0x723a20` and `0x723a50` lerp scale and rotation between their authored minima and maxima, and `0x7237f0` returns the shared colour when `ColorMin == ColorMax`, otherwise lerps four independent draws channel by channel and truncates each through the float-to-int helper `0x892e78` — with the alpha channel additionally multiplied by emitter`+0x8c` before its truncation.

## What was wrong, and what changed

Three of the viewer's emission rules disagreed with the above. All three are corrected in `src/gfx_player.rs` and each is now pinned by a test, because in every case the difference is distributional and a single sample cannot show it.

**1. `AreaSize` was read as a box.** `spawn` emitted `rng.signed() * area * 0.5` per axis — a box of half-extents `AreaSize/2`, corners included. The client emits an ellipsoid of the same semi-axes, so every corner particle the viewer produced is a particle the client never produces, and the viewer's volume is `8/((4/3)π) ≈ 1.91` times too large. This affects **11,928 elements** (id 121: 320, id 123: 9,869, id 124: 1,739), which is 87% of all particle elements in the archive. `AreaSize` is isotropic in the great majority of records — the corpus's most common triples are `(0.1)³` 1,457 times, `(1)³` 1,361, `(0.2)³` 840, `(0.5)³` 835, all-zero 504 — which is why the bound/unbound distinction in `0x729670` (world axes versus the caller's basis) rarely shows: an isotropic ellipsoid is invariant under the element's rotation. It is now `random_unit_ball(rng) * area * 0.5`, with the ball sample shared with the `Radius` path so both are uniform in volume. Test: `area_size_spawns_inside_its_ellipsoid` (4,000 spawns against an anisotropic `(2, 1, 0.5)` volume, asserting the ellipsoid inequality and that the semi-axes are actually reached) and `unit_ball_samples_fill_the_volume` (mean radius 3/4, which is what separates a volume sample from a surface one).

**2. The cone was sampled by solid angle and clamped to `[0, π]`.** `cone_direction` drew `z` uniform in `[cos(Angle), 1]`, which is uniform in solid angle, and clamped the angle first; the gate in `spawn` was `angle > 0.`. The client draws the polar angle uniform in `[0, Angle]` and never clamps. The corpus needs the native reading on both counts:

| `Angle` value | Elements | Client | Old viewer |
| --- | --- | --- | --- |
| `0` | 9,719 | straight along the basis | same |
| `6.283185` (2π) | 2,130 | whole sphere, uniform in polar angle | whole sphere, uniform in solid angle |
| `3.141593` (π) | 248 | whole sphere, polar angle axis→anti-axis | whole sphere, uniform in solid angle |
| `0.523599` (30°) | 416 | 30° cap, denser at the rim | 30° cap, denser at the pole |
| `0.052360` (3°) | 256 | narrow cap | narrow cap, different density |
| `0.349066` (20°) | 131 | 20° cap | 20° cap, different density |
| negative | 36 | cap mirrored through the emitter | collapsed onto a single ray |

A half turn and a full turn cover the same directions — the full turn folds `(π + t, β)` onto `(π − t, β + π)` — but both distribute by angle, not by solid angle, which puts visibly more particles near the axis and its opposite than the viewer did. The 36 negative angles were the hard failure: `angle > 0.` sent them down the `else` branch, so those emitters fired a single ray instead of a mirrored cone. The gate is now `angle != 0.` and the sample is `(sin α cos β, sin α sin β, cos α)` with `α = u₁·Angle`, `β = u₂·2π`, drawn in the client's order. Tests: `cone_polar_angle_is_uniform_in_the_angle` (half of a 90° cone's samples must fall inside 45° of the axis, where a solid-angle sample puts 0.293) and `cone_angle_is_never_clamped` (30° stays inside 30°, a full turn reaches behind the axis, a negative angle opens a mirrored cone rather than a ray).

**3. The colour sample rounded.** `random_color` computed `min + (max − min) * u` and rounded it. The client computes `min * (1 − u) + max * u` and truncates through `0x892e78`. The two differ by up to one unit per channel on almost every draw, and the truncated form is the same convention the verified static path already uses for alpha (`gfx_decal.rs:440`) and that controller 105 uses for its per-channel steps. Both the expression and the truncation are now the client's. Test: `color_channels_truncate_instead_of_rounding`, which replays the draw sequence and asserts equality with truncation *and* inequality with rounding, so the assertion has teeth.

Nothing else in the emission path moved. The draw counts per spawn are unchanged (three for a ball sample, two for a cone draw), so the player stays deterministic under a fixed seed and the corpus sweep stays comparable.

## Measured, documented, deliberately not reproduced

Five native behaviours are now understood well enough to describe and not well enough to copy honestly. Each is recorded in the source comments where the viewer diverges.

**`ParIniDir` is three Euler angles, not a direction vector.** The parser at `0x7242e2` reads the key's `%f,%f,%f`, passes the three floats to `0x7b6f10` — which halves each argument by the 0.5 at `0x8eff98` and takes sines and cosines of the halves, the `sin(a/2)cos(b/2)…` expansion of three successive rotations, i.e. Euler→quaternion — then rotates the quaternion already at emitter`+0x74…+0x80` by it and stores four floats at `+0xa0…+0xac`, which the birth routine copies into node`+0x34`/`+0x50`. The corpus agrees with the reading: the only non-zero values are exact degree conversions (`0.017453` = 1°, `1.570796` = 90°), which no unit direction would be, and about a hundred of the 13,665 particle elements author one at all (9,326 write zeros). The viewer still normalizes `ParIniDir` as a vector in `emission_axis`, which is wrong for those hundred and right for the rest. Reproducing it needs the provenance of the quaternion at `+0x74`, which is written outside the parse path and was not recovered; guessing it would trade a known small error for an unknown one.

**`IsUseHSVInterp` selects a second colour path.** `0x7237f0` tests emitter`+0xb0` at `0x72380c` and branches to `0x723812`, which lerps the HSV copies at `+0x38…+0x44` and `+0x48…+0x54` per channel — hue wrapping at ±1.0 — and converts back through `0x7898f0` (HSV→RGB). Those copies are made at parse time by `0x724422…0x724444`, which always runs the authored RGB through `0x789ad0` (RGB→HSV, unpacking ARGB and scaling by `1/255` at `0x8f0928`). **168 elements** ask for it (id 123: 161, id 124: 4, id 125: 3). The viewer lerps in RGB for all of them; the difference is a hue-path artefact on interpolated colours, visible as a grey band where HSV would sweep through the hue.

**Emission count is detail-scaled.** Two gates reduce how many particles the client spawns, and the viewer applies neither, which means it always renders at maximum detail:

- `0x715b88…0x715bfd`, inside the emit routine: when the flag at root`+0x2a0` is set, the count is multiplied by `(root+0x298 * global@0x9c1318) / root+0x29c` and truncated, but only if that factor is below 1.0 and the divisor at root`+0x29c` passes a range check against the two constants `40.0` (`0x8edaac`) and `10.0` (`0x8edaa8`) — above 40 always, between 10 and 40 only when the count exceeds 3, at or below 10 never. The global at `0x9c1318` sits in `.data` past the end of the raw section, so it is initialized at run time and the actual factor is not recoverable statically; the shape of the check reads as a distance-or-budget denominator against the user's particle-detail setting.
- `0x70ba00`, the element update: `Priority` (element`+0xdc`, constructor default 4 at `0x6f8fdb`, values 1…4 across ~30,000 corpus records) is compared against a detail threshold from the settings object (`call 0x41a3c0`, threshold at `+0x134`) and the element is skipped when it loses (`0x70bd0b`). `Priority` is a level-of-detail gate, not a sort key.

**`IsSurface` needs the owner model.** `0x727e40` diverts to `0x727f00` when emitter`+0x84` is set, which places particles on the surface of the bound model and needs its triangles. **2,665 elements** author it (id 123: 1,520, id 124: 1,036, id 120: 86, id 121: 11, id 125: 12). The viewer has the owner's model queue but no triangle sampling path into the emitter, so these particles spawn from the element's ordinary volume instead of the mesh. Id 120's surface block also uses `π/2 − Angle` (`0x8ee228`) rather than `Angle`, a second reason not to approximate it.

**Ground snapping at spawn needs terrain.** `0x715cee` gates a call to `0x41a430` with the position at node`+0x10` and writes the returned height into node`+0x14`, when `(IsBind && element+0x1b5) || element+0x23e`. The viewer's player has no terrain handle at effect level, so `MatchGrnd`/`IsStartOnGrnd` remain parsed and unused for particles (the static decal path does use ground matching, through a different route).

Two smaller divergences are noted in the source rather than fixed: the alpha factor at emitter`+0x8c` that `0x7237f0` folds into the alpha channel is the element's live alpha, which the player already applies once while building the quad, so applying it at spawn as well would double-count it; and id 125's average-generation sequence (`IsAvgGen` on 373 of its 382 elements, `AvgGenSeg`, `IsAvgGenSeq`, dispatched at `0x7256d0` through the jump table `0x725aac`) plus id 123's `IsAvgGen` branch to `0x728420` (45 elements) are counted but not decoded — `AlphaSeg`/`BetaSeg` (6,641 records, 6,604 of them the constructor default 10) belong to the same family.

## Parity census

The player reads 137 of the 205 fields `gfx_file` exposes; the remaining 68 are parsed, kept and enumerated in the `gfx_file` audit output rather than dropped, so a later pass can pick them up without re-reading the grammar. Measured over the corpus:

| Group | Measurement |
| --- | --- |
| Roots | 5,459 members, 5,457 parse (one empty container, one version-4 root that predates the client's own gate of 24), 31,019 elements |
| Particle elements | 13,665: id 120 1,315, 121 320, 122 40, 123 9,869, 124 1,739, 125 382 |
| `Angle` | 9,719 zero, 2,130 at 2π, 416 at 30°, 256 at 3°, 248 at π, 131 at 20°, 36 negative; range ±6.283185 |
| `Speed` | min −100, max 150, 10,590 zero, 340 negative |
| `EmissionRate` | max 20,000 |
| `TTL` | max 1e6 |
| `AreaSize` | 912 distinct triples, only on ids 121/123/124 (11,928 elements) |
| `Radius` | 15 distinct values, only on id 125 (382 elements) |
| `DragPow` | 9,854 records, max 10; `IsDrag` on 242 |
| `ParIniDir` | 9,326 zeros, ~100 exact degree conversions |
| `IsSurface` | 2,665; `IsBind` 8,259 (id 123 alone: 5,682) |
| `IsUseHSVInterp` | 168 |
| `IsAvgGen` | id 123: 45, id 125: 373; `AlphaSeg` only on id 123 (6,641, default 10 in 6,604) |
| `Priority` | ~30,000 records, values 1…4 |
| `ScaleNoOff` | 249; `IsUseParUV` 10 |
| Root keys | `UseAABB` 3,490, `ShakeCam` 241, `Raytrace` 109, `NoChangeDir` 81, `2DBackLayer` 12, `2DRender` 3, `PhysExist` 0 |
| Children | 1,530 `GfxPath` references, 1,448 resolve inside the archive, 82 do not (31 distinct authoring mistakes), 484 distinct child roots loaded by the sweep |

## Verification

The three corrections were compiled and tested against a vendored glam 0.27 with `encoding_rs` replaced by a byte-preserving latin-1 stub, which is the only offline configuration available here; `src/modules/**` and `gfx_effect_gpu.rs` are compiled by CI alone. 42 library tests pass, five of them new (the ellipsoid, the ball volume, the polar-angle distribution, the unclamped cone and the truncating colour sample). One test fails in the offline crate only — `gfx_file::tests::gbk_decode_reports_damage` asserts real GBK behaviour that the stub cannot provide — and passes in CI.

The whole-corpus sweep was re-run after the changes: every root parsed, a player built per root with children resolved against the corpus itself, 100 frames of 100 ms advanced each, then frame zero compared against `gfx_decal`, the independently verified static reader, for every root that reader accepts.

```
frames 100 x 100 ms / files 5459 / parsed 5457 / played 5457 / child-roots 484 /
quads 19861347 / max-per-frame 4079 / max-particles 4257 / finished 2658 /
warnings 84 / cycles 0 / PARSE 2 / BUILD 0 / ADVANCE 0
decal equivalence: roots 13, quads 19, mismatches 0
```

The two parse failures are the known pair (an empty container, `gfx/人物/技能/羽芒/翼击击中.gfx`, and the version-4 `gfx/sgc/刺客_致盲_技能击中.gfx`). All 84 warnings are unresolved `GfxPath` children — retail files that do not ship — and no root warns about a cycle or a nesting depth. The earlier key census counted 82 such references over 5,458 files; this sweep covers 5,459, because it recurses rather than globbing `*.gfx` and so picks up the one member named exactly `.gfx`. The decal equivalence line is the one that matters for these changes: the sprite path the repository already verified against the client's decal calls is byte-identical to the player's frame zero after the emission rewrite, so the corrections touched particles only.

The sweep harness and the corpus live outside the repository (the archive is a client asset and is not committed); the harness configuration above is what makes its numbers reproducible.

## Fixed while writing this up

`gfx_light` built its controller-108 `ColorTrack` from the file's raw `StartTime`/`EndTime`, which are milliseconds, while a track's window is in seconds (`gfx_color_track`: "Start/end are seconds; segment lengths are milliseconds") and `Emitter::tick` samples it with the seconds elapsed since the element's own `StartTime`. A controller starting at 500 ms therefore never opened — the light held its keypoint colour for its whole life — and a window that ended never expired. Every existing test authored `StartTime: 0` with `EndTime: -1`, where the two units coincide, which is why it stayed latent. The window is now converted at construction and the segment lengths stay in milliseconds, as the track's table walk expects; `the_color_ramp_window_is_converted_from_milliseconds` pins a non-zero window against the untouched keypoint colour.

The same reading opened a question this pass does **not** settle. `ColorTrack::sample` gates on its own `start`/`end` and then walks the 128-entry tables from zero of the same argument, so one value carries both the gate and the walk origin. `gfx_light` passes unshifted seconds since the element's `StartTime`; `gfx_player` passes `age − control.start / 1000` for controllers 108 and 111, which moves the gate as well as the walk, so a controller starting at 500 ms would open at 1,000 ms of element time. Controllers 100 and 101 were read out of the binary as skipping while `age < StartTime` yet integrating the *absolute* `age`, which favours `gfx_light`'s unshifted form, but the controller-108 update routine was not re-read this pass and neither call site was changed on a guess; both carry a comment saying so. Settling it needs the executable again.

## Still open

Unchanged from the previous write-up and not addressed here: the fractal-noise generator behind controllers 107, 109 and 112 (241 root controllers in the corpus, all type 112 — the most-used unverified path), `gfx_light` passing raw milliseconds into `ColorTrack::new` whose `sample()` takes seconds (latent and harmless), element 190 curves being counted rather than drawn, models/sounds/lights belonging to other subsystems, and the 82 child references to files that do not ship.
