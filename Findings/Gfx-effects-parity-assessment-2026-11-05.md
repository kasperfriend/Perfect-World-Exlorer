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
          out+0x34 = emitter+0xa0                    ; the composed orientation quaternion
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

Six native behaviours are described below. Five are understood well enough to describe and not well enough to copy honestly; the sixth, `IsSurface`, is now reproduced for the two emitter classes whose samplers are decoded and still described for the other two. Each is recorded in the source comments where the viewer diverges. A seventh, `IsUseHSVInterp`, was reproduced on 6 November from the binary alone; its entry is kept below and marked, and the decode is in *The HSV colour path, reproduced*.

**`ParIniDir` is three Euler angles, not a direction vector.** The parser at `0x7242e2` reads the key's `%f,%f,%f`, passes the three floats to `0x7b6f10` — which halves each argument by the 0.5 at `0x8eff98` and takes sines and cosines of the halves, the `sin(a/2)cos(b/2)…` expansion of three successive rotations, i.e. Euler→quaternion — then rotates the quaternion already at emitter`+0x74…+0x80` by it and stores four floats at `+0xa0…+0xac`, which the birth routine copies into node`+0x34`/`+0x50`. The corpus agrees with the reading: the only non-zero values are exact degree conversions (`0.017453` = 1°, `1.570796` = 90°), which no unit direction would be, and about a hundred of the 13,665 particle elements author one at all (9,326 write zeros). The viewer still normalizes `ParIniDir` as a vector in `emission_axis`, which is wrong for those hundred and right for the rest. Reproducing it needs the provenance of the quaternion at `+0x74`, which is written outside the parse path and was not recovered; guessing it would trade a known small error for an unknown one. *(Superseded in part — the writer is recovered: `0x716025`, inside the element's spawn loop at `0x715f00`, with `0x71b450` recomposing `+0xa0` immediately afterwards at `0x716040`. See "The emitter object" in the reverse-engineering document. The two source quaternions it combines — `&element+0x21c` and `[esp+0xb8]`, through the four-argument `0x7b7010` — are still unidentified, and so is the condition under which that loop runs, so the composition would still be a guess rather than a reading.)*

**`IsUseHSVInterp` selects a second colour path.** *(Superseded 6 November: this is no longer a divergence, it is implemented; see the section below. The original measurement is kept as written.)* `0x7237f0` tests emitter`+0xb0` at `0x72380c` and branches to `0x723812`, which lerps the HSV copies at `+0x38…+0x44` and `+0x48…+0x54` per channel — hue wrapping at ±1.0 — and converts back through `0x7898f0` (HSV→RGB). Those copies are made at parse time by `0x724422…0x724444`, which always runs the authored RGB through `0x789ad0` (RGB→HSV, unpacking ARGB and scaling by `1/255` at `0x8f0928`). **168 elements** ask for it (id 123: 161, id 124: 4, id 125: 3). The viewer lerps in RGB for all of them; the difference is a hue-path artefact on interpolated colours, visible as a grey band where HSV would sweep through the hue.

**Emission count is detail-scaled.** Two gates reduce how many particles the client spawns, and the viewer applies neither, which means it always renders at maximum detail:

- `0x715b88…0x715bfd`, inside the emit routine: when the flag at root`+0x2a0` is set, the count is multiplied by `(root+0x298 * global@0x9c1318) / root+0x29c` and truncated, but only if that factor is below 1.0 and the divisor at root`+0x29c` passes a range check against the two constants `40.0` (`0x8edaac`) and `10.0` (`0x8edaa8`) — above 40 always, between 10 and 40 only when the count exceeds 3, at or below 10 never. The global at `0x9c1318` sits in `.data` past the end of the raw section, so it is initialized at run time and the actual factor is not recoverable statically; the shape of the check reads as a distance-or-budget denominator against the user's particle-detail setting.
- `0x70ba00`, the element update: `Priority` (element`+0xdc`, constructor default 4 at `0x6f8fdb`, values 1…4 across ~30,000 corpus records) is compared against a detail threshold from the settings object (`call 0x41a3c0`, threshold at `+0x134`) and the element is skipped when it loses (`0x70bd0b`). `Priority` is a level-of-detail gate, not a sort key.

