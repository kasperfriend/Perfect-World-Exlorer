# GFX effect container: grammar, runtime player and corpus verification

Three new modules carry the retail `.gfx` effect format end to end: `gfx_file` (the container grammar), `gfx_player` (the clock, controller, particle, ribbon, bolt and child runtime that turns a parsed root into textured quads) and `gfx_effect_gpu` (a per-blend-bucket wgpu receiver). The walker's persistent character effects now try the verified static subset first (`gfx_decal`) and hand every root it rejects to the player, so animated weapon and body effects play instead of being dropped. Addresses, offsets and instruction sequences below were read out of the inspected `elementclient.exe`; corpus counts were measured over every `.gfx` member of the retail `gfx.pck` (5,459 members, 10,104 in the archive overall).

## Container grammar

Text, GBK encoded, CRLF terminated, `Key: value` per line with the first colon as the separator. `MOXTVersion` gates the rest: versions above 79 or below 24 are rejected, because 24 is the client's own gate for the effect container and 79 is the newest version in the corpus. The root header carries `DedaultScale` (sic), `PlaySpeed`, `DefaultAlpha`, `Raytrace`, `FaceToViewer` (version 39+), `FadeByDist`/`FadeStart`/`FadeEnd`, two `Vec` lines for the AABB centre and half extents, `UseAABB` (57+), `ShakeCam`, then zero or more root controllers, then `NoChangeDir`/`2DRender`/`2DBackLayer` (50+) and `PhysExist` (64+), then `GFXELEMENTCOUNT` and the elements. Every element shares 26 optional, version-gated keys plus a class-specific block selected by `GFXELEMENTID`, then its affector list, then `StartTime` and its keypoints. 146 distinct keys occur across the corpus and all of them are named in `KNOWN_KEYS`; an unnamed key is a hard error rather than a silent skip, and the element-125 sphere emitter additionally writes a keypoint-shaped generation sequence before its affectors.

Element IDs in the corpus, with counts: 100 sprite 3,537; 101 trail/quad 5,244; 102 mesh-facing quad 83; 110 ribbon 2,956; 120–125 particle emitters 1,315/320/40/9,869/1,739/382; 130 light 546; 140 wave surface 1,116; 150/151/152 lightning 734/11/156; 160 model 447; 170 sound 491; 180 79; 190 curve 208; 200 nested child 1,530; 210/211 215/1. Sprites, particles, ribbons, waves and bolts produce quads; lights belong to `gfx_light`, models to the model queue, sounds to the audio path and curves are counted but not drawn, so the player reports them in `Stats` instead of approximating them.

## Native evidence

