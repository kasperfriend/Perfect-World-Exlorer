#!/usr/bin/env python3
"""Live World gait-retarget evidence, straight from the retail archives.

The Populator's Live World prototype borrows locomotion (walk/run) from a
donor skeleton and retargets the rotation tracks onto the numbered
male/female city NPC rigs the residents are drawn with. This tool answers,
from the files themselves, why the generated residents' arms/hands come out
broken and which donors can drive them cleanly:

  * every player body rig (the Walker's rigs) and every Guard rig: combined
    actions, ordinary actions, the clips a ``gait_kind`` walk/run scan
    resolves to, their frame ranges, and their track inventories;
  * every numbered male/female city NPC rig: the rest-matrix facts the
    retargeter depends on (column scale, determinant, orthonormality) for
    every bone, and the per-clip accept/reject decision of the shipped
    ``retarget_gait_clip`` logic, ported line by line;
  * a per-bone map, for a few representative pairs, of the bones the shipped
    algorithm rejects, skips, or rebases with large angular deviation, under
    both candidate rebase formulas (shipped local-frame vs parent-frame).

Nothing here poses a mesh; the numbers are the inputs the Rust code reads.

    python3 tools/gait_report.py --models client/models.pck \\
        --out Findings/recovery/gait-evidence.md
"""

from __future__ import annotations

import argparse
import math
import re
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import pck_lib  # noqa: E402

import npc_idle_report as nig  # noqa: E402  (ECM/SMD/BON primitives)

PLAYER_FOLDERS = [
    # models/players/形象/<folder>/躯干/<folder>.ecm — the Walker's body rigs
    ("武侠男", "m"), ("法师男", "m"), ("巫师男", "m"), ("妖兽男", "m"),
    ("刺客男", "m"), ("羽芒男", "m"), ("羽灵男", "m"),
    ("武侠女", "f"), ("法师女", "f"), ("巫师女", "f"), ("妖精", "f"),
    ("刺客女", "f"), ("羽芒女", "f"), ("羽灵女", "f"),
]
GUARDS = [f"models/npcs/npc/卫兵npc{i}/卫兵npc{i}.ecm" for i in range(1, 5)]
PROP_RIGS = {
    "models/npcs/npc/男npc5/男npc5.ecm",
    "models/npcs/npc/妖兽npc4/妖兽npc4.ecm",
}

# Mirrors the shipped retarget fix (src/modules/populator/mod.rs):
# acceptance is full coverage of the locomotion bones, and a bone whose rest
# frames diverge beyond MAX_GAP_DEG keeps its bind pose instead of being
# re-oriented onto unrelated axes.
CORE_BONES = (
    "Bip01", "Bip01 Pelvis", "Bip01 Spine", "Bip01 Spine1", "Bip01 Spine2",
    "Bip01 Neck", "Bip01 Head",
    "Bip01 L Clavicle", "Bip01 L UpperArm", "Bip01 L Forearm", "Bip01 L Hand",
    "Bip01 R Clavicle", "Bip01 R UpperArm", "Bip01 R Forearm", "Bip01 R Hand",
    "Bip01 L Thigh", "Bip01 L Calf", "Bip01 L Foot", "Bip01 L Toe0",
    "Bip01 R Thigh", "Bip01 R Calf", "Bip01 R Foot", "Bip01 R Toe0",
)
CORE_SET = frozenset(CORE_BONES)
MAX_GAP_DEG = 45.0


# --------------------------------------------------------------------------
# matrices (4x4 as row-major lists) and quaternions (x, y, z, w)


def mat_from_cols(values):
    """glam::Mat4::from_cols_array: column-major storage -> row-major math."""
    m = [[0.0] * 4 for _ in range(4)]
    for c in range(4):
        for r in range(4):
            m[r][c] = values[c * 4 + r]
    return m


def mat3_det(m):
    (a, b, c), (d, e, f), (g, h, i) = (row[:3] for row in m[:3])
    return a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)


def columns(m):
    return [[m[0][c], m[1][c], m[2][c]] for c in range(3)]


def col_len(col):
    return math.sqrt(sum(x * x for x in col))


def normalized_cols(m):
    cols = columns(m)
    scales = [col_len(c) for c in cols]
    unit = [
        [x / s if s > 0.0 else 0.0 for x in c] for c, s in zip(cols, scales)
    ]
    return unit, scales


def orthonormality_error(unit):
    """max |N^T N - I| for the column-orthonormalized basis."""
    worst = 0.0
    for i in range(3):
        for j in range(3):
            dot = sum(unit[k][i] * unit[k][j] for k in range(3))
            worst = max(worst, abs(dot - (1.0 if i == j else 0.0)))
    return worst


