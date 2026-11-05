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
    "01b-celestial-init-tail": (0x4B392C, 0x4B4290),
    "01c-flare-stream-builder": (0x4B6250, 0x4B7000),
    "02-sun-moon-render": (0x4B3D00, 0x4B4290),
    "03-flare-fill": (0x4B4290, 0x4B5F00),
    "04-celestial-clock": (0x4B5F00, 0x4B6100),
    "05-sky-band-object": (0x799E00, 0x79A040),
    "06-sky-tick": (0x79A040, 0x79A330),
    "07-sky-builders": (0x79A330, 0x79A900),
    "08-sky-textures": (0x79AC00, 0x79B200),
    "09-sky-set-select": (0x4203E0, 0x420700),
    "10-sky-timers": (0x79B150, 0x79B1D0),
    "11-sky-ctor-init": (0x799570, 0x799A40),
    "13-sky-matrix-helpers": (0x789E60, 0x789FC0),
    "14-sky-draws": (0x799A40, 0x799E00),
    "15-flare-caller": (0x4B5600, 0x4B5B60),
    "16-flare-helper": (0x450900, 0x450C40),
    "12-sky-load": (0x951900, 0x951C80),
    # The star question: `A3DSkySphere::TransSky` and the world/scene sky name.
    "17-sky-trans": (0x79B000, 0x79B600),
    # `CECWorld::LoadSky`, the caller of the `Textures\Sky\%s` template.
    "18-world-loadsky": (0x951300, 0x951940),
    # The 54-point occlusion mask that gates the flare, and the flare alpha cover.
    "19-flare-occlusion": (0x4B5A00, 0x4B6100),
    # The flare's sprite metric: `0x4b6270` copies six dwords from `arg1+0x14`
    # and scales every sprite by `[arg1+0x24]` as an integer. This is the getter
    # that produces `arg1`.
    "22-flare-metric-source": (0x4360A0, 0x436220),
    # `A3DSky::Tick` sets the drift matrix through this call, so its identity
    # decides whether the drift is a texture transform or a world transform.
    "23-device-stage-matrix": (0x779D20, 0x779E60),
    # The 3x3/4x4 helpers `A3DSky::TickMatrix` composes the drift from:
    # 0x787170 multiply, 0x787240 translation, 0x787260 rotation, 0x787460 copy.
    "24-matrix-math": (0x787100, 0x787300),
}

# Archive entries whose text can name a sky set, a flare program or a zone rule.
TEXT_HINT = re.compile(rb"sky|flare|sunshine|autofamily|autoscene", re.I)
NAME_HINT = re.compile(r"sky|flare|solar|scene", re.I)


# Windows are chosen from call sites and string cross-references, so many of
# them open in the middle of an instruction; the dump is worth little until it
# starts on a real function boundary, and every earlier round mis-read an
# operand because of that.
#
# This is an optimised MSVC build with no frame pointers - `push ebp; mov ebp,
# esp` does not appear - so a boundary is found the way the linker makes it: the
# tail of the previous function (`ret` / `ret imm16`) followed by `int3` or `nop`
# padding.
BOUNDARY = re.compile(rb"(?:\xc3|\xc2..)[\xcc\x90]{2,}")
BOUNDARY_SCAN = 0x300
# How far before a window to look for the padding that ends the previous
# function. Windows are picked from call sites, so the enclosing function almost
# always starts earlier than the window does.
BACK_SCAN = 0x2000


def resync(code_off, blob):
    """Offset (possibly negative) of the nearest function boundary.

    The last padding run before the window is the start of the function that
    contains the window's opening address; with no such run close behind, the
    window is mid-instruction and the first boundary ahead is used instead.
    """
    head = blob_head(code_off)
    matches = list(BOUNDARY.finditer(head))
    if matches:
        return matches[-1].end() - len(head)
    match = BOUNDARY.search(blob[:BOUNDARY_SCAN])
    return match.end() if match else 0


def blob_head(code_off):
    """The BACK_SCAN bytes of image that precede a window."""
    return PE_DATA[0][max(0, code_off - BACK_SCAN):code_off]


# The image bytes `resync` searches behind a window; set once per dump run.
PE_DATA = [b""]


# A window can only show what someone already suspected. These are the whole
# bodies instead: the entire sky module (ceiling, sphere, tick, textures) and
# every function anywhere in the image that names a sky, flare, sun, moon, star
# or cloud string. That is how the mesh, the UV maths and the sprite tables are
# found without guessing a start address first.
MODULES = {
    "20-sky-module": (0x799500, 0x79C400),
}

