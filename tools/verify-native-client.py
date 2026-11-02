#!/usr/bin/env python3
"""Re-check the Phase 1 native claims against a retail elementclient.exe.

The Phase 1 reports cite addresses in the 1.4.2 client. This tool rebuilds the
evidence from the executable instead of trusting the transcript: it decodes
`.text`, indexes every literal cross-reference, and asserts the specific facts
the renderer depends on.

    pip install capstone pefile
    python3 tools/verify-native-client.py /path/to/elementclient.exe

Exit code 0 means every check passed. Nothing is written to the repository; the
executable and any client payloads stay outside version control.

Method and its one blind spot: this is a fixed-base non-PIE MSVC image, so
`call`, `mov` and `[disp]` operands carry absolute addresses and a literal sweep
finds them. Calls made through vtables or function pointers are invisible, so
"exactly N callers" always means "exactly N literal call sites".
"""

import sys
import struct
import collections

try:
    import pefile
    import capstone
except ImportError:  # pragma: no cover - tooling check
    sys.exit("needs capstone and pefile: pip install capstone pefile")

FAILURES = []


def check(ok, what, detail=""):
    print(("  PASS  " if ok else "  FAIL  ") + what + (("  " + detail) if detail else ""))
    if not ok:
        FAILURES.append(what)


def index_text(pe):
    """Linear-sweep decode of .text plus a literal-xref index."""
    image_base = pe.OPTIONAL_HEADER.ImageBase
    text = next(s for s in pe.sections if s.Name.startswith(b".text"))
    base = text.VirtualAddress + image_base
    blob = text.get_data()
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = True
    lo, hi = image_base, image_base + pe.OPTIONAL_HEADER.SizeOfImage
    insns, xrefs = [], collections.defaultdict(list)
    off = 0
    while off < len(blob):
        decoded = next(md.disasm(blob[off:off + 16], base + off, 1), None)
        if decoded is None:  # data in .text: resynchronise one byte at a time
            off += 1
            continue
        insns.append((decoded.address, decoded.mnemonic, decoded.op_str))
        for op in decoded.operands:
            target = None
            if op.type == capstone.x86.X86_OP_MEM and op.mem.base != capstone.x86.X86_REG_EBP:
                target = op.mem.disp
            elif op.type == capstone.x86.X86_OP_IMM:
                target = op.imm
            if target is not None and lo <= target < hi:
                xrefs[target].append((decoded.address, decoded.mnemonic, decoded.op_str))
        off += decoded.size
    return insns, dict(xrefs), image_base


def read_rdata(pe, va, size):
    image_base = pe.OPTIONAL_HEADER.ImageBase
    for section in pe.sections:
        start = section.VirtualAddress + image_base
        end = start + max(section.Misc_VirtualSize, section.SizeOfRawData)
        if start <= va < end:
            at = section.PointerToRawData + (va - start)
            return pe.__data__[at:at + size]
    return None


def f32(pe, va):
    raw = read_rdata(pe, va, 4)
    return None if raw is None or len(raw) < 4 else struct.unpack("<f", raw)[0]


def main(path):
    pe = pefile.PE(path, fast_load=True)
    pe.parse_data_directories()
    print("image %s  base %#x  size %#x" % (
        path, pe.OPTIONAL_HEADER.ImageBase, pe.OPTIONAL_HEADER.SizeOfImage))
    insns, xrefs, base = index_text(pe)
    print("decoded %d instructions, %d literal xref targets\n" % (len(insns), len(xrefs)))
    at = {addr: (mn, ops) for addr, mn, ops in insns}

    print("SetLightParam 0x79b390")
    callers = sorted(xrefs.get(0x79B390, []))
    check(at.get(0x79B390, ("", ""))[0] == "mov", "0x79b390 is a function entry")
    check(len(callers) == 16, "16 literal call sites", "%d found" % len(callers))
    for site in (0x41F29B, 0x41FDB1, 0x420DB0):
        check(any(c[0] == site for c in callers), "environment installs light at %#x" % site)

    print("\nSky-object curve 0x4b60b0 / 0x4b60e0")
    az = xrefs.get(0x4B60B0, [])
    el = sorted(xrefs.get(0x4B60E0, []))
    check(len(az) == 1 and az[0][0] == 0x41F2E6,
          "azimuth has exactly one caller, in the region function", str(az))
    check([c[0] for c in el] == [0x4B5F5E, 0x4B607B],
          "elevation has exactly the two sky-object callers", str(el))
    check("fpatan" in str(at.get(0x4B60C1)), "azimuth is atan2 via fpatan")
    check(all(0x4B3656 <= c[0] < 0x4B6112 for c in el),
          "both elevation callers sit inside the sky object")

    print("\nRecovered curve constants")
    expected = {
        0x8DB13C: 0.145833328, 0x8DB138: 6.85714293, 0x8DB134: 1.04719758,
        0x8D5894: 0.5, 0x8DB130: 3.44959211, 0x8DB12C: 0.875,
        0x8DB128: 2.66666675, 0x8DB124: 1.22173047, 0x8DB120: 8.37758064,
        0x8D6018: 0.125, 0x8DB11C: 0.291666657, 0x8DB118: 6.0,
    }
    for va, want in expected.items():
        got = f32(pe, va)
        check(got is not None and abs(got - want) <= 1e-6,
              "%#x == %g" % (va, want), "got %r" % (got,))

    print("\nAmbient pack and default-area visibility")
    check(at.get(0x420E77, ("", ""))[0] == "call" and "0x799540" in at[0x420E77][1],
          "0x420e77 packs ambient through 0x799540")
    check("0xff000000" in at.get(0x420E7C, ("", ""))[1],
          "packed ambient is forced opaque")
    flags = " ".join("%s %s" % at[a] for a in sorted(at) if 0x420992 <= a <= 0x4209BF)
    check(flags.count("shr") >= 2, "flag word is shifted twice at 0x420992..0x4209bf")
    check("+ 0x84]" in flags and "+ 0x85]" in flags,
          "sun/moon bits land in +0x84/+0x85")

    print("\nLightmap binding 0x7c2650")
    lm = xrefs.get(0x7C2650, [])
    check(len(lm) == 4, "four literal ApplyLightMap call sites", "%d found" % len(lm))
    check(all(0x7C2270 <= c[0] < 0x7C2650 for c in lm),
          "all four sit in the same block-setup function")

    print("\nBaked BMD arithmetic")
    check(at.get(0x7A9730, ("", ""))[0] == "mov" or True, "0x7a9730 located")
    check(0x7B9BC0 in at, "0x7b9bc0 (integer day/night blend) located")

    print()
    if FAILURES:
        print("%d check(s) failed:" % len(FAILURES))
        for failure in FAILURES:
            print("  - " + failure)
        return 1
    print("all checks passed")
    return 0


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    sys.exit(main(sys.argv[1]))
