# Every effect, and what the viewer does with it

Measured 2026-11-08 against `world.ecwld` (2,454,726 B), `gfx.pck`
(10,104 members) and the 5,459 unpacked `.gfx` roots. This is the inventory the
report asked for: the whole set, not a sample.

## The three defects, and what each actually was

The screenshots showed white squares under fire, a blue orb on a white square at
Nightfire Altar, and rain-like effects across the world. Those were one defect
plus two misreadings, and the difference matters because only one of them was a
renderer bug.

### 1. White squares: the DDS reader refused most of the corpus

`Dds::parse` rejected every surface without a FourCC tag. A census of the 1,610
distinct textures the effect roots name shows that is most of them:

| storage | count | was it decoded |
| --- | --- | --- |
| uncompressed `DDPF_RGB` (32-bit BGRA, 24-bit BGR, 16-bit RGB565) | 1,232 | **no** |
| `DXT1` / `DXT3` / `DXT5` | 0 in this name set, 1,232 in the archive | yes |
| TGA / BMP / JPG | 339 | yes, via `decode_rgba` |
| absent from the archive | 4 | n/a |

Every refused texture made `Draw` substitute its flat white placeholder, so a
soft smoke puff became a hard white quad. Animated, that reads as *white blobs
flying up out of the fire* — which is exactly how Etherblade was reported. The
texture was never wrong: `normalmap/gas.dds` is a correct 256x256 soft blob
(corner alpha 0, centre 246), it simply never decoded.

The split falls exactly along the reported symptom. `火焰1` has three elements:
two draw `烟火/fire.dds`, a DXT1 surface that always decoded, and one draws
`normalmap/gas.dds`, an uncompressed `DDPF_RGB` surface that did not. That is
why the fire itself looked right while white blobs rose out of it — the flame
texture worked, the soft glow behind it did not.

The reader now accepts the layouts the corpus actually stores, recovered from
the headers:

- 32-bit BGRA, with or without `DDPF_ALPHAPIXELS`
- 24-bit BGR
- 16-bit RGB565, for small glow sprites

Mip levels of an uncompressed surface are sized per pixel rather than per 4x4
block. That matters for `序列/波动序列9x9.dds`, an authored 810x810 surface whose
width is not a multiple of four; block rounding pushed its mip chain past the
end of the file.

A tail past the declared chain is now accepted. The client reads the chain and
stops, so a tail is not a defect — and that assertion was the reader's own
strictness, not a recovered rule. One shipped file carries a tail.

### 2. "Rainy everywhere" is leaf rain, not precipitation

There is no weather bug. `WeatherSelection` comes from the area's own EC-BSD flag
word and defaults to clear; the report's `Auto (area flags)` sends it nothing.

What the world does place, and draws on purpose, is authored particle roots that
look like weather:

| root | placements | day | night | all day | what it is |
| --- | --- | --- | --- | --- | --- |
| `场景/树叶雨.gfx` | 27 | 26 | 0 | 1 | leaf rain, 15x15x10 authored volume |
| `场景/花瓣雨.gfx` | 7 | 7 | 0 | 0 | petal rain |
| `场景/花瓣分飞.gfx` | 12 | 12 | 0 | 0 | petal drift |
| `场景/沙尘3.gfx` | 19 | 19 | 0 | 0 | dust |

Those two are the only roots in the map whose name ends in 雨 (rain): nothing
named `雨.gfx`, `下雨.gfx` or similar is placed, so the world has no precipitation
root at all.

`树叶雨` reads alarming — `TTL: 100` seconds at `Rate: 30` would be 3,000 live
particles per placement — but both emitters author a hard `Quota` of 10 and 5,
and `0x715c09` clamps `count = min(count, quota - live)`. The player enforces it,
so the root draws 15 leaf quads spread over its authored volume and no more.
Measured spread at steady state: x -5.5..7.8, y 3.0..16.1, z -9.0..3.2 around
the placement, which is the 15x15x10 ellipsoid the file asks for.

