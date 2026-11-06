#!/usr/bin/env python3
"""Evidence table for what the shipped 1.4.2 client data gives a scene NPC.

Reads the retail archives directly - no counters, no guesses:

  * ``data/elements.data``  v27, section 58 ``NPC_ESSENCE`` (852-byte records:
    id @ 0, wide name @ 4, ``.ecm`` path @ 80, greeting @ 216).
  * ``data/task_npc.data``  placement records (u16 count @ 10, then
    12-byte records ``id, x, z, height, task``). Records with ``x == z == 0``
    are template rows, not placements, and are counted separately.
  * ``models.pck``          the rig itself: the ``.ecm`` combined-action table,
    the ``.smd`` ordinary actions and the ``.bon`` skeleton.

Section sizes are read out of ``src/modules/designer/catalog.rs`` (the
``SECTION_SIZES`` table) so this tool cannot drift from the viewer's own
parser: if the Rust table changes, the offsets here follow it.

For every distinct rig the report resolves the idle the Populator would play,
under both the requested-name policies, and records the clip range and frame
rate that decides whether the actor can move at all:

    python3 tools/npc_idle_report.py --models google_drive/models.pck \
        --elements google_drive/elements.data --task-npc google_drive/task_npc.data \
        --out Findings/npc-idle-evidence.md
"""

from __future__ import annotations

import argparse
import collections
import re
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pck_lib  # noqa: E402

# Names the viewer looks up, in the order it looks them up.
IDLE_NAMES = ("待机", "站立", "休闲", "idle")
# Requested scene-action names, and the literal the client falls back to
# (elementclient.exe 0x95a750) when the requested name is absent.
REQUESTED_NAME = "站立"


# --------------------------------------------------------------------------
# elements.data


def section_sizes(catalog_rs: Path):
    """The viewer's own section table, so record strides cannot drift."""
    text = catalog_rs.read_text(encoding="utf-8")
    body = text.split("const SECTION_SIZES: [Size; 131] = [", 1)[1].split("];", 1)[0]
    sizes = []
    for token in re.finditer(r"F\((\d+)\)|Timestamp|Plain|\bT\b", body):
        sizes.append(("fixed", int(token.group(1))) if token.group(1) else (token.group(0), None))
    if len(sizes) != 131:
        raise SystemExit(f"expected 131 sections, parsed {len(sizes)}")
    return sizes


class Cursor:
    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def i32(self) -> int:
        value = struct.unpack_from("<i", self.data, self.pos)[0]
        self.pos += 4
        return value

    def u32(self) -> int:
        value = struct.unpack_from("<I", self.data, self.pos)[0]
        self.pos += 4
        return value

    def take(self, size: int) -> bytes:
        chunk = self.data[self.pos : self.pos + size]
        if len(chunk) != size:
            raise SystemExit("elements.data is truncated")
        self.pos += size
        return chunk


def _skip_talk(cursor: Cursor) -> None:
    cursor.u32()
    cursor.take(128)
    for _ in range(max(0, cursor.i32())):
        cursor.u32()
        cursor.u32()
        cursor.take(cursor.u32() * 2)
        cursor.take(max(0, cursor.i32()) * 136)


def _wide(record: bytes, offset: int, size: int) -> str:
    return record[offset : offset + size].decode("utf-16-le", "replace").split("\x00")[0].strip()


def _gbk(record: bytes, offset: int, size: int) -> str:
    text = record[offset : offset + size].decode("gbk", "replace").split("\x00")[0]
    return "/".join(part for part in text.replace("\\", "/").split("/") if part)


def npc_templates(elements: Path, catalog_rs: Path):
    """id, name, .ecm path for every NPC_ESSENCE record, in file order."""
    data = elements.read_bytes()
    cursor = Cursor(data)
    cursor.u32()  # 0x3000001b for the 1.4.2 v27 table
    cursor.u32()  # build timestamp
    out = []
    for index, (kind, size) in enumerate(section_sizes(catalog_rs)):
        if kind == "fixed":
            count = max(0, cursor.i32())
            if index != 58:
                cursor.take(count * size)
                continue
            for _ in range(count):
                record = cursor.take(size)
                out.append(
                    (
                        struct.unpack_from("<I", record, 0)[0],
                        _wide(record, 4, 64),
                        _gbk(record, 80, 128),
                    )
                )
        elif kind == "T":
            for _ in range(max(0, cursor.i32())):
                _skip_talk(cursor)
        elif kind == "Timestamp":
            cursor.u32()
            cursor.take(cursor.u32())
            cursor.u32()
        elif kind == "Plain":
            cursor.u32()
            cursor.take(cursor.u32())
        else:
            raise SystemExit(f"unknown section kind {kind}")
    if cursor.pos != len(data):
        raise SystemExit(f"elements.data left {len(data) - cursor.pos} bytes unread")
    return out


