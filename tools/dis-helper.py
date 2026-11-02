#!/usr/bin/env python3
"""Read disassembly windows from /tmp/dis/text.asm by virtual address.

Usage:
  dis-helper.py window <va-start> [count]     # print `count` instructions from va
  dis-helper.py range <va-start> <va-end>     # print instructions in [start,end)
  dis-helper.py xref-data <va>                # find raw file offsets holding struct.pack('<I', va)
  dis-helper.py xref-code <va>                # find code references to va in the disassembly
"""
import re
import struct
import sys

ASM = "/tmp/dis/text.asm"
EXE = "google_drive/elementclient.exe"

LINE = re.compile(r"^\s+([0-9a-f]+):\t(?:[0-9a-f]{2} )+\s*\t(.*)$")

# section table: (name, va, vsize, raw, rsize)
SECTIONS = [
    (".text", 0x401000, 0x4D3F25, 0x1000, 0x4D4000),
    (".rdata", 0x8D5000, 0x77AAE, 0x4D5000, 0x78000),
    (".data", 0x94D000, 0xA42E0, 0x54D000, 0x67000),
    (".tls", 0x9F2000, 0x1044, 0x5B4000, 0x2000),
    (".data1", 0x9F4000, 0x8E0, 0x5B6000, 0x1000),
    (".rsrc", 0x9F5000, 0x1108, 0x5B7000, 0x2000),
]


def va_to_off(va):
    for _name, sva, vsize, raw, rsize in SECTIONS:
        if sva <= va < sva + vsize:
            off = va - sva + raw
            if off < rsize:
                return off
    return None


def off_to_va(off):
    for _name, sva, vsize, raw, rsize in SECTIONS:
        if raw <= off < raw + rsize:
            va = off - raw + sva
            if va < sva + vsize:
                return va
    return None


def index_lines():
    """Return list of (va, text) parsed lines and dict va->line number."""
    lines = []
    with open(ASM, "r", errors="replace") as fh:
        for ln in fh:
            m = LINE.match(ln)
            if m:
                lines.append((int(m.group(1), 16), ln.rstrip("\n")))
    return lines


def cmd_window(args):
    va = int(args[0], 16)
    count = int(args[1]) if len(args) > 1 else 40
    lines = index_lines()
    # binary search
    lo = 0
    hi = len(lines)
    while lo < hi:
        mid = (lo + hi) // 2
        if lines[mid][0] < va:
            lo = mid + 1
        else:
            hi = mid
    for i in range(lo, min(lo + count, len(lines))):
        print(lines[i][1])


def cmd_range(args):
    start = int(args[0], 16)
    end = int(args[1], 16)
    lines = index_lines()
    for v, text in lines:
        if start <= v < end:
            print(text)


def cmd_xref_data(args):
    va = int(args[0], 16)
    pat = struct.pack("<I", va)
    data = open(EXE, "rb").read()
    pos = 0
    out = []
    while True:
        pos = data.find(pat, pos)
        if pos < 0:
            break
        ref = off_to_va(pos)
        out.append((pos, ref))
        pos += 1
    for off, ref in out:
        if ref is None:
            print(f"file offset {off:#x}: no VA")
        else:
            sect = next((n for n, sva, vs, r, rs in SECTIONS if sva <= ref < sva + vs), "?")
            print(f"file offset {off:#x}: VA {ref:#x} in {sect}")
    print(f"total {len(out)} occurrences")


def cmd_xref_code(args):
    va = int(args[0], 16)
    needle = f"{va:x}"
    with open(ASM, "r", errors="replace") as fh:
        for ln in fh:
            m = LINE.match(ln)
            if not m:
                continue
            text = m.group(2)
            if needle in text and ("call" in text or "jmp" in text or "push" in text or "mov" in text):
                print(ln.rstrip())


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    cmd = sys.argv[1]
    rest = sys.argv[2:]
    {"window": cmd_window, "range": cmd_range, "xref-data": cmd_xref_data,
     "xref-code": cmd_xref_code}[cmd](rest)
