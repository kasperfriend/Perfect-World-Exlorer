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
    algorithm rejects, skips, or plays verbatim, and of the composed absolute
    pose at window entry/middle/exit (the resident plays the donor's pose, so
    that is the only invariant the transfer can break).

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
# acceptance is full coverage of the locomotion bones. Keys are played
# verbatim onto the same parent chain, so no rest-gap bind-keep is applied;
# broken (non-rotation) bones keep their bind pose instead.
CORE_BONES = (
    "Bip01", "Bip01 Pelvis", "Bip01 Spine", "Bip01 Spine1", "Bip01 Spine2",
    "Bip01 Neck", "Bip01 Head",
    "Bip01 L Clavicle", "Bip01 L UpperArm", "Bip01 L Forearm", "Bip01 L Hand",
    "Bip01 R Clavicle", "Bip01 R UpperArm", "Bip01 R Forearm", "Bip01 R Hand",
    "Bip01 L Thigh", "Bip01 L Calf", "Bip01 L Foot", "Bip01 L Toe0",
    "Bip01 R Thigh", "Bip01 R Calf", "Bip01 R Foot", "Bip01 R Toe0",
)
CORE_SET = frozenset(CORE_BONES)


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


def mat3_to_quat(m):
    tr = trace3(m)
    if tr > 0.0:
        s = math.sqrt(tr + 1.0) * 2.0
        w, x, y, z = 0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s = math.sqrt(max(0.0, 1.0 + m[0][0] - m[1][1] - m[2][2])) * 2.0
        w, x, y, z = (m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s
    elif m[1][1] > m[2][2]:
        s = math.sqrt(max(0.0, 1.0 + m[1][1] - m[0][0] - m[2][2])) * 2.0
        w, x, y, z = (m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s
    else:
        s = math.sqrt(max(0.0, 1.0 + m[2][2] - m[0][0] - m[1][1])) * 2.0
        w, x, y, z = (m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s
    return quat_norm([x, y, z, w])


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


def gait_candidate_rank(name):
    """Selection tier mirroring src/modules/populator/mod.rs::gait_candidate_rank.

    The client's file order puts the bound/grapple slices first, which fold the
    legs into a squat on a resident rig. The generic cycle slices win outright.
    """
    plain = {"行走_通用", "移动_通用", "行走", "移动", "走",
             "奔跑_通用", "跑_通用", "奔跑", "跑"}
    return 0 if name.lower() in plain else 1


def _ranked_candidates(model_actions, kind):
    indexed = [
        (action_index, action)
        for action_index, action in enumerate(model_actions)
        if gait_kind(action["name"]) == kind
    ]
    indexed.sort(key=lambda pair: gait_candidate_rank(pair[1]["name"]))
    return indexed


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

    def resolve(i, depth=0):
        if abs_rot[i] is None:
            if depth >= len(bones):
                return None  # corrupt cyclic parent data
            parent = bones[i]["parent"]
            if 0 <= parent < len(bones):
                parent_rot = resolve(parent, depth + 1)
                if parent_rot is None:
                    return None
                abs_rot[i] = mat3_mul(parent_rot, units[i])
            else:
                abs_rot[i] = [row[:] for row in units[i]]
        return abs_rot[i]

    for i in range(len(bones)):
        resolve(i)
    return abs_rot


def _chain_divergence(src_abs, tgt_abs, sbone, tbone):
    """Angle between the two skeletons' rest PARENT frames for this bone.

    Diagnostic only. With verbatim keys the resident reproduces the donor's
    composed absolute pose exactly (see formula_probe), no matter how far
    apart the rest frames point, so this quantifies context instead of
    predicting failures.
    """
    sp, tp = sbone["parent"], tbone["parent"]
    if not (0 <= sp < len(src_abs)) or not (0 <= tp < len(tgt_abs)):
        return None
    if src_abs[sp] is None or tgt_abs[tp] is None:
        return None
    if mat3_det(src_abs[sp]) <= 0.0 or mat3_det(tgt_abs[tp]) <= 0.0:
        return 180.0
    return angle_between_rotation_mats(src_abs[sp], tgt_abs[tp])




def _transport_track(track):
    """The donor's authored local rotations are played verbatim on the
    target, mirroring src/modules/populator/mod.rs::retarget_gait_clip: both
    rigs trace the same parent chain (name- and parent-checked), so the
    composed pose is the donor's pose rooted at the target's placement."""
    rotation = track["rotation"]
    keys = [quat_norm(key) for key in rotation["keys"]]
    return {**track, "rotation": {**rotation, "keys": keys}}


def _evaluate_candidate(source, target, kind, t_joints, action_index, action, gate):
    """Evaluate one walk/run-named action. Returns (entry, stop): stop=True
    means the caller should break the candidate loop (accepted or aborted)."""
    if True:
        entry = {"name": action["name"], "index": action_index}
        clip = clip_ms(action, source["skeleton"]["fps"])
        entry["clip_ms"] = clip
        if clip is None:
            entry["verdict"] = "unsampleable clip"
            return entry, False
        track_set = source["tracks"][action_index]
        if track_set is None:
            embedded = source["skeleton"]["embedded"]
            entry["source_tracks"] = len(embedded)
            track_set = {"tracks": embedded} if embedded else None
        elif "error" in track_set:
            entry["verdict"] = f"no tracks ({track_set['error']})"
            return entry, False
        if track_set is None:
            entry["verdict"] = "no tracks"
            return entry, False
        source_tracks = track_set["tracks"]
        entry["source_tracks"] = len(source_tracks)
        try:
            entry["motion_deg"] = round(clip_motion_deg(track_set, clip), 2)
        except Exception:
            entry["motion_deg"] = None
        if not source_tracks:
            entry["verdict"] = "empty track set"
            return entry, False

        mapped, seen, skipped = [], set(), []
        aborted = None
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
                else None
            )
            chain_gap = _chain_divergence(src_abs, tgt_abs, sbone, tbone)
            # Only a reflection/degenerate rest frame keeps the bone at
            # bind. There is no ancestor-frame concept anymore: the donor's
            # keys are played verbatim on top of the resident's own chain.
            broken = mat3_det(s_unit) <= 0.0 or mat3_det(t_unit) <= 0.0
            mapped.append(
                {"joint": sjoint["name"], "bone": sbone["name"],
                 "src": sb, "tgt": tb,
                 "src_flags": sbone["flags"], "tgt_flags": tbone["flags"],
                 "rest_gap": rest_gap,
                 "chain_gap": chain_gap,
                 "broken": broken,
                 "track": track}
            )
        if aborted:
            entry["verdict"] = aborted
            return entry, True
        mapped_names = {m["bone"] for m in mapped if not m.get("broken")}
        core_gaps = [
            m["rest_gap"]
            for m in mapped
            if m["bone"] in CORE_SET and m["rest_gap"] is not None
        ]
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
                return entry, False
            entry["verdict"] = "accepted"
            return entry, True
        need = min(len(source_tracks), 8)
        coverage = sum(1 for m in mapped if not m.get("broken"))
        if coverage < need or coverage * 100 < len(source_tracks) * 70:
            entry["verdict"] = f"insufficient coverage {coverage}/{len(source_tracks)}"
            entry["mapped"] = len(mapped)
            entry["skipped"] = skipped
            entry["bones"] = mapped
            return entry, False
        entry["verdict"] = "accepted"
        entry["mapped"] = len(mapped)
        entry["skipped"] = skipped
        entry["bones"] = mapped
        return entry, True


# --------------------------------------------------------------------------
# Runtime selection mirror: Clip::new + ordered candidate loop + visibly_moves.
# The game walks the SAME model.actions order, rejects what Clip::new rejects,
# skips candidates that fail mapping or locomotion coverage, and - only then -
# demands the retargeted clip visibly move a sampled vertex. This is the only
# part of the acceptance path the offline table cannot see, so it gets its own
# section. The bomben bone-level signature is strictly more sensitive than the
# game's 8-vertices-per-mesh hash: if nothing here moves, nothing moves there.

def _slerp(a, b, t):
    dot = sum(a[i] * b[i] for i in range(4))
    y = b
    if dot < 0.0:
        y = [-v for v in b]
        dot = -dot
    if dot > 0.9995:
        q = [a[i] + (y[i] - a[i]) * t for i in range(4)]
        n = math.sqrt(sum(v * v for v in q)) or 1.0
        return [v / n for v in q]
    theta_0 = math.acos(max(-1.0, min(1.0, dot)))
    sin_0 = math.sin(theta_0)
    if sin_0 == 0.0:
        return a
    theta = theta_0 * t
    s0 = math.sin(theta_0 - theta) / sin_0
    s1 = math.sin(theta) / sin_0
    return [s0 * a[i] + s1 * y[i] for i in range(4)]


def sample_keytrack(track, seconds):
    """animation::KeyTrack::sample: frame = seconds * fps, lerp keys."""
    keys = track["keys"]
    frames = track["frames"]
    fps = track["fps"]
    if not keys or fps <= 0:
        return None
    frame = max(0.0, seconds * fps)
    i = 0
    while i < len(frames) and frames[i] <= frame:
        i += 1
    if i == 0:
        return keys[0]
    if i == len(frames):
        return keys[-1]
    a = i - 1
    f0, f1 = frames[a], frames[i]
    t = 0.0 if f1 == f0 else (frame - f0) / (f1 - f0)
    if len(keys[a]) == 3:
        return [keys[a][j] + (keys[i][j] - keys[a][j]) * t for j in range(3)]
    return _slerp(quat_norm(keys[a]), quat_norm(keys[i]), t)


def _pose_signature(matrices):
    values = []
    for m in matrices:
        for row in m[:3]:
            values.extend(int(math.trunc(v * 100)) for v in row)
    return hash(tuple(values))


from collections import deque

def _fk_absolute(target, rt_by_bone, seconds):
    bones = target["skeleton"]["bones"]
    count = len(bones)
    base = [mat_from_cols(b["relative"]) for b in bones]
    children = [[] for _ in range(count)]
    queue = deque()
    for i, bone in enumerate(bones):
        parent = bone["parent"]
        if parent == -1:
            queue.append(i)
        else:
            children[parent].append(i)
    absolute = [None] * count
    while queue:
        i = queue.popleft()
        local = base[i]
        rt = rt_by_bone.get(i)
        if rt is not None:
            pos = sample_keytrack(rt["position"], seconds)
            rot = sample_keytrack(rt["rotation"], seconds)
            if pos is None or rot is None:
                return None
            r3 = quat_to_mat3(_slerp(rot, rot, 0.0))  # rot already normalized
            local = [
                [r3[0][0], r3[0][1], r3[0][2], pos[0]],
                [r3[1][0], r3[1][1], r3[1][2], pos[1]],
                [r3[2][0], r3[2][1], r3[2][2], pos[2]],
                [0.0, 0.0, 0.0, 1.0],
            ]
        parent = bones[i]["parent"]
        absolute[i] = local if parent == -1 else _mat4_mul(absolute[parent], local)
        for child in children[i]:
            queue.append(child)
    return absolute


def _mat4_mul(a, b):
    return [
        [sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)]
        for i in range(4)
    ]


def visibly_moves_proxy(source, target, entry):
    """Model of the game's visibly_moves gate on an accepted candidate.

    The game hashes 8 sampled vertices of every skinned mesh; this proxy
    hashes the mapped bones' absolute matrices (12 floats each, quantized to
    0.01), which flips whenever the game's hash would - plus on any in-place
    rotation of a sampled bone. Strictly more sensitive on the mapped set.
    """
    rt_by_bone = {}
    tgt_index = {b["name"]: i for i, b in enumerate(target["skeleton"]["bones"])}
    mapped_indices = []
    for mapped in entry.get("bones", []):
        i = tgt_index.get(mapped["bone"])
        if i is None:
            continue
        track = _transport_track(mapped["track"])
        rest = mat_from_cols(target["skeleton"]["bones"][i]["relative"])
        # The game pins the position track to the target rest translation.
        track["position"] = {
            "fps": 1,
            "keys": [[rest[0][3], rest[1][3], rest[2][3]]],
            "frames": [0.0],
        }
        rt_by_bone[i] = track
        mapped_indices.append(i)
    if not mapped_indices or "clip_ms" not in entry or entry["clip_ms"] is None:
        return None
    start_ms, _, length_ms = entry["clip_ms"]
    length_ms = max(1, length_ms)

    def seconds_at(clock):
        if clock <= 0:
            return start_ms * 0.001
        return (start_ms + int(clock * 1000) % length_ms) * 0.001

    start = _fk_absolute(target, rt_by_bone, seconds_at(0.))
    if start is None:
        return None
    base_indices = sorted(rt_by_bone)
    base_sig = _pose_signature([start[i] for i in base_indices])
    max_move_mm = 0.0
    moves = False
    for offset in (0.13, 0.21, 0.29, 0.37, 0.49):
        at = seconds_at(offset)
        if abs(at - seconds_at(0.)) < 1e-9:
            continue
        pose = _fk_absolute(target, rt_by_bone, at)
        if pose is None:
            continue
        sig = _pose_signature([pose[i] for i in base_indices])
        for i in base_indices:
            dx = pose[i][0][3] - start[i][0][3]
            dy = pose[i][1][3] - start[i][1][3]
            dz = pose[i][2][3] - start[i][2][3]
            max_move_mm = max(max_move_mm, math.sqrt(dx * dx + dy * dy + dz * dz) * 1000.0)
        if sig != base_sig:
            moves = True
    return {"moves": moves, "max_bone_move_mm": max_move_mm}


def runtime_gait(source, target, kind):
    """Mirror the game's ranked loop over walk/run candidates.

    Returns {"selected": entry-or-None, "trace": [...]}. Every candidate the
    game would consider is listed with the stage that decided its fate.
    visibly_moves is evaluated on the retargeted pose exactly when the game
    evaluates it: after the mapping and locomotion-coverage gates pass.
    """
    t_joints = {}
    for i, j in enumerate(target["skeleton"]["joints"]):
        t_joints.setdefault(j["name"], []).append(i)
    trace = []
    for action_index, action in _ranked_candidates(source["model"]["actions"], kind):
        clip = clip_ms(action, source["skeleton"]["fps"])
        if clip is None:
            trace.append((action["name"], "Clip::new rejects", None))
            continue
        entry, _ = _evaluate_candidate(source, target, kind, t_joints, action_index, action, "core")
        if entry["verdict"] != "accepted":
            trace.append((action["name"], entry["verdict"], None))
            continue
        vise = visibly_moves_proxy(source, target, entry)
        if vise is not None and not vise["moves"]:
            trace.append((action["name"], "visibly_moves FAIL", vise))
            continue
        trace.append((action["name"], "SELECTED", vise))
        return {"selected": entry, "trace": trace, "vise": vise}
    return {"selected": None, "trace": trace, "vise": None}


def runtime_selection_section(donors, resident_paths, load_cached):
    """Per numbered resident rig, mirror the game's clip selection end to end.

    `resident_paths` load lazily in the caller's cache; donors come from the
    same cache the acceptance table uses.
    """
    from collections import Counter
    lines = [
        "## Runtime selection mirror (per resident, gender donor)\n\n",
        "What the game would actually play for each numbered resident rig, in "
        "the game's selection order (generic cycle names first, then file "
        "order), with the `visibly_moves` gate evaluated on the "
        "retargeted pose exactly where the game evaluates it. `vise` is a "
        "bone-level proxy strictly more sensitive than the game's "
        "8-vertices-per-mesh vertex hash; `max bone move` is the largest "
        "mapped-bone translation between the clip start and the sampled "
        "offsets, in millimetres. When nothing is SELECTED the resident draws "
        "its bind pose while sliding along its route - the in-game "
        "'stopped walking'.\n\n",
        "| rig | walk selection | run selection |\n| --- | --- | --- |\n",
    ]
    picks_walk = Counter()
    picks_run = Counter()
    none_walk = none_run = 0
    fell = []
    for path in resident_paths:
        asset, error = load_cached(path)
        if error or asset is None:
            lines.append(f"| `{path}` | load error | |\n")
            continue
        gender = "m" if path.split("/")[3].startswith("男") else (
            "f" if path.split("/")[3].startswith("女") else None)
        donor = "models/players/形象/武侠男/躯干/武侠男.ecm" if gender == "m" else (
            "models/players/形象/武侠女/躯干/武侠女.ecm" if gender == "f" else None)
        donor_asset = donors.get(donor) if donor else None
        if donor_asset is None:
            lines.append(f"| `{path}` | donor unloadable | |\n")
            continue
        cells = []
        for kind, picks, none_counter in (("walk", picks_walk, None), ("run", picks_run, None)):
            result = runtime_gait(donor_asset, asset, kind)
            if result["selected"] is None:
                picks["(bind pose)"] += 1
                if kind == "walk":
                    none_walk += 1
                else:
                    none_run += 1
                reasons = "; ".join(
                    f"{n}: {v}" for n, v, _ in result["trace"][:4]
                ) or "no locomotion candidates"
                cells.append(f"**bind pose** ({reasons})")
            else:
                name_sel = result["selected"]["name"]
                picks[name_sel] += 1
                if len(result["trace"]) > 1:
                    fell.append((path, kind, name_sel, len(result["trace"])))
                vise = result["vise"]
                move = f", max {vise['max_bone_move_mm']:.0f} mm" if vise else ""
                cells.append(f"`{name_sel}`{move}")
        lines.append(f"| `{path}` | {cells[0]} | {cells[1]} |\n")
    lines.append("\nbreak-down of the walk candidates that won at runtime:\n\n")
    for name_sel, count in picks_walk.most_common():
        lines.append(f"- walk -> `{name_sel}` x{count}\n")
    for name_sel, count in picks_run.most_common():
        lines.append(f"- run -> `{name_sel}` x{count}\n")
    if fell:
        lines.append("\ncandidates that only won after an earlier clip was rejected:\n\n")
        for path, kind, name_sel, depth in fell:
            lines.append(f"- `{path}` {kind} -> `{name_sel}` (after {depth - 1} rejected)\n")
    lines.append(
        f"\nsummary: walk bind pose {none_walk}/{len(resident_paths)} rigs, "
        f"run bind pose {none_run}/{len(resident_paths)} rigs\n\n"
    )
    return "".join(lines)



def retarget_clip(source, target, kind, gate="legacy"):
    t_joints = {}
    for i, j in enumerate(target["skeleton"]["joints"]):
        t_joints.setdefault(j["name"], []).append(i)

    report = {"kind": kind, "actions": [], "accepted": None}
    for action_index, action in _ranked_candidates(source["model"]["actions"], kind):
        entry, stop = _evaluate_candidate(source, target, kind, t_joints, action_index, action, gate)
        report["actions"].append(entry)
        if entry["verdict"] == "accepted":
            report["accepted"] = entry
            break
        if stop:
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


def _absolute_rotations(bones, local_by_bone):
    """absolute[i] = absolute[parent] * local[i], composing parent-first like
    the engine does; roots compose with nothing."""
    children = [[] for _ in range(len(bones))]
    queue = deque()
    for i, bone in enumerate(bones):
        parent = bone["parent"]
        if parent == -1:
            queue.append(i)
        elif 0 <= parent < len(bones):
            children[parent].append(i)
    absolute = [None] * len(bones)
    while queue:
        i = queue.popleft()
        local = local_by_bone[i]
        parent = bones[i]["parent"]
        base = None if parent == -1 else absolute[parent]
        if parent != -1 and base is None:
            return None  # unresolvable chain: report nothing rather than crash
        absolute[i] = local if parent == -1 else mat3_mul(base, local)
        for child in children[i]:
            queue.append(child)
    if any(value is None for value in absolute):
        return None
    return absolute


def _chain_locals(asset, tracks_by_joint, seconds):
    """Rest rotation, or the sampled authored rotation, for every bone of a rig."""
    bones = asset["skeleton"]["bones"]
    joint_of_bone = {}
    for joint_index, joint in enumerate(asset["skeleton"]["joints"]):
        joint_of_bone.setdefault(joint["child"], joint_index)
    locals_ = []
    for i, bone in enumerate(bones):
        joint_index = joint_of_bone.get(i)
        track = tracks_by_joint.get(joint_index) if joint_index is not None else None
        if track is not None:
            rot = sample_keytrack(track["rotation"], seconds)
            if rot is not None:
                locals_.append(quat_to_mat3(quat_norm(rot)))
                continue
        rel = mat_from_cols(bone["relative"])
        cols, _ = normalized_cols(rel)
        locals_.append(unit_cols_to_mat(cols))
    return locals_


def _tracks_by_joint(asset, entry):
    """All authored tracks of the entry's action, keyed by joint index."""
    action_index = entry["index"]
    track_set = asset["tracks"][action_index]
    if track_set is None and asset["skeleton"]["embedded"]:
        track_set = {"tracks": asset["skeleton"]["embedded"]}
    if not track_set or "tracks" not in track_set:
        return {}
    return {track["id"]: track for track in track_set["tracks"]}


def formula_probe(source, target, entry):
    """Per mapped bone, the FK invariant the verbatim transfer actually holds.

    Played keys are copied verbatim; the resident's absolute rotation then
    differs from the donor's by exactly the rest-layout divergence of the
    bone's ancestor chain - constant across samples as long as every ancestor
    in the played chain is itself played. `pose match first/mid/last` reports
    that deviation at the window ends/middle (it mirrors how far apart the
    two rigs point the chain - it is NOT expected to be 0.0 on a divergent
    rig, and that is fine). `pose spread` is max minus min of the three: it
    is ~0.0 whenever the whole ancestor chain is played, and it grows exactly
    when a mid-chain ancestor is held at bind (unmapped, skipped or broken)
    while donor-played descendants swing on top of it. Diagnostic only;
    nothing is gated on it.
    """
    rows = []
    for bone in entry.get("bones", []):
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
            "rest_gap": bone.get("rest_gap"),
            "chain_gap": bone.get("chain_gap"),
        }
        rows.append(row)

    mapped = [bone for bone in entry.get("bones", []) if not bone.get("broken")]
    start_ms, end_ms, _ = entry["clip_ms"]
    src_all = _tracks_by_joint(source, entry)
    src_fps = None
    for bone in mapped:
        keys = bone["track"]["rotation"]
        if keys["fps"] > 0:
            src_fps = keys["fps"]
            break
    if src_fps and src_fps > 0:
        src_bones = source["skeleton"]["bones"]
        tgt_bones = target["skeleton"]["bones"]
        src_bone_of_name = {b["name"]: i for i, b in enumerate(src_bones)}
        tgt_bone_of_name = {b["name"]: i for i, b in enumerate(tgt_bones)}
        src_joint_of_bone = {j["child"]: idx for idx, j in enumerate(source["skeleton"]["joints"])}
        tgt_joint_of_bone = {j["child"]: idx for idx, j in enumerate(target["skeleton"]["joints"])}
        tgt_mapped = {}
        for bone in mapped:
            if bone.get("broken"):
                continue  # bind-kept in game: stays at bind in the FK probe too
            i = tgt_bone_of_name.get(bone["bone"])
            if i is None:
                continue
            tgt_mapped[tgt_joint_of_bone[i]] = _transport_track(bone["track"])
        first = mapped[0]["track"]["rotation"] if mapped else None
        frames = first["frames"] if first else []
        window = [
            i for i, f in enumerate(frames)
            if start_ms * src_fps / 1000.0 - 0.5 <= f <= end_ms * src_fps / 1000.0 + 0.5
        ]
        picks = (
            [window[0], window[len(window) // 2], window[-1]] if window else []
        )
        for tag, pick in zip(("first", "mid", "last"), picks):
            seconds = frames[pick] / src_fps
            src_locals = _chain_locals(source, src_all, seconds)
            tgt_locals = _chain_locals(target, tgt_mapped, seconds)
            src_abs = _absolute_rotations(src_bones, src_locals)
            tgt_abs = _absolute_rotations(tgt_bones, tgt_locals)
            if src_abs is None or tgt_abs is None:
                continue
            for row in rows:
                bone = next((b for b in mapped if b["bone"] == row["bone"]), None)
                if bone is None:
                    continue
                s_i = src_bone_of_name[bone["bone"]]
                t_i = tgt_bone_of_name[bone["bone"]]
                row[f"pose_match_{tag}"] = angle_between_rotation_mats(src_abs[s_i], tgt_abs[t_i])
        for row in rows:
            values = [
                row[f"pose_match_{tag}"]
                for tag in ("first", "mid", "last")
                if f"pose_match_{tag}" in row
            ]
            if values:
                row["pose_spread"] = max(values) - min(values)
    return rows


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


def clip_probe_lines(donors):
    lines = []
    lines.append(
        "## Clip content probe (shipped gender donors, window-restricted)\n\n"
        "What each locomotion slice actually contains INSIDE its authored clip "
        "window - not over the whole master track. The combined actions are "
        "windows into shared master recordings, so whole-track statistics "
        "describe jumps and falls, not the walk the game plays. `root`/`pelvis` "
        "are the authored position-key axis ranges within the window next to "
        "the bone's rest translation; the angle columns are each bone's "
        "authored rotation vs its rest pose in degrees, inside the window. "
        "An authored root/pelvis "
        "drop or folded thigh reads as a crouch or a sit in game.\n\n"
    )
    for donor in (
        "models/players/形象/武侠男/躯干/武侠男.ecm",
        "models/players/形象/武侠女/躯干/武侠女.ecm",
    ):
        asset = donors.get(donor)
        if asset is None:
            continue
        sk = asset["skeleton"]
        bones = sk["bones"]
        joints = sk["joints"]
        bone_by_index = bones
        joint_of_bone = {}
        for joint_index, joint in enumerate(joints):
            try:
                joint_of_bone.setdefault(joint["child"], joint_index)
            except Exception:
                continue
        lines.append(f"### `{donor}`\n\n")
        for kind in ("walk", "run"):
            for action_index, action in enumerate(asset["model"]["actions"]):
                if gait_kind(action["name"]) != kind:
                    continue
                clip = clip_ms(action, sk["fps"])
                if clip is None:
                    lines.append(f"- {kind} `{action['name']}` (#{action_index}): unsampleable clip\n")
                    continue
                start_ms, end_ms, length_ms = clip
                track_set = asset["tracks"][action_index]
                if track_set is None and sk["embedded"]:
                    track_set = {"tracks": sk["embedded"]}
                if not track_set or "tracks" not in track_set:
                    lines.append(f"- {kind} `{action['name']}` (#{action_index}): no tracks\n")
                    continue
                by_joint = {t["id"]: t for t in track_set["tracks"]}

                def track_for(bone_name):
                    for index, bone in enumerate(bone_by_index):
                        if bone["name"] == bone_name:
                            joint_index = joint_of_bone.get(index)
                            if joint_index is None:
                                return None, None
                            return by_joint.get(joint_index), bone
                    return None, None

                def rest_translation(bone):
                    m = mat_from_cols(bone["relative"])
                    return m[0][3], m[1][3], m[2][3]

                def window_indices(track):
                    fps = track["rotation"]["fps"]
                    if fps <= 0:
                        return []
                    start_f = start_ms * fps / 1000.0
                    end_f = end_ms * fps / 1000.0
                    return [
                        i for i, frame in enumerate(track["rotation"]["frames"])
                        if start_f - 0.5 <= frame <= end_f + 0.5
                    ]

                def pos_range(track):
                    if track is None:
                        return None, 0
                    fps = track["position"]["fps"]
                    keys = track["position"]["keys"]
                    frames = track["position"]["frames"]
                    pick = [
                        i for i, frame in enumerate(frames)
                        if start_ms * fps / 1000.0 - 0.5 <= frame <= end_ms * fps / 1000.0 + 0.5
                    ]
                    if not pick:
                        pick = [0]
                    axes = list(zip(*(keys[i] for i in pick)))
                    return [(min(a), max(a)) for a in axes], len(pick)

                def delta_angles(track, bone):
                    if track is None:
                        return None, None
                    rel = mat_from_cols(bone["relative"])
                    s_cols, _ = normalized_cols(rel)
                    s_mat = unit_cols_to_mat(s_cols)
                    keys = track["rotation"]["keys"]
                    indices = window_indices(track)
                    if not indices:
                        indices = list(range(len(keys)))
                    angles = {}
                    for i in indices:
                        rs = quat_to_mat3(quat_norm(keys[i]))
                        angles[i] = angle_between_rotation_mats(s_mat, rs) if mat3_det(rs) > 0 else 999.0
                    ordered = [angles[i] for i in sorted(angles)]
                    return ordered, None

                root_track, root_bone = track_for("Bip01")
                pelvis_track, pelvis_bone = track_for("Bip01 Pelvis")
                lines.append(
                    f"- {kind} `{action['name']}` (#{action_index}): "
                    f"{start_ms}..{end_ms} ms (len {length_ms} ms), tracks={len(track_set['tracks'])}\n"
                )
                for tag, track, bone in (("root", root_track, root_bone), ("pelvis", pelvis_track, pelvis_bone)):
                    if track is None or bone is None:
                        continue
                    rng, k = pos_range(track)
                    rest = rest_translation(bone)
                    lines.append(
                        f"  - {tag} pos window-keys={k} rest=({rest[0]:.3f}, {rest[1]:.3f}, {rest[2]:.3f}) "
                        f"x[{rng[0][0]:.3f}..{rng[0][1]:.3f}] "
                        f"y[{rng[1][0]:.3f}..{rng[1][1]:.3f}] "
                        f"z[{rng[2][0]:.3f}..{rng[2][1]:.3f}]\n"
                    )
                for bone_name in (
                    "Bip01 Spine", "Bip01 L Thigh", "Bip01 L Calf", "Bip01 L Foot",
                    "Bip01 R Thigh", "Bip01 L UpperArm", "Bip01 L Forearm", "Bip01 Head",
                ):
                    track, bone = track_for(bone_name)
                    if track is None or bone is None:
                        continue
                    angles, _ = delta_angles(track, bone)
                    if not angles:
                        lines.append(f"  - {bone_name}: no keys in window\n")
                        continue
                    lines.append(
                        f"  - {bone_name}: delta max {max(angles):.1f} deg, "
                        f"mean {sum(angles) / len(angles):.1f} deg over {len(angles)} window keys\n"
                    )
        lines.append("\n")

    return lines


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

    lines.extend(clip_probe_lines(donors))

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
        "`player *` columns use the shipped core-coverage gate on retargeting "
        "(the donor's authored local rotations played verbatim onto the "
        "resident's own same-named parent chain, as shipped) with the "
        "gender-correct player donor. `guard *` columns keep the legacy 70% "
        "gate (Guard rigs, the old behaviour) for contrast. `max chain gap` "
        "is the largest rest parent-frame divergence over mapped locomotion "
        "bones for the same-gender player donor; it is diagnostic only - "
        "verbatim keys reproduce the donor's composed absolute pose at any "
        "gap - and never rejects a clip.\n\n"
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
                "rest gap° | chain gap° | pose match first | pose match mid | pose match last | pose spread° |\n"
                "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n"
            )
            for row in formula_probe(d_asset, r_asset, entry):
                def fnum(key):
                    v = row.get(key)
                    return "—" if v is None or (isinstance(v, float) and math.isnan(v)) else f"{v:.2f}"

                lines.append(
                    f"| `{row['bone']}` | {row['src_flags']} | {row['tgt_flags']} | "
                    f"{row['src_det']:.3f} | {row['tgt_det']:.3f} | "
                    f"{row['src_ortho']:.4f} | {row['tgt_ortho']:.4f} | "
                    f"{tuple(round(s, 4) for s in row['src_scale'])} | "
                    f"{tuple(round(s, 4) for s in row['tgt_scale'])} | "
                    f"{fnum('rest_gap')} | {fnum('chain_gap')} | "
                    f"{fnum('pose_match_first')} | {fnum('pose_match_mid')} | {fnum('pose_match_last')} | {fnum('pose_spread')} |\n"
                )
            lines.append("\n")

    lines.extend(runtime_selection_section(donors, resident_paths, load_cached))

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
