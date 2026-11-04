#!/usr/bin/env python3
"""Compare experimental Rust tree-wind noise with the isolated client EXE.

Requires rustc, pefile and unicorn. This validates only the scalar client noise
against emulation, not SpeedTree leaf rocking, geometry, or rendered images.
"""
import argparse
from pathlib import Path
import random
import struct
import subprocess
import tempfile

from probe_client_wind import ClientWind

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('exe', type=Path)
    args = parser.parse_args()
    native = ClientWind(args.exe)
    rng = random.Random(341)
    times = [0., .125, 1., 16., 65536., 999999.]
    times += [struct.unpack('<f', struct.pack('<f', rng.uniform(0, 1000000)))[0]
              for _ in range(2000)]
    source = '''#[allow(dead_code)]
#[path = %s] mod wind;
use std::io::{self, BufRead};
fn main() {
    let wind = wind::ClientWind::new();
    for line in io::stdin().lock().lines() {
        let time: f32 = line.unwrap().parse().unwrap();
        println!("{:.9}", wind.sample(time).unwrap());
    }
}
''' % ('"' + str(ROOT / 'src/tree_wind.rs').replace('\\', '\\\\').replace('"', '\\"') + '"')
    with tempfile.TemporaryDirectory() as tmp:
        src, exe = Path(tmp) / 'compare.rs', Path(tmp) / 'compare'
        src.write_text(source)
        subprocess.run(['rustc', '--edition=2021', str(src), '-o', str(exe)], check=True)
        output = subprocess.run([str(exe)], input=''.join(f'{t}\n' for t in times),
                                text=True, capture_output=True, check=True).stdout.splitlines()
    if len(output) != len(times):
        raise AssertionError('wrong number of Rust samples')
    errors = [abs(float(result) - native.sample(time)) for time, result in zip(times, output)]
    maximum = max(errors)
    if maximum > 1e-6:
        index = errors.index(maximum)
        raise AssertionError(f'time {times[index]}: error {maximum}')
    print(f'{len(times)} cases match within 1e-6; maximum error {maximum:.9g} (Unicorn, not native Windows)')


if __name__ == '__main__':
    main()