def unit_cols_to_mat(unit):
    return [
        [unit[0][0], unit[1][0], unit[2][0]],
        [unit[0][1], unit[1][1], unit[2][1]],
        [unit[0][2], unit[1][2], unit[2][2]],
    ]


def mat3_mul(a, b):
    return [
        [sum(a[r][k] * b[k][c] for k in range(3)) for c in range(3)]
        for r in range(3)
    ]


def trace3(a):
    return a[0][0] + a[1][1] + a[2][2]


def quat_to_mat3(q):
    x, y, z, w = q
    xx, yy, zz = x * x, y * y, z * z
    xy, xz, yz = x * y, x * z, y * z
    wx, wy, wz = w * x, w * y, w * z
    return [
        [1 - 2 * (yy + zz), 2 * (xy - wz), 2 * (xz + wy)],
        [2 * (xy + wz), 1 - 2 * (xx + zz), 2 * (yz - wx)],
        [2 * (xz - wy), 2 * (yz + wx), 1 - 2 * (xx + yy)],
    ]


def quat_norm(q):
    n = math.sqrt(sum(v * v for v in q))
    return [v / n for v in q] if n else [0.0, 0.0, 0.0, 1.0]


def angle_between_rotation_mats(a, b):
    c = trace3(mat3_mul(_t3(a), b))
    return math.degrees(math.acos(max(-1.0, min(1.0, (c - 1.0) / 2.0))))


def _t3(a):
    return [[a[c][r] for c in range(3)] for r in range(3)]


# --------------------------------------------------------------------------
# BON / STCK parsing (layouts per src/skeleton.rs, src/animation.rs;
# ECM/SMD primitives shared with npc_idle_report.py)


def parse_bon_full(data):
    data = data[len(b"MOXB"):] if data.startswith(b"MOXB") else data
    r = nig.Reader(data)
    if r.take(4) != b"EKSA":
        raise ValueError("invalid BON signature")
    version = r.u32()
    n_bones, n_joints, n_hooks = r.i32(), r.i32(), r.i32()
    start, end, fps = r.i32(), r.i32(), r.i32()
    r.take(64)
    bones = []
    for _ in range(max(0, n_bones)):
        name = r.string()
        flags = r.take(4)[0]
        parent = r.i32()
        first_joint = r.i32()
        children = max(0, r.i32())
        relative = [r.f32() for _ in range(16)]
        initial = [r.f32() for _ in range(16)]
        child_ids = [r.i32() for _ in range(children)]
        bones.append(
            {
                "name": name, "flags": flags, "parent": parent,
                "first_joint": first_joint, "relative": relative,
                "initial": initial, "children": child_ids,
            }
        )
    classes = [r.u32() for _ in range(max(0, n_joints))]
    joints = []
    embedded = []
    for index, klass in enumerate(classes):
        joints.append(
            {
                "class": klass,
                "name": r.string(),
                "parent": r.i32(),
                "child": r.i32(),
                "sibling": r.i32(),
            }
        )
        if version < 6:
            position = _key_track(r, 1, False)
            rotation = _key_track(r, 1, True)
            embedded.append({"id": index, "position": position, "rotation": rotation})
    hooks = []
    for _ in range(max(0, n_hooks)):
        name = r.string()
        kind = r.u32()
        bone = r.i32()
        [r.f32() for _ in range(16)]
        hooks.append({"name": name, "kind": kind, "bone": bone})
    return {
        "version": version, "fps": fps, "start": start, "end": end,
        "bones": bones, "joints": joints, "embedded": embedded, "hooks": hooks,
    }


def _key_track(r, version, quaternion):
    count = max(0, r.i32())
    segments = max(0, r.i32())
    fps = r.i32()
    if version >= 2 and quaternion:
        packed = r.i32()
        if packed not in (0, 1):
            raise ValueError(f"unsupported quaternion format {packed}")
    else:
        packed = 0
    components = 3 if packed == 1 else (4 if quaternion else 3)
    keys = []
    for _ in range(count):
        a = [r.f32() for _ in range(components)]
        if packed == 1:
            w = max(0.0, 1.0 - sum(v * v for v in a)) ** 0.5
            a = [a[0], a[1], a[2], w]
        keys.append(a)
    if version == 1:
        ordered, f = [], []

        def add(idx, frame):
            if (not f) or frame > f[-1]:
                ordered.append(idx)
                f.append(frame)

        for _ in range(segments):
            s_start, s_end = r.i32(), r.i32()
            first, last = r.i32(), r.i32()
            if s_start < 0 or s_end < s_start or first < 0 or last < first or first >= count:
                raise ValueError("invalid track segment")
            sf = s_start * fps * 0.001
            ef = s_end * fps * 0.001
            if first == last:
                add(first, sf)
                tail = math.ceil(ef) - 1.0
                if tail > sf:
                    add(first, tail)
            else:
                for k in range(first, min(last, count - 1) + 1):
                    add(k, sf + (k - first))
        frames = f if ordered else [float(i) for i in range(count)]
        if ordered:
            keys = [keys[i] for i in ordered]
    else:
        length = r.i32()
        compression = r.i32()
        if length < 0 or compression not in (0, 1, 2):
            raise ValueError("invalid track compression")
        if compression == 0:
            frames = [float(i) for i in range(count)]
        else:
            frames = [float(int.from_bytes(r.take(2), "little")) for _ in range(count)]
    return {"fps": fps, "keys": keys, "frames": frames, "length_ms": length if version >= 2 else 0}


