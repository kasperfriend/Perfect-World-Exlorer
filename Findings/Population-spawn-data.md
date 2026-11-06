# Population and spawn data — what the client actually ships

Source: the user's PW 1.4.2 client data in the shared Drive folder (`pw142`).
Everything below is measured against the shipped files, not assumed.

## `element/data/task_npc.data` — the only client-side spawn table

18,876 B. Header: `u32 total_size (18876)`, `u32 timestamp (0x4b39c937)`,
`u16 version (1)`, `u16 count (1572)`; then 1,572 records of
`u32 npc_id`, `i16 x`, `i16 z`, `i16 height`, `u16 task_count`.
Stride 12, no padding, end of file exactly at `12 + 1572 * 12`.

| property | value |
|---|---|
| records | 1,572, all distinct ids |
| ids resolving to `NPC_ESSENCE` | 1,572 (0 to `MONSTER_ESSENCE`) |
| records at exactly `x = z = 0` | **1,057** |
| records with a real position | **515** |
| x range (real) | −2,914 … 3,065 |
| z range (real) | −2,439 … 4,497 |
| height = 0 | 1,553 overall; 496 of the 515 real ones |
| height > 0 | 19, up to 934 (all real) |
| `task_count` = 0 | 0 |

**The `0,0` records are the server's.** 1,057 entries sit at exactly `x = z = 0`,
every one of them with `height = 0` and a `task_count` between 23 and 30,657.
They are the quest NPCs the server spawns; the client never places them, and a
viewer that trusted the column would stack a thousand characters on the map
origin. They are counted (`Population::unplaced`) and not drawn. With them out
of the way, 515 NPCs remain, all of them named, 514 under `models/npcs/` and one
animated scene creature under `models/场景活物/`.

**Coordinate convention.** Column 1 is world X and column 2 is world Z. Against
the client's own destinations (`element/maps/world/world_targets.txt`) within a
400-unit radius:

| destination | x = column 1 | swapped |
|---|---:|---:|
| Archosaur North District `1470.490, 219.608, 1180.864` | 60 | 17 |
| City of a Thousand Streams `−2825.774, 3092.210` | 14 | 0 |
| Raging Tides `2681.930, −4268.359` | 0 | 0 |

The swapped mapping also fails the world bounds for the taller half of the map,
so the shipped file can only be read one way.

**Nameless records.** 282 of the 1,572 have an empty `NPC_ESSENCE` name and a
valid rig — the `宫灯` palace lanterns and similar decorative props. All 282 are
`0,0` records, i.e. server-placed scenery, so spawnability tests the rig rather
than the name and none of the 515 drawn placements is nameless.

**No facing.** The record carries id + XZ + height + task count only. The
Populator therefore leaves yaw at 0 unless the user opts into facing the
camera; the ROT1/ROT2/ROT3 table quoted on community forums belongs to the
server's `d_showpos`, not to this file.

## `element/data/elements.data` — templates (v27, 28,613,176 B)

131 sections, 37,676 records, EOF exact. Two matter here:

| section | record | count | fields used |
|---|---:|---:|---|
| 39 `MONSTER_ESSENCE` | 1,528 B | 4,208 | `monster_type @4`, `name @8..72` (UTF-16), `short_desc @104..136`, `.ecm @144..272` (GBK), `level @544`, `hp @552` |
| 58 `NPC_ESSENCE` | 852 B | 2,838 | `name @4..68` (UTF-16), `.ecm @80..208` (GBK), `greeting @216..728` (UTF-16) |

Spot checks: NPC 2109 `Apothecary Cheng` → `Models\NPCs\npc\男npc4\男npc4.ecm`,
greeting starts "The apothecary's approach is much more profound…";
monster 986 `Tauroc Valorian`, `+WT-ER`, level 48, 11,983 HP →
`…\c身高\暴牙牛士兵.ecm`. Paths normalize to `models/npcs/…`, which is exactly
how `models.pck` stores them (3,745 `.ecm`, 2,228 `.smd`, 3,579 `.bon`,
10,898 `.ski`, 14,065 `.stck`, 6,005 `.dds`).

