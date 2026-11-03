#!/usr/bin/env python3
"""Survey region, sky and weather evidence in the supplied PW 1.4.2 client.

Run from the repository root after staging ignored assets under google_drive/:
  python3 tools/region-weather-survey.py
"""
from __future__ import annotations

import collections
import pathlib
import struct
import sys

sys.path.insert(0, "tools")
import pck_lib

ROOT = pathlib.Path("google_drive")


def u32(data: bytes, pos: int) -> int:
    return struct.unpack_from("<I", data, pos)[0]


def f32(data: bytes, pos: int) -> float:
    return struct.unpack_from("<f", data, pos)[0]


def text(data: bytes, pos: int) -> tuple[str, int]:
    size = u32(data, pos)
    pos += 4
    raw = data[pos : pos + size]
    if len(raw) != size:
        raise ValueError("truncated ECWLD string")
    return raw.decode("gbk", "replace"), pos + size


def world_scene() -> tuple[list[dict], list[dict]]:
    world = (ROOT / "world.ecwld").read_bytes()
    if u32(world, 0) != 0x45435744 or u32(world, 4) != 12:
        raise ValueError("expected ECWLD v12")
    rows, cols = u32(world, 28), u32(world, 32)
    count = u32(world, 24)
    tree_off = u32(world, 36)
    offsets = [u32(world, 116 + 4 * i) for i in range(count)]
    order = sorted(offsets)
    boundaries = {off: (order[i + 1] if i + 1 < len(order) else tree_off) for i, off in enumerate(order)}
    refs: list[dict] = []
    effects: list[dict] = []
    for off in offsets:
        end = boundaries[off]
        block = world[off:end]
        row, col = struct.unpack_from("<HH", block, 0)
        n = [u32(block, 4 + i * 4) for i in range(10)]
        p = 44 + n[0] * 16
        p += n[1] * 8  # water references
        p += n[3] * 8  # ornament references
        for _ in range(n[4]):
            refs.append({"id": u32(block, p), "offset": u32(block, p + 4), "block": (row, col)})
            p += 8
        p += n[2] * 8  # grass references
        for _ in range(n[5]):
            rec = block[p : p + 60]
            if len(rec) != 60:
                raise ValueError(f"short scene effect record in block {(row, col)}")
            effect_id = u32(rec, 0)
            valid_time = struct.unpack_from("<i", rec, 48)[0]
            path, p = text(block, p + 60)
            effects.append({"id": effect_id, "time": valid_time, "path": path, "block": (row, col)})
        # Parsing later arrays is unnecessary for the survey because every block
        # boundary and the native parser's effect records constrain this prefix.
        # Ensure the scene's declared grid is sane while here.
        if row >= rows or col >= cols:
            raise ValueError(f"invalid block coordinate {(row, col)}")
    return refs, effects


def main() -> None:
    configs = ROOT / "configs.pck"
    _, config_entries = pck_lib.directory(str(configs))
    sky_entry = next((e for e in config_entries if e[0].lower().endswith("/sky.lst")), None)
    print(f"configs.pck entries={len(config_entries)}")
    if sky_entry:
        print("configs/autofamilyconfigs/sky.lst:")
        print(pck_lib.read_entry(str(configs), sky_entry).decode("gbk", "replace").rstrip())

    if (ROOT / "textures.pck").is_file():
        _, texture_entries = pck_lib.directory(str(ROOT / "textures.pck"))
        sky_dirs = sorted({e[0].split("/")[2] for e in texture_entries if e[0].lower().startswith("textures/sky/") and len(e[0].split("/")) > 3})
        named = [e[0] for e in texture_entries if any(k in e[0].lower() for k in ("rain", "snow", "cloud", "fog"))]
        print(f"textures.pck entries={len(texture_entries)} sky-directories={len(sky_dirs)} sky-dir-samples={sky_dirs[:24]}")
        print(" textures atmospheric-name hits:", named[:30])
    if (ROOT / "gfx.pck").is_file():
        _, gfx_entries = pck_lib.directory(str(ROOT / "gfx.pck"))
        hits = [e[0] for e in gfx_entries if any(k in e[0].lower() for k in ("rain", "snow", "cloud", "fog"))]
        print(f"gfx.pck entries={len(gfx_entries)} atmospheric-name hits={len(hits)}")
        for name in hits:
            print(" ", name)

    refs, effects = world_scene()
    world = (ROOT / "world.ecwld").read_bytes()
    print(f"world.ecwld rows={u32(world, 28)} cols={u32(world, 32)} blocks={u32(world, 24)} box-refs={len(refs)} placed-effects={len(effects)}")
    bsd = (ROOT / "world.ecbsd").read_bytes()
    if (u32(bsd, 0), u32(bsd, 4), u32(bsd, 8)) != (0x42584f4d, 0x42534446, 12):
        raise ValueError("expected EC-BSD v12")
    unique = {}
    for ref in refs:
        unique.setdefault(ref["id"], ref["offset"])
    areas = []
    for ident, off in sorted(unique.items()):
        areas.append({"id": ident, "global": bool(u32(bsd, off)), "flags": u32(bsd, off + 4), "transition_ms": u32(bsd, off + 0x9C)})
    print(f"world.ecbsd unique-boxes={len(areas)} local={sum(not a['global'] for a in areas)} default={sum(a['global'] for a in areas)}")
    print(" celestial flags:", dict(sorted(collections.Counter(a["flags"] for a in areas).items())))
    print(" transition ms:", dict(sorted(collections.Counter(a["transition_ms"] for a in areas).items())))
    print(" box samples:")
    for a in areas[:12]:
        print(" ", a)
    print(" scene effect valid_time counts:", dict(sorted(collections.Counter(e["time"] for e in effects).items())))
    by_path = collections.Counter(e["path"] for e in effects)
    print(" unique placed effect paths:", len(by_path))
    atmospheric = [(name, count) for name, count in by_path.items() if any(k in name.lower() for k in ("rain", "snow", "cloud", "fog", "wind", "weather"))]
    print(" atmospheric-named placed effects:", sorted(atmospheric, key=lambda item: (-item[1], item[0])))
    print(" most common scene effects:", by_path.most_common(20))

    exe = (ROOT / "elementclient.exe").read_bytes()
    print("elementclient.exe relevant ASCII strings:")
    markers = (b"A3DRain::Init", b"rain.tga", b"snow.bmp", b"Textures\\\\Sky", b"Textures\\Sky\\\\%s", b"fog_start_day", b"fog_end_day", b"fog_color_water", b"sky_id_day", b"sky_id_night")
    for needle in markers:
        pos = exe.find(needle)
        if pos >= 0:
            end = exe.find(b"\0", pos)
            print(f"  file+0x{pos:x}: {exe[pos:end].decode('ascii', 'replace')}")


if __name__ == "__main__":
    main()