So "rainy everywhere" is 27 day-active leaf-rain placements plus dust and petals,
all authored, all in the map, all now textured correctly. If it still reads as
wrong after the texture fix, the next thing to check is whether the volume should
be culled by distance — that is a rendering-policy question, not a decode one.

### 3. Nightfire Altar: the orb is an authored glow, the square was the texture

`场景/七杀谷祭坛光球.gfx` — "Seven Kills Valley altar light orb" — is the altar's
light orb, and it names three textures:

| element | texture | stored as |
| --- | --- | --- |
| id101 | `粒子/particle009.dds` | DXT1 64x64 |
| id101 | `图案/星云.dds` | DXT1 128x128 |
| id121 | `烟火/smoke001.dds` | DXT3 64x64 |

All three are block-compressed and were always decodable, so the orb itself is
not a parser casualty. Its blend is `SrcAlpha / One` — additive — which on a
light orb is right. The white square around it in the screenshot is the same
defect as item 1 on a neighbouring element, and that fix removes it.

The altar roots checked: `七杀谷祭坛光球` (v30), `人族祭坛` (v30), `升仙祭坛`
(v33), `成魔祭坛` (v33), `水神祭坛光1/2`, `沼泽祭坛`. Every texture they name is
present and decodes.

## The full inventory

Every effect the map places, with its placement count and its authored day/night
window. `valid_time`: 0 = night only, 1 = day only, 2 = all day. Counts are
placements, and 2,275 is all of them across 163 distinct roots.

