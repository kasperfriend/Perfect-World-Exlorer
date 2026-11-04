#!/usr/bin/env python3
"""Dump everything the sky / flare / sky-set recovery needs from a retail client.

The sky work is blocked on questions whose answers are inside the client files,
not inside this repository:

  * the per-flare quad sizes and offsets (`CECSunMoon::Init` 0x4b3650 and the
    per-frame fill 0x4b4290-0x4b5ef4) - the viewer's lens flare is smaller than
    the retail one;
  * the object drawn over the 38.16-72.5 degree band (`0x799f88`,
    `[A3DSky+0x54]`) and whether the sky sphere itself turns with the clock
    (`0x79a040`, `0x79a0e0`);
  * which sky set a zone asks for (`0x4203e0` and the AutoFamily configuration),
    because Tideborn regions show a starry night sky in retail;
  * which of the 59 `textures/sky/*` sets actually contains stars.

This tool answers all four without a GPU and without running the game: it reads
the two Angelica archives and disassembles the named windows of the client with
Capstone.  It is meant to run on a machine (or CI runner) that has the staged
client files, and it writes plain text so the results can be reviewed and
committed.

Usage:
    tools/recover-sky-client.py --exe elementclient.exe \
        --configs configs.pck --shaders shaders.pck --textures textures.pck \
        --out Findings/recovery

Requires: capstone, pillow (both optional for the archive-only parts).
"""
import argparse
import importlib.util
import os
import pathlib
import re
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


# `pe.py` maps file offsets to virtual addresses; `extract-angelic-pck.py`
# reads the two Angelica archives shipped beside the client.
PE = _load("client_pe", HERE / "pe.py")
PCK = _load("angelica_pck", HERE / "extract-angelic-pck.py")

# Windows of the client that answer the open sky questions.  Each window is
# named after the question it settles so the dump stays readable.
WINDOWS = {
    "01-celestial-init": (0x4B2E60, 0x4B3E00),
    "02-sun-moon-render": (0x4B3D00, 0x4B4290),
    "03-flare-fill": (0x4B4290, 0x4B5F00),
    "04-celestial-clock": (0x4B5F00, 0x4B6100),
    "05-sky-band-object": (0x799E00, 0x79A040),
    "06-sky-tick": (0x79A040, 0x79A330),
    "07-sky-builders": (0x79A330, 0x79A900),
    "08-sky-textures": (0x79AC00, 0x79B200),
    "09-sky-set-select": (0x4203E0, 0x420700),
    "10-sky-timers": (0x79B150, 0x79B1D0),
}

# Archive entries whose text can name a sky set, a flare program or a zone rule.
TEXT_HINT = re.compile(rb"sky|flare|sunshine|autofamily|autoscene", re.I)
NAME_HINT = re.compile(r"sky|flare|solar|scene", re.I)


def disassemble(pe, start, end, text_strings):
    """Disassemble [start, end) and annotate image addresses with their C string."""
    import capstone

    code_off = pe.va2off(start)
    if code_off is None:
        return "; window %#x is not inside the image\n" % start
    blob = pe.d[code_off:pe.va2off(end) or len(pe.d)]
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = False
    out = []
    for insn in md.disasm(blob, start):
        note = ""
        for token in re.findall(r"0x[0-9a-f]{5,8}", insn.op_str):
            note = annotate(pe, text_strings, int(token, 16))
            if note:
                break
        out.append("%08x: %s%s" % (insn.address, "%-40s" % ("%s %s" % (insn.mnemonic, insn.op_str)), note))
    return "\n".join(out) + "\n"


def annotate(pe, text_strings, value):
    """Name what an image address operand points at: a string or a float constant."""
    if value in text_strings:
        return "  ; %r" % text_strings[value]
    offset = pe.va2off(value)
    if offset is None or offset + 4 > len(pe.d):
        return ""
    raw = pe.d[offset:offset + 4]
    text = raw.split(b"\0")[0]
    if len(text) >= 4 and all(0x20 <= c < 0x7F for c in text):
        return "  ; %r" % text.decode("latin1")
    number = struct.unpack("<f", raw)[0]
    if number == 0. or 1e-6 < abs(number) < 1e6:
        return "  ; f32 %g" % number
    return ""


def image_strings(pe, limit=40000):
    """Map virtual address -> printable C string for every short string in the image."""
    found = {}
    for name, vaddr, vsize, rawptr, rawsize in pe.secs:
        data = pe.d[rawptr:rawptr + rawsize]
        for match in re.finditer(rb"[\x20-\x7e]{4,}\x00", data):
            if len(found) >= limit:
                return found
            address = pe.base + vaddr + match.start()
            found[address] = match.group(0)[:-1].decode("latin1")
    return found


