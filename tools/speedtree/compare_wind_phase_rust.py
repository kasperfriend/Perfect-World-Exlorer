#!/usr/bin/env python3
"""Compare Rust's experimental phase-state update to the isolated DLL routine."""
import argparse
import random
import struct
import subprocess
import tempfile
from pathlib import Path
from probe_wind_phase_unicorn import WindPhaseProbe

ROOT = Path(__file__).resolve().parents[2]


def f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('dll', type=Path)
    args = p.parse_args()
    rng = random.Random(9348)
    cases = [list(map(f32, [rng.uniform(0, 50000), rng.uniform(0, 1),
                            rng.uniform(0, 1), rng.uniform(-10000, 10000),
                            rng.uniform(0, 2), rng.uniform(0, 2)])) for _ in range(3000)]
    cases += [[0., .5, .25, 1., .75, .125], [10., .5, .25, 1., .75, .125],
              [100., .1, .9, -2., .75, .125]]
    # Feed prior DLL output back into the next call, as the wrapper does for
    # its -1/-1 parameters. This checks state continuity over 20 trajectories.
    probe = WindPhaseProbe(args.dll)
    for _ in range(20):
        previous, phase, time = f32(0.25), f32(0.), f32(0.)
        amplitude, frequency = f32(rng.uniform(0, 2)), f32(rng.uniform(0, 2))
        for _ in range(120):
            time = f32(time + rng.uniform(0, 2))
            strength = f32(rng.uniform(0, 1))
            case = [time, strength, previous, phase, amplitude, frequency]
            cases.append(case)
            phase, _, _, _, _ = probe.update(*case)
            previous = strength
    src = '''#[allow(dead_code)] #[path = %s] mod wind;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let x: Vec<f32> = line.unwrap().split_whitespace().map(|v| v.parse().unwrap()).collect();
        let state = wind::RuntimeWindPhase { strength:x[2], phase:x[3], angular_rate:0., amplitude_degrees:0. };
        let result=state.update(x[0],x[1],x[4],x[5]).unwrap();
        println!("{:08x} {:08x} {:08x} {:08x}",result.phase.to_bits(),result.angular_rate.to_bits(),result.amplitude_degrees.to_bits(),result.strength.to_bits());
    }
}
''' % ('"' + str(ROOT / 'src/tree_wind.rs').replace('\\', '\\\\').replace('"', '\\"') + '"')
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / 'compare.rs'; exe = Path(tmp) / 'compare'
        path.write_text(src)
        subprocess.run(['rustc', '--edition=2021', str(path), '-o', str(exe)], check=True)
        output = subprocess.run([str(exe)], input=''.join(' '.join(map(str, c))+'\n' for c in cases),
                                capture_output=True, text=True, check=True).stdout.splitlines()
    if len(output) != len(cases): raise AssertionError('incomplete Rust output')
    probe = WindPhaseProbe(args.dll)
    errors = []
    for case, actual in zip(cases, output):
        phase, rate, amplitude, strength_bits, flags = probe.update(*case)
        expected = [struct.unpack('<I', struct.pack('<f', x))[0] for x in (phase, rate, amplitude)] + [strength_bits]
        if [int(x, 16) for x in actual.split()] != expected or flags != (0, 0):
            errors.append((case, actual, expected, flags))
    if errors: raise AssertionError(f'{len(errors)} mismatches; first {errors[0]}')
    print(f'{len(cases)} synthetic updates matched bitwise (Unicorn, not native Windows)')


if __name__ == '__main__':
    main()
