#!/usr/bin/env python3
"""Exercise SpeedTreeRT GetTransitionValues for a synthetic three-level tree.

The isolated DLL export is emulated, with replacement x87 _ftol and _CIpow
routines. The inputs are illustrative SPT-like parameters, not proof of a
particular tree's generated LOD counts or of client render-state behavior.
"""
import argparse
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EIP, UC_X86_REG_ESP
from probe_lod_unicorn import Probe, BASE, OBJECT, END
from probe_materials_emulated import FTOL, POW


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dll', type=Path)
    args = parser.parse_args()
    u = Probe(args.dll).uc
    u.mem_write(END, FTOL)
    u.mem_write(END + 0x100, POW)
    u.mem_write(BASE + 0x5410c, struct.pack('<I', END))
    u.mem_write(BASE + 0x540fc, struct.pack('<I', END + 0x100))
    ret = END + 0x200
    out = OBJECT + 0x400
    stack = OBJECT + 0xf000

    def evaluate(level):
        # float level, uint16 count (32-bit stack slot), float half-width,
        # float alpha-bias, float exponent, float alpha reference, two float*
        # and two short* outputs. Arguments traced at 0x10024e80..10024ed7.
        u.mem_write(stack, struct.pack('<IfIffffIIII', ret, level, 3, .07, .2, 1.5, 84,
                                      out, out + 4, out + 8, out + 10))
        u.mem_write(out, b'\xcc' * 12)
        u.reg_write(UC_X86_REG_ESP, stack)
        u.emu_start(BASE + 0x254f0, ret, count=1000)
        if u.reg_read(UC_X86_REG_EIP) != ret:
            raise RuntimeError('GetTransitionValues did not return')
        return struct.unpack('<ffhh', u.mem_read(out, 12))

    # Around the first and second internal boundaries. Tolerances account for
    # float/x87 details and the substituted _CIpow, not native equivalence.
    fixtures = [
        (.738, (84, 255, 0, -1)),
        (.736, (84, 253.475494, 0, 1)),
        (.700, (87.680840, 178.330017, 0, 1)),
        (.600, (247.423080, 84, 0, 1)),
        (.596, (84, 255, 1, -1)),
        (.404, (84, 255, 1, -1)),
        (.402, (84, 251.955597, 1, 2)),
        (.300, (178.329956, 87.680862, 1, 2)),
        (.264, (253.475449, 84, 1, 2)),
        (.263, (84, 255, 2, -1)),
    ]
    for level, expected in fixtures:
        actual = evaluate(level)
        if actual[2:] != expected[2:] or any(abs(a - b) > 0.001 for a, b in zip(actual[:2], expected[:2])):
            raise AssertionError(f'{level}: got {actual}, expected {expected}')
        print(f'{level:.3f}: alpha {actual[0]:.3f}/{actual[1]:.3f}, LODs {actual[2]}/{actual[3]}')
    print('ten transition fixtures passed (synthetic inputs; emulated CRT)')


if __name__ == '__main__':
    main()
