#!/usr/bin/env python3
"""Bake and reader-check one independent-component diagnostic cache per type.

The cache is not production-ready. Each failed type is reported and causes a
nonzero exit; the summary is written even when some types fail.
"""
import argparse
import csv
import subprocess
from pathlib import Path
from compare_component_leaves import leaf_hashes
from compare_lod_counts import read_counts
from survey_explicit_lods import check_rows


def main():
    p = argparse.ArgumentParser(description=__doc__)
    for name in ('runner', 'probe', 'checker', 'dll', 'sources', 'traces', 'counts', 'archives', 'output'):
        p.add_argument(name, type=Path)
    args = p.parse_args()
    counts = read_counts(args.counts, 'type', ('branch', 'frond', 'leaf'))
    args.archives.mkdir(parents=True, exist_ok=True)
    rows = []
    for i in range(59):
        source = (args.sources / f'tree-{i:03}.spt').resolve()
        archive = (args.archives / f'{i:03}.pwc').resolve()
        archive.unlink(missing_ok=True)
        base = [str(x.resolve()) for x in (args.runner, args.probe, args.dll)]
        bake = subprocess.run(base + [str(source), str(archive), str(i)], capture_output=True, text=True)
        if bake.returncode != 0 or not archive.is_file():
            detail = (bake.stderr.strip().replace('\t', ' ').replace('\n', ' ') or 'no output')[:300]
            rows.append((i, 'bake_failed', '', '', '', '', '', '', detail))
            continue
        check = subprocess.run([str(args.checker.resolve()), str(archive), str(source),
                                str(args.dll.resolve()), str(i)], capture_output=True, text=True)
        if check.returncode:
            rows.append((i, 'read_failed', '', '', '', '', '', '', check.stderr.strip().replace('\n', ' ')[:300]))
            continue
        try:
            values = tuple(map(int, check.stdout.strip().split('\t')))
            if len(values) != 6 or values[:3] != counts[i]:
                raise ValueError(f'component counts mismatch: {values}')
            text = (args.traces / f'{i:03}.tsv').read_text()
            check_rows(text, counts[i])
            expected = [r['auto_hash'] for r in csv.DictReader(text.splitlines(), delimiter='\t') if r['component'] == '2']
            if leaf_hashes(archive.read_bytes()) != expected:
                raise ValueError('leaf geometry hash mismatch')
            rows.append((i, 'ok', *values, '-'))
        except (ValueError, OSError) as error:
            rows.append((i, 'compare_failed', '', '', '', '', '', '', str(error).replace('\t', ' ')[:300]))
    with args.output.open('w', encoding='utf-8', newline='') as f:
        writer = csv.writer(f, delimiter='\t', lineterminator='\n')
        writer.writerow(('type', 'status', 'branch_lods', 'frond_lods', 'leaf_lods',
                         'branch_indices', 'frond_indices', 'leaf_cards', 'detail'))
        writer.writerows(rows)
    failures = [r[0] for r in rows if r[1] != 'ok']
    print(f'{59-len(failures)} checked; failed types: {failures}')
    return int(bool(failures))


if __name__ == '__main__':
    raise SystemExit(main())
