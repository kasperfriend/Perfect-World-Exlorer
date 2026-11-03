#!/usr/bin/env python3
"""Inventory ECWLD tree type assets; this does not execute SpeedTreeRT.

Usage: python3 tools/speedtree/asset_survey.py WORLD.ecwld trees.pck > survey.tsv
The output contains hashes of the *uncompressed* PCK records so that it can
be compared across differently compressed copies of the client archives.
"""
import argparse
import hashlib
from pathlib import Path
import struct
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pck_lib


def tree_types(data):
    if len(data) < 116:
        raise ValueError('short ECWLD header')
    magic, version = struct.unpack_from('<II', data)
    if (magic, version) != (0x45435744, 12):
        raise ValueError('expected ECWLD version 12')
    start, end, count = struct.unpack_from('<III', data, 36)
    if not 116 <= start <= end <= len(data) or count > 10000:
        raise ValueError('invalid tree table bounds or count')
    pos = start
    result = []
    for _ in range(count):
        strings = []
        for _ in range(2):
            if pos + 4 > end:
                raise ValueError('truncated tree string length')
            length, = struct.unpack_from('<I', data, pos)
            pos += 4
            if length > 4096 or pos + length > end:
                raise ValueError('invalid tree string length')
            strings.append(data[pos:pos + length].decode('gbk').replace('\\', '/'))
            pos += length
        result.append(tuple(strings))
    if pos != end:
        raise ValueError('tree table does not end at grass table')
    return result


def survey(world, archive):
    types = tree_types(world.read_bytes())
    _, records = pck_lib.directory(archive)
    entries = {}
    for entry in records:
        key = entry[0].lower()
        if key in entries:
            raise ValueError(f'ambiguous archive path: {entry[0]}')
        entries[key] = entry
    print('type_id\tspt_path\tspt_bytes\tspt_sha256\tcomposite_path\tcomposite_bytes\tcomposite_sha256')
    for i, paths in enumerate(types):
        fields = [str(i)]
        for path in paths:
            if any(c in path for c in '\t\r\n'):
                raise ValueError(f'type {i}: path cannot be represented as TSV: {path!r}')
            entry = entries.get(path.lower())
            if entry is None:
                raise ValueError(f'type {i}: missing tree asset {path}')
            data = pck_lib.read_entry(archive, entry)
            if len(data) != entry[2]:
                raise ValueError(f'type {i}: wrong decoded size for {path}')
            fields.extend((path, str(len(data)), hashlib.sha256(data).hexdigest()))
        print('\t'.join(fields))
    return len(types)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('world', type=Path)
    parser.add_argument('trees', type=Path)
    args = parser.parse_args()
    try:
        count = survey(args.world, args.trees)
    except (OSError, ValueError, UnicodeError, AssertionError) as exc:
        parser.exit(1, f'asset survey failed: {exc}\n')
    print(f'{count} types inventoried; runtime material, LOD and wind not measured', file=sys.stderr)


if __name__ == '__main__':
    main()