## What the client does **not** ship

* **Mob placement.** The server's `gamedata/npcgen.data` holds monster and
  world-NPC spawns (`d_showpos`, `ai`, `path_id`). It is not in the client, so
  the 4,208 real monster templates stay unspawned rather than being placed at
  invented coordinates.
* **Patrol routes.** No client file carries NPC waypoints.
  * `path.data` (1,684,392 B, second data set) is *not* a waypoint table: it
    opens with `DIMP`, a `u32` count, then repeated 4-byte tags with a length
    and a GBK asset path (`models\error\error.ecm`, `surfaces\…\钻石球.dds`).
    It is an asset manifest, with no float triples.
  * `element/data/aipolicy.data` (980,119 B) is the server AI policy table and
    is keyed by `npcgen.data` records, so it cannot place or route anything on
    its own.
  * `element/data/domain.data` describes regions, not movement.

The Populator therefore implements the movement rule (`patrol::Follow`: walk
each leg at the authored speed, dwell at a waypoint, loop or stop) and applies
it only to actors that arrive with a route; today none do.

## Native distance ladder (why 420 units is the default)

| quantity | value | source |
|---|---|---|
| terrain block window | 7 blocks × 64 units = 420 | `view_settings::DetailSettings.radius`, `world+0x1bc`, presets `0x40e41f` |
| scenery `Sight` preset | preset index, not units | `0x40e155` / `0x424230`; shipped `detail_scene = 1` |
| ornament culling | 120 units | `0x42449c` |
| terrain LOD | 180 / 320 units | `0x77e360`, `0x77f450` |
| fog | 60 … 400 | `element_client.cfg` |

`SystemSettings.ini` has no numeric view distance (`Sight = 4` is a preset
index), so 420 units — the client's own scene window — is the module default,
with 120 (ornament range) and 800 as further presets.

## Module behaviour that follows from this

* Spawn, appear, idle: the 515 client-placed NPCs at authored positions and
  heights, rigs, overhead names and the idle `站立` scene action straight out of
  `models.pck`. The 1,057 server-placed records are reported, never stacked at
  the origin.
* Standing surface: a record whose height is `0` has no Y at all — the server
  supplies it — so the placement is resolved against the client's own BMD
  collision hulls before the terrain sample: the lowest walkable building floor
  within 16 units above the terrain (bridge deck, platform, interior floor).
  That is what puts the bridge and platform placements on their decks instead
  of in the arch or the wall below them. The hulls are the same ones the Walker
  module collides with, loaded on demand; an authored height always wins and is
  never moved. Many props (stalls, wagons, low platforms, crates) ship render
  geometry and **no collision hull at all**, which is why their placements used
  to fall through to the terrain: the same lit-model load now also derives each
  model's upward-facing surfaces (triangles with `normal.y > 0.5`, models under
  8 units tall and under 24 units wide, bucketed into 2-unit tiles so a step or
  a ramp keeps its own height instead of the model's tallest point) and uses
  them when no hull floor exists. A hull floor still wins, so a bridge deck is
  never replaced by a prop standing under the span, and the surface a placement
  gets is always the tile directly beneath it.
* Idle playback: a combined action the viewer can schedule runs through
  `Playback`; otherwise the ordinary clip that combined action names loops on
  the module clock. `pose_action_at_time` takes absolute track time and clamps
  at the last key, so a clock that only grew would freeze a rig mid-idle after
  one cycle — the loop wraps inside the clip's own millisecond range instead.
* Which idle a rig gets is the rig's own data, in this order: the combined
  action `站立` — the standing action, with the client's own fallback to the
  literal `休闲` (`EcModel::scene_action`) — else the ordinary clip that combined
  action's first base names, else the rig's own clip called `待机`, `站立`,
  `休闲` or `idle`, else the authored bind pose. A rig with no usable idle is
  drawn in its bind pose rather than vanishing, and the reason is logged.
  The request used to be `休闲`, which is the leisure gesture: for the 1,486
  shipped rigs that carry one, that asked a trader to stretch and look around on
  a loop instead of standing. `站立` is the action those rigs actually author as
  the standing loop, and its base carries `LoopCount: -1`, so it schedules
  through `Playback` as an infinite loop instead of a re-triggered gesture.
