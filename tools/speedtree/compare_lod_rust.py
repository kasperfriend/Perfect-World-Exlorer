#!/usr/bin/env python3
"""Compare the experimental Rust distance-to-level translation with DLL emulation.

Requires rustc, pefile and unicorn; see probe_lod_unicorn.py. This does not run
Windows or the client renderer and does not validate geometry or transitions.
"""
import argparse
import random
from pathlib import Path
import struct
import subprocess
import tempfile

from probe_lod_unicorn import Probe

ROOT = Path(__file__).resolve().parents[2]


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dll', type=Path)
    args = parser.parse_args()
    probe = Probe(args.dll)
    rng = random.Random(41)
    cases = []
    for _ in range(2000):
        position = [rng.uniform(-500, 500) for _ in range(3)]
        camera = [rng.uniform(-500, 500) for _ in range(3)]
        near = rng.uniform(1, 600)
        far = near + rng.uniform(1, 1000)
        cases.append(list(map(f32, position + camera + [near, far])))

    source = '''#[allow(dead_code)]
#[path = %s] mod lod;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let x: Vec<f32> = line.split_whitespace().map(|v| v.parse().unwrap()).collect();
        let value = lod::client_level([x[0], x[1], x[2]], [x[3], x[4], x[5]], x[6], x[7]).unwrap();
        println!("{:08x}", value.to_bits());
    }
}
''' % ('"' + str(ROOT / 'src/tree_lod.rs').replace('\\', '\\\\').replace('"', '\\"') + '"')
    with tempfile.TemporaryDirectory() as tmp:
        src = Path(tmp) / 'compare.rs'
        exe = Path(tmp) / 'compare'
        src.write_text(source)
        subprocess.run(['rustc', '--edition=2021', str(src), '-o', str(exe)], check=True)
        result = subprocess.run([str(exe)], input=''.join(' '.join(map(str, x)) + '\n' for x in cases),
                                text=True, capture_output=True, check=True)
    lines = result.stdout.splitlines()
    if len(lines) != len(cases):
        raise AssertionError(f'Rust returned {len(lines)} results for {len(cases)} cases')
    mismatches = []
    for values, actual in zip(cases, lines):
        expected = probe.lod(values[:3], values[3:6], values[6], values[7])
        expected_bits = struct.unpack('<I', struct.pack('<f', expected))[0]
        if int(actual, 16) != expected_bits:
            mismatches.append((values, actual, f'{expected_bits:08x}'))
    if mismatches:
        raise AssertionError(f'{len(mismatches)} mismatches; first: {mismatches[0]}')
    print(f'{len(cases)} deterministic cases matched bitwise (Unicorn, not native Windows)')


if __name__ == '__main__':
    main()