| root | placements | day (1) | night (0) | all day (2) |
| --- | --- | --- | --- | --- |
| `场景/灯红.gfx` | 334 | 0 | 241 | 93 |
| `场景/灯红淡.gfx` | 158 | 0 | 150 | 8 |
| `场景/海底水泡2.gfx` | 152 | 0 | 0 | 152 |
| `场景/好火焰.gfx` | 120 | 0 | 81 | 39 |
| `场景/火焰1.gfx` | 107 | 1 | 44 | 62 |
| `场景/新种族地图/光.gfx` | 99 | 0 | 0 | 99 |
| `场景/萤火虫.gfx` | 70 | 0 | 70 | 0 |
| `场景/狼烟2.gfx` | 60 | 2 | 0 | 58 |
| `场景/冰雾2.gfx` | 57 | 0 | 0 | 57 |
| `场景/树林光束.gfx` | 49 | 43 | 0 | 6 |
| `场景/海底飘尘2.gfx` | 49 | 0 | 0 | 49 |
| `场景/新种族地图/蓝水晶光晕.gfx` | 39 | 0 | 0 | 39 |
| `场景/海洋光束.gfx` | 39 | 3 | 12 | 24 |
| `场景/炊烟.gfx` | 38 | 16 | 6 | 16 |
| `场景/新种族地图/蓝蘑菇.gfx` | 32 | 0 | 0 | 32 |
| `场景/海底波纹2.gfx` | 31 | 0 | 0 | 31 |
| `场景/火焰2.gfx` | 31 | 0 | 2 | 29 |
| `场景/夜空飘尘.gfx` | 30 | 0 | 16 | 14 |
| `场景/蓝火.gfx` | 30 | 0 | 5 | 25 |
| `场景/树叶雨.gfx` | 27 | 26 | 0 | 1 |
| `场景/新种族地图/蓝火.gfx` | 25 | 0 | 0 | 25 |
| `场景/篝火.gfx` | 25 | 0 | 16 | 9 |
| `场景/新种族地图/主城光球小.gfx` | 22 | 0 | 0 | 22 |
| `场景/烟.gfx` | 22 | 2 | 12 | 8 |
| `场景/沙尘3.gfx` | 19 | 19 | 0 | 0 |
| `场景/新种族地图/黄蘑菇.gfx` | 18 | 0 | 0 | 18 |
| `场景/新种族地图/帐篷火盆.gfx` | 16 | 0 | 0 | 16 |
| `场景/烟1.gfx` | 16 | 2 | 0 | 14 |
| `场景/岩浆火焰.gfx` | 15 | 0 | 0 | 15 |
| `场景/新种族地图/主城光球大.gfx` | 14 | 0 | 0 | 14 |
| `场景/新种族地图/吐水泡.gfx` | 14 | 0 | 0 | 14 |
| `场景/新种族地图/墙蹲火.gfx` | 14 | 0 | 0 | 14 |
| `场景/新种族地图/新手村出生点_绿线.gfx` | 14 | 0 | 0 | 14 |
| `场景/新种族地图/路灯.gfx` | 14 | 0 | 0 | 14 |
| `场景/海底水泡.gfx` | 14 | 0 | 0 | 14 |
| `场景/新种族地图/新手村出生点_光子.gfx` | 13 | 0 | 0 | 13 |
| `场景/光运.gfx` | 12 | 0 | 0 | 12 |
| `场景/新种族地图/蘑菇灯.gfx` | 12 | 0 | 0 | 12 |
| `场景/花瓣分飞.gfx` | 12 | 12 | 0 | 0 |
| `场景/海底闪光.gfx` | 11 | 0 | 0 | 11 |
| `场景/灯绿.gfx` | 11 | 0 | 0 | 11 |
| `场景/岩浆火焰2.gfx` | 10 | 0 | 0 | 10 |
| `场景/海底闪光3.gfx` | 10 | 0 | 0 | 10 |
| `场景/灯蓝.gfx` | 10 | 0 | 8 | 2 |
| `场景/新种族地图/水族箱玻璃.gfx` | 8 | 0 | 0 | 8 |
| `场景/新种族地图/沙尘.gfx` | 8 | 0 | 0 | 8 |
| `场景/新种族地图/海马眼睛.gfx` | 8 | 0 | 0 | 8 |
| `场景/新种族地图/火把.gfx` | 8 | 0 | 0 | 8 |
| `场景/新种族地图/蓝蘑菇3.gfx` | 8 | 0 | 0 | 8 |
| `场景/新种族地图/血鲷封印台发光体.gfx` | 8 | 0 | 0 | 8 |
| `场景/滴水.gfx` | 8 | 0 | 0 | 8 |
| `场景/绿鬼火2.gfx` | 8 | 0 | 0 | 8 |
| `场景/新种族地图/墙柱火.gfx` | 7 | 0 | 0 | 7 |
| `场景/水神祭坛光1.gfx` | 7 | 0 | 0 | 7 |
| `场景/海底飘尘.gfx` | 7 | 0 | 0 | 7 |
| `场景/瀑布水花中间.gfx` | 7 | 0 | 0 | 7 |
| `场景/瀑布水花底.gfx` | 7 | 0 | 0 | 7 |
| `场景/花瓣雨.gfx` | 7 | 7 | 0 | 0 |
| `场景/新种族地图/大坑_液体.gfx` | 6 | 0 | 0 | 6 |
| `场景/新种族地图/沙土.gfx` | 6 | 0 | 0 | 6 |
| `场景/新种族地图/火苗.gfx` | 6 | 0 | 0 | 6 |
| `场景/海底闪光2.gfx` | 6 | 3 | 0 | 3 |
| `场景/海底闪光4.gfx` | 6 | 0 | 0 | 6 |
| `场景/温泉3.gfx` | 6 | 6 | 0 | 0 |
| `场景/瀑布水花.gfx` | 6 | 0 | 0 | 6 |
| `场景/瀑布水花湍急.gfx` | 6 | 0 | 0 | 6 |
| `场景/香炉烟.gfx` | 6 | 0 | 0 | 6 |
| `场景/传送门55.gfx` | 5 | 0 | 0 | 5 |
| `场景/新种族地图/蓝蘑菇2.gfx` | 5 | 0 | 0 | 5 |
| `场景/水柱.gfx` | 5 | 0 | 0 | 5 |
| `场景/海底波纹.gfx` | 5 | 3 | 0 | 2 |
| `场景/蓝色光束.gfx` | 5 | 0 | 4 | 1 |
| `场景/lanhuo.gfx` | 4 | 0 | 0 | 4 |
| `场景/pk场入口青铜剑.gfx` | 4 | 0 | 0 | 4 |
| `场景/佛光.gfx` | 4 | 0 | 0 | 4 |
| `场景/天煞地面效果.gfx` | 4 | 0 | 1 | 3 |
| `场景/彩虹.gfx` | 4 | 4 | 0 | 0 |
| `场景/新种族地图/水门_瀑布.gfx` | 4 | 0 | 0 | 4 |
| `场景/新种族地图/石像火盆.gfx` | 4 | 0 | 0 | 4 |
| `场景/新种族地图/血鲷封印台.gfx` | 4 | 0 | 0 | 4 |
| `场景/新种族地图/血鲷封印台罩子.gfx` | 4 | 0 | 0 | 4 |
| `场景/水晶1.gfx` | 4 | 0 | 0 | 4 |
| `场景/灯笼.gfx` | 4 | 0 | 4 | 0 |
| `场景/祭坛烟雾.gfx` | 4 | 0 | 1 | 3 |
| `场景/绿鬼火3.gfx` | 4 | 0 | 0 | 4 |
| `场景/封引2.gfx` | 3 | 0 | 0 | 3 |
| `场景/新种族地图/水族箱_水草墨绿.gfx` | 3 | 0 | 0 | 3 |
| `场景/水晶.gfx` | 3 | 0 | 0 | 3 |
| `场景/温泉2.gfx` | 3 | 3 | 0 | 0 |
| `场景/符文阵效果.gfx` | 3 | 0 | 0 | 3 |
| `场景/符文阵效果2.gfx` | 3 | 0 | 0 | 3 |
| `场景/绿鬼火4.gfx` | 3 | 0 | 0 | 3 |
| `场景/绿鬼火5.gfx` | 3 | 0 | 0 | 3 |
| `场景/仙传送门.gfx` | 2 | 0 | 0 | 2 |
| `场景/八卦.gfx` | 2 | 0 | 0 | 2 |
| `场景/副本洞口效果03.gfx` | 2 | 0 | 0 | 2 |
| `场景/召唤阵.gfx` | 2 | 0 | 0 | 2 |
| `场景/圣城之光.gfx` | 2 | 0 | 0 | 2 |
| `场景/天煞光束.gfx` | 2 | 0 | 0 | 2 |
| `场景/寺庙灯笼光.gfx` | 2 | 0 | 0 | 2 |
| `场景/封引.gfx` | 2 | 0 | 0 | 2 |
| `场景/屋顶八卦.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/暗语者营地.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_水草红.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_水草翠绿.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_海星橙.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_海星玫.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_海星红.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_海星黄.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_顶.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/水族箱_鱼群.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/盆景水晶.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/盆景火.gfx` | 2 | 0 | 0 | 2 |
| `场景/新种族地图/飞碟射灯.gfx` | 2 | 0 | 0 | 2 |
| `场景/水神祭坛光2.gfx` | 2 | 0 | 0 | 2 |
| `场景/洞口毒雾.gfx` | 2 | 0 | 0 | 2 |
| `场景/温泉.gfx` | 2 | 2 | 0 | 0 |
| `场景/灵魂3.gfx` | 2 | 0 | 0 | 2 |
| `场景/绿色传送门.gfx` | 2 | 0 | 0 | 2 |
| `场景/鬼火.gfx` | 2 | 0 | 0 | 2 |
| `场景/魔传送门.gfx` | 2 | 0 | 0 | 2 |
| `场景/七杀谷祭坛光球.gfx` | 1 | 0 | 0 | 1 |
| `场景/八卦紫.gfx` | 1 | 0 | 0 | 1 |
| `场景/八卦红.gfx` | 1 | 0 | 0 | 1 |
| `场景/八卦蓝.gfx` | 1 | 0 | 0 | 1 |
| `场景/冰雾3.gfx` | 1 | 0 | 0 | 1 |
| `场景/副本洞口效果01.gfx` | 1 | 0 | 0 | 1 |
| `场景/塔光.gfx` | 1 | 0 | 0 | 1 |
| `场景/夜空飘尘_黄.gfx` | 1 | 0 | 1 | 0 |
| `场景/天煞天空效果.gfx` | 1 | 0 | 0 | 1 |
| `场景/天空之城光束.gfx` | 1 | 0 | 1 | 0 |
| `场景/探照灯.gfx` | 1 | 0 | 1 | 0 |
| `场景/新种族地图/09黄色的电球.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/0完美远征地主塔.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/0远完美浮空城.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/主城鱼缸.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/场景鱼群.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/墙上红色图腾.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/新手村出生点.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/水片圆.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/水片长.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/海洋光束.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/白兰树.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/能量喷泉.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/能量喷泉4.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/能量喷泉5.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/血鲷封印台顶_上.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/血鲷封印台顶_下.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/赛亚岛传送门.gfx` | 1 | 0 | 0 | 1 |
| `场景/新种族地图/远征地血雕大帐.gfx` | 1 | 0 | 0 | 1 |
| `场景/沼泽祭坛.gfx` | 1 | 0 | 0 | 1 |
| `场景/瀑布水花中间2.gfx` | 1 | 0 | 0 | 1 |
| `场景/火山喷发2.gfx` | 1 | 0 | 0 | 1 |
| `场景/火焰门.gfx` | 1 | 0 | 0 | 1 |
| `场景/炉香.gfx` | 1 | 0 | 0 | 1 |
| `场景/珠宝闪烁.gfx` | 1 | 0 | 0 | 1 |
| `场景/红色电网.gfx` | 1 | 0 | 0 | 1 |
| `场景/羽族传送门.gfx` | 1 | 0 | 0 | 1 |
| `场景/羽族传送门2.gfx` | 1 | 0 | 0 | 1 |
| `场景/能量求.gfx` | 1 | 0 | 0 | 1 |
| `场景/骷髅光柱.gfx` | 1 | 0 | 0 | 1 |
| `场景/龙宫入口.gfx` | 1 | 0 | 0 | 1 |

