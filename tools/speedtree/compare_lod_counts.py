#!/usr/bin/env python3
"""Compare a completion-checked Speakeasy survey with lod-count-probe output.

The second file may be from Windows or a compatibility layer; matching counts
are not a test of the emitted geometry, materials, or rendering.
"""
import argparse
import csv
from pathlib import Path


def read_counts(path, id_column, count_columns):
    with path.open(encoding='utf-8', newline='') as source:
        rows = list(csv.DictReader(source, delimiter='\t'))
    result = {}
    for row in rows:
        tree_id = int(row[id_column])
        if tree_id in result:
            raise ValueError(f'{path}: duplicate type {tree_id}')
        if 'emulated_compute_status' in row and row['emulated_compute_status'] != 'ok':
            raise ValueError(f'{path}: type {tree_id} did not complete')
        result[tree_id] = tuple(int(row[column]) for column in count_columns)
    if set(result) != set(range(59)):
        raise ValueError(f'{path}: expected exactly tree IDs 0 through 58')
    return result


def compare(emulated, execution):
    left = read_counts(emulated, 'type_id', ('branch_count', 'frond_count', 'leaf_count'))
    right = read_counts(execution, 'type', ('branch', 'frond', 'leaf'))
    mismatch = [(i, left[i], right[i]) for i in sorted(left) if left[i] != right[i]]
    if mismatch:
        raise ValueError(f'LOD count mismatches: {mismatch}')
    return len(left)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('emulated', type=Path)
    parser.add_argument('execution', type=Path)
    args = parser.parse_args()
    print(f'{compare(args.emulated, args.execution)} tree types matched')


if __name__ == '__main__':
    main()