| Address | What it establishes |
| --- | --- |
| `0x5b33f3`, `0x5b3700`, `0x788960` | ECWLD effect record (60 bytes) and the placement matrix path a world effect is parented through |
| `0x5b18a1`, `0x5b1a00`, `0x70bd20`, `0x7b2520` | day/night-only roots rejected at 0.5, at most one new root admitted per frame, duplicate roots updated in place, hook resolution |
| `0x71a1f1`, `0x71a217` | particle tick converts the integer millisecond delta (`fild; fmul 0.001`) and scales it by the root's live `PlaySpeed` copy at root+0x1ec |
| `0x71a247` | lifetime: `age += dt; if (age >= ttl) unlink`, age at node+0x00, TTL at node+0x04 |
| `0x715b50`, `0x715c09` | emission, and the quota rule `count = min(count, quota - live)` with −1 unlimited (live at +0x1a8, quota at +0x1a4) |
| `0x716240` | affector order: the node is copied into a 0x3c-byte temp struct (`+0x00` position, `+0x0c` colour, `+0x10` scale, `+0x24` rotation, `+0x28` velocity, `+0x34` age), every affector's `Update` (vtable slot 1) runs in list order, the result is copied back |
| `0x7165b0` | particle integration: `speed += ParAcc*dt; d = (speed_old + speed_new)*0.5*dt; pos += d*dir`, with the displacement mirrored into node+0x50 and a second accumulator at node+0x5c for `Acc` along the authored `AccDir` (settings+0x1c) |
| `0x6eaf60`, `0x6eb230` | controllers 100 and 101: `v = Vel + Acc*age`, `dspl = (2*v + Acc*dt) * dt * 0.5` — the client adds `v` to itself, adds `Acc*dt`, multiplies by `dt` and by the 0.5 it loads from `0x8d5894`. 100 adds `dspl*Dir` to both the position (temp+0x00) and the velocity accumulator (temp+0x28); 101 adds `dspl` to temp+0x24, the rotation slot, which is why the corpus writes `Vel: 1.745329` (100°/s in radians). Both skip the step while `age < StartTime`, and both compare `EndTime` against the 0.0 at `0x8d58a0`, so *any* negative end means forever — the corpus only ever writes −1 (74,490 of 76,496 controller records) |
| `0x6eb400`, `0x6eb860`, `0x7b6fd0` | controllers 102/103 feed the same `dspl` to the axis-angle helper and rotate about `Axis` through `Pos` |
| `0x6ebba0`, `0x6ebd0a` | controller 104: `target = Pos + velocity_accumulator`, `n = normalize(position - target)`, `position += n*dspl`; `Vel == -1` disables it, `dspl == -1` snaps onto the target |
| `0x6ebf70`, `0x6ebfab`, `0x6ec00b`, `0x6ec02a`, `0x6ec03f` | controller 105 unpacks the ARGB dword at temp+0x0c into red, green, blue, alpha **in that order**, so `ColorDelta`'s four integers are R, G, B, A and the corpus's `ColorDelta: 0, 0, 0, -1000` is an alpha fade. Per channel: `acc = delta*dt + remainder; i = (int)acc; remainder = acc - i; channel = clamp(channel + i, 0, 255)`, with one remainder per channel at ctrl+0x24…+0x30 that is cleared when the byte saturates, then the four bytes are repacked |
| `0x6ecad0` | controller 106: `scale += ScaleChage*dt`, clamped low then high to the authored bounds (defaults 0.01 and 9999) |
| `0x6ec240` | controller 108's colour ramp is the client's own 128-entry PC24 table, which this repository already reproduces in `gfx_color_track` |
| `0x41a430` | ground snapping of spawned particles |

Sprite geometry is not re-derived here: it is copied from `gfx_decal.rs:405–500`, the one sprite path in this repository that was written against the client's decal calls. Dimensions are doubled and split at `OrgPt` (`w0 = Width*2*OrgPt[0]`, `w1 = Width*2 − w0`, `h0 = Height*2*OrgPt[1]`, `h1 = Height*2 − h0`), the *top* extent is `h1` and the bottom is `h0`, both axes are multiplied by the keypoint scale unless `NoScale` suppresses it, alpha is folded in 0..255 units and truncated, and a quad below 5/255 is dropped. Depth testing is `version < 76 || ZEnable`. 704 corpus `OrgPt` lines are off centre, so the split is not a theoretical case.

## The client's vector library

The emission routines are written against a small vector and quaternion library.
Eight of its routines are decoded completely; each is short enough to read end to
end, and together they account for every helper the surface and basis code calls.

| Address | Convention | Semantics |
| --- | --- | --- |
| `0x401ca0` | thiscall, `ret $0xc` | `Vec3::new(x, y, z)`: stores the three float arguments to `this+0/+4/+8`, returns `this` in `eax` |
| `0x401cc0` | thiscall, `ret $0x4` | copy-assign: copies 12 bytes from the argument into `this` |
| `0x401d10` | thiscall, `ret $0x4` | `+=`: adds the argument's three floats into `this` |
| `0x633700` | thiscall, `ret $0x4` | `*= scalar`: multiplies all three components of `this` by the one float argument |
| `0x4093d0` | cdecl, three pointers | `cross(out, a, b)`: `out.x = a.y*b.z - a.z*b.y`, `out.y = a.z*b.x - a.x*b.z`, `out.z = a.x*b.y - a.y*b.x` |
| `0x4071f0` | cdecl, three pointers | `add(out, a, b)` |
| `0x407220` | cdecl, three pointers | `sub(out, a, b)` |
| `0x7b6fd0` | cdecl, `(axis, angle, out)` | axis-angle to quaternion: `out+0..0xc = axis * sin(angle/2)`, `out+0xc = cos(angle/2)`, with the halving by `0.5` at `0x8eff98` |

