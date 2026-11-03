#!/usr/bin/env python3
"""Read the "Angelic File Package" archives shipped as configs.pck / shaders.pck.

The retail v2 reader in `src/pck.rs` does not open these files: the directory is
still a list of 276-byte records (name, offset, size, compressed size) but the
trailer guards and the payload layout differ, and the directory sits *before* the
trailer with an 8-byte header in front of each record instead of a length table.

    0x000  u32 0x4dca23ef          magic
    0x004  u32 total file size
    0x008  u32 0x56a089b7          magic
    0x00c  payload: the file data
    ...    directory: `count` times [8-byte header][zlib stream of one 276-byte
           record]; a stored file's payload is the raw bytes, a compressed one is
           a bare zlib stream (`clen < usize`), both at `offset`
    -280   280-byte trailer
           +0    u32 0xa508bdc4    guard
           +4    u32 version       0x00020002 in both shipped archives
           +8    u32 unknown       per-archive (configs 0x21cfb4f6, shaders 0x21be2dbf)
           +16   ASCII "Angelic File Package, ..." description, NUL padded
           +268  u32 0x853567c8    guard
           +272  u32 record count
           +276  u32 version, again

Usage:
    tools/extract-angelic-pck.py google_drive/shaders.pck --list
    tools/extract-angelic-pck.py google_drive/configs.pck --cat autofamilyconfigs/sky.lst
    tools/extract-angelic-pck.py google_drive/shaders.pck --out /tmp/shader-texts
"""
import argparse
import pathlib
import struct
import zlib

MAGIC = (0x4DCA23EF, 0x56A089B7)
GUARD = (0xA508BDC4, 0x853567C8)
HEADER = 280
PREFIX = 12
RECORD = 276
VERSIONS = (0x00020001, 0x00020002)
WINDOWS = (1 << 16, 1 << 19, 1 << 22)


def header(blob):
    """Validate one archive's prefix and trailer and return its fixed fields."""
    if len(blob) < PREFIX + HEADER:
        raise ValueError("too short for a header")
    first, version = struct.unpack_from("<II", blob, len(blob) - HEADER)
    if version not in VERSIONS:
        raise ValueError("unknown version %08x" % version)
    guard, count = struct.unpack_from("<II", blob, len(blob) - HEADER + 268)
    if (first, guard) != GUARD:
        raise ValueError("bad trailer guards %08x / %08x" % (first, guard))
    magic, size, tag = struct.unpack_from("<III", blob, 0)
    if (magic, tag) != MAGIC or size != len(blob):
        raise ValueError("bad prefix %08x / %d / %08x" % (magic, size, tag))
    return {
        "count": count,
        "version": version,
        "unknown": struct.unpack_from("<I", blob, len(blob) - HEADER + 8)[0],
        "description": blob[len(blob) - HEADER + 16:len(blob) - HEADER + 80]
        .split(b"\0")[0]
        .decode("latin1"),
    }


def _stream_ending_at(blob, end, limit):
    """Find a zlib stream that stops exactly at `end`; return (payload, start)."""
    for span in WINDOWS:
        low = max(end - span, 0)
        for start in range(end - 1, low - 1, -1):
            d = zlib.decompressobj()
            try:
                out = d.decompress(blob[start:end])
            except zlib.error:
                continue
            if out and d.eof and not d.unused_data:
                return out, start
    raise ValueError("no record ends at %#x" % end)


def directory(blob):
    """Return [(name, offset, size, compressed size)] in stored order."""
    count = header(blob)["count"]
    pos = len(blob) - HEADER
    entries = []
    for _ in range(count):
        record, start = _stream_ending_at(blob, pos, RECORD)
        if len(record) != RECORD:
            raise ValueError("record at %#x is %d bytes" % (start, len(record)))
        name = record[:record.find(b"\0")].decode("gbk", "replace")
        offset, size, compressed = struct.unpack_from("<III", record, 260)
        entries.append((name.replace("\\", "/"), offset, size, compressed))
        pos = start - 8
    entries.reverse()
    return entries


def read_entry(blob, entry):
    _, offset, size, compressed = entry
    raw = blob[offset:offset + compressed]
    return zlib.decompress(raw) if compressed < size else raw


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("pck")
    ap.add_argument("--list", action="store_true", help="print name, sizes and offset")
    ap.add_argument("--cat", help="print one entry to stdout")
    ap.add_argument("--out", help="write every entry under this directory")
    args = ap.parse_args()

    blob = pathlib.Path(args.pck).read_bytes()
    info = header(blob)
    entries = directory(blob)
    print(
        "%s: %d entries, version %08x, header field %08x, %r"
        % (args.pck, len(entries), info["version"], info["unknown"], info["description"]),
        file=__import__("sys").stderr,
    )
    if args.cat:
        wanted = args.cat.replace("\\", "/").lower()
        for entry in entries:
            if entry[0].lower() == wanted:
                data = read_entry(blob, entry)
                __import__("sys").stdout.buffer.write(data)
                return
        raise SystemExit("no entry named %s" % args.cat)
    for entry in entries:
        if args.list:
            print("%-56s %7d %7d %#08x" % entry)
        if args.out:
            path = pathlib.Path(args.out) / entry[0]
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(read_entry(blob, entry))


if __name__ == "__main__":
    main()
