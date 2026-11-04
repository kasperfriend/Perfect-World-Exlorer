#!/usr/bin/env python3
"""Survey region, sky and weather evidence in the supplied PW 1.4.2 client.

Run from the repository root after staging ignored assets under google_drive/:
  python3 tools/region-weather-survey.py
"""
from __future__ import annotations

import collections
import math
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


def parse_autoscene(data: bytes) -> tuple[dict[str, str], dict[str, dict[str, str]]]:
    text = data.decode("gbk", "replace")
    section = ""
    values: dict[str, dict[str, str]] = collections.defaultdict(dict)
    for raw in text.splitlines():
        line = raw.split(";", 1)[0].strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("[") and line.endswith("]"):
            section = line[1:-1].strip().lower()
        elif "=" in line:
            key, value = line.split("=", 1)
            values[section][key.strip().lower()] = value.strip()
    resource = values.get("resource", {})
    sky = values.get("sky", {})
    sky_list = resource.get("sky_list")
    if not sky_list:
        raise ValueError("AutoFamily config is missing Resource/sky_list")
    ids = {
        "day": int(sky["sky_id_day"]),
        "night": int(sky["sky_id_night"]),
        "list": sky_list.replace(chr(92), "/"),
    }
    return ids, values


def parse_sky_list(data: bytes) -> dict[int, dict[str, str]]:
    lines = [line.strip() for line in data.decode("gbk", "replace").splitlines() if line.strip()]
    count = int(lines[0].lstrip(chr(0xFEFF)))
    result: dict[int, dict[str, str]] = {}
    at = 1
    for _ in range(count):
        ident = int(lines[at]); label = lines[at + 1]
        roof, pano1, pano2 = [lines[at + i].replace(chr(92), "/") for i in (2, 3, 4)]
        int(lines[at + 5])  # native record's trailing legacy flag
        if ident in result:
            raise ValueError(f"duplicate sky.lst ID {ident}")
        result[ident] = {"label": label, "roof": roof, "01": pano1, "02": pano2}
        at += 6
    if at != len(lines):
        raise ValueError(f"unexpected trailing sky.lst records: {len(lines) - at} lines")
    return result


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


def dot(a: tuple[float, float, float], b: tuple[float, float, float]) -> float:
    return sum(x * y for x, y in zip(a, b))


def cross(a: tuple[float, float, float], b: tuple[float, float, float]) -> tuple[float, float, float]:
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def local_obb(data: bytes, offset: int) -> tuple[tuple[float, float, float], tuple[float, float, float], list[tuple[float, float, float]]]:
    center = struct.unpack_from("<3f", data, offset + 8)
    extents = struct.unpack_from("<3f", data, offset + 0x14)
    axes = [struct.unpack_from("<3f", data, offset + 0x20 + 12 * i) for i in range(3)]
    return center, extents, axes


def obbs_overlap(a: tuple, b: tuple) -> bool:
    ca, ea, aa = a
    cb, eb, ab = b
    delta = tuple(y - x for x, y in zip(ca, cb))
    for axis in aa + ab + [cross(x, y) for x in aa for y in ab]:
        length = math.sqrt(dot(axis, axis))
        if length < 1e-8:
            continue
        axis = tuple(value / length for value in axis)
        ra = sum(ea[i] * abs(dot(aa[i], axis)) for i in range(3))
        rb = sum(eb[i] * abs(dot(ab[i], axis)) for i in range(3))
        if abs(dot(delta, axis)) > ra + rb + 1e-5:
            return False
    return True