def placements(task_npc: Path):
    """Placement records: (id, x, z, height, task), placements only."""
    data = task_npc.read_bytes()
    count = struct.unpack_from("<H", data, 10)[0]
    placed, template_rows = [], 0
    for index in range(count):
        npc_id, x, z, height, task = struct.unpack_from("<IhhhH", data, 12 + index * 12)
        if x == 0 and z == 0:
            template_rows += 1
            continue
        placed.append((npc_id, x, z, height, task))
    return placed, template_rows


# --------------------------------------------------------------------------
# models.pck


class Pack:
    def __init__(self, path: Path):
        self.path = str(path)
        _, entries = pck_lib.directory(self.path)
        self.index = {entry[0].lower(): entry for entry in entries}
        self.count = len(entries)

    def read(self, name: str):
        entry = self.index.get(name.lower())
        return None if entry is None else pck_lib.read_entry(self.path, entry)

    def has(self, name: str) -> bool:
        return name.lower() in self.index


def normalize(path: str) -> str:
    return "/".join(part for part in path.replace("\\", "/").split("/") if part).lower()


class Reader:
    def __init__(self, data: bytes):
        self.data = data
        self.pos = 0

    def take(self, size: int) -> bytes:
        chunk = self.data[self.pos : self.pos + size]
        if len(chunk) != size:
            raise ValueError("truncated model data")
        self.pos += size
        return chunk

    def u32(self) -> int:
        return struct.unpack("<I", self.take(4))[0]

    def i32(self) -> int:
        return struct.unpack("<i", self.take(4))[0]

    def f32(self) -> float:
        return struct.unpack("<f", self.take(4))[0]

    def string(self) -> str:
        return self.take(self.u32()).decode("gbk", "replace").rstrip("\x00")


def parse_ecm(data: bytes):
    """Combined-action table and the skin model path (text, GBK).

    Mirrors ``ecmodel::parse_ecm``: after ``BaseActCount`` the following
    ``LoopCount`` belongs to the base action (min = max = that value), which is
    what decides whether ``Playback`` can schedule the combined action.
    """
    combos, current, skin = [], None, None
    in_bases = False
    for raw in data.decode("gbk", "replace").splitlines():
        line = raw.strip()
        if ":" not in line:
            continue
        key, value = (part.strip() for part in line.split(":", 1))
        if key == "SkinModelPath":
            skin = value
        elif key == "CombineActName":
            current = {"name": value, "bases": [], "loops": None, "speed": None}
            combos.append(current)
            in_bases = False
        elif current is None:
            continue
        elif key == "BaseActCount":
            in_bases = True
        elif key == "BaseActName":
            current["bases"].append({"name": value, "start_ms": None, "min": None, "max": None})
        elif key == "LoopCount":
            if in_bases and current["bases"]:
                current["bases"][-1]["min"] = current["bases"][-1]["max"] = int(value)
            else:
                current["loops"] = int(value)
        elif key == "PlaySpeed":
            current["speed"] = float(value)
        elif key in ("ActStartTime", "LoopMinNum", "LoopMaxNum") and in_bases and current["bases"]:
            base = current["bases"][-1]
            number = int(value)
            if key == "ActStartTime":
                base["start_ms"] = number
            elif key == "LoopMinNum":
                base["min"] = number
            else:
                base["max"] = number
        elif key == "ScriptCount":
            current = None
            in_bases = False
    return combos, skin


def parse_smd(data: bytes):
    data = data[len(b"MOXB") :] if data.startswith(b"MOXB") else data
    reader = Reader(data)
    if reader.take(4) != b"DMSA":
        raise ValueError("invalid SMD signature")
    version = reader.u32()
    skins, actions_count, hangers, props = reader.i32(), reader.i32(), reader.i32(), reader.i32()
    reader.take(60)
    skeleton = reader.string()
    for _ in range(max(0, skins)):
        reader.string()
    reader.string()
    track_dir = reader.string() if version >= 8 else ""
    actions = []
    for _ in range(max(0, actions_count)):
        action = {"name": reader.string(), "start": 0.0, "end": 0.0, "fps": None, "track": ""}
        if version < 6:
            reader.i32()
            joints = max(0, reader.i32())
            reader.i32()
            if joints:
                # Action.start/end come from the first joint, as in ecmodel::parse_smd.
                reader.i32()  # joint index
                action["start"] = reader.f32()
                action["end"] = reader.f32()
                action["loops"] = reader.i32()
                reader.f32()
                reader.take((joints - 1) * 20)
        else:
            action["start"] = reader.f32()
            action["end"] = reader.f32()
            if version >= 9:
                action["fps"] = reader.i32()
            if version >= 7:
                action["track"] = reader.string()
        actions.append(action)
    return {"version": version, "skeleton": skeleton, "track_dir": track_dir, "actions": actions}


