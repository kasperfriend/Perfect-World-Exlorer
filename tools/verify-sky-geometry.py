#!/usr/bin/env python3
"""Regenerate the native sky meshes from the recovered formulas and diff them
against the emulated originals.

`tools/emulate-sky-sphere.py` runs A3DSkySphere::Init (0x79a330 / 0x79a810) in
Unicorn and returns the client's own vertex and index buffers.  This script
re-derives both buffers from the formulas that `src/sky_geometry.rs` uses, so a
formula drift breaks the comparison instead of silently changing the sky.

Usage:
  python3 tools/verify-sky-geometry.py            # summary, exit 1 on mismatch
  python3 tools/verify-sky-geometry.py --native   # dump the native buffers
"""
import argparse
import importlib.util
import math
import struct
import sys

F32 = lambda x: struct.unpack("<f", struct.pack("<f", x))[0]

# A3DSkySphere ctor defaults (0x799610): band count, angular step, radius.
BANDS = 6                 # word at this+0x4c0
STEP = struct.unpack("<f", struct.pack("<I", 0x3E9C61AA))[0]   # this+0x4c4
RADIUS = 50.0             # this+0x4c8
HALF_PI = struct.unpack("<f", struct.pack("<I", 0x3FC90FDB))[0]
QUARTER_PI = struct.unpack("<f", struct.pack("<I", 0x3F490FDB))[0]
V_BIAS = 0.05             # 0x8f14c4
V_SCALE = 0.45            # 0x8f14c8
# Row heights and their two texture runs (tables at 0x79a4af and 0x79a52f),
# mirrored by ROW_Y / ROW_V_FRONT / ROW_V_BACK in src/sky_geometry.rs.
ROW_Y = [39.270_000_5, 19.635_000_2, 6.558_089_7, 0.0, -10.602_9]
ROW_V_FRONT = [0.05, 0.275, 0.424_85, 0.5, 0.5]
ROW_V_BACK = [0.95, 0.725, 0.575_15, 0.5, 0.5]


def surround():
    """Horizon curtain, A3DSkySphere::Init 0x79a330 -> stream 0x584."""
    verts, idx = [], []
    for row, y in enumerate(ROW_Y):
        for half in (0, 1):
            base = HALF_PI if half == 0 else 0.0
            for column in range(BANDS + 1):
                angle = F32(base + F32(F32(HALF_PI * column) / BANDS))
                x = F32(RADIUS * F32(math.cos(angle)))
                z = F32(RADIUS * F32(math.sin(angle)))
                u = F32(1.0 - F32(column / BANDS))
                v = (ROW_V_FRONT if half == 0 else ROW_V_BACK)[row]
                # The rim row carries a zero alpha byte (0x00ffffff); every
                # other row is opaque white (0xffffffff).
                colour = 0x00FFFFFF if row == 0 else 0xFFFFFFFF
                verts.append((x, y, z, colour, u, v))
    row_span = 2 * (BANDS + 1)
    for row in range(len(ROW_Y) - 1):
        for half in (0, 1):
            for column in range(BANDS):
                v0 = row * row_span + half * (BANDS + 1) + column
                v1, v2 = v0 + 1, v0 + row_span
                idx += [v0, v2, v2 + 1, v0, v2 + 1, v1]
    return verts, idx


def roof():
    """Polar cap, A3DSkySphere::Init 0x79a810 -> stream 0x588."""
    verts = [(0.0, RADIUS, 0.0, 0xFFFFFFFF, 0.5, 0.5)]
    # Normalisation disc: 1 / (2 * R * sin(step)) — 0x79a930 computes the same
    # product before the loop, so every ring shares one planar projection.
    disc = F32(2.0 * F32(RADIUS * F32(math.sin(STEP))))
    sectors = 4 * BANDS
    for ring in range(1, BANDS + 1):
        theta = F32(F32(STEP * ring) / BANDS)
        for sector in range(sectors):
            phi = F32(F32(2.0 * math.pi * sector) / sectors)
            x = F32(F32(RADIUS * F32(math.sin(theta))) * F32(math.cos(phi)))
            y = F32(RADIUS * F32(math.cos(theta)))
            z = F32(F32(RADIUS * F32(math.sin(theta))) * F32(math.sin(phi)))
            u = F32(0.5 + F32(x / disc))
            v = F32(0.5 + F32(z / disc))
            verts.append((x, y, z, 0xFFFFFFFF, u, v))
    idx = []
    for sector in range(sectors):
        nxt = sector + 1 if sector + 1 < sectors else 0
        idx += [0, 1 + sector, 1 + nxt]
    for ring in range(1, BANDS):
        a0 = 1 + (ring - 1) * sectors
        b0 = 1 + ring * sectors
        for sector in range(sectors):
            nxt = sector + 1 if sector + 1 < sectors else 0
            a, b = a0 + sector, a0 + nxt
            c, d = b0 + sector, b0 + nxt
            idx += [a, c, d, a, d, b]
    return verts, idx


def native():
    spec = importlib.util.spec_from_file_location(
        "emulate_sky", "tools/emulate-sky-sphere.py")
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    sky = mod.Sky()
    sky.run()
    out = []
    for which in (0, 1):
        verts, idx, nv, ni = sky.mesh(which)
        out.append(([(v[0], v[1], v[2], v[3], v[4], v[5]) for v in verts], list(idx)))
    return out


def diff(name, mine, theirs):
    mv, mi = mine
    nv, ni = theirs
    problems = []
    if len(mv) != len(nv):
        problems.append("%s vertex count %d != %d" % (name, len(mv), len(nv)))
    if len(mi) != len(ni):
        problems.append("%s index count %d != %d" % (name, len(mi), len(ni)))
    worst = 0.0
    for i, (a, b) in enumerate(zip(mv, nv)):
        for k in range(6):
            if k == 3:
                if a[k] != b[k]:
                    problems.append("%s vertex %d diffuse %#x != %#x" % (name, i, a[k], b[k]))
                continue
            d = abs(a[k] - b[k])
            worst = max(worst, d)
            if d > 1e-4:
                problems.append("%s vertex %d component %d %.8f != %.8f"
                                % (name, i, k, a[k], b[k]))
    for i, (a, b) in enumerate(zip(mi, ni)):
        if a != b:
            problems.append("%s index %d %d != %d" % (name, i, a, b))
            break
    print("%-10s verts %3d/%3d indices %3d/%3d worst component delta %.3e"
          % (name, len(mv), len(nv), len(mi), len(ni), worst))
    return problems


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--native", action="store_true")
    a = ap.parse_args()
    nat = native()
    if a.native:
        for which, (name, mesh) in enumerate(zip(("surround", "roof"), (surround(), roof()))):
            print("== native", name)
            for i, v in enumerate(nat[which][0]):
                print("%3d %9.5f %9.5f %9.5f %#010x u=%.5f v=%.5f" % ((i,) + v))
    problems = []
    for name, mine, theirs in (("surround", surround(), nat[0]), ("roof", roof(), nat[1])):
        problems += diff(name, mine, theirs)
    if problems:
        print("\nMISMATCHES (%d):" % len(problems))
        for p in problems[:40]:
            print(" ", p)
        return 1
    print("\nboth meshes match the emulated native buffers")
    return 0


if __name__ == "__main__":
    sys.exit(main())