def cloud_surface_summary(data: bytes) -> dict[str, object]:
    if data[:4] != b"MOXB" or len(data) < 16:
        raise ValueError("expected MOXB cloud-surface resource")
    width, height, count = struct.unpack_from("<3I", data, 4)
    cells = width * height
    if not width or not height or count > cells or cells > 1_000_000:
        raise ValueError("invalid MOXB dimensions or record count")
    at = 16
    seen: set[int] = set()
    primary_cells = vertices = groups = indices = empty_cells = 0
    group_size_hist: collections.Counter[int] = collections.Counter()
    group_scale_hist: collections.Counter[float] = collections.Counter()
    for _ in range(count):
        if at + 8 > len(data):
            raise ValueError("truncated MOXB cell directory")
        index, size = struct.unpack_from("<2I", data, at)
        at += 8
        end = at + size
        if end > len(data) or index >= cells or index in seen or size < 8:
            raise ValueError(f"invalid MOXB cell {index} directory entry")
        seen.add(index)
        payload = data[at:end]
        vertex_count = struct.unpack_from("<I", payload)[0]
        group_at = 4 + vertex_count * 36
        if group_at + 4 > size:
            raise ValueError(f"truncated MOXB vertex array in cell {index}")
        group_count = struct.unpack_from("<I", payload, group_at)[0]
        cursor = group_at + 4
        if group_count > (size - cursor) // 20:
            raise ValueError(f"truncated MOXB group headers in cell {index}")
        cell_indices = 0
        for group_id in range(group_count):
            params = struct.unpack_from("<4f", payload, cursor)
            if not all(math.isfinite(value) for value in params):
                raise ValueError(f"non-finite MOXB group parameters in cell {index}")
            group_scale_hist[params[3]] += 1
            index_count = struct.unpack_from("<I", payload, cursor + 16)[0]
            cursor += 20
            index_end = cursor + index_count * 4
            if index_end > size:
                raise ValueError(f"truncated MOXB indices in cell {index} group {group_id}")
            values = struct.unpack_from("<" + "I" * index_count, payload, cursor) if index_count else ()
            if any(value >= vertex_count for value in values):
                raise ValueError(f"MOXB index exceeds vertex count in cell {index} group {group_id}")
            cell_indices += index_count
            cursor = index_end
        if cursor != size:
            raise ValueError(f"{size - cursor} unparsed bytes in MOXB cell {index}")
        groups += group_count
        indices += cell_indices
        vertices += vertex_count
        group_size_hist[group_count] += 1
        if vertex_count:
            primary_cells += 1
        else:
            empty_cells += 1
        at = end
    if at != len(data):
        raise ValueError(f"{len(data) - at} trailing bytes after MOXB cell records")
    return {"width": width, "height": height, "records": count, "primary_cells": primary_cells,
            "vertices": vertices, "groups": groups, "indices": indices, "empty_cells": empty_cells,
            "group_counts_per_cell": dict(sorted(group_size_hist.items())),
            "group_scale_parameters": dict(sorted(group_scale_hist.items())), "opaque_trailing_bytes": 0}


