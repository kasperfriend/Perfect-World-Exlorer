#!/usr/bin/env python3
"""Census every terrain source in a retail `textures.pck` (P2.07).

The point of this tool is that the survey is reproducible from the archive the
client actually ships, not from a sample. It applies the same acceptance rules as
`src/terrain_sampling.rs::parse_terrain_dds` and `src/dxt.rs::decode_chain`, and it
can emit the test vectors that pin the Rust DXT decoder to an independent
implementation.

Usage:
    terrain-format-survey.py TEXTURES_PCK [--emit-test-vectors FILE]

With no `--emit-test-vectors` it prints:

  * the per-format census of `textures/maps/` (the terrain layer sources),
  * the dimension x authored-mip-count table,
  * the result of running the production acceptance rules over every source,
  * the full-archive census for context.

Every entry is named, so each survey case records the archive member it came from.
"""
import argparse
import collections
import hashlib
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pck_lib as pck  # noqa: E402  (port of src/pck.rs)

BLOCK = {0x31545844: 8, 0x32545844: 16, 0x33545844: 16, 0x34545844: 16, 0x35545844: 16}
NAME = {0x31545844: "DXT1", 0x32545844: "DXT2", 0x33545844: "DXT3",
        0x34545844: "DXT4", 0x35545844: "DXT5"}


