#!/usr/bin/env python3
"""Cross-reference the client's shaders.pck against the strings in elementclient.exe.

The 1.4.2 client names most shaders as literal paths in its .rdata. Model materials
instead reach the pixel/vertex programs through `.sdr` descriptors (which themselves
name a program), and skinned/morph programs are assembled with `s_%d.txt`-style
format strings, so a shader that is missing from the literal list is not evidence
that the client never loads it. This tool reports the three sets separately:

    resolved   literal exe reference that exists in the pack
    absent     literal exe reference with no pack entry (a real gap)
    unreferenced  pack entries the exe never names literally

It also extracts the register/constant comment header of a named shader so the
recovered constants can be checked without re-reading the pack by hand.

Usage:
    tools/survey-shader-pack.py                       # coverage report
    tools/survey-shader-pack.py --show ps/water_expensive.txt
    tools/survey-shader-pack.py --json /tmp/coverage.json
Requires google_drive/elementclient.exe and google_drive/shaders.pck.
"""
import argparse
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
sys.path.insert(0, str(ROOT / "tools"))
from pck_lib import directory, read_entry  # noqa: E402

EXE = ROOT / "google_drive" / "elementclient.exe"
PACK = ROOT / "google_drive" / "shaders.pck"

# Anchor on the "shaders\" prefix and allow nested folders. Bare "<x>.txt" strings
# are mostly log text ("failed to load <x>.txt") and config paths, and matching
# them made the report list 60 phantom gaps.
PATH_RE = re.compile(rb"shaders[\\/][A-Za-z0-9_\\/]{1,60}\.(?:txt|sdr|vso|pso)", re.I)
SDR_RE = re.compile(rb"[A-Za-z\\/]{0,20}\.sdr")
# `s_%d.txt` and friends: programs chosen at run time from a numeric quality/mode.
FORMAT_RE = re.compile(rb"[A-Za-z_/\\]{0,24}_%[ds][^\x00]{0,24}\.txt")


def norm(path):
    return path.replace("\\", "/").lower()


def exe_references(blob):
    # Only shader paths: configs/*.txt and log strings live in the same .rdata.
    literal = {norm(m.group().decode("latin-1")) for m in PATH_RE.finditer(blob)}
    # Bare "Body.sdr" strings sit in Shaders\, so give them the folder the pack uses.
    for m in SDR_RE.finditer(blob):
        name = norm(m.group().decode("latin-1"))
        if name != ".sdr":
            literal.add("shaders/" + name)
    formats = sorted({m.group().decode("latin-1") for m in FORMAT_RE.finditer(blob)})
    return literal, formats


def survey():
    if not EXE.exists() or not PACK.exists():
        raise SystemExit(f"missing input: {EXE} / {PACK}")
    blob = EXE.read_bytes()
    _, entries = directory(str(PACK))
    pack = {e[0].lower(): e for e in entries}
    by_name = {}
    for key in pack:
        by_name.setdefault(key.split("/")[-1], key)
    literal, formats = exe_references(blob)

    resolved, absent = [], []
    for ref in sorted(literal):
        if ref in pack:
            resolved.append(ref)
        elif ref.split("/")[-1] in by_name:
            # Different directory in the pack than in the exe string.
            resolved.append(ref)
            by_name.pop(ref.split("/")[-1], None)
        else:
            absent.append(ref)

    named = set()
    for ref in resolved:
        named.add(ref)
        named.add(by_name.get(ref.split("/")[-1], ref))
    unreferenced = sorted(p for p in pack if p not in named and p.split("/")[-1] not in
                          {r.split("/")[-1] for r in resolved})
    return {
        "pack_version_entries": len(entries),
        "resolved": sorted(set(resolved)),
        "absent": absent,
        "unreferenced": unreferenced,
        "runtime_format_strings": formats,
    }


def show(name):
    """Print a shader's leading comment block plus every def/constant line."""
    _, entries = directory(str(PACK))
    want = name.lower()
    entry = next((e for e in entries if e[0].lower().endswith(want)), None)
    if entry is None:
        raise SystemExit(f"no pack entry ending in {name!r}")
    text = read_entry(str(PACK), entry).decode("latin-1").replace("\r\n", "\n")
    for line in text.split("\n"):
        stripped = line.strip()
        if not stripped:
            continue
        if stripped.startswith(";") or stripped.startswith("//") or re.match(
                r"^(?:def\s+c|ps\.|vs\.)", stripped):
            print(line)
        elif "oFog" in stripped or "oD0" in stripped or "oD1" in stripped:
            print(line)


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--show", metavar="NAME", help="print one shader's header and constants")
    ap.add_argument("--json", metavar="PATH", help="write the coverage report as JSON")
    a = ap.parse_args()
    if a.show:
        show(a.show)
        return 0
    report = survey()
    if a.json:
        pathlib.Path(a.json).write_text(json.dumps(report, indent=1, ensure_ascii=False) + "\n")
    print(f"pack entries: {report['pack_version_entries']}")
    print(f"resolved literal references: {len(report['resolved'])}")
    print(f"absent  literal references: {len(report['absent'])}")
    for a_ in report["absent"]:
        print("   ABSENT", a_)
    print(f"runtime format strings: {report['runtime_format_strings']}")
    print(f"pack entries the exe never names literally: {len(report['unreferenced'])}")
    for u in report["unreferenced"]:
        print("   ", u)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