XREF_HINT = re.compile(r"sky|flare|sun|moon|shine|cloud|star|night|day", re.I)
FUNCTION_BACK_SCAN = 0x8000
FUNCTION_LIMIT = 0x3000
FUNCTION_HEADER = "# %#x-%#x  (%d bytes)%s\n"


def function_start(pe, off):
    """Offset of the function boundary nearest behind `off`."""
    head = pe.d[max(0, off - FUNCTION_BACK_SCAN):off]
    matches = list(BOUNDARY.finditer(head))
    if not matches:
        return None
    return off - (len(head) - matches[-1].end())


def function_extent(pe, start):
    """Offset just past the first boundary after `start`, capped at the limit."""
    match = BOUNDARY.search(pe.d[start:start + FUNCTION_LIMIT])
    if match:
        return start + match.end()
    return start + FUNCTION_LIMIT


def xref_functions(pe, text_strings, out):
    """Disassemble every function that mentions a sky-related string."""
    owners = {}
    for address, text in text_strings.items():
        if not XREF_HINT.search(text):
            continue
        for ref in pe.find_u32(address):
            offset = pe.va2off(ref)
            if offset is None:
                continue
            start = function_start(pe, offset)
            if start is None:
                continue
            owners.setdefault(start, []).append("%#x %r" % (ref, text))
    chunks = []
    for start in sorted(owners):
        end = function_extent(pe, start)
        va = pe.off2va(start)
        if va is None:
            continue
        notes = sorted(set(owners[start]))
        end_va = pe.off2va(end) or (va + end - start)
        chunks.append(FUNCTION_HEADER % (va, end_va, end - start,
                                         "" if len(notes) < 6 else " (+%d refs)" % len(notes)))
        for note in notes[:6]:
            chunks.append("#   ref %s\n" % note)
        chunks.append(disassemble(pe, va, pe.off2va(end) or (va + end - start),
                                  text_strings))
        chunks.append("\n")
    (out / "asm-21-xref-functions.txt").write_text("".join(chunks))
    return len(owners), len(chunks)


# ---------------------------------------------------------------------------
# The size of one authored day is decided by the caller of the sky clock, not
# by the constant inside it.  `CECSunMoon::Tick` 0x4b5f10 takes the elapsed
# milliseconds as its only argument (`mov edi, [esp+0x18]`, 0x4b5f16) and
# `CECSunMoon::SetTime` 0x4b6030 takes an authored hour/24 as an f32
# (`fld dword ptr [esp+4]`, 0x4b6030).  Whatever feeds those two decides how
# fast a day passes, so the dump has to contain the call sites, not just the
# callee.  These were the missing windows in rounds 1-10.
CALL_TARGETS = {
    "25-sunmoon-tick-callers": 0x4B5F10,
    "26-sunmoon-settime-callers": 0x4B6030,
}
# `ds:0x8db108` holds the recovered 6.944444e-8 day-fraction-per-ms double.
# Anything else that reads the address is another path into the same clock.
DAY_FRACTION = 0x8DB108
CALL_SITE_CONTEXT = 0x140


def direct_callers(pe, target):
    """Virtual addresses of direct `call rel32` instructions that reach target."""
    hits = []
    for _name, vaddr, _vsize, rawptr, rawsize in pe.secs:
        data = pe.d[rawptr:rawptr + rawsize]
        base = pe.base + vaddr
        for i in range(len(data) - 5):
            if data[i] != 0xE8:
                continue
            rel = struct.unpack_from("<i", data, i + 1)[0]
            if base + i + 5 + rel == target:
                hits.append(base + i)
    return hits


def caller_dump(pe, text_strings, out):
    """Write one file per call target with the enclosing function of every call."""
    written = []
    for name, target in sorted(CALL_TARGETS.items()):
        sites = direct_callers(pe, target)
        chunks = ["# callers of %#x (direct call rel32): %d\n" % (target, len(sites))]
        for site in sites:
            offset = pe.va2off(site)
            if offset is None:
                continue
            start = function_start(pe, offset)
            if start is None:
                start = max(0, offset - CALL_SITE_CONTEXT)
            end = function_extent(pe, start)
            start_va = pe.off2va(start) or site
            end_va = pe.off2va(end) or (site + CALL_SITE_CONTEXT)
            chunks.append(FUNCTION_HEADER % (start_va, end_va, end - start, ""))
            chunks.append("#   call at %#x\n" % site)
            chunks.append(disassemble(pe, start_va, end_va, text_strings))
            chunks.append("\n")
        if not sites:
            chunks.append("#   none: the call is made through a vtable slot\n")
        (out / ("asm-%s.txt" % name)).write_text("".join(chunks))
        written.append((name, len(sites)))
    # Every code reference to the day-fraction constant itself.
    chunks = ["# references to the day-fraction double at %#x\n" % DAY_FRACTION]
    for ref in pe.find_u32(DAY_FRACTION):
        offset = pe.va2off(ref)
        chunks.append("#   %#x\n" % ref)
        if offset is None:
            continue
        start = max(0, offset - 0x40)
        end = min(len(pe.d), offset + 0x40)
        start_va = pe.off2va(start)
        end_va = pe.off2va(end)
        if start_va is None or end_va is None:
            continue
        chunks.append(disassemble(pe, start_va, end_va, text_strings))
        chunks.append("\n")
    (out / "asm-27-day-fraction-refs.txt").write_text("".join(chunks))
    return written