def _key_track(reader: Reader, version: int, quaternion: bool, keep: bool = False):
    """One ``KeyTrack``; returns its key count plus the keys when asked."""
    count = max(0, reader.i32())
    segments = max(0, reader.i32())
    fps = reader.i32()
    packed = reader.i32() if (version >= 2 and quaternion) else 0
    components = 3 if packed == 1 else (4 if quaternion else 3)
    keys = [] if keep else None
    for _ in range(count * components):
        value = reader.f32()
        if keys is not None:
            keys.append(value)
    if version == 1:
        reader.take(segments * 16)
    else:
        reader.i32()
        reader.i32()
    if keys is not None:
        keys = [keys[i : i + components] for i in range(0, len(keys), components)]
    return count, fps, keys


def parse_bon(data: bytes):
    """Skeleton header plus, for v5 skeletons, the embedded track key counts.

    v5 ``.bon`` files carry the bone tracks inline; v6+ skeletons read them from
    the per-action ``.stck`` the SMD names. An idle clip can only move if the
    tracks it samples exist, so both numbers are reported.
    """
    data = data[len(b"MOXB") :] if data.startswith(b"MOXB") else data
    reader = Reader(data)
    if reader.take(4) != b"EKSA":
        raise ValueError("invalid BON signature")
    version = reader.u32()
    bones, joints, hooks = reader.i32(), reader.i32(), reader.i32()
    start, end, fps = reader.i32(), reader.i32(), reader.i32()
    reader.take(64)
    for _ in range(max(0, bones)):
        reader.string()
        reader.take(4)
        reader.i32()
        reader.i32()
        children = max(0, reader.i32())
        reader.take(32 * 4)
        reader.take(children * 4)
    for _ in range(max(0, joints)):
        reader.u32()
    rotation_keys, widest, tracks = [], None, []
    for _ in range(max(0, joints)):
        reader.string()
        reader.i32()
        reader.i32()
        reader.i32()
        if version < 6:
            _key_track(reader, 1, False)
            count, track_fps, keys = _key_track(reader, 1, True, keep=True)
            rotation_keys.append(count)
            tracks.append({"count": count, "fps": track_fps, "keys": keys})
            if widest is None or count > widest["count"]:
                widest = tracks[-1]
    return {
        "version": version,
        "fps": fps,
        "start": start,
        "end": end,
        "joints": max(0, joints),
        "rotation_keys": rotation_keys,
        "widest_track": widest,
        "tracks": tracks,
        "hooks": max(0, hooks),
    }


# --------------------------------------------------------------------------
# idle resolution, mirroring src/modules/populator/mod.rs


def scene_action(combos, requested):
    """Client 0x5b3a4f: combined action by requested name, else literal 休闲."""
    for combo in combos:
        if combo["name"] == requested:
            return combo
    for combo in combos:
        if combo["name"] == "休闲":
            return combo
    return None


def playback_ok(combo) -> str:
    """Why Playback::new would or would not schedule this combined action."""
    if combo is None:
        return "no combined action"
    if len(combo["bases"]) != 1 or (combo["bases"][0]["start_ms"] or 0) != 0:
        return "multi-base/delayed"
    if combo["loops"] is not None and combo["loops"] not in (1, -1):
        return "finite repeated loop count"
    base = combo["bases"][0]
    low, high = base["min"], base["max"]
    if low is None or high is None:
        return "no loop bounds"
    if low == -1 and high == -1:
        return "ok"
    if low == high and low > 0:
        return "ok"
    return "random/zero loop policy"


def clip_ms(action, skeleton_fps):
    fps = action["fps"] if action["fps"] and action["fps"] > 0 else skeleton_fps
    if not fps or fps <= 0 or action["end"] < action["start"] or action["start"] < 0:
        return None
    start = int(action["start"] * 1000.0 / fps)
    end = int(action["end"] * 1000.0 / fps)
    return start, end, end - start + 1


