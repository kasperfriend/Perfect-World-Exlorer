#!/usr/bin/env python3
"""Survey T2MK layer/mask files and T2LM lightmap files without dependencies.

Recovered native anchors (elementclient 1.4.2, md5 5f02895688fa580506e869156340a664):

  A3DTerrain2::LoadT2MK region, 0x7a1599-0x7a18b7
    007a1599: cmpl $0x544d4b42,0x20(%esp)   ; 'BKMT' magic, little endian
    007a15a1: je ...
    007a15c1: mov 0x24(%esp),%eax
    007a15c5: cmp  $0x2,%eax                ; version, rejects > 2
    007a15f3: cmp  $0x2,%eax
    007a15fb: jae  0x7a1634                 ; v2 reads 12 bytes, v1 reads 8
    007a161b: -> v1 header is 8 bytes (no grid dimension)
    007a163d: call *0x10(%edx)              ; read 12 bytes
    007a1662: mov 0x1c(%esp),%dl            ; v2 byte at header+8
    007a1666: mov %dl,0x891(%ebp)           ; "layer dimension" flag
    007a1676: movl $0x64,0x8cc(%ebp)        ; 100 = hard layer cap per area
    007a178c: push $0x1c                    ; 28-byte layer record
    007a1837: mov  $0x7,%ecx ; rep movsl    ; 7 dwords per record
    007a1916: cmpl $0x54324c4d              ; 'ML2T' magic of the companion lightmap

  T2LM v2 header, 0x7a195e onward: magic, version, tile count 16, tile size,
  16 offsets. Matches src/terrain_lightmaps.rs.

Usage:
  tools/terrain-layer-survey.py google_drive/world_35.t2mk [more files...]
  tools/terrain-layer-survey.py --manifest Findings/Phase-1-lightmap-payload-manifest.csv
"""
import struct
import sys
import zlib
from collections import Counter, defaultdict

MAGIC_T2MK = 0x544D4B42
MAGIC_T2LM = 0x54324C4D
MAX_LAYERS_PER_AREA = 100  # 0x7a1676
RECORD = 28  # 0x7a178c
T2LM_TILES = 16


class Unsupported(Exception):
    """A combination the native client would also reject."""


def t2mk(path):
    data = open(path, "rb").read()
    u = lambda o: struct.unpack_from("<I", data, o)[0]
    f = lambda o: struct.unpack_from("<f", data, o)[0]
    if u(0) != MAGIC_T2MK:
        raise Unsupported(f"{path}: bad T2MK magic {u(0):08x}")
    version = u(4)
    if version > 2:
        raise Unsupported(f"{path}: T2MK version {version} unsupported")
    areas = u(8)
    layers = u(12)
    if version >= 2:
        grid = u(16)
        if grid not in (0, 1):
            raise Unsupported(f"{path}: T2MK grid dimension byte {grid}")
    if areas == 0 or layers == 0:
        raise Unsupported(f"{path}: empty T2MK")
    table = 20 + areas * 4
    records = []
    for i in range(layers):
        p = table + i * RECORD
        records.append(
            {
                "index": i,
                "texture": u(p),
                "u": f(p + 16),
                "v": f(p + 20),
                "layer": u(p + 24),
                "mask_size": u(p + 8),
                "axis_raw": data[p + 12],
                "flags": u(p + 4),
            }
        )
    out = {"path": path, "version": version, "areas": [], "layers": records,
           "layer_count": layers}
    for ai in range(areas):
        p = u(20 + ai * 4)
        n = u(p)
        if n > MAX_LAYERS_PER_AREA:
            raise Unsupported(f"{path}: area {ai} has {n} layers")
        area = {"area": ai, "stack": []}
        for li in range(n):
            r = p + 4 + li * 12
            idx, off, length = u(r), u(r + 4), u(r + 8)
            if idx >= layers:
                raise Unsupported(f"{path}: area {ai} layer {li} index {idx}")
            rec = dict(records[idx])
            rec["area_layer"] = li
            if li == 0:
                rec["mask_bytes"] = 0
                rec["mask_zlib"] = False
            else:
                size = rec["mask_size"]
                if size == 0:
                    raise Unsupported(f"{path}: area {ai} layer {li} has no mask")
                raw = data[off:off + length]
                if len(raw) != length:
                    raise Unsupported(f"{path}: area {ai} layer {li} mask truncated")
                comp = u(16) != 0
                if comp:
                    raw = zlib.decompress(raw)
                if len(raw) != size * size:
                    raise Unsupported(
                        f"{path}: area {ai} layer {li} mask decodes to {len(raw)}"
                    )
                rec["mask_bytes"] = size * size
                rec["mask_zlib"] = comp
            area["stack"].append(rec)
        out["areas"].append(area)
    return out