* "Not stuck still" is checked against the drawn geometry, not the schedule: the
  viewer fingerprints a spread of posed vertices each frame, and an actor whose
  vertices have not changed for half a second is counted as holding a still pose
  and reported (settings line `N animating`, plus a one-shot console line after
  three drawn seconds). Two defects that produced statues are fixed by this: an
  absent combined action used to be accepted as an inert `Playback` (every rig
  without a `休闲` combined action then kept its bind pose for the session), and
  a pose that failed to evaluate fell back to the bind pose silently.
* The settings window reports how many placements are inside the spawn radius
  and can move the camera to the nearest one: at the client's native 420 units
  the origin has 9 placements, Archosaur North 61, Raging Tides 0.
* Streamed around the camera, at most 6 new rigs per frame (an 888 MB archive
  is decompressed on the render thread), 48 actors by default with a hard cap
  at 96.
* No combat, dialogue or interaction; positions, names, heights and greetings
  are the files' own, and nothing is synthesised to fill the mob/route gaps.

## What the shipped files give every placed NPC

Measured from the retail archives, not counted from the viewer:
`tools/npc_idle_report.py` regenerates the table in
[`npc-idle-evidence.md`](npc-idle-evidence.md).

| evidence | value | source |
| --- | --- | --- |
| `models.pck` entries | 40,753 | pack directory, version `0x20002` |
| NPC `.ecm` rigs in the pack | 1,871 | `models/npcs/**/*.ecm` |
| `elements.data` §58 `NPC_ESSENCE` | 2,838 templates | 852-byte records, v27 (`0x3000001b`) |
| `task_npc.data` records | 1,572 | 515 placed + 1,057 template rows with no position |
| distinct rigs behind the 515 placements | 92 | template `.ecm` path, case-folded |
| rigs whose idle clip spans > 1 ms | 86 | SMD action range ÷ skeleton fps |
| of those, rigs whose rotation keys actually change across the clip | 85 | keys sampled at the clip's start, middle and end |
| rigs with no idle action at all | 6 | scenery: chest, rocks, furnace, tree, altar, skeleton |
| idle clip length | 1.6 – 2.3 s | 24–36 frames at the skeletons' 15 fps |
| rotation keys in the v5 `.bon` | 107 – 478 per rig | the rig's largest rotation track |
| combined actions per rig | 6 – 11 | ECM `ComActCount` |

One rig does not move inside its own clip: `圣诞老人` (Santa Claus) authors
`站立` as frames 107–132 while its largest rotation track holds 107 keys, so the
clip maps past the end of the track and the sampler clamps at the last key — the
same clamp `pose_action_at_time` applies, so the actor is drawn in the pose the
files give it rather than being animated by guesswork. That is the only rig of
the 86 with a non-degenerate clip and no changing keys; the six scenery rigs
(chest, rock, furnace, tree, altar, bone pile) ship no idle action at all.

The v5 `.bon` files carry their bone tracks inline (`embedded_tracks`), which is
why a v5 rig with no `.stck` still animates: the SMD action range plus the
skeleton's own keys are all the pose sampler needs. Where a rig ships a `.stck`
(v6+), the SMD names it and the same code path reads it.

## Facing (investigated, not present)

The user-visible facing of every client-placed NPC is **not in the client's
files**, and this was checked rather than assumed:

* `task_npc.data` is a 12-byte fixed header followed by 1,572 fixed 12-byte
  records: `id: u32`, `x: i16`, `z: i16`, `height: i16`, `task_count: u16`
  (18,876 bytes = 12 + 1,572 × 12, so there are no trailing fields at all).
* The `[58] NPC_ESSENCE` record (2,838 × 852 bytes) was scanned column by
  column: no u32 column is dominated by 1..7 or 1..360 values, no byte column
  in the tail is a uniform 0..255 direction byte, and no float column holds an
  angle. Its tail bytes from 728 on are a text block (GBK/UTF-16 ASCII),
  matching the greeting field layout.
