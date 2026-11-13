#!/usr/bin/env python3
"""Count keypoint modes, spans and the scales no geometry can use.

Three questions the effect player answers from the client's own grammar, and
which a per-root spot check cannot settle:

  * Does any *interpolate-mode* keypoint hold forever (`TimeSpan: -1`)? The
    client's `A3DGFXKeyPointSet::TickAnimation` runs a keypoint's controllers
    only in the `enumInterpNone` branch, whatever its span, so a mode-1 keypoint
    with `TimeSpan: -1` would be the one case where gating on the span as well
    as the mode changes the picture.
  * How many element-101 records author a non-zero `ZOffset`, the depth-only key
    `Fill_Verts_2D` reads?
  * Which keypoints hold the impossible scales §6.2 measured, and are they
    reachable at all?

`KEYPOINTCOUNT` is honoured, because the block that follows it is the only
keypoint text an element owns: a record whose keypoint list was cut short has
its remaining `InterpolateMode:` lines belonging to the *next* element, and a
naive split over the whole file counts those twice.

Usage: python3 tools/gfx-keypoint-census.py google_drive/gfx.pck
"""
import re
import sys

sys.path.insert(0, __file__.rsplit('/', 1)[0])
import pck_lib

ELEMENT = re.compile(r'GFXELEMENTID:\s*(\d+)')
FIELD = re.compile(r'^([A-Za-z_0-9]+):[ \t]*(.*)$')


def blocks(text):
    """(element id, fields) per element record, in file order.

    Splitting on `GFXELEMENTID:` alone is enough to read a file the parser reads:
    `SPEC_TERMINATORS` stops the class block at `StartTime`/`KEYPOINTCOUNT`/
    `InterpolateMode`/`AffectorCount`/`GFXELEMENTID`, so every `Scale:` line
    before the element's first `KEYPOINTCOUNT` belongs to a *controller* or to an
    element-125 generation sequence, never to a keypoint.
    """
    for part in re.split(r'(?=^GFXELEMENTID:)', text, flags=re.M):
        head = ELEMENT.match(part)
        if not head:
            continue
        fields = []
        for line in part.splitlines():
            found = FIELD.match(line)
            if found:
                fields.append((found.group(1), found.group(2)))
        yield int(head.group(1)), fields


def keypoints(fields):
    """The declared keypoint blocks of one element, and nothing after them."""
    try:
        start = next(i for i, (k, _) in enumerate(fields) if k == 'KEYPOINTCOUNT')
    except StopIteration:
        return []
    count = int(fields[start][1] or 0)
    blocks_out = []
    index = start + 1
    for _ in range(count):
        while index < len(fields) and fields[index][0] != 'InterpolateMode':
            index += 1
        if index >= len(fields):
            break
        one = {}
        while index < len(fields) and fields[index][0] not in one:
            one[fields[index][0]] = fields[index][1]
            index += 1
        blocks_out.append(one)
    return blocks_out


def main(path):
    _, entries = pck_lib.directory(path)
    gfx = [e for e in entries if e[0].lower().endswith('.gfx')]
    print(f'roots: {len(gfx)}')
    roots = elements = keypoints_seen = 0
    modes = {}
    held = []
    sprites = 0
    z101 = []
    odd_scale = []
    for name, off, usize, clen in gfx:
        roots += 1
        text = pck_lib.read_entry(path, (name, off, usize, clen)).decode('gbk', 'replace')
        for eid, fields in blocks(text):
            elements += 1
            common = dict(fields)
            # The first KEYPOINTCOUNT: everything before it is the class block.
            if eid == 101:
                sprites += 1
                value = common.get('ZOffset')
                if value is not None and float(value) != 0.:
                    z101.append((name, float(value)))
            for key in keypoints(fields):
                keypoints_seen += 1
                mode = int(key.get('InterpolateMode', 0))
                modes[mode] = modes.get(mode, 0) + 1
                span = float(key.get('TimeSpan', 0))
                if mode != 0 and span < 0:
                    held.append((name, eid, mode, span > 0))
                scale = abs(float(key.get('Scale', 1)))
                if scale > 1e6:
                    odd_scale.append((name, eid, float(key.get('Scale', 1)), span))
    print(f'elements: {elements}')
    print(f'element-101 records: {sprites}')
    print(f'101 records with a non-zero ZOffset: {len(z101)}')
    for name, value in z101[:10]:
        print(f'    {value:+8.3f}  {name}')
    print(f'keypoints: {keypoints_seen}')
    print(f'modes: {dict(sorted(modes.items()))}')
    print(f'interpolate-mode keypoints with TimeSpan: -1: {len(held)}')
    for row in held:
        print(f'    {row}')
    print(f'keypoints with |Scale| > 1e6: {len(odd_scale)}')
    for row in odd_scale:
        print(f'    {row}')


if __name__ == '__main__':
    main(sys.argv[1] if len(sys.argv) > 1 else 'google_drive/gfx.pck')
