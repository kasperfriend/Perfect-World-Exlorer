#!/usr/bin/env python3
"""Compare experimental Rust discrete LOD selection with emulated DLL getters.

Synthetic component counts, explicit [0,1] levels and optional billboard slot;
not loaded SPTs, generated geometry, or an end-to-end client comparison.
Requires pefile, unicorn and rustc. Finite-input _ftol is substituted.
"""
import argparse
import random
from pathlib import Path
import struct
import subprocess
import tempfile

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from probe_lod_unicorn import Probe, BASE, OBJECT, END
from probe_discrete_lod_unicorn import FTOL

ROOT = Path(__file__).resolve().parents[2]


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dll', type=Path)
    args = parser.parse_args()
    u = Probe(args.dll).uc
    # Assumed CRT default; actual client control word is not captured.
    u.reg_write(UC_X86_REG_FPCW, 0x027f)
    u.mem_write(END, FTOL)
    u.mem_write(BASE + 0x5410c, struct.pack('<I', END))
    u.mem_write(OBJECT, struct.pack('<I', OBJECT + 0x100))
    u.mem_write(OBJECT + 0x4c, struct.pack('<I', OBJECT + 0x200))
    u.mem_write(OBJECT + 0x208, struct.pack('<I', 1))
    ret, stack = END + 0x100, OBJECT + 0xf000

    rng = random.Random(2026)
    cases = []
    for count in [0, 1, 2, 3, 4, 6, 10, 50, 32767]:
        for billboard in [False, True]:
            cases.extend((f32(level), count, billboard) for level in [0., .25, .5, .75, 1.])
    for _ in range(2000):
        cases.append((f32(rng.random()), rng.randrange(51), bool(rng.randrange(2))))
    source = '''#[allow(dead_code)]
#[path = %s] mod lod;
use std::io::{self, BufRead};
fn main() {
    for line in io::stdin().lock().lines() {
        let line = line.unwrap();
        let x: Vec<&str> = line.split_whitespace().collect();
        let level: f32 = x[0].parse().unwrap();
        let count: u16 = x[1].parse().unwrap();
        let billboard = x[2] == "1";
        match lod::client_discrete_level(level, count, billboard) {
            Some(value) => println!("{}", value), None => println!("-1")
        }
    }
}
''' % ('"' + str(ROOT / 'src/tree_lod.rs').replace('\\', '\\\\').replace('"', '\\"') + '"')
    with tempfile.TemporaryDirectory() as temp:
        src, exe = Path(temp) / 'compare.rs', Path(temp) / 'compare'
        src.write_text(source)
        subprocess.run(['rustc', '--edition=2021', str(src), '-o', str(exe)], check=True)
        result = subprocess.run([str(exe)], input=''.join(f'{x} {n} {int(b)}\n' for x, n, b in cases),
                                text=True, capture_output=True, check=True).stdout.splitlines()
    if len(result) != len(cases):
        raise AssertionError('wrong number of Rust outputs')
    for (level, count, billboard), actual in zip(cases, result):
        u.mem_write(OBJECT + 0x15c, struct.pack('<H', count))
        u.mem_write(OBJECT + 0x198, struct.pack('<H', count))
        u.mem_write(OBJECT + 0x60, struct.pack('<H', count))
        u.mem_write(BASE + 0x652a3, bytes([billboard]))
        addresses = [0x10022300] if billboard else [0x10022260, 0x100222b0, 0x10022300]
        for address in addresses:
            u.mem_write(stack, struct.pack('<If', ret, level))
            u.reg_write(UC_X86_REG_ESP, stack)
            u.reg_write(UC_X86_REG_ECX, OBJECT)
            u.emu_start(address, ret, count=500)
            if u.reg_read(UC_X86_REG_EIP) != ret:
                raise RuntimeError(f'getter 0x{address:x} did not return')
            expected = u.reg_read(UC_X86_REG_EAX) & 0xffff
            # No-level sentinel from the getters.
            expected = -1 if expected == 0xffff else expected
            if int(actual) != expected:
                raise AssertionError(f'{level=} {count=} {billboard=} {address=:#x}: {actual=} {expected=}')
    print(f'{len(cases)} synthetic inputs matched DLL getters (Unicorn, not a client render)')


if __name__ == '__main__':
    main()