Two conventions matter when reading a caller. The x87 helpers reserve argument
slots with `push %reg` / `fstps (%esp)`, so the frame shifts by four bytes per
push and the same textual offset names different slots at different points;
`0x727f00` reads what looks like three separate values at `0x74/0x7c/0x80(%esp)`
that are all one argument. And a call's argument count follows from its `ret`,
not from the pushes around it: `ret $0x4` means exactly one.

Rotation by a quaternion is spelled out inline rather than living in the library.
Given `q = (v, w)`, the callers compute `u + 2w(v x u) + 2(v x (v x u))` with one
`0x4093d0` per cross, one `0x633700` per scaling and two `0x4071f0` adds.

Reading those call sites needs one more convention. The second `0x4071f0` of a
rotation is preceded by only two pushes and followed by no stack adjustment,
because the first call's four pushes were cleaned by an `add $0xc` that
deliberately leaves the outermost argument in place for the next call to consume.
Counting pushes per call therefore misreads the argument list; the balanced form
is `G2 + A + B`, the rotation itself.

### Per-class birth flow

The four classes that honour `IsSurface` do not share a birth flow, and the
difference decides whether a surface particle keeps the emission cone.

| Class | Divert | Surface sampler | Volume sampler | After both |
| --- | --- | --- | --- | --- |
| 120 | `0x72729c` | inline at `0x7272aa` | `0x727230` | — |
| 121 | `0x7277c1` | `0x727800` | `0x727ab0` | `vtable[1](&node+0x44)` |
| 123 | `0x727ec2` | `0x727f00` | `0x728270` | `vtable[2]` (TTL) only, at `0x727ed8` |
| 124 | `0x729ba1` | `0x729be0` | `0x729df0` | `vtable[1](&node+0x44)` at `0x729bc1`, then `vtable[2]` |

So id 123's sampler is the last word on the direction — it writes the negated
sample to node+0x44 itself and nothing overwrites it — while ids 121 and 124 have
the direction slot called afterwards and their samplers write no direction at
all. Ids 122 and 125 never read emitter`+0x84`.

### The emitter object

The vtables are consecutive in `.rdata`, twelve slots each, so all seven can be
read off in one dump (`objdump -s -j .rdata`, `0x8edf24`–`0x8ee0ec`). The base
row is what the shared constructor writes before the per-class tail overwrites
its own slots.

| Class | Vtable | [0] | [1] | [2] | [3] | [4] | [5] | [6] | [7] | [8] birth | [9] basis | [10] | [11] |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| base | `0x8edf24` | `723410` | `723440` | `7237e0` | `7237f0` | `723a80` | `723a20` | `723a50` | `892f56` | `724ab0` | `56fbb0` | `724b20` | `7248d0` |
| 120 | `0x8ee0bc` | `726540` | " | " | " | " | " | " | `726530` | `727230` | **`56fbb0`** | `724b20` | `7248d0` |
| 121 | `0x8ee078` | `725e20` | " | " | " | " | " | " | `726530` | `727760` | `729670` | `724b20` | `729b00` |
| 122 | `0x8ee034` | `7264b0` | " | " | " | " | " | " | `7288e0` | `7288f0` | **`56fbb0`** | `724b20` | `7290b0` |
| 123 | `0x8edff0` | `725e20` | " | " | " | " | " | " | `726530` | `727e40` | `729670` | `724b20` | `727c20` |
| 124 | `0x8edfac` | `725e20` | " | " | " | " | " | " | `726530` | `729b40` | `729670` | `724b20` | `729b00` |
| 125 | `0x8edf68` | `725c00` | " | " | " | " | " | " | `7256b0` | `726570` | `727220` | `724b20` | `726ff0` |

