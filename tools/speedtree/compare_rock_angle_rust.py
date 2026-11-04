#!/usr/bin/env python3
"""Compare the experimental first rocking angle with isolated x87 execution."""
import argparse
import random
import subprocess
import tempfile
from pathlib import Path
from compare_wind_phase_rust import f32
from probe_wind_phase_unicorn import WindPhaseProbe

ROOT = Path(__file__).resolve().parents[2]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('dll', type=Path)
    args = p.parse_args()
    rng = random.Random(46717)
    cases = []
    for _ in range(4000):
        case = list(map(f32, [rng.uniform(0, 20000), rng.uniform(-1000, 1000),
                              rng.uniform(0, 8), rng.uniform(0, 180),
                              rng.uniform(0, 2), rng.uniform(-50, 50)]))
        cases.append(case + [rng.randrange(2)])
    source = '''#[allow(dead_code)] #[path = %s] mod wind;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let x: Vec<f32> = line.unwrap().split_whitespace().map(|v|v.parse().unwrap()).collect();
        let state=wind::RuntimeWindPhase { strength:0., phase:x[1], angular_rate:x[2], amplitude_degrees:x[3] };
        println!("{:.9}",state.rock_angle(x[0],x[5],x[4],x[6]!=0.).unwrap());
    }
}
''' % ('"' + str(ROOT / 'src/tree_wind.rs').replace('\\', '\\\\').replace('"', '\\"') + '"')
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / 'compare.rs'; exe = Path(tmp) / 'compare'
        path.write_text(source)
        subprocess.run(['rustc', '--edition=2021', str(path), '-o', str(exe)], check=True)
        output = subprocess.run([str(exe)], input=''.join(' '.join(map(str, c))+'\n' for c in cases),
                                capture_output=True, text=True, check=True).stdout.splitlines()
    if len(output) != len(cases): raise AssertionError('incomplete Rust output')
    probe = WindPhaseProbe(args.dll)
    maximum = 0.
    for case, actual in zip(cases, output):
        expected = probe.rock_angle(*case)
        error = abs(float(actual)-expected)
        maximum = max(maximum, error)
        if error > 0.0001: raise AssertionError(f'{case}: Rust {actual}, DLL {expected}, error {error}')
    print(f'{len(cases)} scalar angles matched within 0.0001 degrees; max error {maximum:.9g} (Unicorn)')


if __name__ == '__main__':
    main()