**`IsSurface` needs the owner model.** *(Corrected 6 November: the sampler reads no model at all, so the interpretation and the blocker do not stand — see the next entry. The divert address and id 120's `π/2 − Angle` stand. The element counts were re-measured later the same day and the id-124 figure below is 9 short: see the census note.)* `0x727e40` diverts to `0x727f00` when emitter`+0x84` is set, which places particles on the surface of the bound model and needs its triangles. **2,674 elements** author it (id 123: 1,520, id 124: 1,045, id 120: 86, id 121: 11, id 125: 12). *(The 2,665/1,036 figures recorded earlier came from a raw text census that undercounted id 124 by nine; two independent counts now agree — the parser, walking every corpus element, and a fresh `GFXELEMENTID`-split scan of the archive text, which also finds 13,675 `IsSurface` keys across ids 120 to 125 and no value other than 0 and 1. The other per-id counts are unchanged.)* The viewer has the owner's model queue but no triangle sampling path into the emitter, so these particles spawn from the element's ordinary volume instead of the mesh. Id 120's surface block also uses `π/2 − Angle` (`0x8ee228`) rather than `Angle`, a second reason not to approximate it.

**Surface emission is the emitter's own ellipsoid, and it is not blocked on the model.** Every memory operand inside id 123's surface sampler `0x727f00` is based on `%esp` (88 of them), `%esi` (16, the emitter) or `%edi` (8, the `rand` IAT slot it calls twice). There is no third object, no vertex buffer and no triangle list. The only emitter fields it reads are the nine basis floats at `+0xc0…+0xe0` — the same matrix the volume sampler `0x728270` multiplies by, and neither routine reads `AreaSize` at `+0xb4…+0xbc` directly. So "surface" means the surface of the emitter's own `AreaSize` ellipsoid. What was missing then was narrower than a model dependency: the vector library the sampler is written in. `0x401ca0` and `0x401cc0` construct and convert, `0x4093d0` and `0x4071f0` are the two binary operations that build a direction out of a quaternion and a global axis, `0x633700` is called with 2.0 and with a doubled quaternion component, and `0x401d10` once. *(All eight are decoded now — see the next entry and the library table in the reverse-engineering document.)* The routine's shape is readable without them: two `rand()` draws, each scaled by 2π/32767 (`0x8e6f14`), each turned into a quaternion by `0x7b6fd0` about the globals `0x9c0348` and `0x9c0358`, combined into a direction, then multiplied into the three basis vectors at `+0xc0`, `+0xcc` and `+0xd8` alongside a negated copy.

**Id 123's surface sampler is decoded, and reproduced.** *(6 November, later the same day.)* With the vector library decoded, `0x727f00` reads end to end. It spends two `rand()` draws. Each is scaled by 2π/32767 (`0x8e6f14`) and turned into a quaternion by `0x7b6fd0`; the first is about the global axis at `0x9c0348`, and the cross/scale/add chain that follows evaluates `u + 2w(v x u) + 2(v x (v x u))`, which is the rotation of the global axis at `0x9c0358` by that quaternion. The second draw rotates `0x9c0348` about the first rotation's own output (`0x728004` takes it as its axis argument), which sweeps the sphere uniformly. Call the result `D`.

The position is a linear combination, not a component-wise product: the three `0x401ca0` constructions at `0x7280ff`, `0x728132` and `0x728165` build `D.x * basis0`, `D.y * basis1` and `D.z * basis2` from the nine floats at `+0xc0…+0xe0`, and `0x7281d5` and `0x728202` sum them into `node+0x10`. Since the bound basis builder `0x729670` makes those vectors `AreaSize * 0.5` along the three axes, the sample lands exactly on the ellipsoid — the same matrix the volume path multiplies a unit-ball sample by, which is why the two differ only by the sample's radius. The direction written to `node+0x44…0x4c` (`0x7281ad` through `0x7281b3`) is the negated `D` built by the `fchs` chain at `0x72816a`: the sampler emits inward along the radius, and because id 123's birth flow calls the TTL slot next rather than the direction slot, the emission cone never applies to a surface particle. The tail is the volume path's tail: `+= emitter+0x68` unless `IsBind` (`0x728216`), `IsDrag` copies the emitter position to `node+0x28`, and `node+0x34` takes the four floats from emitter`+0xa0` — which are the composed orientation quaternion, not a colour *(corrected; see "The emitter object" in the reverse-engineering document)*.

The bound basis pairs `0x9c0358` with `AreaSize.x`, `0x9c0348` with `.y` and `0x9c0338` with `.z`, which identifies the two globals as the x and y axes without needing their run-time values. The viewer now reproduces this for id 123 (`surface_direction` in `gfx_player.rs`, five tests: the ellipsoid equation holds with equality, the direction negates the sample, the samples cover the sphere in both signs on every axis, exactly two draws are spent, and another class keeps the volume path).

**Verified against the archive.** Two corpus tests now live in `gfx_player.rs` and skip when the archive is absent, so CI is unaffected and the harness survives the loss of a scratch directory. The first walks all 5,459 members, finds the same **1,520** id-123 surface elements the census counted, and checks each against its own authored `AreaSize`: the direction is unit length, the position equals `-direction * (AreaSize * 0.5)` component-wise, and where no extent is zero the ellipsoid equation holds with equality. **94 of the 1,520** author a zero extent, which collapses the ellipsoid to a plane or a line, and that is why the invariant is written component-wise instead of as a division by the semi-axes; none author a negative extent and none omit `AreaSize`. The second plays all 5,457 parseable files for 120 frames of 16 ms with children resolved from the same archive: **34,416,130 quads, none of them non-finite**, 1,151 files reaching completion, and the same **64 distinct warnings** as the earlier sweep. The quad count is not comparable to the 19,861,420 recorded above, which a since-lost harness produced with frame parameters that were never written down. It is not invariant under a sampler change either: adding id 124's cylinder took this harness from 34,416,130 quads to 34,416,109, because a sampler that spends a different number of draws shifts the stream feeding every later random value in the same element. The parsed, played and distinct-warning counts are comparable, and they match.

**Id 124's surface is a cylinder, and it is reproduced too.** *(6 November.)* Id 124's `0x729be0` opens with the identical call chain — draw, `0x7b6fd0`, cross, copy, cross, copy, two scalings, two adds — but about `0x9c0338` rather than `0x9c0348`, and the second half is different in kind. Its second draw is not an angle: `0x729cc4` scales it by `2/32767` (`0x8e6efc`) and `0x729cca` subtracts `1.0` (`0x8d5c28`), giving a uniform in `[-1, 1]`, and there is no second quaternion at all. The three basis products at `0x729cd2`, `0x729cf6` and `0x729d1c` therefore take their coefficients from the rotated vector's **x and y only** — its z is zero and never read — with the raw second draw standing in for the third. Since the bound basis pairs `0x9c0338` with `AreaSize.z`, the sample is `(cos θ · Ax/2, sin θ · Ay/2, u · Az/2)`: the lateral surface of the cylinder the extents describe, sweeping a full turn around z and the whole height, rather than id 123's ellipsoid.

Its direction differs too. The sampler writes only node+0x10/0x14/0x18, the `IsDrag` copy at node+0x28 and the colour at node+0x34 — never node+0x44 — and the birth flow at `0x729bb9` calls the direction slot `vtable[1]` with `&node+0x44` after *both* samplers, then the TTL slot. Id 123's flow at `0x727ed4` goes straight to the TTL slot. So a surface particle keeps the emission cone on id 124 and loses it on id 123, which is exactly the kind of per-class divergence that made borrowing one formula for the other a guess worth not making. The tail contract is otherwise the same: position `+= emitter+0x68` unless `IsBind` (`0x729d83`), and node`+0x34` from emitter`+0xa0` (the composed orientation, as above).

Id 124 is implemented as `cylinder_sample` with four tests, including a corpus test over the archive's **1,045** id-124 surface elements: the cross-section sits on the ellipse to within 1e-3 and the height stays inside the extent, with **341** of them authoring a zero extent, which collapses the cylinder onto an axis or a plane and is skipped for the cross-section check. That leaves ids 120 and 121 — **97 of the 2,662 elements that matter**. Id 121's `0x727800` spends ten draws and calls none of the vector helpers at all, so it is a different algorithm outright; id 120's block at `0x7272aa` runs the same chain but adds a call to `0x70a320` and uses `π/2 − Angle`. Id 120's block is read in full now (`0x7272aa`–`0x7275d6`): one uniform draw, a rotation about the vector part of the emitter's composed orientation at `+0xa0` (`0x7b6fd0` at `0x7272d1`, then the Rodrigues expansion spelled out with `0x401ca0`, `0x4093d0`, two `0x633700` and two `0x4071f0`), a second draw of `π/2 − Angle` (`0x8ee228` at `0x72737b`, the subtraction at `0x727381`, `0x7b6fd0` again at `0x727395`), and both handed to `0x70a320` at `0x727448` — which takes four floats, stores them at `this+0xc/+0/+4/+8` and returns with `ret $0x10`, so the block composes a *quaternion*, not a position. A scan of the whole block finds no read of `+0xc0…+0xe0`, which the vtable table now explains rather than leaves anomalous.

The per-class split is measured now too. Id 121 diverts to `0x727800` (volume `0x727ab0`), id 123 to `0x727f00` (volume `0x728270`), id 124 to `0x729be0` (volume `0x729df0`), and id 120 inlines its surface block at `0x7272aa`. Id 125 authors `IsSurface` on 12 elements but its birth routine `0x726570` never reads `+0x84`, so those 12 are inert; the 2,662 that matter are ids 120, 121, 123 and 124. *(Corrected: they do not all read the same basis. Reading the seven emitter vtables out of `.rdata` shows slot [9] is the basis builder `0x729670` for ids 121, 123 and 124, but `0x56fbb0` — an engine routine outside the gfx region — for ids 120 and 122, so id 120's class never builds an `AreaSize` basis and its samplers cannot read one.)*

**The unbound emission basis is rotated, and the viewer's is not.** `0x729670`, the routine that fills the basis both samplers read, branches on `IsBind` at emitter`+0x85` (`0x72967e`). Bound emitters get `diag(AreaSize * 0.5)` laid against the three global unit axes — `0x9c0358` with `AreaSize.x` into `+0xc0…+0xc8`, `0x9c0348` with `.y` into `+0xcc…+0xd4`, `0x9c0338` with `.z` into `+0xd8…+0xe0` — which is what `spawn` reproduces. Unbound emitters take `0x729763`, which multiplies a single scalar argument into all three `AreaSize` components (the `push`/`fstps (%esp)` idiom shifts the frame, so the reads at `0x74(%esp)`, `0x7c(%esp)` and `0x80(%esp)` are the same slot read three times), builds a quaternion from emitter`+0x74…+0x7c` at `0x7297ad`, converts it to a matrix — `0x7297f4` doubles its w, the first term of that conversion — and writes the same nine slots at `0x72987c`. The unbound emission volume therefore carries the emitter's orientation and the viewer's does not. It is the same blocker as `ParIniDir`. *(Superseded in part: emitter`+0x74` does have a writer — `0x716025`, in the element's spawn loop at `0x715f00`, which also lerps the emitter's position `+0x68` per spawned node, so the orientation the unbound basis is built from is a live quantity rather than the constructor's initial one. The scalar argument's value was still not recovered, and neither was the meaning of the two quaternions that writer combines, so both the extent and the rotation would still be guesses.)* 8,259 of the corpus's 13,665 particle elements author `IsBind`, leaving 5,406 on the unbound path.

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
| `IsSurface` | 2,674 (id 123: 1,520, id 124: 1,045, id 120: 86, id 121: 11, id 125: 12 inert); `IsBind` 8,259 (id 123 alone: 5,682) |
| `IsUseHSVInterp` | 168 elements in 101 roots; 147 with distinct endpoints, 144 of those differing in hue and sweeping 0.567 of the circle on average, 4 taking the wrap branch, 52 fully saturated at both ends |
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

## The HSV colour path, reproduced

`IsUseHSVInterp` was the one behaviour in the list above that the binary could settle on its own, and it now is: the viewer runs the client's second colour sampler for the 168 retail elements that ask for it.

**The branch.** `0x7237f0` loads the two packed colours from emitter`+0x30`/`+0x34`, returns the minimum untouched when they are equal (`0x7237fc`), and otherwise tests the byte at emitter`+0xb0` (`0x723804`): zero goes to the RGB sampler at `0x7238c3`, nonzero to `0x723812`. The two are not variants of one idea.

- RGB (`0x7238c3`) calls `rand` four times (`0x7238cc`, `0x7238e0`, `0x7238f4`, `0x723908`), scales each by 1/32767 (`0x8e6f10`) and combines one per channel as `min * (1 - u) + max * u`, truncating through `_ftol` — the thunk `0x892e78` resolves to `MSVCRT._ftol`, which pops, and that pop is what fixes the stack accounting for both converters. The alpha is then multiplied by emitter`+0x8c` at `0x723955` and truncated a second time.
- HSV (`0x723812`) calls `rand` **once** and uses that single `u` for all four channels, lerping the other way round, `min * u + max * (1 - u)`. For a uniform draw that is the same per-channel distribution, but it couples the channels: a particle cannot come out bright and pale by coincidence the way the RGB path allows.

**Hue wraps.** `0x723829` compares the two hues and, when the minimum is the larger, `0x72383b` adds a full turn to the maximum (`0x72383e`) and `0x72385e` takes it back off any result above one, tested at `0x723851`. An author writing 0.9 down to 0.1 gets the short way round through red rather than the long way back through the spectrum. The four lerped values are stored as singles (`0x723876`, `0x723890`, `0x7238a0`, `0x7238ae`) into a four-float struct that `0x7898f0` converts back; the packed result is read at `0x7238b7`.

**`0x7898f0`, HSV→RGB**, cdecl `(const float hsva[4], DWORD* out)`. The out pointer is read at `0x789aab`, after the `pop %edi` at `0x789aa5` has shifted the frame — which is why the same slot offset serves as the value channel's local earlier and as the out pointer at the end. Alpha is `trunc(a * 255)` (`0x789916`, scale constant `0x8f0ae8` = 255.0). Value is `v * 255 + 0.5` (`0x789906` then the bias `0x8eff98` = 0.5 at `0x78990c`) before `_ftol`, so **value rounds to nearest while alpha truncates**. A zero saturation, compared against `0x8effc4` = 0.0 at `0x789924`, short-circuits to grey at `0x789933`. A hue of exactly 1.0 is folded to 0.0 (`0x789947` compares against `0x8effa0` = 1.0, `0x789956` loads the zero) so the top of the circle lands in sector 0 instead of off the end of the switch. Otherwise `h * 6` (`0x8f0d80` = 6.0) is floored through `MSVCRT.floor` at the IAT slot `0x8d54b4` and truncated to the sector at `0x789975`; the fraction feeds the three textbook products `p = (1-S)V`, `q = (1-Sf)V`, `t = (1-S(1-f))V` at `0x789997`, `0x7899ab` and `0x7899c5`. The switch at `0x7899d1` runs through the jump table `0x789ab8`, and its six cases are exactly the hexcone: (V,t,p), (q,V,p), (p,V,t), (p,q,V), (t,p,V), (V,p,q). `cmp $5,%eax; ja 0x789a8f` sends a seventh sector to a path that reads the value slot as an integer; hue is normalised into [0, 1], so it cannot be reached, and the port clamps to the sixth case rather than copying dead code. Packing is `(A<<24)|(R<<16)|(G<<8)|B` at `0x789a9b`.

**`0x789ad0`, RGB→HSV**, cdecl `(DWORD packed, float out[4])` — confirmed by the single `add $0x10,%esp` at `0x724445` cleaning up both calls' arguments at once. It unpacks R/G/B/A at `0x789ae1`, `0x789af4`, `0x789afd` and `0x789b0d`, each scaled by the f32 1/255 at `0x8f0928`, and copies the alpha through as that float at `0x789be8` rather than converting it back. Value is the channel maximum (`0x789bf3`); the maximum is tested against zero at `0x789bf6` before dividing, so black stores a zero saturation instead of a quotient, and the saturation is tested again at `0x789c15` so an achromatic colour stores a literal zero hue at `0x789c29`. Hue comes from the three-way maximum test (`0x789c33` red, `0x789c52` green, `0x789c77` blue) with the sector offsets +2 (`0x8f0d90`) and +4 (`0x8f0af8`), scaled to degrees by 60.0 (`0x8f0d8c`) at `0x789c98`, wrapped by +360 (`0x8f0d88`) at `0x789cab` when negative, and normalised by 1/360 (`0x8f0d84`) at `0x789cb1`.

**The parse-time conversion is unconditional.** `0x724434` converts `ColorMin` into emitter`+0x38` and `0x724440` converts `ColorMax` into `+0x48`, from the key-parse epilogue at `0x724428`, whether or not the flag is set; only the choice of sampler depends on it. The flag itself is read as *not zero* rather than *equal to one*: `0x724414` scans the value, `0x72441d` tests it against itself and `0x724422` stores the `setne` result in emitter`+0xb0`. `gfx_file` now reads that key the same way, through a `nonzero_flag` reader — the only flag in the grammar with disassembly behind a wider reading than 0/1, so the flags beside it stay strict.

**What it changes for retail content.** 168 elements in 101 roots. 21 author equal endpoints and short-circuit before the flag matters. Of the other 147, 144 differ in hue and sweep 0.567 of the circle on average, 4 take the wrap branch, and 52 have both endpoints fully saturated — the case where the RGB path's independent draws put grey midtones between two pure hues and the HSV path cannot.

**Verification.** Nine new tests: the converter against hand-checked primaries, greys, black and a half alpha (`rgb_to_hsva_matches_the_client_converter`); a 16,384-colour round trip through both routines (`hsv_conversion_round_trips_the_palette`), which is exact and is the strongest available check that the two agree on layout, sectors and bias; the rounding bias against the truncating alpha (`the_value_channel_rounds_and_the_alpha_truncates`); the grey short-circuit ignoring hue (`zero_saturation_is_grey_whatever_the_hue`); the hue fold (`a_hue_of_exactly_one_folds_into_sector_zero`); the draw counts (`the_hsv_path_shares_one_draw_between_its_channels`); the saturation consequence (`the_hsv_path_holds_saturation_across_a_hue_sweep`); the wrap (`hue_wraps_the_short_way_when_the_minimum_is_above_the_maximum`); and the flag reading (`the_hsv_flag_is_a_nonzero_test_not_an_equality_test`). 72 library tests pass in the offline crate, which now vendors the real `encoding_rs` 0.8.35 from GitHub alongside `cfg-if` 1.0.5 instead of the latin-1 stub used earlier, so `gbk_decode_reports_damage` passes locally as well as in CI.

The corpus sweep was re-run afterwards. Every line is unchanged — 5,457 of 5,459 roots parsed and played, 484 child roots resolved against the corpus, 84 warnings over 31 distinct missing files, PARSE 2 / BUILD 0 / ADVANCE 0 / cycles 0, decal equivalence 13 roots / 19 quads / 0 mismatches — except the quad total, which moved from 19,861,347 to 19,861,420. That is the sampler's draw count rather than its colour: one draw per spawn instead of four shifts every later value those emitters pull, so a handful of particles land differently across 545,700 frames. Max per frame (4,079), max particles (4,257) and the finished count (2,658) are identical.

## Camera shake, measured and left alone

`ShakeCam` is set on 241 roots and is the last root-level behaviour the viewer ignores. It is now decoded far enough to say what it does and why it stays out.

**The parse.** `0x6e38a4` gates the key on version 24, `0x6e38ce` scans it with `sscanf` against the string `ShakeCam: %d` at `0x8eb790`, `0x6e38d9` tests the value against itself and `0x6e38de` stores the `setne` result in a single byte at root`+0x1f6`. There is no bitfield: the client keeps one bool. The corpus agrees — 5,216 roots author `0`, 241 author `1`, and nothing authors any other value. `gfx_file` had been reading bit 0 as "shake the camera position" and bit 1 as "shake its scale"; both accessors are gone, replaced by `shakes_camera()`, which is the client's own nonzero test. Neither accessor had a caller anywhere in the tree, so nothing else moved and no parse or playback path changed.

**A root controller is an element controller.** When the flag is nonzero, `0x6e38ea` calls `0x6eae10`, which reads `CtrlType: %d` (`0x8ebafc`) and, from version 21 on, `StartTime: %f` (`0x8ebb0c`) and `EndTime: %f` (`0x8ebb1c`); earlier versions default them to 0 and −1.0 at `0x6eaef8`. The type goes to `0x6ea910`, which subtracts 100, range-checks against 12 and jumps through the table at `0x6eaddc` — the same thirteen-case controller space the elements use. That factory has exactly two callers, `0x6eaf0d` here and `0x71b619` in the element path, which is direct evidence for the choice `gfx_file` already made: one `Control` grammar, parsed at root level and element level alike. The constructed object loads itself through its vtable at `0x6eaf2b`, keeps `StartTime` at `+0x8` and `EndTime` at `+0xc` (`0x6eaf4c`, `0x6eaf53`), and is stored on the root at `+0x208`.

**The shake.** One caller, `0x6e4b7d`, runs `0x6e4dc0` once per frame:

- `0x6e4dc9` requires root`+0x1f6` and `0x6e4ddb` requires the controller at `+0x208`.
- The window uses the root's millisecond tick at `+0x1cc` scaled by 0.001 (`0x8d5c70`) at `0x6e4df5`, so the controller's start and end are in seconds. A negative `EndTime` skips the end test altogether (`0x6e4e0c` compares against 0.0 at `0x8d58a0`) — the same "forever" convention the rest of the format uses.
- On the first frame `0x6e4ffb` latches `+0x1f7`, `0x6e5002` zeroes a frame counter at `+0x1f8`, and `0x6e5008` calls `0x41a480` once: the camera is saved before it is disturbed.
- Every frame increments `+0x1f8`. **Odd** frames take `0x6e50f3`, which negates the three floats at root`+0x1fc..+0x204` and re-applies them through `0x41a4e0`. The shake alternates sign at half the frame rate instead of drawing a fresh sample every frame.
- **Even** frames compare root`+0x29c` against 30.0 (`0x8eb710`) at `0x6e5037`. At or beyond 30 the offset is zeroed; inside it, `0x6e5094` forms `1.0 − root+0x29c/30.0`, `0x6e50a5` calls the noise generator `0x766490` with `this = controller + 0x14`, and the result is scaled by that factor and by a fresh random unit direction before it is stored into `+0x1fc` and applied.
- The direction comes from two `rand()` draws, each multiplied by 2π/32767 (`0x8e6f14`), turned into quaternions by `0x7b6fd0` about the globals `0x9c0348` and `0x9c0358`, and composed (`0x6e4e55`, `0x6e4f25`). Both globals live past the raw `.data` section and are initialised at runtime, so their values are not recoverable from the file.
- root`+0x29c` is the same field the emission LOD divides the particle count by, so one distance input drives both the emission rate and the shake amplitude.

**The generator.** `0x766490` is a gradient noise over a table the object owns: `this+0x18` is a count that abandons the whole call when it is not positive (`0x7664a6`, `0x7664af`), `this+0xdc` is a modulus, and `this+0xe0` is an array of twelve-byte records. Each step splits a coordinate into an integer index and a fraction — `_ftol` at `0x7664e6`, then `fsubr` at `0x7664fa` — wraps the index into range by adding the modulus while it is negative (`0x766504`) and taking a signed remainder (`cltd`/`idiv` at `0x766508`), copies the gradient record for that index and for `index+1` with `rep movsl` and a component count clamped to three (`0x76650b`–`0x766564`), and interpolates: a byte argument picks between two variants at `0x766574` and the blend starts from 1.0 (`0x8effa0`) at `0x76657c`.

**Why it stays out.** Three inputs the viewer cannot supply: the camera's distance to the effect (root`+0x29c` is a renderer-side quantity), the camera save-and-apply pair (`0x41a480`, `0x41a4e0`), and the runtime-initialised rotation axes. Behind them the generator's fade curve and octave weighting are still undecoded — the shape is identified, the interpolation is not. Recording the gate, the window and the alternating-sign structure costs nothing and is enough to reimplement this later against a real camera; inventing an amplitude now would put a number in the viewer that no disassembly supports.

## Still open

Unchanged from the previous write-up and not addressed here: the interpolation and octave weighting inside the fractal-noise generator `0x766490` behind controllers 107, 109 and 112 (241 root controllers in the corpus, all type 112 — see *Camera shake, measured and left alone* for how far that decode now goes), element 190 curves being counted rather than drawn, models/sounds/lights belonging to other subsystems, and the 82 child references to files that do not ship. The `gfx_light` millisecond window listed here previously is fixed in *Fixed while writing this up*; the ramp time-base question that fix opened is still unsettled and needs the controller-108 update routine.