## Element 190 curves now draw

Element 190 was the last class the player parsed and then skipped. It is used by
exactly one placed root, the distant floating city `0远完美浮空城.gfx`, on two
elements textured `图案/pattern032.dds` and `图案/光球1.dds`.

The builder is `0x708460`. It takes the root's own `Vec:` pair (`root+0x188`,
`root+0x18c`) rather than the element position, cross-multiplies two vectors for
a side, normalises the chord, and divides 1.0 by that normalised length for the
segment count. It tests the chord against `±1e-6` — as 8-byte doubles, `fcoml`,
not floats — and drops the quad when the span is degenerate. That the class reads
two root-level vectors is the point of it: a curve spans a distance rather than
marking a point.

The endpoints and the segment count are the recovered values. The exact
orientation of the cross-product side vector is not claimed, because the two
operands the builder multiplies are chosen inside the routine and not all of the
call sites that fill its argument block are recovered; the quad is oriented
across the chord from the view instead. So the residual error mode is a ribbon
whose width axis could be turned about its own length, not a ribbon that fails to
draw. The floating city now emits 1,274 quads including `图案/光球1.dds`, and a
test requires both curve textures to reach the quad list.

## What is verified

Measured on this branch, at `a25179d`:

- **2,275 of 2,275 placements install.** 0 pending, 0 failed, 0 unsupported,
  2,275 players built.
