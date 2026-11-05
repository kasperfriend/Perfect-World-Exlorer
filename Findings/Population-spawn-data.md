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
  heights, rigs and idle `休闲` scene action straight out of `models.pck`.
  The 1,057 server-placed records are reported, never stacked at the origin.
* The settings window reports how many placements are inside the spawn radius
  and can move the camera to the nearest one: at the client's native 420 units
  the origin has 9 placements, Archosaur North 61, Raging Tides 0.
* Streamed around the camera, at most 6 new rigs per frame (an 888 MB archive
  is decompressed on the render thread), 48 actors by default with a hard cap
  at 96.
* No combat, dialogue or interaction; positions, names, heights and greetings
  are the files' own, and nothing is synthesised to fill the mob/route gaps.