def parse_stck(data):
    data = data[len(b"MOXB"):] if data.startswith(b"MOXB") else data
    r = nig.Reader(data)
    if r.take(4) != b"KCTS":
        raise ValueError("invalid STCK signature")
    version = r.u32()
    n = r.i32()
    start, end, fps = r.i32(), r.i32(), r.i32()
    tracks = []
    for _ in range(max(0, n)):
        track_id = r.i32()
        position = _key_track(r, version, False)
        rotation = _key_track(r, version, True)
        tracks.append({"id": track_id, "position": position, "rotation": rotation})
    return {"version": version, "fps": fps, "start": start, "end": end, "tracks": tracks}


# --------------------------------------------------------------------------
# ECM/SMD resolution, mirroring the viewer (src/model_asset.rs, src/ecmodel.rs,
# src/model_playback.rs)


def relative(descriptor, reference):
    reference = nig.normalize(reference)
    if reference.startswith("models/"):
        return reference
    parent = descriptor.rsplit("/", 1)[0] if "/" in descriptor else ""
    return nig.normalize(f"{parent}/{reference}")


def action_track_path(ecm_descriptor_path, track_directory, action):
    if not action.get("track"):
        return None
    descriptor = nig.normalize(ecm_descriptor_path)
    parent = descriptor.rsplit("/", 1)[0] if "/" in descriptor else ""
    return nig.normalize(f"{parent}/{track_directory}/{action['track']}")


def load_asset(pack, ecm_path):
    ecm_path = nig.normalize(ecm_path)
    ecm_bytes = pack.read(ecm_path)
    if ecm_bytes is None:
        return None, f"missing {ecm_path}"
    combos, skin_model = nig.parse_ecm(ecm_bytes)
    smd_path = nig.normalize(skin_model)
    if not smd_path.startswith("models/"):
        smd_path = relative(ecm_path, skin_model)
    smd_bytes = pack.read(smd_path)
    if smd_bytes is None:
        return None, f"missing {smd_path}"
    model = nig.parse_smd(smd_bytes)
    skeleton_path = relative(smd_path, model["skeleton"])
    bon_bytes = pack.read(skeleton_path)
    if bon_bytes is None:
        return None, f"missing {skeleton_path}"
    skeleton = parse_bon_full(bon_bytes)
    tracks = []
    for action in model["actions"]:
        # Lazy: the retargeter only ever touches locomotion clips, so parsing
        # every .stck in a player SMD would waste hundreds of megabytes.
        if gait_kind(action["name"]) is None:
            tracks.append(None)
            continue
        spath = action_track_path(ecm_path, model["track_dir"], action)
        if spath is None:
            tracks.append(None)
            continue
        sbytes = pack.read(spath)
        if sbytes is None:
            tracks.append({"error": f"missing {spath}"})
            continue
        try:
            tracks.append(parse_stck(sbytes))
        except Exception as error:  # report wants facts, not exceptions
            tracks.append({"error": f"{spath}: {error}"})
    return {
        "ecm": ecm_path, "smd": smd_path, "bon": skeleton_path,
        "combos": combos, "model": model, "skeleton": skeleton,
        "tracks": tracks,
    }, None


def gait_kind(name):
    lower = name.lower()
    positive = ["walk", "run", "move", "走", "行走", "奔跑", "跑", "移动"]
    forbidden = [
        "attack", "fight", "skill", "idle", "work", "hurt", "dead", "jump",
        "swim", "攻击", "战斗", "技能", "休闲", "待机", "站立", "打铁", "受击",
        "死亡", "跳跃", "游泳", "采集", "施法",
    ]
    if not any(p in lower for p in positive):
        return None
    if any(p in lower for p in forbidden):
        return None
    if any(p in lower for p in ["run", "奔跑", "跑"]):
        return "run"
    if any(p in lower for p in ["walk", "走", "行走", "move", "移动"]):
        return "walk"
    return None