def archive_dump(blob, tag, out, names=None, texts=None):
    """Write an archive listing plus any text entry whose content looks related."""
    info = PCK.header(blob)
    entries = PCK.directory(blob)
    lines = [
        "# %s: %d entries, version %08x, field %08x, %r"
        % (tag, len(entries), info["version"], info["unknown"], info["description"])
    ]
    for name, offset, size, compressed in entries:
        lines.append("%-62s %8d %8d %#08x" % (name, size, compressed, offset))
    (out / ("%s-entries.txt" % tag)).write_text("\n".join(lines) + "\n")

    wanted = []
    for entry in entries:
        name = entry[0]
        if names and NAME_HINT.search(name):
            wanted.append(entry)
            continue
        if texts and entry[2] <= 262144:
            data = PCK.read_entry(blob, entry)
            if TEXT_HINT.search(data) or TEXT_HINT.search(name.encode("latin1", "replace")):
                if data.count(b"\0") < len(data) // 8:
                    wanted.append(entry)
    if not wanted:
        return 0
    chunks = []
    for entry in wanted:
        data = PCK.read_entry(blob, entry)
        try:
            text = data.decode("gbk")
        except UnicodeDecodeError:
            text = data.decode("latin1")
        chunks.append("===== %s (%d bytes) =====\n%s" % (entry[0], entry[2], text.rstrip() + "\n"))
    (out / ("%s-text.txt" % tag)).write_text("\n".join(chunks))
    return len(wanted)


def sky_sets(blob, out):
    """Group `textures/sky/*` members by folder and score each image for stars."""
    entries = PCK.directory(blob)
    folders = {}
    for entry in entries:
        name = entry[0]
        lowered = name.lower()
        if not lowered.startswith("textures/sky/"):
            continue
        rest = name[len("textures/sky/"):]
        folder, _, member = rest.rpartition("/")
        folders.setdefault(folder or ".", []).append(entry)

    lines = ["# %d sky folders, %d files" % (len(folders), sum(len(v) for v in folders.values()))]
    scores = []
    try:
        from PIL import Image
        import io
    except ImportError:
        Image = None
    for folder in sorted(folders):
        lines.append("[%s]" % folder)
        for entry in sorted(folders[folder], key=lambda e: e[0]):
            lines.append("  %-44s %8d %8d" % (entry[0].rsplit("/", 1)[-1], entry[2], entry[3]))
            if Image is None or not entry[0].lower().endswith((".bmp", ".tga")):
                continue
            try:
                image = Image.open(io.BytesIO(PCK.read_entry(blob, entry))).convert("RGB")
            except Exception as error:  # noqa: BLE001 - a survey must not stop on one image
                scores.append((folder, entry[0], 0, 0., 0., str(error)))
                continue
            if image.width * image.height > 4 * 1024 * 1024:
                continue
            pixels = list(image.getdata())
            luma = [(r * 299 + g * 587 + b * 114) // 1000 for r, g, b in pixels]
            mean = sum(luma) / len(luma)
            # A star is a bright pixel with no bright neighbourhood: count the
            # pixels far above the median and report the top percentile too.
            ordered = sorted(luma)
            median = ordered[len(ordered) // 2]
            specks = sum(1 for v in luma if v > median + 60)
            scores.append((folder, entry[0].rsplit("/", 1)[-1], len(luma),
                           mean, float(specks) * 1000. / len(luma), median))
    (out / "textures-sky-folders.txt").write_text("\n".join(lines) + "\n")

    scores.sort(key=lambda row: -row[4])
    table = ["\t".join(("folder", "file", "pixels", "mean", "specks_per_1000", "median"))]
    for row in scores:
        table.append("\t".join(str(value) for value in row))
    (out / "textures-sky-stars.tsv").write_text("\n".join(table) + "\n")
    return len(folders)


def exe_strings(pe, out):
    """Every `Textures\\Sky` style string in the client, with its address."""
    strings = image_strings(pe)
    hits = [(address, text) for address, text in sorted(strings.items())
            if NAME_HINT.search(text)]
    (out / "exe-sky-strings.txt").write_text(
        "\n".join("%08x %s" % (address, text) for address, text in hits) + "\n")
    return strings, len(hits)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--exe", help="elementclient.exe")
    parser.add_argument("--configs", help="configs.pck")
    parser.add_argument("--shaders", help="shaders.pck")
    parser.add_argument("--textures", help="textures.pck")
    parser.add_argument("--out", default="Findings/recovery")
    args = parser.parse_args()

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    report = []

    if args.configs:
        blob = pathlib.Path(args.configs).read_bytes()
        kept = archive_dump(blob, "configs", out, texts=True)
        report.append("configs.pck: %d entries, %d related text entries" % (len(PCK.directory(blob)), kept))
    if args.shaders:
        blob = pathlib.Path(args.shaders).read_bytes()
        kept = archive_dump(blob, "shaders", out, texts=True)
        report.append("shaders.pck: %d entries, %d related text entries" % (len(PCK.directory(blob)), kept))
    if args.textures:
        blob = pathlib.Path(args.textures).read_bytes()
        report.append("textures.pck: %d sky folders" % sky_sets(blob, out))
    if args.exe:
        pe = PE.PE(args.exe)
        strings, hits = exe_strings(pe, out)
        report.append("elementclient.exe: base %#x, %d sections, %d sky strings" % (
            pe.base, len(pe.secs), hits))
        for name, (start, end) in WINDOWS.items():
            text = disassemble(pe, start, end, strings)
            (out / ("asm-%s.txt" % name)).write_text(
                "# %s  %#x-%#x\n%s" % (name, start, end, text))
            report.append("  %s: %d lines" % (name, text.count("\n")))

    (out / "SUMMARY.txt").write_text("\n".join(report) + "\n")
    print("\n".join(report))


if __name__ == "__main__":
    main()