def resolve(combos, actions, skeleton_fps, requested):
    """(mode, action name, note) under one requested-name policy."""
    combo = scene_action(combos, requested)
    verdict = playback_ok(combo)
    if verdict == "ok":
        base = combo["bases"][0]["name"]
        if base not in actions:
            return "bind pose", None, f"combined {combo['name']} names missing base {base}"
        return "playback", base, f"combined {combo['name']} loops={combo['loops']}"
    note = f"combined {combo['name']}: {verdict}" if combo else verdict
    if combo and combo["bases"]:
        base = combo["bases"][0]["name"]
        if base in actions and clip_ms(actions[base], skeleton_fps):
            return "module clock", base, note
    for name in IDLE_NAMES:
        for action in actions.values():
            if action["name"].lower() == name.lower():
                if clip_ms(action, skeleton_fps):
                    return "module clock", action["name"], note + "; ordinary clip by name"
                return "bind pose", None, note + f"; ordinary {action['name']} has no sampleable range"
    return "bind pose", None, note + "; no idle clip at all"


def clip_motion(tracks, action, skeleton_fps):
    """How far any rotation track moves across the clip, in quaternion units.

    A clip that spans more than a millisecond can still be a held pose if every
    key holds the same value, so the keys themselves are sampled at three points
    of the clip's own frame range instead of trusting the range alone. Every
    track is checked: most of a skeleton is still while one limb carries the
    idle, so the widest single track is not the same as "nothing moves".
    """
    if not tracks or not action or not skeleton_fps:
        return None
    widest = 0.0
    seen = False
    for track in tracks:
        keys = track["keys"]
        if not keys:
            continue
        last = len(keys) - 1
        samples = []
        for frame in (action["start"], (action["start"] + action["end"]) / 2.0, action["end"]):
            seconds = max(0.0, frame / skeleton_fps)
            index = min(last, max(0, int(seconds * track["fps"])))
            samples.append(keys[index])
        for left in samples:
            for right in samples:
                widest = max(widest, max(abs(a - b) for a, b in zip(left, right)))
        seen = True
    return widest if seen else None