def clip_ms(action, skeleton_fps):
    fps = action["fps"] if action.get("fps") and action["fps"] > 0 else skeleton_fps
    if not fps or fps <= 0 or action["end"] < action["start"] or action["start"] < 0:
        return None
    start = int(action["start"] * 1000.0 / fps)  # Rust: f32::trunc
    end = int(action["end"] * 1000.0 / fps)
    if end >= 2**32 - 1:
        return None
    return start, end, end - start + 1


# --------------------------------------------------------------------------
# the retargeter, ported from src/modules/populator/mod.rs::retarget_gait_clip


def bone_report(bone):
    m = mat_from_cols(bone["relative"])
    cols = columns(m)
    scales = [col_len(c) for c in cols]
    det = mat3_det([row[:3] for row in m[:3]])
    unit, _ = normalized_cols(m)
    return {
        "scale": scales,
        "det": det,
        "ortho": orthonormality_error(unit),
        "unit": unit,
    }


def bone_parent_name(asset, parent):
    if 0 <= parent < len(asset["skeleton"]["bones"]):
        return asset["skeleton"]["bones"][parent]["name"]
    return None


def _abs_rest_rotations(bones):
    """Rotation-only absolute rest orientation per bone (row-major mat3s)."""
    units = [unit_cols_to_mat(normalized_cols(mat_from_cols(b["relative"]))[0]) for b in bones]
    abs_rot = [None] * len(bones)

    def resolve(i):
        if abs_rot[i] is None:
            parent = bones[i]["parent"]
            if 0 <= parent < len(bones):
                abs_rot[i] = mat3_mul(resolve(parent), units[i])
            else:
                abs_rot[i] = [row[:] for row in units[i]]
        return abs_rot[i]

    for i in range(len(bones)):
        resolve(i)
    return abs_rot


def _chain_divergence(src_abs, tgt_abs, sbone, tbone):
    """Angle between the two skeletons' rest PARENT frames for this bone.

    This is the frame the retargeted delta actually lands in once the engine
    composes absolute = parent * local, so it is the quantity that decides
    whether the donor's swing axis still reads as a forward swing.
    """
    sp, tp = sbone["parent"], tbone["parent"]
    if not (0 <= sp < len(src_abs)) or not (0 <= tp < len(tgt_abs)):
        return 0.0
    if mat3_det(src_abs[sp]) <= 0.0 or mat3_det(tgt_abs[tp]) <= 0.0:
        return 180.0
    return angle_between_rotation_mats(src_abs[sp], tgt_abs[tp])