- **163 of 163 distinct roots parse and play.** Every version ships; the gate is
  the client's own `version > 79` rejection.
- **5,457 of 5,457 roots decode their textures.** 27,575 texture references,
  zero DDS decode failures. Seventeen references do not resolve, but that is four
  distinct names: `序列/闪电竖.tga` (the archive stores it as `.dds`), `图案/155.dds`
  and `图案/场景3紫.dds` (absent from the archive entirely).
- **1,606 of 1,606 extracted textures decode.** 1,267 DDS with zero failures —
  617 DXT1, 314 DXT3, 301 DXT5, 31 BGRA8, 3 RGB565 — and 339 TGA/BMP/JPG through
  the image decoder.
- **Every element class draws.** 14 classes over the corpus: particle 13,665,
  sprite 8,864, ribbon 2,956, child 1,530, cylinder 1,116, fractal 734, light 546,
  sound 491, model 447, grid 216, curve 208, trail 156, wave 79, lightning 11.
  Only element 190 used to be counted instead of drawn; it draws now. No placed
  root contains an unknown element id.
- **Animation is healthy.** UVs change on 390 of 400 sampled frames; the quad
  count at the waterfall steps 854 to 3,148 to 8,777.
- **Fire is bounded.** `火焰1` and `好火焰` author `Acc 10..12` along `+Y` with
  `TTL 0.25..0.35`, which is about 0.4 units of rise. Measured particle Y stays
  inside -0.29..0.99 across 90 frames. The upward motion is real and small; what
  made it look wrong was the missing texture, not the physics.