* The remaining orientation source is the server's `gamedata/npcgen.data`,
  which the client does not ship.

Every placement therefore keeps the rig's own authored forward axis (yaw 0,
local `+Z` forward, exactly `walker::character::character_transform`), which is
why the observed facing looks right in game. The viewer's earlier optional
"turn to face the camera" is removed.

## Overhead names (font and drawing, from the client's own files)

The client does not decide the label look in code. `element/configs/fonts.txt`
(UTF-16LE, inside `configs.pck`) is a table of
`"family" height bold italic sample // comment` rows, and the 1.4.2 file shipped
with this client contains exactly this (decoded, comments included):

```text
//  Element Client 字体描述

//  字体名 ｜ 大小 ｜ 粗体 ｜ 斜体 ｜字符样本

"Arial Unicode MS"  12  0  0  W	//  玩家和NPC头顶称谓文字
"宋体"  20  0  0  W	//  弹出的伤害值的文字
//"UseAUIFont" 0 0 0 W // 控制使用AUIFont作为头顶文字
```

That is the whole answer for the label font: **`Arial Unicode MS`, 12 points,
regular**, and the file itself says so — the row is labelled "player and NPC
overhead name text". The second row (20-point `宋体`) is the floating damage
number, and `UseAUIFont`, which would switch the labels to the interface font,
is commented out in the shipped file.

How the client consumes it (`elementclient.exe`, base `0x400000`):

* `CECGame::CreateFonts` @ `0x42ec80` opens `Configs\fonts.txt` (`0x951fa0`),
  logs `Failed to open file Configs\fonts.txt` (`0x951f64`) when it cannot, and
  otherwise walks the table line by line (`0x76e120` / `0x78d570`).
* Each row's family is compared against the UTF-16LE literal `UseAUIFont` @
  `0x951f4c`; every other row creates an `A3DFont`
  (`A3DFontMan::CreateA3DFont`) and is appended to the font vector at
  `this+0x4ec` / count `this+0x4f0` / capacity `this+0x4f4`.
* `CECGame::GetFont(index)` @ `0x42f080` hands those out, falling back to the
  interface font when the index is missing — index 0 is the overhead-name font,
  index 1 the damage-number font (`0x5ad040` releases both).
* `A3DFont` is GDI-backed (`A3DFontMan::Init` creates a bitmap + memory DC) and
  draws glyph quads through `A3DFont::CalculateCharPos` /
  `A3DFont::PrintCharsOnTextures`, so labels are **fixed-size screen text**: no
  distance scaling and no distance fade. The separately registered TTFs
  (`configs/font.ini`: `FontPath1: data\FZL2JW.ttf`,
  `FontPath2: data\FZXH1JW.ttf`, also shipped in `fonts.pck` as
  `fonts/fzl2jw.ttf` and `fonts/fzxh1jw.ttf`) are the AUI fonts, used only when
  `UseAUIFont` is enabled.

The viewer therefore reads the same table out of the client's `configs.pck` at
run time and uses that family and size for every label:

* the family is resolved to a real font file, because GDI finds "Arial Unicode
  MS" by name from the system font folder while a viewer needs the bytes — the
  file is taken from `%WINDIR%\Fonts` (`arialuni.ttf`), then from the client's
  own `data\fzl2jw.ttf` / `data\fzxh1jw.ttf` (or `fonts.pck`) when the system
  font is missing, and the client's CJK font is stacked behind it so Chinese
  and symbol names always resolve;
* labels are drawn at the table's 12 px in every direction — no distance-based
  size change, no fade — centred on the head with one pixel of black outline,
  matching the client's own `字体描边` (font border) option. The shipped table
  carries no colour column and no other client file names an overhead-name
  colour, so the label is white on the client's black border rather than a
  colour invented here;
* a label is dropped when the head is not visible: the viewer samples the
  terrain along the camera-to-head ray and sweeps the same BMD building/tree
  hulls the Walker collides with, so no name is ever visible through a wall,
  a roof or a hill. Bodies stay visible; only the label goes away.