def t2lm(path):
    data = open(path, "rb").read()
    u = lambda o: struct.unpack_from("<I", data, o)[0]
    if u(0) != MAGIC_T2LM:
        raise Unsupported(f"{path}: bad T2LM magic {u(0):08x}")
    version, tiles, side, dds = u(4), u(8), u(12), u(16)
    if version != 2 or tiles != T2LM_TILES or side != 256:
        raise Unsupported(f"{path}: T2LM v{version} {tiles} tiles of {side}")
    size = u(20)
    offsets = [u(24 + i * 4) for i in range(tiles)]
    return {"path": path, "tiles": tiles, "side": side, "dds": dds,
            "tile_bytes": size, "offsets": offsets}


def survey(paths):
    print(f"files surveyed: {len(paths)}")
    axes = Counter()
    uv = Counter()
    textures = set()
    masks = Counter()
    stacks = Counter()
    comp = Counter()
    per_file = []
    for p in paths:
        t = t2mk(p)
        per_file.append(t)
        textures.update(r["texture"] for r in t["layers"])
        for r in t["layers"]:
            axes[r["axis_raw"]] += 1
            uv[(round(r["u"], 6), round(r["v"], 6))] += 1
            masks[r["mask_size"]] += 1
        for a in t["areas"]:
            stacks[len(a["stack"])] += 1
            for l in a["stack"][1:]:
                comp[l["mask_zlib"]] += 1
        print(
            f"  {p}: v{t['version']} areas={len(t['areas'])} layers={t['layer_count']} "
            f"stacks={sorted(len(a['stack']) for a in t['areas'])}"
        )
    print(f"axis byte histogram (0=x/z, 1=x/-y, 2=z/y): {dict(sorted(axes.items()))}")
    print(f"uv scale histogram (u, v): {dict(uv)}")
    print(f"mask resolution histogram: {dict(sorted(masks.items()))}")
    print(f"layers per area histogram: {dict(sorted(stacks.items()))}")
    print(f"mask compression: {dict(comp)}")
    print(f"distinct texture indices: {len(textures)} -> {sorted(textures)}")
    return per_file


def csv(rows, out):
    with open(out, "w") as fh:
        fh.write("subterrain,area,area_layer,texture,axis,u,v,mask,texture_bytes,mask_bytes,zlib\n")
        for t in rows:
            name = t["path"].rsplit("/", 1)[-1].replace(".t2mk", "")
            for a in t["areas"]:
                for l in a["stack"]:
                    axis = {0: "x/-y", 1: "x/-z", 2: "z/-y"}.get(l["axis_raw"], "?")
                    fh.write(
                        f"{name},{a['area']},{l['area_layer']},{l['texture']},"
                        f"{axis},{l['u']:.6f},{l['v']:.6f},{l['mask_size']},"
                        f"{l.get('mask_bytes', 0)},{l['mask_bytes'] if 'mask_bytes' in l else 0},"
                        f"{int(l.get('mask_zlib', False))}\n"
                    )


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    csv_out = None
    rest = []
    i = 0
    while i < len(argv):
        if argv[i] == "--csv":
            csv_out = argv[i + 1]
            i += 2
            continue
        if argv[i] == "--manifest":
            rest = []
            for line in open(argv[i + 1]).read().splitlines()[1:]:
                name = line.split(",")[0]
                if name.endswith(".t2lm"):
                    print(f"  manifest: {name} lightmap payload, not a T2MK source")
            print("The manifest lists shipped T2LM payloads; pass T2MK files to survey layers.")
            return 0
        rest.append(argv[i])
        i += 1
    rows = survey(rest)
    if csv_out:
        csv(rows, csv_out)
        print(f"wrote {csv_out}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