def disassemble(pe, start, end, text_strings):
    """Disassemble [start, end) and annotate image addresses with their C string."""
    import capstone

    code_off = pe.va2off(start)
    if code_off is None:
        return "; window %#x is not inside the image\n" % start
    PE_DATA[0] = pe.d
    end_off = pe.va2off(end) or len(pe.d)
    blob = pe.d[code_off:end_off]
    out = []
    skip = resync(code_off, blob)
    if skip:
        out.append(
            "; resynced %+#x: nearest function boundary, dumping from %#x"
            % (skip, start + skip)
        )
        blob = pe.d[code_off + skip:end_off]
        start += skip
    md = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)
    md.detail = False
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


def archive_dump(blob, tag, out, names=None, texts=None, all_texts=False):
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
        if all_texts and entry[2] <= 65536:
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


RAMP = " .:-=+*#%@"


def ascii_preview(image, columns=64, rows=32):
    """Max-pool luminance so single-pixel stars survive the downsample."""
    step_x = max(1, image.width // columns)
    step_y = max(1, image.height // rows)
    pixels = image.load()
    lines = []
    for row in range(rows):
        line = []
        for column in range(columns):
            best = 0
            for y in range(row * step_y, min((row + 1) * step_y, image.height), 2):
                for x in range(column * step_x, min((column + 1) * step_x, image.width), 2):
                    r, g, b = pixels[x, y]
                    best = max(best, (r * 299 + g * 587 + b * 114) // 1000)
            line.append(RAMP[min(len(RAMP) - 1, best * len(RAMP) // 256)])
        lines.append("".join(line))
    return lines


def sky_sets(blob, out, preview=False):
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
    previews = []
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
            if preview and (median < 80 or "night" in folder.lower() or "夜" in folder):
                previews.append("--- %s/%s  mean %.0f median %d ---" % (
                    folder, entry[0].rsplit("/", 1)[-1], mean, median))
                previews.extend(ascii_preview(image))
                previews.append("")
    (out / "textures-sky-folders.txt").write_text("\n".join(lines) + "\n")

    if preview:
        (out / "textures-sky-ascii.txt").write_text("\n".join(previews) + "\n")

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
    parser.add_argument("--all-configs", action="store_true",
                        help="also dump every configs.pck text entry under 64 KiB")
    parser.add_argument("--ascii", action="store_true",
                        help="render a coarse ASCII preview of each dark sky image")
    args = parser.parse_args()

    out = pathlib.Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    report = []

    if args.configs:
        blob = pathlib.Path(args.configs).read_bytes()
        kept = archive_dump(blob, "configs", out, texts=True, all_texts=args.all_configs)
        report.append("configs.pck: %d entries, %d related text entries" % (len(PCK.directory(blob)), kept))
    if args.shaders:
        blob = pathlib.Path(args.shaders).read_bytes()
        kept = archive_dump(blob, "shaders", out, texts=True)
        report.append("shaders.pck: %d entries, %d related text entries" % (len(PCK.directory(blob)), kept))
    if args.textures:
        blob = pathlib.Path(args.textures).read_bytes()
        report.append("textures.pck: %d sky folders" % sky_sets(blob, out, args.ascii))
    if args.exe:
        pe = PE.PE(args.exe)
        strings, hits = exe_strings(pe, out)
        report.append("elementclient.exe: base %#x, %d sections, %d sky strings" % (
            pe.base, len(pe.secs), hits))
        for name, (start, end) in list(WINDOWS.items()) + list(MODULES.items()):
            text = disassemble(pe, start, end, strings)
            (out / ("asm-%s.txt" % name)).write_text(
                "# %s  %#x-%#x\n%s" % (name, start, end, text))
            report.append("  %s: %d lines" % (name, text.count("\n")))
        count, chunks = xref_functions(pe, strings, out)
        report.append("  xref functions: %d bodies" % count)
        for name, sites in caller_dump(pe, strings, out):
            report.append("  %s: %d call site(s)" % (name, sites))

    (out / "SUMMARY.txt").write_text("\n".join(report) + "\n")
    print("\n".join(report))


if __name__ == "__main__":
    main()
