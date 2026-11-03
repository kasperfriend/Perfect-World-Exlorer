#!/usr/bin/env python3
"""Compare Rust LOD translations against full DLL GetGeometry trace files.

The caller must prepare 000.tsv through 058.tsv with lod-geometry-probe.cpp.
This verifies those samples, not camera distance, rendering, or wind.
"""
import argparse
import csv
import json
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('traces', type=Path)
    p.add_argument('--counts', type=Path, default=ROOT / 'Findings/SpeedTree-wibo-lod-counts.tsv')
    p.add_argument('--materials', type=Path, default=ROOT / 'Findings/SpeedTree-emulated-materials.jsonl')
    p.add_argument('--types', help='comma-separated type IDs (default: all 59)')
    args = p.parse_args()
    type_ids = list(range(59)) if args.types is None else [int(value) for value in args.types.split(',')]
    if not type_ids or len(set(type_ids)) != len(type_ids) or any(i < 0 or i >= 59 for i in type_ids):
        raise ValueError('type IDs must be distinct values in 0..58')
    with args.counts.open(encoding='utf8', newline='') as f:
        count_rows = list(csv.DictReader(f, delimiter='\t'))
    with args.materials.open(encoding='utf8') as f:
        material_rows = list(map(json.loads, f))
    counts = {int(row['type']): row for row in count_rows}
    materials = {row['type_id']: row for row in material_rows}
    if (len(count_rows) != 59 or len(material_rows) != 59 or
            set(counts) != set(range(59)) or set(materials) != set(range(59))):
        raise ValueError('expected all 59 material and count records')
    cases = []
    for tree_id in type_ids:
        path = args.traces / f'{tree_id:03}.tsv'
        with path.open(encoding='utf8', newline='') as f:
            rows = list(csv.DictReader(f, delimiter='\t'))
        if not rows:
            raise ValueError(f'empty trace: {path}')
        raw = materials[tree_id]['lod_raw']
        width, exponent, _, bias = raw['floats_1c_20_24_28']
        mode = raw['mode_18']
        if mode not in (0, 1):
            raise ValueError(f'type {tree_id}: unsupported mode {mode}')
        c = counts[tree_id]
        for row in rows:
            cases.append((tree_id, row, int(c['branch']), int(c['frond']), int(c['leaf']),
                          mode, width, bias, exponent, raw['byte_44']))
    source = r'''#[allow(dead_code)]
#[path = %s] mod lod;
use std::io::{self, BufRead};
fn main() {
  for line in io::stdin().lock().lines() {
    let line=line.unwrap(); let a: Vec<&str> = line.split_whitespace().collect();
    let level: f32=a[0].parse().unwrap();
    let branch: u16=a[1].parse().unwrap(); let frond: u16=a[2].parse().unwrap();
    let leaf: u16=a[3].parse().unwrap(); let mode: u32=a[4].parse().unwrap();
    let c=lod::client_components(level, [branch, frond, leaf], mode, a[5].parse().unwrap(),
        a[6].parse().unwrap(), a[7].parse().unwrap(), a[8].parse().unwrap()).unwrap();
    let index = |v: Option<lod::ComponentLod>| v.map(|x| x.index as i32).unwrap_or(-1);
    let alpha = |v: Option<lod::ComponentLod>| v.map(|x| x.alpha_reference).unwrap_or(255.0);
    println!("{} {} {} {} {:.9} {:.9}", index(c.branch), index(c.frond),
        index(c.leaf[0]), index(c.leaf[1]), alpha(c.leaf[0]), alpha(c.leaf[1]));
  }
}''' % json.dumps(str(ROOT / 'src/tree_lod.rs'))
    with tempfile.TemporaryDirectory() as temp:
        src, exe = Path(temp)/'main.rs', Path(temp)/'compare'
        src.write_text(source)
        subprocess.run(['rustc', '--edition=2021', str(src), '-o', str(exe)], check=True)
        text = ''.join(f"{r['level']} {b} {f} {l} {m} {w} {bias} {e} {alpha}\n"
                       for _, r, b, f, l, m, w, bias, e, alpha in cases)
        out = subprocess.run([str(exe)], input=text, text=True, capture_output=True, check=True).stdout.splitlines()
    if len(out) != len(cases):
        raise ValueError('unexpected Rust output length')
    max_error = 0.
    for (tree_id, row, _, _, _, mode, _, _, _, alpha), line in zip(cases, out):
        b, f, l0, l1, a0, a1 = line.split()
        actual = (int(b), int(f), int(l0), int(l1))
        expected = (int(row['branch_lod']), int(row['frond_lod']),
                    int(row['leaf0_lod']) if int(row['leaf0_active']) else -1,
                    int(row['leaf1_lod']) if int(row['leaf1_active']) else -1)
        if actual != expected:
            raise AssertionError(f'type {tree_id} level {row["level"]}: indices {actual} != {expected}')
        if float(row['branch_alpha']) != alpha or float(row['frond_alpha']) != alpha:
            raise AssertionError(f'type {tree_id} level {row["level"]}: branch/frond alpha differs')
        errors = [abs(float(a0) - float(row['leaf0_alpha']))] if int(row['leaf0_active']) else []
        if int(row['leaf1_active']):
            errors.append(abs(float(a1) - float(row['leaf1_alpha'])))
        error = max(errors, default=0.)
        max_error = max(max_error, error)
        if error > .002:
            raise AssertionError(f'type {tree_id} level {row["level"]}: leaf alpha error {error}')
    print(f'{len(cases)} full-geometry LOD samples matched; maximum leaf alpha error {max_error:.8g}')


if __name__ == '__main__':
    main()