Six of the twelve slots are identical in all seven tables — `[1]` the direction,
`[2]` the TTL, `[3]` the colour, `[4]` the phase, `[5]` the scale, `[6]` the
rotation and `[10]` `0x724b20`. Slot `[9]` is *not*: ids 121, 123 and 124 get the
basis builder `0x729670` that fills `+0xc0…+0xe0` from `AreaSize`, while 120 and
122 get `0x56fbb0`, an engine routine outside the gfx region, and 125 gets its own
`0x727220`. That is why id 120's surface block reads no basis at all — its class
never builds one.

The shared constructor is `0x723250`. It zeroes `+0x1c/+0x20`, sets `+0x24` to
`-1.0f` (the constant at `0x8d5c2c`), zeroes the position at `+0x68…+0x70`, loads
the orientation quaternion at `+0x74…+0x80` from four globals
(`0x9c0318`, `0x9c031c`, `0x9c0320`, `0x9c0324`), zeroes the three Euler angles at
`+0x94/+0x98/+0x9c` and `+0xb0`, writes the base vtable at `0x7232b2`, sets
`+0x04/+0x30/+0x34` to `-1` and calls `0x789ad0`. The four orientation globals sit
on the same `.data` page as the three unit axes the samplers use
(`0x9c0338`, `0x9c0348`, `0x9c0358`) and are initialized at run time, so their
values are not in the file.

`+0xa0…+0xac` is not the colour the earlier offset map called it. Two sites write
it, and both compute the same thing: the emitter's three Euler angles converted to
a quaternion and Hamilton-multiplied by the orientation quaternion at
`+0x74…+0x80`. At parse time that is `0x7242ee` (`sscanf` through `%ebx` fills
`+0x94/+0x98/+0x9c`), `0x724313` (`0x7b6f10`, Euler→quaternion), the products
against `+0x80/+0x74/+0x78/+0x7c` from `0x72431c`, and the destination
`lea 0xa0(%edi),%eax` at `0x7243b8`. At run time `0x71b450` repeats it exactly —
`flds +0x9c/+0x98/+0x94`, `call 0x7b6f10` at `0x71b479`, the same four products,
then `add $0xa0,%esi` at `0x71b492` and four dword writes at
`0x71b524/0x71b52c/0x71b537/0x71b53a`, `ret` at `0x71b541`. The birth routines copy
those four floats into node`+0x34` (`0x728241`–`0x72825d` for id 123), which is
what that node field carries.

The orientation quaternion itself does have a writer, at `0x716025`, inside the
element's spawn loop (`0x715f00`–`0x716176`, with a second copy of the tail at
`0x7160ca`–`0x716146`). Per iteration the loop takes a node from the free list at
element`+0x198` (`call 0x6e00b0` at `0x715f14`), links it (`call 0x4287c0` at
`0x715f22`), interpolates a position as `t·(*ebx) + (1−t)·(element+0x210,
+0x214, +0x218)` with `t = index × [esp+0x34]` (two `Vec3::new` calls at
`0x715f9d` and `0x715fca`), builds a quaternion with four arguments at
`call 0x7b7010` (`0x716003`, one of them `&element+0x21c`), then takes
`ebp = element+0x140` and writes **emitter`+0x68…+0x70`** from the interpolated
vector (`0x716019`, via `0x401cc0`) and **emitter`+0x74…+0x80`** as four dwords
copied from `[esp+0xb8]` (`0x716025`–`0x71603b`), followed immediately by
`call 0x71b450` at `0x716040` to recompose `+0xa0`. So the emitter's position and
orientation both move as the element spawns, and the unbound emission basis is
rotated by a real orientation rather than the constructor's initial one. What is
still not decoded is the meaning of `element+0x210…+0x21c` and of the quaternion at
`[esp+0xb8]`, and the condition under which this loop runs at all — the entry
guard at `0x715f00` is only `element+0x198 != 0`.