def parse_terrain_dds(b):
    """Mirror of `parse_terrain_dds`. Returns (info, None) or (None, error)."""
    if len(b) < 128:
        return None, "truncated terrain DDS header"
    w = lambda i: struct.unpack_from("<I", b, i)[0]
    if b[:4] != b"DDS " or w(4) != 124 or w(76) != 32:
        return None, "invalid terrain DDS header"
    if w(80) & 0x4 == 0:
        return None, "not FourCC-compressed; 0x7f7f74 only creates DXT textures"
    if w(112) != 0:
        return None, "cube map"
    fourcc = w(84)
    if fourcc not in BLOCK:
        return None, "unsupported FourCC 0x%08x" % fourcc
    height, width = w(12), w(16)
    if not width or not height or width > 16384 or height > 16384:
        return None, "dimensions out of bounds"
    authored = max(w(28), 1)
    full = max(width, height).bit_length()
    if authored > full:
        return None, "declares %d mip levels but %d fit" % (authored, full)
    offset = 128
    for level in range(authored):
        lw, lh = max(width >> level, 1), max(height >> level, 1)
        offset += ((lw + 3) // 4) * ((lh + 3) // 4) * BLOCK[fourcc]
        if offset > len(b):
            return None, "truncated mip level %d" % level
    return dict(fourcc=fourcc, width=width, height=height, authored=authored,
                full=full, consumed=offset, exact=(offset == len(b))), None


def rgb565(c):
    r, g, bb = (c >> 11) & 0x1F, (c >> 5) & 0x3F, c & 0x1F
    return ((r << 3) | (r >> 2), (g << 2) | (g >> 4), (bb << 3) | (bb >> 2))


def decode(b, fourcc, w, h):
    """Independent DXT1/DXT3/DXT5 decoder, used to generate and check test vectors."""
    bs = BLOCK[fourcc]
    out = bytearray(w * h * 4)
    bw, bh = (w + 3) // 4, (h + 3) // 4
    for by in range(bh):
        for bx in range(bw):
            o = (by * bw + bx) * bs
            blk = b[o:o + bs]
            if fourcc in (0x32545844, 0x33545844):
                ca, aplane = 8, blk[:8]
            else:
                ca, aplane = 0, None
            c0, c1 = struct.unpack_from("<HH", blk, ca)
            idx = struct.unpack_from("<I", blk, ca + 4)[0]
            A, B = rgb565(c0), rgb565(c1)
            if fourcc == 0x31545844 and c0 <= c1:
                pal = [A, B, tuple((A[k] + B[k]) // 2 for k in range(3)), (0, 0, 0)]
                pal_a = [255, 255, 255, 0]
            else:
                pal = [A, B, tuple((2 * A[k] + B[k]) // 3 for k in range(3)),
                       tuple((A[k] + 2 * B[k]) // 3 for k in range(3))]
                pal_a = [255] * 4
            atab = None
            if fourcc in (0x34545844, 0x35545844):
                a0, a1 = blk[0], blk[1]
                atab = [a0, a1] + [0] * 6
                if a0 > a1:
                    for i in range(6):
                        atab[2 + i] = ((6 - i) * a0 + (1 + i) * a1) // 7
                else:
                    for i in range(4):
                        atab[2 + i] = ((4 - i) * a0 + (1 + i) * a1) // 5
                    atab[6], atab[7] = 0, 255
                abits = int.from_bytes(blk[2:8], "little")
            for y in range(4):
                for x in range(4):
                    px, py = bx * 4 + x, by * 4 + y
                    if px >= w or py >= h:
                        continue
                    sel = (idx >> (2 * (y * 4 + x))) & 3
                    d = (py * w + px) * 4
                    out[d:d + 3] = bytes(pal[sel])
                    if aplane is not None:
                        n = y * 4 + x
                        byte = aplane[n // 2]
                        out[d + 3] = ((byte >> 4) if n % 2 else (byte & 0xF)) * 17
                    elif atab is not None:
                        out[d + 3] = atab[(abits >> (3 * (y * 4 + x))) & 7]
                    else:
                        out[d + 3] = pal_a[sel]
    return bytes(out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("pck")
    ap.add_argument("--emit-test-vectors")
    args = ap.parse_args()

    version, entries = pck.directory(args.pck)
    total = os.path.getsize(args.pck)
    print("archive: %s" % args.pck)
    print("version: %08x   entries: %d   file: %d bytes" %
          (version, len(entries), total))
    print("uncompressed total: %d bytes" % sum(e[2] for e in entries))

    def ext(name):
        return os.path.splitext(name)[1].lower()

    print("\n== whole archive ==")
    print("extensions: %s" % dict(collections.Counter(ext(e[0]) for e in entries)))
    all_dds = [e for e in entries if ext(e[0]) == ".dds"]
    all_fmt, all_bad = collections.Counter(), []
    for e in all_dds:
        info, err = parse_terrain_dds(pck.read_entry(args.pck, e))
        if err:
            # Non-FourCC DDS are legitimate for non-terrain art, so record them by
            # kind rather than as failures.
            all_fmt["non-FourCC"] += 1
            continue
        all_fmt[NAME[info["fourcc"]]] += 1
    print("dds entries: %d" % len(all_dds))
    print("formats: %s" % dict(all_fmt))

    maps = [e for e in entries if e[0].startswith("textures/maps/")]
    terrain = [e for e in maps if ext(e[0]) == ".dds"]
    print("\n== terrain layer sources (textures/maps/*.dds) ==")
    print("entries: %d" % len(terrain))
    ok_rows, fails = [], []
    for e in terrain:
        raw = pck.read_entry(args.pck, e)
        info, err = parse_terrain_dds(raw)
        if err:
            fails.append((e[0], err))
        else:
            info["name"] = e[0]
            info["bytes"] = len(raw)
            ok_rows.append(info)
    print("accepted by parse_terrain_dds: %d" % len(ok_rows))
    print("rejected: %d %s" % (len(fails), fails[:5]))

    print("\nformat x dimensions x authored mip levels:")
    table = collections.Counter(
        (NAME[r["fourcc"]], "%dx%d" % (r["width"], r["height"]), r["authored"])
        for r in ok_rows)
    print("%-6s %-10s %-9s %s" % ("fmt", "dims", "authored", "count"))
    for (f, d, m), c in sorted(table.items()):
        print("%-6s %-10s %-9d %d" % (f, d, m, c))

    levels = collections.Counter(r["authored"] for r in ok_rows)
    full_chain = sum(1 for r in ok_rows if r["authored"] == r["full"])
    print("\nauthored levels: %s" % dict(sorted(levels.items())))
    print("complete power-of-two chains: %d of %d" % (full_chain, len(ok_rows)))
    print("payload exactly 128 + sum(levels): %d of %d" %
          (sum(1 for r in ok_rows if r["exact"]), len(ok_rows)))
    print("sources needing client-generated mips (<=1 authored level): %d" %
          sum(1 for r in ok_rows if r["authored"] <= 1))

    if args.emit_test_vectors:
        picks = []
        for fourcc in (0x31545844, 0x33545844):
            for dims in ((256, 256), (512, 512), (256, 512), (1024, 1024)):
                for e in terrain:
                    raw = pck.read_entry(args.pck, e)
                    info, err = parse_terrain_dds(raw)
                    if not err and info["fourcc"] == fourcc and \
                            (info["width"], info["height"]) == dims:
                        picks.append((e, info, raw))
                        break
        cases = []
        for e, info, raw in picks:
            w, h = info["width"], info["height"]
            n = ((w + 3) // 4) * ((h + 3) // 4) * BLOCK[info["fourcc"]]
            src = raw[128:128 + n]
            cases.append(dict(name=e[0], fourcc=info["fourcc"], w=w, h=h,
                              src=src.hex(),
                              expect=hashlib.sha256(
                                  decode(src, info["fourcc"], w, h)).hexdigest()))
        import json
        json.dump(cases, open(args.emit_test_vectors, "w"), indent=0)
        print("\nwrote %d test vectors to %s" %
              (len(cases), args.emit_test_vectors))


if __name__ == "__main__":
    main()