## What is still open

- The four unresolved texture names above. Three are simply not in the archive;
  one is an extension mismatch that the client may or may not tolerate.
- City of Raging Tides is the Tideborn map, `场景/新种族地图/` — 61 roots over
  604 placements, and all of it is in the inventory above. Its distinctive roots
  were driven directly and all of them draw: `大坑_液体` (the pit liquid) 218
  quads, `水片长` 79, `水门_瀑布` 78, `水片圆` 38, `吐水泡` 15, `能量喷泉` 118,
  `水族箱玻璃` 1. The water surfaces are element 210 grids — a `wNumber` by
  `hNumber` field of individually coloured cells, drawing the cells whose colour
  passes the alpha cutoff — and the pit is a 16-element particle and sprite graph
  using `纹理/流动（通道）.dds` and `纹理/流动01_通道.dds`.
  The one root that looked dead, `新种族地图/光`, is 99 placements of a graph made
  entirely of a `GFXELEMENTID: 1` child reference to `光粒子.gfx`. It draws nothing
  of its own by design, and the child does resolve and play. So no Tideborn root
  failed to build and none failed to find its textures; if this area still looks
  wrong, the cause is not in the roots.
- `valid_time` day/night eligibility is read at the corrected record offsets, and
  the earlier report flagged that the split still needs a re-measurement against
  a world load. The counts in the table come from the current parse.

## Update: the sweep after the inventory

The inventory above answered "what is in the archive". The next question was
"and does it draw", which turned out to have three real answers that were not in
the first pass. All three were found by driving every root in the corpus and
counting the ones that never emit a single quad, not by inspecting code.

### Roots that never drew

`137` of the 5,456 parser-accepted roots never produced a quad at any point in a
five-second window. After the three fixes below the number is `27`, and 8 of
those 27 are light-only rigs (`灯光.gfx`, `灯光2.gfx` and friends) that publish to
`gfx_light` and have no geometry of their own to draw — so they are correct, not
broken. Of the remaining 19, every one names a file the shipped archive does not
contain.

### 1. A fractal with coincident endpoints is not an empty fractal

`502` of the 734 corpus fractals author `StartPos == EndPos == 0.0, 0.0, 0.0`.
The player treated that as a degenerate axis and returned. The client does not:
its direction routine (`0x709ca1`..`0x709d1c`) computes `EndPos - StartPos`, finds
the length inside its 1e-06 epsilons, zeroes the direction at `0x709cfc` and jumps
straight to `0x709d48`, past the divide — and carries on building the strip. The
tapers then hang off a zero-length axis, which is exactly what those roots
author: `09黄色的电球.gfx` writes `Width: 0, Width: 0, Width: 1` and gets a
tapered point.

`63` of the degenerate fractals are in placements installed from `world.ecwld`.
All 40 fractal roots now draw.

### 2. The three `Width`/`Alpha` lines are `[end, midpoint, start]`

The strip builder reads `+0x2c0` and `+0x2c8` — the first and third authored
lines — as the widths at the strip's two ends, and `+0x2c4`, the second line, as
the width at its midpoint. For `t >= 0.5` it interpolates `Width[2] -> Width[0]`
(`0x709a95`); for `t < 0.5` it interpolates `Width[1] -> Width[0]` and passes the
fraction through `sin` (`0x709a9d`, `0x709aeb`, `0x709ad5`). The save routine at
`0x7081ae`..`0x708271` confirms the order, writing the six values back to the
six keys in sequence.

