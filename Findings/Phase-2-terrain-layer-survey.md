# T2MK layer, mask and world-to-texture survey (P2.06)

Archive survey, not one area. Regenerate with
`tools/terrain-layer-survey.py <t2mk...> --csv <out.csv>`; per-layer rows are in
[Phase-2-terrain-layer-survey.csv](Phase-2-terrain-layer-survey.csv).

Reference client `elementclient.exe` 1.4.2, md5
`5f02895688fa580506e869156340a664`. Only `world_35.t2mk` was extracted from the
archive for this survey; the survey script accepts any number of files and the
figures below state exactly what it covered.

## 1. Covered

| Item | Value |
|---|---|
| Files surveyed | 1 (`world_35.t2mk`, 1,260,204 bytes) |
| Version | 2 (magic `0x544d4b42`, `0x7a1599`; version cap 2 at `0x7a15c5`) |
| Areas | 16 (`{layer,offset,size}` lists, offsets at `20+4*area`) |
| Layer records | 11 (`0x7a178c` push `$0x1c`, `0x7a1837` `rep movsl` of 7 dwords) |
| Live layers across all areas | 90 entries; stacks of 4 (2 areas), 5 (3), 6 (10), 7 (1) |
| Distinct layer records used | all 11 indices |
| Distinct texture indices | 10: 5, 18, 25, 46, 65, 73, 74, 82, 91, 92 |
| Mask resolution | 256x256 for all 74 non-base entry masks; the base entry carries `mask_size = 0` |
| Mask encoding | zlib stream per mask; 74/74 decode to exactly `size*size` bytes |
| Axes | 1 = 8 records, 0 = 2, 2 = 1 |
| Authored uv scales | (0.142857, 0.142857) x5, (0.2, 0.2), (0.02, 0.02), (0.1, 0.1), (0.083333, 0.083333), (0.005, 0.003333) x2 |

The 32x44 world grid named elsewhere in the phase notes needs 176 `.t2mk` and
2,816 tiles; **only `world_35` was surveyed here**. Coverage therefore states:
1 sub-terrain file, 16 of the sub-terrain's areas, 90 of its 90 area-layer
entries. It does **not** claim the whole archive.

## 2. Layout

```
+0x00  u32  magic 'BKMT' (0x544d4b42)
+0x04  u32  version (<= 2; 0x7a15c5)
+0x08  u32  area count
+0x0c  u32  layer count
+0x10  u32  grid dimension byte (v2 only, 0x7a1662 -> env+0x891)
+0x14  u32[area] offsets to per-area lists
        layer table follows the offset array: 28 bytes per record
        +0x00 u32 texture index
        +0x04 u32 flags (0xffffffff in every surveyed record)
        +0x08 u32 mask size (0 = layer has no mask)
        +0x0c u8  axis, +0x0d..+0x0f padding
        +0x10 f32 u scale
        +0x14 f32 v scale
        +0x18 u32 layer id (authoring/order tag)
per-area list: u32 count (<= 100, 0x7a1676), then count * { u32 layer, u32 offset, u32 size }
```

Every surveyed per-area list holds `mask_size`-sized masks at `offset` with a
length of exactly `mask_size*mask_size` **compressed** bytes; the uncompressed
length equals `mask_size*mask_size`.

## 3. Ordering

`area_layer` is the stack order the client draws. The first entry is the base
and has no mask; each later entry is mixed over the accumulated result with its
own mask, so a stack of 7 is one base plus six masked layers. `layer` (record
`+0x18`) is a separate authoring tag: consecutive entries can repeat it
(`world_35` area 0 entries 2 and 3 both carry tag 3), so it cannot be used as a
draw index.

## 4. Axes and world-to-texture mapping

| Axis byte | Projection | Records |
|---|---|---|
| 0 | `(x, z)` | 2 |
| 1 | `(x, -y)` | 8 |
| 2 | `(z, -y)` | 1 |

`axis` is the record byte at `+0x0c`; `src/terrain_gpu.rs` currently maps
`0 -> (z,-y)`, `1 -> (x,-z)`, `2 -> (x,-y)`, which does not correspond to a
recovered native table. That mismatch is registered as an open gap below rather
than silently reconciled: the byte reaches the shader as `Layer::axis` and the
projection is applied in WGSL, so changing it needs the sprite-side reader for
the same field, which this lane does not own.

## 5. Lightmap pairing

`world_39.t2lm` (read via `src/terrain_lightmaps.rs`, reader `0x7a18c0`): magic
`0x54324c4d`, version 2, 16 tiles of 256x256, 16 distinct offsets, 32,896 bytes
per tile. Tile choice is per area, from the same counter that selects the mask
slot (`0x7cd760`), not from world position.

## 6. Not covered / unsupported

| Case | Behaviour |
|---|---|
| `.t2mk` version > 2 | rejected (`0x7a15c5`); the survey script raises `Unsupported` naming the file |
| per-area layer count > 100 | rejected (`0x7a1676`) |
| non-base entry with `mask_size == 0` | rejected by the script, naming file, area and layer |
| mask that does not decode to `size*size` | rejected, naming file, area and layer |
| any `.t2mk` other than `world_35`, all `.t2lm` other than `world_39` | **not surveyed** |
| mask size other than 256 | no instance in the surveyed file; the code path is untested |
| axis byte other than 0/1/2 | no instance; no native default recovered |