def main() -> None:
    configs = ROOT / "configs.pck"
    _, config_entries = pck_lib.directory(str(configs))
    autoscene_entry = next((e for e in config_entries if e[0].lower().endswith("/autoscene.cfg")), None)
    if autoscene_entry is None:
        raise ValueError("configs.pck is missing AutoFamily autoscene.cfg")
    auto_bytes = pck_lib.read_entry(str(configs), autoscene_entry)
    auto_ids, auto_values = parse_autoscene(auto_bytes)
    sky_key = auto_ids["list"].lower()
    sky_entry = next((e for e in config_entries if e[0].lower() == sky_key), None)
    if sky_entry is None:
        raise ValueError(f"AutoFamily sky_list resource is missing: {sky_key}")
    sky_sets = parse_sky_list(pck_lib.read_entry(str(configs), sky_entry))
    print(f"configs.pck entries={len(config_entries)}")
    print(f"AutoFamily scene config={autoscene_entry[0]} sky IDs day={auto_ids['day']} night={auto_ids['night']} list={auto_ids['list']}")
    for label, ident in (("day", auto_ids["day"]), ("night", auto_ids["night"])):
        sky_set = sky_sets.get(ident)
        if sky_set is None:
            raise ValueError(f"AutoFamily {label} sky ID {ident} is absent from sky.lst")
        print(f" AutoFamily {label} sky ID {ident} ({sky_set['label']}): {sky_set['roof']}, {sky_set['01']}, {sky_set['02']}")
    fog = auto_values.get("fog", {})
    fog_keys = ("fog_enable", "fog_start_day", "fog_end_day", "fog_color_day", "fog_start_night", "fog_end_night", "fog_color_night", "fog_color_water")
    missing_fog = [key for key in fog_keys if key not in fog]
    if missing_fog:
        raise ValueError(f"AutoFamily config is missing Fog keys: {missing_fog}")
    def fog_color(key: str) -> tuple[int, int, int]:
        values = tuple(int(part.strip()) for part in fog[key].replace(",", " ").split())
        if len(values) != 3 or any(value < 0 or value > 255 for value in values):
            raise ValueError(f"invalid Fog/{key} RGB value: {fog[key]!r}")
        return values
    day_fog = (float(fog["fog_start_day"]), float(fog["fog_end_day"]), fog_color("fog_color_day"))
    night_fog = (float(fog["fog_start_night"]), float(fog["fog_end_night"]), fog_color("fog_color_night"))
    water_color = fog_color("fog_color_water")
    print(f" AutoFamily fog_enable={fog['fog_enable']}; day={day_fog}; night={night_fog}; water_rgb={water_color}")

    if (ROOT / "textures.pck").is_file():
        _, texture_entries = pck_lib.directory(str(ROOT / "textures.pck"))
        texture_names = {e[0].lower() for e in texture_entries}
        for label, ident in (("day", auto_ids["day"]), ("night", auto_ids["night"])):
            sky_set = sky_sets[ident]
            selected = [f"textures/sky/{sky_set[name]}".lower() for name in ("01", "02", "roof")]
            missing = [name for name in selected if name not in texture_names]
            if missing:
                raise ValueError(f"AutoFamily {label} sky textures missing from textures.pck: {missing}")
            print(f" AutoFamily {label} sky resource validation: all 3 referenced textures present")
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
    cld = cloud_surface_summary((ROOT / "world.cld").read_bytes())
    print("world.cld native MOXB payload:", cld)
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
    local = [a for a in areas if not a["global"]]
    local_boxes = {a["id"]: local_obb(bsd, unique[a["id"]]) for a in local}
    local_overlaps = []
    for i, a in enumerate(local):
        for other in local[i + 1 :]:
            if not obbs_overlap(local_boxes[a["id"]], local_boxes[other["id"]]):
                continue
            blocks_a = {tuple(ref["block"]) for ref in refs if ref["id"] == a["id"]}
            blocks_b = {tuple(ref["block"]) for ref in refs if ref["id"] == other["id"]}
            shared = sorted(blocks_a & blocks_b)
            order = next(([ref["id"] for ref in refs if tuple(ref["block"]) == block and ref["id"] in (a["id"], other["id"])] for block in shared), [])
            local_overlaps.append((a["id"], other["id"], len(shared), order))
    global_areas = [a for a in areas if a["global"]]
    global_contacts = sum(
        obbs_overlap(local_obb(bsd, unique[a["id"]]), local_obb(bsd, unique[other["id"]]))
        for i, a in enumerate(global_areas)
        for other in global_areas[i + 1 :]
    )
    print(" intersecting local OBB pairs (ids, shared ECWLD blocks, first shared-block reference order):", local_overlaps)
    print(f" intersecting default/global OBB pairs including touching boundaries: {global_contacts}")
    print(" scene effect valid_time counts:", dict(sorted(collections.Counter(e["time"] for e in effects).items())))
    by_path = collections.Counter(e["path"] for e in effects)
    print(" unique placed effect paths:", len(by_path))
    atmospheric = [(name, count) for name, count in by_path.items() if any(k in name.lower() for k in ("rain", "snow", "cloud", "fog", "wind", "weather"))]
    print(" atmospheric-named placed effects:", sorted(atmospheric, key=lambda item: (-item[1], item[0])))
    # The surveyed world's weather-like effect roots are named in GBK/Chinese,
    # so an ASCII keyword scan alone silently misses real placements.
    authored_atmosphere = {
        "冰雾": "ice mist",
        "树叶雨": "leaf rain",
        "花瓣雨": "petal rain",
        "沙尘": "dust",
        "彩虹": "rainbow",
    }
    for token, label in authored_atmosphere.items():
        matched = sorted(name for name in by_path if token in name)
        if not matched:
            continue
        print(f" {label} placed-effect paths:")
        for name in matched:
            times = collections.Counter(e["time"] for e in effects if e["path"] == name)
            print(f"  {name}: total={by_path[name]} valid_time={dict(sorted(times.items()))}")
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