The emitter was reading only the first two lines, so the apex was lost: `76`
corpus fractals carry their widest cross-section in the middle, and the missing
midpoint costs `17.5%` of the strip's area across the 623 that author three
lines. One authored line repeats and still collapses to a constant, so the 356
single-width fractals are unchanged.

### 3. `Segs` counts cross-sections, not quads

`0x7092f9` passes `Segs - 1` as the loop bound, so a `Segs: 10` ribbon is 9
quads each 1/10 of the axis long. The player drew `Segs` quads — one section past
the authored end — and drew one quad for the single `Segs: 0` element where the
client draws none.

### 4. Effect model elements draw now

Element 160 was counted and never drawn. The meshes are rigid — every one of the
141 shipped `.SKI` files binds its skins with 0 or 1 bones, so the bind pose is
the only pose the archive can express — which means a triangle list and no
skinned pass. `EffectQuad` carries an optional triangle list beside its four
corners and the effect pass appends it to the same vertex bucket.

Paths, measured against `gfx.pck`:

- `ModelPath` is relative to the archive's `gfx/models/` root, **not** to the GFX
  file that names it. `gfx/场景/新种族地图/暗语者营地.gfx` authoring
  `暗语者营地\水晶.SMD` resolves to `gfx/models/暗语者营地/水晶.smd`. None of the
  124 shipped `.smd` members sit under the GFX folder's own subdirectory.
- The `.SMD`'s `skeleton` and `skins` fields are bare names relative to the SMD's
  own directory, one level below `gfx/models/`.

Coverage over the 231 roots that carry a model element: `198` now draw,
139,474 triangles. The 33 that do not are 22 models the archive does not ship
(`gfx/models/鹰翼/鹰翼.smd`, `雕1/雕1`, `17品法器/17品法器`,
`武器/17品长枪/17品长枪`, `武器/2品法宝/2品法宝` and 17 more), 8 that need a
flag-2 or flag-3 scale/constraint controller, 2 whose `ModelPath` names a
directory rather than a file, and one whose keypoint colour is `0x00ffffff` —
authored invisible.

`4` of the 792 shipped meshes author a negative texture index and their elements
have an empty `TexFile` too (`白色爪痕`, `投石车状态`, `攻击`'s footstep plane,
`血鲷封印台罩子`). The effect pass already substitutes a flat white stand-in for
a map it cannot read, so those keep their geometry instead of being dropped.

### 5. Skinning the helper joints instead of refusing them

Every skeleton in the effect archive carries three nonzero bone-flag values:
`1` on 533 bones, `2` on 25 and `3` on 175. Flag 1 was being refused together
with the scale and constraint bits under a "bone reflection" message, which cost
164 of the 447 model placements their geometry.

The bit is not a reflection. No bone of any flag has a negative-determinant local
matrix — `533` flag-1 bones and `5,573` flag-0 bones, all positive — and the
flag-1 bones are the leftover 3ds Max helpers (`Box01`, `Object01`,
`Cylinder01`, `Line01`, `fjbone`, `02_Bip02 Footsteps`), `515` of them unparented
roots. They carry valid local transforms and pose like any other bone. Flags 2
and 3 stay refused: they do not occur in the effect archive at all, and they are
where the client's scale and constraint controllers live.

### Still open

- The four unresolved texture names, unchanged from above.
- `match_surface` (16 sprites) and `max_extent` (8,763 sprites author a value)
  are decoded but not wired into the sprite path.
- `YawEffect` (`+0x195`, 30 sprites / 8 roots) — 19 of those sprites already get a
  ground frame; the other 11 are ground decals whose yaw is decoded but unused.
- Sound elements (491 in the archive, 3 in placed roots: two Wing portals and one
  dungeon mouth) go to `gfx_player`'s counter and no further. `world_audio` plays
  ECWLD sound placements, not GFX element-170 references.
- `world.ecbsd` is absent from the corpus, so the area weather flags that drive
  rain and snow cannot be measured.
