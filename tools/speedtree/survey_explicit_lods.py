#!/usr/bin/env python3
"""Run and completion-check the explicit-component diagnostic for 59 SPTs.

The runner may be a compatibility wrapper (e.g. wibo). This comparison covers
only fields hashed by the probe, not material state, rendering, or animation.
"""
import argparse
import csv
import io
import subprocess
from pathlib import Path
from compare_lod_counts import read_counts

FIELDS = ('component', 'requested', 'auto_lod', 'explicit_lod',
          'auto_count', 'explicit_count', 'auto_hash', 'explicit_hash')


def check_rows(text, counts):
    reader = csv.DictReader(io.StringIO(text), delimiter='\t')
    if tuple(reader.fieldnames or ()) != FIELDS:
        raise ValueError('invalid explicit LOD trace header')
    expected = {(c, n) for c, count in enumerate(counts) for n in range(count)}
    seen = set()
    for row in reader:
        key = (int(row['component']), int(row['requested']))
        if key not in expected or key in seen:
            raise ValueError(f'unexpected or duplicate component/level {key}')
        seen.add(key)
        if (int(row['auto_lod']) != key[1] or int(row['explicit_lod']) != key[1]
                or int(row['auto_count']) != int(row['explicit_count'])
                or row['auto_hash'] != row['explicit_hash']):
            raise ValueError(f'automatic and explicit geometry differ: {row}')
        for field in ('auto_hash', 'explicit_hash'):
            if len(row[field]) != 16:
                raise ValueError('invalid geometry hash')
            int(row[field], 16)
    if seen != expected:
        raise ValueError(f'missing component levels: {sorted(expected - seen)}')
    return len(seen)


def check_combinations(text, counts):
    fields = text.strip().split('\t')
    if len(fields) != 2:
        raise ValueError('invalid combination summary')
    cases, mismatches = map(int, fields)
    expected = counts[0] * counts[1] * counts[2]
    if cases != expected or mismatches != 0:
        raise ValueError(f'combination result {cases}/{mismatches}, expected {expected}/0')
    return cases


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('runner', type=Path)
    p.add_argument('probe', type=Path)
    p.add_argument('dll', type=Path)
    p.add_argument('sources', type=Path)
    p.add_argument('counts', type=Path)
    p.add_argument('output', type=Path)
    args = p.parse_args()
    counts = read_counts(args.counts, 'type', ('branch', 'frond', 'leaf'))
    summaries = []
    for i in range(59):
        cmd = [str(args.runner.resolve()), str(args.probe.resolve()), str(args.dll.resolve()),
               str((args.sources / f'tree-{i:03}.spt').resolve())]
        trace = subprocess.run(cmd, capture_output=True, text=True, check=True)
        matched = check_rows(trace.stdout, counts[i])
        combo = subprocess.run(cmd + ['--combinations'], capture_output=True, text=True, check=True)
        cases = check_combinations(combo.stdout, counts[i])
        summaries.append((i, *counts[i], matched, cases))
    with args.output.open('w', encoding='utf-8', newline='') as output:
        writer = csv.writer(output, delimiter='\t', lineterminator='\n')
        writer.writerow(('type', 'branch', 'frond', 'leaf', 'matched_levels', 'matched_combinations'))
        writer.writerows(summaries)
    print(f'{len(summaries)} types, {sum(row[4] for row in summaries)} levels, '
          f'{sum(row[5] for row in summaries)} combinations matched')


if __name__ == '__main__':
    main()