## Runtime player

The root clock advances in milliseconds and is scaled by `PlaySpeed`; controllers integrate in seconds of scaled time. An element's clock is its keypoint spans plus `RepeatDelay`, repeated `RepeatCount` times (1 = once, −1 = open-ended, N = N passes, and the corpus's 10,000/2147483647 values are "effectively forever"); a phase no keypoint covers draws nothing but does not end the element, and the root reports itself finished only when *every* element is over and none loops. `RotFromView` decides where a sprite's basis comes from — 598 retail sprites take it from the camera, 8,271 keep the authored keypoint direction — and the root's `FaceToViewer` forces the camera basis for all of them. Flipbook cells walk `floor(clock/TexInterval) % clamp(TexAnimMaxTime/TexInterval, 1, rows*cols)` with v growing downwards, and `TexSpeed` scrolls U and V continuously. `InterpolateMode` is 0 in every corpus keypoint, so linear blending between keypoints is implemented but never exercised by retail data.

Children (element 200) resolve their `GfxPath` at load time, exactly like the client's `A3DGFXMan::LoadGFXFromFile`, with a depth cap of 6 and a path stack to stop cycles. Of the corpus's 1,530 child references, 1,448 resolve inside `gfx.pck` after `\`→`/`, lowercasing and the `gfx/` prefix; the 82 that do not are 31 distinct paths that genuinely do not ship (a doubled directory `人物\技能\巫师\技能\…`, a stray space in `4品法宝_冷凝术_ 子效果.gfx`). Those are authoring mistakes, not a resolver bug, so an unresolvable child is a named warning through `Player::warnings()` and the element degrades to a clock that emits nothing, instead of the whole root being dropped. 49 roots (0.9%) are affected.

## Verification

`gfx_file` carries 23 tests over verbatim retail records and rejection paths; `gfx_player` carries 15 more, including the controller integrals, the colour-remainder carry, quota and TTL unlinking, flipbook walking, basis selection, distance fade and the alpha cutoff, repeat/delay gating, and the child warning, cycle and nesting cases. One test is a cross-module equivalence check: for a root both readers accept, the player's frame zero must match `gfx_decal::Root::quads()` corner for corner, UV for UV and channel for channel. The same check was run over the whole corpus — 13 retail roots, 19 quads, 0 mismatches — which is the strongest available evidence for the sprite path, because `gfx_decal` is the reader with native decal calls behind it.

The full-corpus sweep parses every member, builds a player for each root, advances two players 90 frames each under `catch_unwind` and compares them for determinism. Final run: 5,458 files walked, 5,456 parsed, 5,456 played, 662 child roots loaded, 25,455,994 quads emitted, 4,095 quads in a single frame, 4,257 live particles, 2 parse failures (an empty container and a version-4 root that predates the effect format), 0 build failures, 0 advance failures, 0 non-deterministic roots, 0 panics, 168 child warnings, 0 cycles. One member is named exactly `.gfx` and is skipped by extension-based walkers; it is a valid version-33 root with 15 elements.

Four real bugs were found by that verification rather than by inspection, and all four are now pinned by tests:

- The corner construction mixed scaled and unscaled extents (`right = Width*2 − left` after `left` had already been scaled) and put the origin split on the top edge instead of the bottom. Invisible at `OrgPt: 0.5`, wrong for the 704 off-centre lines.
- The controller displacement was transcribed as `v*dt + 0.5*(2*v*dt + Acc*dt²)`, which double-counts the velocity term and would have made every controller-driven motion twice as fast. Re-reading `0x6eaf60` and `0x6eb230` settled it: the expression is the trapezoid `0.5*(2*v + Acc*dt)*dt`, which reduces to `v*dt` at zero acceleration and is what makes `Vel: 1.745329` mean 100°/s.
- `ColorDelta`'s four integers were applied as A, R, G, B. The client's unpack at `0x6ebfab` and repack at `0x6ec03f` are R, G, B, A, so every authored alpha fade was fading blue instead, and one shared remainder was used where the client keeps one per channel and clears it on saturation.
- Alpha was folded continuously (`a/255 * scale`) where the verified decal folds in 0..255 units and truncates. At `DefaultAlpha: 0.5` that is 127/255, not 127.5/255, and the two readers disagreed on every half-transparent effect.

Seven validations were also removed after the corpus disproved them: negative `Width`/`Height` (530 occurrences), out-of-range `OrgPt` (704), inverted `ScaleMin`/`ScaleMax` (817), non-positive keypoint `Scale` (18, including uninitialized editor garbage such as `Scale: -94883208` and `Position: 1.8e28` in `gfx/策划联入/人物技能/击中/巫师_冷凝术_击中.gfx`), non-positive non-keypoint `TimeSpan` (440) and `DefaultAlpha > 1` (2). They are now clamped or sampled as authored instead of rejecting a shipping file. What is still rejected: non-finite values, versions outside 24…79, an empty container, a malformed `ScaleChage`, a grid colour count that disagrees with `wNumber*hNumber`, unknown keys, keys out of positional order, counts above their ceilings, `FadeStart > FadeEnd` (0 occurrences in 5,457 files), negative `PlaySpeed` and non-positive `DedaultScale`.

## Interpreted, not verified

The emitter shape classes were never located in the binary, so box and sphere spawns distribute uniformly inside the authored `AreaSize`/`Radius`; that is the one native behaviour deliberately not copied. **Superseded the same day:** the shape classes were subsequently located (factory `0x723ab0`, birth `0x727e40`, shape `0x728270`, matrix build `0x729670`), `AreaSize` turned out to be an ellipsoid of semi-axes `AreaSize/2` rather than a box, and the cone and colour draws were corrected with it — see [the parity assessment](Gfx-effects-parity-assessment-2026-11-05.md). The fractal-noise generator behind controllers 107, 109 and 112 is not decoded — the player implements value noise over `BufLen`, `Amplitude`, `WaveLen`, `Persistence` and `OctaveNum` with a documented hash, and 241 root controllers in the corpus are all type 112, so this is the most-used unverified path. Ground matching of spawned particles (`0x41a430`) is skipped. Controller 105's native remainder slots sit at ctrl+0x24…+0x30 on the controller object, which the earlier reading confused with the particle temp's rotation slot at temp+0x24; the interference that confusion implied does not exist and is no longer claimed. `gfx_light` still passes raw milliseconds into `ColorTrack::new`, whose `sample()` takes seconds — latent and harmless, but unresolved. Element 180 and any ID the client never instantiates are counted as unrendered rather than guessed at.

## Integration

`src/modules/walker/gfx.rs` keeps every existing event gate (BindParent 1, Once 0, StartTime 0, TimeSpan −1, no `GfxOuterPath`/`GfxRelToECM`, model alpha) and now decodes through `gfx_file::decode`, offers the text to `gfx_decal::Root::parse` first, and falls back to `gfx_file::parse` plus `Player::new` with a `load_child` closure that reads the nested member from the same archive. The event's `GfxSpeed` finally has something to act on: dynamic roots are advanced by `dt * speed`, which is the slot the client scales its millisecond delta by. `src/modules/walker/mod.rs` gained a `gfx_effect_gpu::Draw` next to the static decal draw, sharing the reverse-Z `Depth32Float`/`Greater` attachment, and draws it immediately after the static quads. `render` carries no delta, so the wall clock between frames supplies one, clamped to 0…100 ms, and the camera basis is recovered from the inverse of the frame's view-projection. The whole-project build, the GPU checks and the formatting gate can only be confirmed by CI: wgpu and egui are not reachable from the sandbox, so `gfx_effect_gpu.rs` and the two walker files were never compiled locally, while `gfx_file.rs`, `gfx_player.rs`, `gfx_color_track.rs` and `gfx_decal.rs` were compiled and tested against a vendored glam with the real `encoding_rs` replaced by a byte-preserving stub.