def main() -> int:
    root = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--models", default="google_drive/models.pck")
    parser.add_argument("--elements", default="google_drive/elements.data")
    parser.add_argument("--task-npc", default="google_drive/task_npc.data")
    parser.add_argument("--catalog", default="src/modules/designer/catalog.rs")
    parser.add_argument("--out", default="Findings/npc-idle-evidence.md")
    args = parser.parse_args()

    pack = Pack(root / args.models)
    templates = npc_templates(root / args.elements, root / args.catalog)
    by_id = {row[0]: row for row in templates}
    placed, template_rows = placements(root / args.task_npc)

    rigs = collections.defaultdict(list)
    for npc_id, *_ in placed:
        template = by_id.get(npc_id)
        if template is None:
            continue
        rigs[normalize(template[2])].append((npc_id, template[1]))

    rows, missing_rig, missing_smd = [], 0, 0
    for rig in sorted(rigs):
        data = pack.read(rig)
        if data is None:
            missing_rig += 1
            continue
        combos, skin = parse_ecm(data)
        smd_data = pack.read(normalize(skin)) if skin else None
        if smd_data is None:
            missing_smd += 1
            continue
        smd = parse_smd(smd_data)
        bon_path = normalize(rig.rsplit("/", 1)[0] + "/" + smd["skeleton"])
        bon_data = pack.read(bon_path)
        bon = None
        try:
            bon = parse_bon(bon_data) if bon_data else None
        except Exception as exc:  # a rig whose skeleton cannot be read cannot move
            bon = None
            bon_error = str(exc)
        else:
            bon_error = ""
        actions = {action["name"]: action for action in smd["actions"]}
        skeleton_fps = bon["fps"] if bon else 0
        new_mode, new_action, new_note = resolve(combos, actions, skeleton_fps, REQUESTED_NAME)
        old_mode, old_action, old_note = resolve(combos, actions, skeleton_fps, "休闲")
        action = actions.get(new_action) if new_action else None
        span = clip_ms(action, skeleton_fps) if action else None
        rotation_keys = max(bon["rotation_keys"], default=0) if bon else 0
        motion = clip_motion(bon["tracks"] if bon else None, action, skeleton_fps)
        rows.append(
            {
                "rig": rig,
                "npcs": rigs[rig],
                "smd_version": smd["version"],
                "bon_version": bon["version"] if bon else None,
                "bon_fps": skeleton_fps,
                "mode": new_mode,
                "action": new_action,
                "note": new_note,
                "old_mode": old_mode,
                "old_action": old_action,
                "old_note": old_note,
                "ms": span[2] if span else None,
                "range": f"{action['start']:g}..{action['end']:g}" if action else "",
                "keys": rotation_keys,
                "motion": motion,
                "bon_error": bon_error,
            }
        )

    moves = [row for row in rows if row["mode"] in ("playback", "module clock") and (row["ms"] or 0) > 1]
    moving = [row for row in moves if (row["motion"] or 0.0) > 1e-3]
    changed = [row for row in rows if (row["mode"], row["action"]) != (row["old_mode"], row["old_action"])]
    modes = collections.Counter(row["mode"] for row in rows)
    policies = collections.Counter(row["note"].split(";")[0] for row in rows)
    still = [row for row in rows if row not in moves]

    lines = [
        "# What the shipped 1.4.2 data gives a scene NPC",
        "",
        "Generated by `tools/npc_idle_report.py` from the retail archives; regenerate it",
        "instead of editing the numbers by hand.",
        "",
        "## Inputs",
        "",
        f"* `{args.models}` - {pack.count:,} entries.",
        f"* `{args.elements}` - section 58 `NPC_ESSENCE`, {len(templates):,} templates.",
        f"* `{args.task_npc}` - {len(placed):,} placement records plus {template_rows:,} "
        "template rows with no position.",
        "",
        "## Coverage",
        "",
        f"* {len(placed):,} placed NPCs use **{len(rows):,} distinct rigs**.",
        f"* {len(moves):,} of those rigs resolve to an idle clip that spans more than one "
        "millisecond, i.e. one the pose sampler can actually move through.",
        f"* {len(moving):,} of those {len(moves):,} also have a rotation track that changes "
        "value across the clip, so the movement is in the keys and not only in the range."
        + (
            " The rest keep their authored pose: "
            + ", ".join(
                f"`{row['rig']}` ({row['range']} over a {row['keys']}-key track)"
                for row in moves
                if row not in moving
            )
            + " - the clip maps past the end of the track, and the sampler clamps at the"
            " last key exactly as `pose_action_at_time` does."
            if len(moving) < len(moves)
            else ""
        ),
        f"* {len(still):,} keep a still pose: "
        + ", ".join(f"{count:,} {mode}" for mode, count in sorted(modes.items()) if mode == "bind pose")
        + ("." if modes.get("bind pose") else "none."),
        "",
        f"Idle request `{REQUESTED_NAME}` (the standing scene action, with the client's literal",
        "`休闲` fallback at elementclient.exe 0x95a750):",
        "",
    ]
    for note, count in policies.most_common():
        lines.append(f"* {count:4d} {note}")
    lines += [
        "",
        f"Requesting `休闲` instead - what the viewer did before - picks a different action for",
        f"{len(changed):,} rigs; the leisure gesture loops forever instead of the standing loop.",
        "",
        "## Per-rig evidence",
        "",
        "| rig | npcs | smd | bon | fps | idle action | frames | ms | keys | motion | how |",
        "| --- | ---: | ---: | ---: | ---: | --- | --- | ---: | ---: | ---: | --- |",
    ]
    for row in rows:
        sample = ", ".join(f"{npc_id} {name}" for npc_id, name in row["npcs"][:2])
        if len(row["npcs"]) > 2:
            sample += f", +{len(row['npcs']) - 2}"
        lines.append(
            "| `{rig}` | {count} | v{smd} | v{bon} | {fps} | {action} | {range} | {ms} | {keys} | {motion} | {note} |".format(
                rig=row["rig"],
                count=f"{len(row['npcs'])} ({sample})",
                smd=row["smd_version"],
                bon=row["bon_version"],
                fps=row["bon_fps"],
                action=row["action"] or "-",
                range=row["range"] or "-",
                ms=row["ms"] if row["ms"] is not None else "-",
                keys=row["keys"],
                motion=f"{row['motion']:.3f}" if row["motion"] is not None else "-",
                note=row["note"],
            )
        )
    if missing_rig or missing_smd:
        lines += ["", f"Unread rigs: {missing_rig} missing `.ecm`, {missing_smd} missing `.smd`."]
    out = root / args.out
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(f"wrote {out} ({len(rows)} rigs, {len(moves)} animate)")
    for mode, count in sorted(modes.items()):
        print(f"  {count:5d} {mode}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