def retarget_clip(source, target, kind, gate="legacy"):
    t_joints = {}
    for i, j in enumerate(target["skeleton"]["joints"]):
        t_joints.setdefault(j["name"], []).append(i)

    report = {"kind": kind, "actions": [], "accepted": None}
    for action_index, action in enumerate(source["model"]["actions"]):
        if gait_kind(action["name"]) != kind:
            continue
        entry = {"name": action["name"], "index": action_index}
        clip = clip_ms(action, source["skeleton"]["fps"])
        entry["clip_ms"] = clip
        if clip is None:
            entry["verdict"] = "unsampleable clip"
            report["actions"].append(entry)
            continue
        track_set = source["tracks"][action_index]
        if track_set is None:
            embedded = source["skeleton"]["embedded"]
            entry["source_tracks"] = len(embedded)
            track_set = {"tracks": embedded} if embedded else None
        elif "error" in track_set:
            entry["verdict"] = f"no tracks ({track_set['error']})"
            report["actions"].append(entry)
            continue
        if track_set is None:
            entry["verdict"] = "no tracks"
            report["actions"].append(entry)
            continue
        source_tracks = track_set["tracks"]
        entry["source_tracks"] = len(source_tracks)
        try:
            entry["motion_deg"] = round(clip_motion_deg(track_set, clip), 2)
        except Exception:
            entry["motion_deg"] = None
        if not source_tracks:
            entry["verdict"] = "empty track set"
            report["actions"].append(entry)
            continue

        mapped, seen, skipped = [], set(), []
        aborted = None
        if gate == "core":
            src_abs = _abs_rest_rotations(source["skeleton"]["bones"])
            tgt_abs = _abs_rest_rotations(target["skeleton"]["bones"])
        for track in source_tracks:
            sid = track["id"]
            sjoint = source["skeleton"]["joints"][sid] if 0 <= sid < len(source["skeleton"]["joints"]) else None
            if sjoint is None:
                skipped.append((f"joint#{sid}", "invalid source joint"))
                continue
            cand = t_joints.get(sjoint["name"])
            if not cand:
                skipped.append((sjoint["name"], "no target joint by name"))
                continue
            tgt_joint_index = cand[0]
            tjoint = target["skeleton"]["joints"][tgt_joint_index]
            if tjoint["class"] != sjoint["class"]:
                skipped.append((sjoint["name"], "joint class mismatch"))
                continue
            sb_idx = sjoint["child"]
            tb_idx = tjoint["child"]
            sbone = source["skeleton"]["bones"][sb_idx] if 0 <= sb_idx < len(source["skeleton"]["bones"]) else None
            tbone = target["skeleton"]["bones"][tb_idx] if 0 <= tb_idx < len(target["skeleton"]["bones"]) else None
            if sbone is None or tbone is None:
                skipped.append((sjoint["name"], "invalid child bone"))
                continue
            if sbone["name"] != tbone["name"]:
                skipped.append((sbone["name"], "bone name mismatch"))
                continue
            if bone_parent_name(source, sbone["parent"]) != bone_parent_name(target, tbone["parent"]):
                skipped.append((sbone["name"], "bone parent mismatch"))
                continue
            if tgt_joint_index in seen:
                aborted = "duplicate target joint mapping"
                break
            seen.add(tgt_joint_index)
            sb, tb = bone_report(sbone), bone_report(tbone)
            if any(abs(s - 1.0) > 0.02 for s in sb["scale"]) or any(
                abs(s - 1.0) > 0.02 for s in tb["scale"]
            ):
                aborted = f"non-unit bone scale on {sbone['name']} (clip rejected)"
                break
            s_unit = unit_cols_to_mat(sb["unit"])
            t_unit = unit_cols_to_mat(tb["unit"])
            rest_gap = (
                angle_between_rotation_mats(s_unit, t_unit)
                if mat3_det(s_unit) > 0.0 and mat3_det(t_unit) > 0.0
                else 999.0
            )
            if gate == "core":
                chain_gap = _chain_divergence(src_abs, tgt_abs, sbone, tbone)
                if chain_gap > MAX_GAP_DEG:
                    skipped.append(
                        (sbone["name"], f"parent-frame divergence {chain_gap:.1f} deg > {MAX_GAP_DEG:.0f}")
                    )
                    continue
            else:
                chain_gap = None
            mapped.append(
                {"joint": sjoint["name"], "bone": sbone["name"],
                 "src": sb, "tgt": tb,
                 "src_flags": sbone["flags"], "tgt_flags": tbone["flags"],
                 "rest_gap": rest_gap,
                 "chain_gap": chain_gap,
                 "track": track}
            )
        if aborted:
            entry["verdict"] = aborted
            report["actions"].append(entry)
            continue
        mapped_names = {m["bone"] for m in mapped}
        core_gaps = [m["rest_gap"] for m in mapped if m["bone"] in CORE_SET]
        entry["max_core_gap"] = max(core_gaps) if core_gaps else None
        chain_gaps = [m["chain_gap"] for m in mapped if m["bone"] in CORE_SET and m["chain_gap"] is not None]
        entry["max_chain_gap"] = max(chain_gaps) if chain_gaps else None
        if gate == "core":
            core_missing = [c for c in CORE_BONES if c not in mapped_names]
            entry["bones"] = mapped
            entry["mapped"] = len(mapped)
            entry["skipped"] = skipped
            if core_missing:
                entry["verdict"] = "missing locomotion bones: " + ", ".join(core_missing)
                entry["core_missing"] = core_missing
                report["actions"].append(entry)
                continue
            entry["verdict"] = "accepted"
            report["actions"].append(entry)
            report["accepted"] = entry
            break
        need = min(len(source_tracks), 8)
        if len(mapped) < need or len(mapped) * 100 < len(source_tracks) * 70:
            entry["verdict"] = f"insufficient coverage {len(mapped)}/{len(source_tracks)}"
            entry["mapped"] = len(mapped)
            entry["skipped"] = skipped
            entry["bones"] = mapped
            report["actions"].append(entry)
            continue
        entry["verdict"] = "accepted"
        entry["mapped"] = len(mapped)
        entry["skipped"] = skipped
        entry["bones"] = mapped
        report["actions"].append(entry)
        report["accepted"] = entry
        break
    return report


def clip_motion_deg(track_set, clip):
    """Widest rotation-key spread across every track in the clip, in degrees.

    A named clip that never moves would pass the accept/reject path yet still
    draw a frozen resident, so the donor table reports how much rotation the
    clip actually carries.
    """
    widest = 0.0
    for track in track_set.get("tracks", []):
        keys = track["rotation"]["keys"]
        if len(keys) < 2:
            continue
        for i in range(len(keys)):
            for j in (i + 1, len(keys) - 1):
                spread = angle_between_rotation_mats(
                    quat_to_mat3(quat_norm(keys[i])),
                    quat_to_mat3(quat_norm(keys[j])),
                )
                widest = max(widest, spread)
    return widest


