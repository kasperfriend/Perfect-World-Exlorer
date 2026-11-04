#!/usr/bin/env python3
"""Completion-check leaf-normal wind traces for all 59 world tree types."""
import argparse
import csv
import io
import math
import subprocess
from pathlib import Path
from compare_lod_counts import read_counts

FIELDS = ('lod', 'leaves', 'changed_components', 'tested_components', 'max_abs_difference')


def check_trace(text, count):
    reader = csv.DictReader(io.StringIO(text), delimiter='\t')
    if tuple(reader.fieldnames or ()) != FIELDS:
        raise ValueError('invalid normal trace header')
    seen = set()
    leaves = changed = tested = 0
    maximum = 0.0
    for row in reader:
        lod = int(row['lod'])
        n = int(row['leaves'])
        c = int(row['changed_components'])
        t = int(row['tested_components'])
        d = float(row['max_abs_difference'])
        if lod not in range(count) or lod in seen or n < 0 or t != n * 3 * 64:
            raise ValueError(f'invalid leaf LOD row {row}')
        if c < 0 or c > t or not math.isfinite(d) or d < 0 or ((c == 0) != (d == 0)):
            raise ValueError(f'invalid normal-difference row {row}')
        seen.add(lod)
        leaves += n
        changed += c
        tested += t
        maximum = max(maximum, d)
    if seen != set(range(count)):
        raise ValueError('missing leaf LODs')
    return leaves, changed, tested, maximum


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('runner', 'probe', 'dll', 'sources', 'counts', 'output'):
        p.add_argument(name, type=Path)
    args = p.parse_args()
    counts = read_counts(args.counts, 'type', ('branch', 'frond', 'leaf'))
    rows = []
    for i in range(59):
        cmd = [str(args.runner.resolve()), str(args.probe.resolve()), str(args.dll.resolve()),
               str((args.sources / f'tree-{i:03}.spt').resolve())]
        trace = subprocess.run(cmd, capture_output=True, text=True, check=True)
        rows.append((i, counts[i][2], *check_trace(trace.stdout, counts[i][2])))
    with args.output.open('w', encoding='utf-8', newline='') as f:
        writer = csv.writer(f, delimiter='\t', lineterminator='\n')
        writer.writerow(('type', 'leaf_lods', 'leaves', 'changed_components', 'tested_components', 'max_abs_difference'))
        writer.writerows(rows)
    print(f'{len(rows)} types, {sum(r[1] for r in rows)} LODs, '
          f'{sum(r[2] for r in rows)} leaves, {sum(r[3] for r in rows)} changes '
          f'in {sum(r[4] for r in rows)} components')


if __name__ == '__main__':
    main()
