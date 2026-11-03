#!/usr/bin/env python3
"""Execute SpeedTreeRT internal wind-strength state update 0x1002bc00.

Only this isolated routine is executed, with synthetic object state. It is not
SetWindStrength's validation wrapper, leaf animation, Windows, or the client.
"""
import argparse
import math
import struct
from pathlib import Path
from unicorn.x86_const import UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW
from probe_lod_unicorn import BASE, END, OBJECT, Probe


class WindPhaseProbe(Probe):
    def update(self, time, strength, previous_strength, phase, amplitude, frequency):
        if not all(math.isfinite(v) for v in (time, strength, previous_strength, phase, amplitude, frequency)):
            raise ValueError('nonfinite input')
        self.uc.mem_write(OBJECT, bytes(64))
        self.uc.mem_write(OBJECT + 0x30, struct.pack('<II', 0x12345678, 0x87654321))
        self.floats(OBJECT + 0x18, amplitude, frequency)
        self.floats(BASE + 0x652e8, time)
        stack = OBJECT + 0xf000
        self.uc.mem_write(stack, struct.pack('<Ifff', END, strength, previous_strength, phase))
        # Clear the prior call's x87 return value and reset the full FPU state.
        self.uc.mem_write(END + 0x100, b'\xdb\xe3')  # fninit
        self.uc.emu_start(END + 0x100, END + 0x102, count=1)
        self.uc.reg_write(UC_X86_REG_ESP, stack)
        self.uc.reg_write(UC_X86_REG_ECX, OBJECT)
        self.uc.reg_write(UC_X86_REG_FPCW, 0x027f)
        self.uc.emu_start(BASE + 0x2bc00, END, count=100)
        if self.uc.reg_read(UC_X86_REG_EIP) != END:
            raise RuntimeError('wind-state routine did not return')
        obj = self.uc.mem_read(OBJECT, 64)
        return struct.unpack_from('<f', obj, 0)[0], struct.unpack_from('<f', obj, 0x20)[0], struct.unpack_from('<f', obj, 0x24)[0], struct.unpack_from('<I', obj, 4)[0], struct.unpack_from('<II', obj, 0x30)

    def rock_angle(self, time, phase, rate, amplitude, base_amplitude, local_phase, rocking):
        """Stop after the first rocking angle is stored, before matrix construction."""
        if not all(math.isfinite(v) for v in (time, phase, rate, amplitude, base_amplitude, local_phase)):
            raise ValueError('nonfinite input')
        self.uc.mem_write(OBJECT, bytes(64))
        self.floats(OBJECT, phase)
        self.uc.mem_write(OBJECT + 0x14, bytes([bool(rocking)]))
        self.floats(OBJECT + 0x18, base_amplitude)
        self.floats(OBJECT + 0x20, rate, amplitude)
        self.floats(BASE + 0x652e8, time)
        stack = OBJECT + 0xf000
        self.uc.mem_write(stack, struct.pack('<IIIIfI', END, 0, 0, 0, local_phase, OBJECT + 0x100))
        self.uc.mem_write(END + 0x100, b'\xdb\xe3')
        self.uc.emu_start(END + 0x100, END + 0x102, count=1)
        self.uc.reg_write(UC_X86_REG_ESP, stack)
        self.uc.reg_write(UC_X86_REG_ECX, OBJECT)
        self.uc.reg_write(UC_X86_REG_FPCW, 0x027f)
        self.uc.emu_start(BASE + 0x2bd00, BASE + 0x2bd94, count=100)
        if self.uc.reg_read(UC_X86_REG_EIP) != BASE + 0x2bd94:
            raise RuntimeError('rock-angle routine did not reach the capture point')
        esp = self.uc.reg_read(UC_X86_REG_ESP)
        return struct.unpack('<f', self.uc.mem_read(esp + 0x18, 4))[0]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('dll', type=Path)
    args = p.parse_args()
    probe = WindPhaseProbe(args.dll)
    for case in [(0., 0.5, 0.25, 1., 0.75, 0.125),
                 (10., 0.5, 0.25, 1., 0.75, 0.125),
                 (100., 0.1, 0.9, -2., 0.75, 0.125)]:
        print(case, probe.update(*case))


if __name__ == '__main__':
    main()