def formula_probe(entry):
    """Per mapped bone, the angular deviation the rebase formulas introduce.

    Shipped (local-frame):   Rm = T * S^-1 * Rs
    Alternative (parent-frame): Rm = Rs * S^-1 * T

    Rs is the authored key treated as a proper rotation; S and T are the
    donor/target rest rotations. Residual = angle between Rs and Rm.
    """
    rows = []
    for bone in entry.get("bones", []):
        s_unit = bone["src"]["unit"]
        t_unit = bone["tgt"]["unit"]
        s_mat = unit_cols_to_mat(s_unit)
        t_mat = unit_cols_to_mat(t_unit)
        s_inv = _t3(s_mat)  # orthonormal inverse
        s_proper = mat3_det(s_mat) > 0.0
        t_proper = mat3_det(t_mat) > 0.0
        keys = bone["track"]["rotation"]["keys"]
        row = {
            "bone": bone["bone"],
            "src_flags": bone.get("src_flags"),
            "tgt_flags": bone.get("tgt_flags"),
            "src_det": bone["src"]["det"],
            "tgt_det": bone["tgt"]["det"],
            "src_ortho": bone["src"]["ortho"],
            "tgt_ortho": bone["tgt"]["ortho"],
            "src_scale": bone["src"]["scale"],
            "tgt_scale": bone["tgt"]["scale"],
        }
        if keys:
            picks = [0, len(keys) // 2, len(keys) - 1]
            for tag, index in zip(("first", "mid", "last"), picks):
                r_key = quat_to_mat3(quat_norm(keys[index]))
                rm_b = mat3_mul(mat3_mul(t_mat, s_inv), r_key)
                rm_a = mat3_mul(mat3_mul(r_key, s_inv), t_mat)
                row[f"resB_{tag}"] = (
                    angle_between_rotation_mats(r_key, rm_b)
                    if mat3_det(rm_b) > 0.0
                    else None
                )
                row[f"resA_{tag}"] = (
                    angle_between_rotation_mats(r_key, rm_a)
                    if mat3_det(rm_a) > 0.0
                    else None
                )
        row["rest_gap"] = (
            angle_between_rotation_mats(s_mat, t_mat) if s_proper and t_proper else None
        )
        rows.append(row)
    return rows


# --------------------------------------------------------------------------
# reporting


def rig_row(name, asset):
    sk = asset["skeleton"]
    flagged = sum(1 for b in sk["bones"] if b["flags"] != 0)
    non15 = sum(1 for j in sk["joints"] if j["class"] != 15)
    improper = []
    ortho_bad = []
    scale_bad = 0
    for b in sk["bones"]:
        rep = bone_report(b)
        if abs(rep["det"]) < 0.5:
            improper.append(b["name"])
        if rep["ortho"] > 0.01:
            ortho_bad.append(b["name"])
        scale_bad += sum(1 for s in rep["scale"] if abs(s - 1.0) > 0.02)
    return {
        "flags": flagged, "non15": non15,
        "bones": len(sk["bones"]), "joints": len(sk["joints"]),
        "improper": improper, "ortho_bad": ortho_bad, "scale_bad": scale_bad,
        "version": sk["version"], "fps": sk["fps"],
    }


def write_markdown(pack, names, out_path):
    lines = []
    lines.append("# Live World gait-retarget evidence\n\n")
    lines.append(
        "Retail `models.pck` read directly. Matrices are the ones the viewer's "
        "`Mat4::from_cols_array` sees (raw BON floats), so a `det` below zero means "
        "an improper (reflected) rest basis and `ortho` above zero means the columns "
        "are not orthonormal (shear or scale mixed into rest).\n\n"
    )

    player_paths = [f"models/players/形象/{folder}/躯干/{folder}.ecm" for folder, _ in PLAYER_FOLDERS]
    donor_paths = player_paths + GUARDS

    lines.append("## Donor rigs\n\n")
    donors = {}
    for ecm in donor_paths:
        asset, error = load_asset(pack, ecm)
        donors[ecm] = asset
        if error:
            lines.append(f"### `{ecm}`\n\nload error: {error}\n\n")
            continue
        row = rig_row(ecm, asset)
        lines.append(f"### `{ecm}`\n\n")
        lines.append(
            f"- bon v{row['version']} fps={row['fps']} bones={row['bones']} joints={row['joints']} "
            f"flagged-bones={row['flags']} non-class-15={row['non15']} "
            f"scale≠1={row['scale_bad']} improper≈0={row['improper'] or 'none'} "
            f"non-orthonormal={row['ortho_bad'] or 'none'}\n"
        )
        lines.append(f"- combined actions: {', '.join(c['name'] for c in asset['combos']) or 'none'}\n")
        for kind in ("walk", "run"):
            rep = retarget_clip(asset, asset, kind)
            if not rep["actions"]:
                lines.append(f"- {kind}: no locomotion-named action in the SMD\n")
            for a in rep["actions"]:
                clip = a.get("clip_ms")
                clip_s = f"{clip[0]}..{clip[1]} ms (length {clip[2]})" if clip else "unsampleable"
                motion = a.get("motion_deg")
                motion_s = f" over {motion:.2f}°" if motion is not None else ""
                lines.append(
                    f"- {kind} candidate `{a['name']}` (action #{a['index']}): {clip_s}, "
                    f"tracks={a.get('source_tracks', '?')}{motion_s} verdict={a.get('verdict')}\n"
                )
        lines.append("\n")

    resident_paths = sorted(
        name for name in names
        if re.fullmatch(r"models/npcs/npc/[男女]npc[0-9]+/[男女]npc[0-9]+\.ecm", name)
        and name not in PROP_RIGS
    )
    lines.append(f"## Resident candidates: {len(resident_paths)} rigs\n\n")

    cache = {}

    def load_cached(path):
        if path not in cache:
            cache[path] = load_asset(pack, path)
        return cache[path]

    for ecm in resident_paths:
        asset, error = load_cached(ecm)
        lines.append(f"### `{ecm}`\n\n")
        if error:
            lines.append(f"load error: {error}\n\n")
            continue
        row = rig_row(ecm, asset)
        lines.append(
            f"bon v{row['version']} fps={row['fps']} bones={row['bones']} joints={row['joints']} "
            f"flagged={row['flags']} non-class-15={row['non15']} scale≠1={row['scale_bad']}\n\n"
        )
        lines.append(f"- improper(det≈0) bones: {row['improper'] or 'none'}\n")
        lines.append(f"- non-orthonormal bones: {row['ortho_bad'] or 'none'}\n")
        male = ecm.split("/")[3].startswith("男")
        gender_donors = [
            p for p, (_, g) in zip(player_paths, PLAYER_FOLDERS) if (g == "m") == male
        ]
        for donor in gender_donors + GUARDS:
            d = donors.get(donor)
            tag = donor.split("/")[-2]
            if d is None:
                lines.append(f"- donor `{tag}`: unloadable\n")
                continue
            verdicts = []
            for kind in ("walk", "run"):
                rep = retarget_clip(d, asset, kind, gate="core")
                acc = rep.get("accepted")
                if acc:
                    gap = acc.get("max_chain_gap")
                    gap_s = f"{gap:.1f} deg" if gap is not None else "n/a"
                    verdicts.append(f"{kind}=accepted {acc['mapped']}/{acc['source_tracks']} (max chain gap {gap_s})")
                else:
                    first = next((a for a in rep["actions"]), None)
                    verdicts.append(
                        f"{kind}=rejected ({first['verdict'] if first else 'no candidates'})"
                    )
            lines.append(f"- donor `{tag}` [core gate]: {', '.join(verdicts)}\n")
        lines.append("\n")

    lines.append(
        "## Summary table\n\n"
        "`player *` columns use the shipped core-coverage gate (45 deg parent-frame "
        "divergence bind cutoff) with the gender-correct player donor. `guard *` columns "
        "keep the legacy 70% gate (Guard rigs, the old behaviour) for contrast. "
        "`max chain gap` is the largest parent-frame divergence over mapped locomotion "
        "bones for the same-gender player donor.\n\n"
    )
    lines.append(
        "| resident | bon | bones | flagged | non15 | det≈0 | nonortho | scale≠1 | "
        "player walk | player run | max chain gap° | guard walk | guard run |\n"
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | --- | --- |\n"
    )
    for ecm in resident_paths:
        asset, error = load_cached(ecm)
        if error:
            lines.append(f"| `{ecm}` | load error | | | | | | | | | | |\n")
            continue
        row = rig_row(ecm, asset)
        male = ecm.split("/")[3].startswith("男")
        gender_donors = [
            p for p, (_, g) in zip(player_paths, PLAYER_FOLDERS) if (g == "m") == male
        ]

        def verdict(donor, kind, gate):
            d = donors.get(donor)
            if d is None:
                return "unloadable", None
            rep = retarget_clip(d, asset, kind, gate=gate)
            acc = rep.get("accepted")
            if not acc:
                return "no", None
            return "yes", (acc.get("max_chain_gap") if gate == "core" else None)

        pw, gap_w = next((verdict(d, "walk", "core") for d in gender_donors), ("n/a", None))
        pr, gap_r = next((verdict(d, "run", "core") for d in gender_donors), ("n/a", None))
        gw, _ = next((verdict(d, "walk", "legacy") for d in GUARDS), ("n/a", None))
        gr, _ = next((verdict(d, "run", "legacy") for d in GUARDS), ("n/a", None))
        gap = gap_w if gap_w is not None else gap_r
        gap_s = f"{gap:.1f}" if gap is not None else "—"
        lines.append(
            f"| `{ecm}` | v{row['version']} | {row['bones']} | {row['flags']} | {row['non15']} | "
            f"{len(row['improper'])} | {len(row['ortho_bad'])} | {row['scale_bad']} | "
            f"{pw} | {pr} | {gap_s} | {gw} | {gr} |\n"
        )

    lines.append("\n## Per-bone detail\n\n")
    detail_pairs = [
        ("models/npcs/npc/男npc1/男npc1.ecm", "models/players/形象/武侠男/躯干/武侠男.ecm"),
        ("models/npcs/npc/男npc1/男npc1.ecm", "models/npcs/npc/卫兵npc1/卫兵npc1.ecm"),
        ("models/npcs/npc/女npc1/女npc1.ecm", "models/players/形象/武侠女/躯干/武侠女.ecm"),
        ("models/npcs/npc/女npc1/女npc1.ecm", "models/npcs/npc/卫兵npc1/卫兵npc1.ecm"),
    ]
    for resident, donor in detail_pairs:
        r_asset, r_error = load_cached(resident)
        d_asset = donors.get(donor)
        if r_error or d_asset is None:
            lines.append(f"### `{resident}` <- `{donor}`: unavailable ({r_error or 'donor'})\n\n")
            continue
        for kind in ("walk", "run"):
            rep = retarget_clip(d_asset, r_asset, kind, gate="core")
            lines.append(f"### `{resident}` <- `{donor}` [{kind}]\n\n")
            acc = rep.get("accepted")
            if not acc:
                for a in rep["actions"]:
                    lines.append(f"- candidate `{a['name']}`: {a.get('verdict')}\n")
                    for joint, reason in a.get("skipped") or []:
                        lines.append(f"  - skipped `{joint}`: {reason}\n")
                entry = next((a for a in rep["actions"] if a.get("bones")), None)
                if entry:
                    lines.append("\nmapped (bind-kept bones are absent):\n\n")
                else:
                    lines.append("\n")
                    continue
            else:
                lines.append(f"accepted `{acc['name']}`: mapped {acc['mapped']}/{acc['source_tracks']} tracks, max chain gap {acc.get('max_chain_gap')}\n\n")
                skipped = acc.get("skipped") or []
                if skipped:
                    lines.append(f"skipped {len(skipped)} tracks:\n\n")
                    for joint, reason in skipped:
                        lines.append(f"- `{joint}`: {reason}\n")
                    lines.append("\n")
                entry = acc
            lines.append(
                "| bone | src flag | tgt flag | src det | tgt det | src ortho | tgt ortho | src scale | tgt scale | "
                "rest gap° | chain gap° | resB first | resB mid | resB last | resA first | resA mid | resA last |\n"
                "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n"
            )
            for row in formula_probe(entry):
                def fnum(key):
                    v = row.get(key)
                    return "—" if v is None or (isinstance(v, float) and math.isnan(v)) else f"{v:.2f}"

                lines.append(
                    f"| `{row['bone']}` | {row['src_flags']} | {row['tgt_flags']} | "
                    f"{row['src_det']:.3f} | {row['tgt_det']:.3f} | "
                    f"{row['src_ortho']:.4f} | {row['tgt_ortho']:.4f} | "
                    f"{tuple(round(s, 4) for s in row['src_scale'])} | "
                    f"{tuple(round(s, 4) for s in row['tgt_scale'])} | "
                    f"{fnum('rest_gap')} | {fnum('chain_gap')} | {fnum('resB_first')} | {fnum('resB_mid')} | {fnum('resB_last')} | "
                    f"{fnum('resA_first')} | {fnum('resA_mid')} | {fnum('resA_last')} |\n"
                )
            lines.append("\n")

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("".join(lines), encoding="utf-8")
    print(f"wrote {out_path} ({out_path.stat().st_size} bytes)")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--models", type=Path, required=True)
    ap.add_argument("--out", type=Path, required=True)
    args = ap.parse_args()
    _, entries = pck_lib.directory(str(args.models))
    names = {e[0].lower() for e in entries}
    pack = nig.Pack(args.models)
    write_markdown(pack, names, args.out)


if __name__ == "__main__":
    main()
