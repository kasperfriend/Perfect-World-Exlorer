#!/usr/bin/env python3
"""Inspect sfx.pck and optionally decode each WAV/MP3/OGG with ffmpeg.

Usage: python tools/audio-census.py element/sfx.pck [--ffmpeg PATH] [--jobs 4]
This checks archive reads and an independent decoder, not the viewer's rodio
backend or audible output. Client assets are never written to this repository.
"""
import argparse
import collections
import concurrent.futures
import os
import struct
import subprocess
import sys

import pck_lib


def signature(data):
    if data[:4] == b"RIFF" and data[8:12] == b"WAVE":
        pos = 12
        while pos + 8 <= len(data):
            chunk, size = struct.unpack_from("<4sI", data, pos)
            if pos + 8 + size > len(data):
                return "truncated WAV chunk"
            if chunk == b"fmt " and size >= 16:
                tag, channels, rate, _, _, bits = struct.unpack_from("<HHIIHH", data, pos + 8)
                return f"WAV tag={tag} channels={channels} rate={rate} bits={bits}"
            pos += 8 + size + (size & 1)
        return "WAV missing fmt"
    if data.startswith(b"OggS"):
        return "Ogg"
    if data.startswith(b"ID3") or data.startswith(b"\xff"):
        return "MP3"
    return "unknown signature"


def inspect(args, entry):
    name, _, expected, _ = entry
    try:
        data = pck_lib.read_entry(args.archive, entry)
        if len(data) != expected:
            raise ValueError(f"size {len(data)} != {expected}")
        kind = signature(data)
        if kind.startswith(("unknown", "truncated", "WAV missing")):
            raise ValueError(kind)
        if args.ffmpeg:
            result = subprocess.run([args.ffmpeg, "-nostdin", "-v", "error", "-xerror", "-i", "pipe:0", "-f", "null", "-"], input=data, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            if result.returncode:
                raise ValueError(result.stderr.decode(errors="replace").strip())
        return kind, None
    except Exception as exc:
        return None, f"{name}: {exc}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("archive")
    parser.add_argument("--ffmpeg")
    parser.add_argument("--jobs", type=int, default=4)
    args = parser.parse_args()
    if args.jobs < 1:
        parser.error("--jobs must be positive")
    _, entries = pck_lib.directory(args.archive)
    extensions = collections.Counter(os.path.splitext(e[0])[1].lower() or "<none>" for e in entries)
    audio = [e for e in entries if os.path.splitext(e[0])[1].lower() in (".wav", ".ogg", ".mp3")]
    print(f"Archive entries: {len(entries)}; extensions: {dict(extensions)}", flush=True)
    counts, errors = collections.Counter(), []
    with concurrent.futures.ThreadPoolExecutor(max_workers=args.jobs) as pool:
        for kind, error in pool.map(lambda e: inspect(args, e), audio):
            if error:
                errors.append(error)
            else:
                counts[kind] += 1
    for kind, n in sorted(counts.items()):
        print(f"{kind}: {n}")
    for error in errors:
        print(f"ERROR {error}", file=sys.stderr)
    print(f"Audio entries checked: {len(audio)}; failures: {len(errors)}")
    return bool(errors)


if __name__ == "__main__":
    sys.exit(main())
