#!/usr/bin/env python3
"""Probe isolated discrete LOD getters with synthetic objects under Unicorn.

Needs pefile and unicorn, and the specific DLL accepted by probe_lod_unicorn.
No SPT was loaded. The CRT _ftol import is replaced by the x87 conversion
stub below; results therefore require independent confirmation on Windows.
"""
import argparse
from pathlib import Path
import struct

from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP
from probe_lod_unicorn import Probe, BASE, OBJECT, END

# 32-bit MSVC _ftol equivalent for finite in-range inputs: save the x87
# control word, set truncate-toward-zero, FISTP a 64-bit integer, restore the
# control word, return EDX:EAX. This substitutes an imported CRT routine; it
# is not native CRT code. The checked examples below are all nonnegative.
FTOL = bytes.fromhex(
    '83ec0cd97c24080fb74424080d000c0000668944240ad96c240a'
    'df3c24d96c24088b04248b54240483c40cc3'
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('dll', type=Path)
    args = parser.parse_args()
    p = Probe(args.dll)
    u = p.uc
    u.mem_write(END, FTOL)
    u.mem_write(BASE + 0x5410c, struct.pack('<I', END))
    u.mem_write(OBJECT, struct.pack('<I', OBJECT + 0x100))
    # GetNumBranch/LeafLodLevels read the pointed-to data; frond count is
    # stored on the outer object.  These are synthetic objects, not SPT data.
    u.mem_write(OBJECT + 0x15c, struct.pack('<H', 3))
    u.mem_write(OBJECT + 0x198, struct.pack('<H', 3))
    u.mem_write(OBJECT + 0x60, struct.pack('<H', 3))
    return_address = END + 0x100

    def get(addr, level):
        stack = OBJECT + 0xf000
        u.mem_write(stack, struct.pack('<If', return_address, level))
        u.reg_write(UC_X86_REG_ESP, stack)
        u.reg_write(UC_X86_REG_ECX, OBJECT)
        u.emu_start(addr, return_address, count=500)
        if u.reg_read(UC_X86_REG_EIP) != return_address:
            raise RuntimeError(f'getter at {addr:#x} did not return')
        return u.reg_read(UC_X86_REG_EAX) & 0xffff

    cases = [(1, 0), (0.7, 0), (0.6, 1), (0.5, 1), (0.3, 2), (0, 2)]
    for kind, addr in [('branch', 0x10022260), ('frond', 0x100222b0), ('leaf', 0x10022300)]:
        for level, expected in cases:
            result = get(addr, level)
            if result != expected:
                raise AssertionError(f'{kind} level {level}: got {result}, expected {expected}')
        print(f'{kind}: six synthetic three-LOD cases passed')

    # Exercise the alternative leaf branch: global drop-to-billboard enabled
    # and the object's billboard data pointer has a positive count at +8.
    u.mem_write(BASE + 0x652a3, b'\x01')
    u.mem_write(OBJECT + 0x4c, struct.pack('<I', OBJECT + 0x200))
    u.mem_write(OBJECT + 0x208, struct.pack('<I', 1))
    for level, expected in [(1, 0), (0.8, 0), (0.75, 1), (0.5, 2), (0.25, 3), (0, 3)]:
        result = get(0x10022300, level)
        if result != expected:
            raise AssertionError(f'billboard leaf level {level}: got {result}, expected {expected}')
    print('leaf with drop-to-billboard flag and billboard data: six cases passed')
    # Either missing prerequisite disables the extra discrete slot.
    u.mem_write(OBJECT + 0x208, struct.pack('<I', 0))
    assert get(0x10022300, 0.25) == 2
    u.mem_write(OBJECT + 0x208, struct.pack('<I', 1))
    u.mem_write(BASE + 0x652a3, b'\x00')
    assert get(0x10022300, 0.25) == 2
    print('leaf billboard prerequisite checks passed')


if __name__ == '__main__':
    main()
