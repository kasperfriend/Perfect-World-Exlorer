#!/usr/bin/env python3
"""Compare experimental Rust transition values to the isolated emulated DLL.

Requires rustc, pefile, unicorn 1.0.2, and speakeasy-emulator. Inputs are
synthetic; x87 precision is assumed and _ftol/_CIpow are substituted. This is
not a client rendering, geometry, or complete asset verification.
"""
import argparse
from pathlib import Path
import random
import struct
import subprocess
import tempfile

from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from probe_lod_unicorn import Probe, BASE, OBJECT, END
from probe_materials_emulated import FTOL, POW

ROOT = Path(__file__).resolve().parents[2]


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dll', type=Path)
    args = parser.parse_args()
    u = Probe(args.dll).uc
    u.reg_write(UC_X86_REG_FPCW, 0x027f)
    u.mem_write(END, FTOL)
    u.mem_write(END + 0x100, POW)
    u.mem_write(BASE + 0x5410c, struct.pack('<I', END))
    u.mem_write(BASE + 0x540fc, struct.pack('<I', END + 0x100))
    ret, output, stack = END + 0x200, OBJECT + 0x400, OBJECT + 0xf000
    rng = random.Random(401)
    cases = []
    for count in [1, 2, 3, 4, 6]:
        for boundary in range(1, count):
            for delta in [-0.051, -0.05, -0.049, 0, 0.049, 0.05, 0.051]:
                cases.append((f32(1 - boundary/count + delta), count, f32(.05), f32(.2), f32(1.5), f32(84)))
    for _ in range(2000):
        n = rng.randrange(1, 12)
        cases.append((f32(rng.random()), n, f32(rng.uniform(.001, .45 / n)),
                      f32(rng.uniform(0, .8)), f32(rng.uniform(.3, 3)), f32(rng.uniform(0, 200))))
    source = '''#[allow(dead_code)]
#[path = %s] mod lod;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap(); let a: Vec<&str> = line.split_whitespace().collect();
        let r = lod::client_transition(a[0].parse().unwrap(), a[1].parse().unwrap(),
            a[2].parse().unwrap(), a[3].parse().unwrap(), a[4].parse().unwrap(), a[5].parse().unwrap()).unwrap();
        println!("{} {} {:.9} {:.9}", r.primary, r.secondary.map(|v| v as i32).unwrap_or(-1), r.alpha_primary, r.alpha_secondary);
    }
}
''' % ('"' + str(ROOT / 'src/tree_lod.rs').replace('\\', '\\\\').replace('"', '\\"') + '"')
    with tempfile.TemporaryDirectory() as temp:
        src, exe = Path(temp) / 'compare.rs', Path(temp) / 'compare'
        src.write_text(source)
        subprocess.run(['rustc', '--edition=2021', str(src), '-o', str(exe)], check=True)
        lines = subprocess.run([str(exe)], input=''.join(' '.join(map(str, c)) + '\n' for c in cases),
                               text=True, capture_output=True, check=True).stdout.splitlines()
    if len(lines) != len(cases):
        raise AssertionError('wrong number of results')
    max_error = 0.
    for (level, count, width, bias, exponent, alpha), line in zip(cases, lines):
        u.mem_write(stack, struct.pack('<IfIffffIIII', ret, level, count, width, bias, exponent, alpha,
                                      output, output + 4, output + 8, output + 10))
        u.mem_write(output, b'\xcc' * 12)
        u.reg_write(UC_X86_REG_ESP, stack)
        u.emu_start(BASE + 0x254f0, ret, count=1000)
        if u.reg_read(UC_X86_REG_EIP) != ret:
            raise RuntimeError('GetTransitionValues did not return')
        a, b, first, second = struct.unpack('<ffhh', u.mem_read(output, 12))
        v = line.split()
        if (int(v[0]), int(v[1])) != (first, second):
            raise AssertionError(f'indices differ for {level,count,width,bias,exponent,alpha}: {line} vs {first,second}')
        error = max(abs(float(v[2])-a), abs(float(v[3])-b))
        max_error = max(max_error, error)
        if error > .002:
            raise AssertionError(f'alpha differs for {level,count,width,bias,exponent,alpha}: {line} vs {a,b}')
    print(f'{len(cases)} synthetic transition inputs matched within 0.002 alpha units; max error {max_error:.7g}')


if __name__ == '__main__':
    main()
