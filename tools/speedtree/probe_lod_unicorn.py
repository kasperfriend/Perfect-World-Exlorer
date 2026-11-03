#!/usr/bin/env python3
"""Execute the isolated SpeedTreeRT distance-to-LOD routine under Unicorn.

Requires `pip install pefile unicorn` (in a virtual environment) and the exact
client DLL below. No Windows API is needed for this particular internal routine.
This is not an execution of LoadTree, GetGeometry, or the client renderer.
"""
import argparse
import hashlib
import math
from pathlib import Path
import struct

import pefile
import unicorn
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_ESP

DLL_SHA256 = '7f0a401688f63c520ab717a4b9939705e5aac635a649a8cfb5df6af19681b46f'
BASE = 0x10000000
OBJECT = 0x20000000
END = 0x30000000


class Probe:
    def __init__(self, dll):
        data = Path(dll).read_bytes()
        if hashlib.sha256(data).hexdigest() != DLL_SHA256:
            raise ValueError('DLL does not match the investigated SpeedTreeRT build')
        pe = pefile.PE(data=data)
        if pe.OPTIONAL_HEADER.ImageBase != BASE or pe.OPTIONAL_HEADER.SizeOfImage > 0x70000:
            raise ValueError('unexpected PE image layout')
        self.uc = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_32)
        self.uc.mem_map(BASE, 0x70000)
        self.uc.mem_write(BASE, pe.get_memory_mapped_image())
        self.uc.mem_map(OBJECT, 0x10000)
        self.uc.mem_map(END, 0x1000)

    def floats(self, addr, *values):
        self.uc.mem_write(addr, struct.pack('<' + 'f' * len(values), *values))

    def lod(self, position, camera, near, far):
        if not all(math.isfinite(x) for x in (*position, *camera, near, far)) or not 0 <= near < far:
            raise ValueError('probe requires finite values and 0 <= near < far')
        self.floats(OBJECT + 4, *position)
        self.floats(OBJECT + 0x34, near)
        self.floats(OBJECT + 0x30, far)
        self.floats(BASE + 0x65134, *camera)
        stack = OBJECT + 0xf000
        self.uc.mem_write(stack, struct.pack('<I', END))
        self.uc.reg_write(UC_X86_REG_ESP, stack)
        self.uc.reg_write(UC_X86_REG_ECX, OBJECT)
        self.uc.emu_start(BASE + 0x282b0, END, count=1000)
        return struct.unpack('<f', self.uc.mem_read(OBJECT + 0x14, 4))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dll', type=Path)
    args = parser.parse_args()
    probe = Probe(args.dll)
    # These expected values are from execution of this exact routine; they are
    # regression fixtures, not independent proof of native Windows behavior.
    cases = [(0, 1.0), (25, 1.0), (49, 0.9931107759475708),
             (50, 0.9860795736312866), (55, 0.9487926363945007),
             (100, 0.5176136493682861), (150, 0.07372158765792847),
             (160, 0.0), (170, 0.0), (200, 0.0)]
    for distance, expected in cases:
        value = probe.lod((distance, 0, 0), (0, 0, 0), 50, 160)
        if abs(value - expected) > 1e-6:
            raise AssertionError(f'distance {distance}: {value} != {expected}')
        print(f'{distance:3} {value:.9f}')
    vertical = probe.lod((0, 100, 0), (0, 0, 0), 50, 160)
    shifted = probe.lod((110, 7, 2), (10, 7, 2), 50, 160)
    assert abs(vertical - cases[5][1]) < 1e-6
    assert abs(shifted - cases[5][1]) < 1e-6
    print('vertical and translated-camera checks passed')


if __name__ == '__main__':
    main()
