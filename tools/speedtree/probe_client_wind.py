#!/usr/bin/env python3
"""Execute the client's tree wind-noise setup and sampling in isolated x86 emulation.

This is not client rendering. Allocators and the imported float-to-integer CRT
helper are substituted; all noise generation/evaluation instructions come
from the identified client executable. Requires pefile and unicorn.
"""
import argparse
import hashlib
from pathlib import Path
import struct

import pefile
import unicorn
from unicorn.x86_const import UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EIP, UC_X86_REG_ESP, UC_X86_REG_FPCW

SHA256 = 'a290998dc13cc5558b54bda3f65d099f95bc0702e54a0613c6d7669f35c6cdb0'
BASE, OBJ, END = 0x400000, 0x20000000, 0x30000000
FTOL = bytes.fromhex('83ec0cd97c24080fb74424080d000c0000668944240ad96c240adf3c24d96c24088b04248b54240483c40cc3')


class ClientWind:
    def __init__(self, path):
        data = Path(path).read_bytes()
        if hashlib.sha256(data).hexdigest() != SHA256:
            raise ValueError('unexpected elementclient.exe hash')
        pe = pefile.PE(data=data)
        if pe.OPTIONAL_HEADER.ImageBase != BASE:
            raise ValueError('unexpected PE image base')
        size = (pe.OPTIONAL_HEADER.SizeOfImage + 0xfff) & ~0xfff
        u = self.u = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_32)
        # Unicorn defaults to zero (24-bit precision); model the conventional
        # MSVC CRT 53-bit precision. The actual running client's control word
        # has not been independently captured.
        u.reg_write(UC_X86_REG_FPCW, 0x027f)
        u.mem_map(BASE, size)
        u.mem_write(BASE, pe.get_memory_mapped_image())
        u.mem_map(OBJ, 0x20000)
        u.mem_map(END, 0x1000)
        u.mem_write(END, FTOL)
        u.mem_write(0x8d540c, struct.pack('<I', END))
        self.next_alloc = OBJ + 0x10000
        u.hook_add(unicorn.UC_HOOK_CODE, self._hook, begin=0x7631a0, end=0x7631b0)
        # 64fcbe..64fcee: table length 256, amplitude 1, period 256,
        # persistence 0x3f7fbe77, 8 octaves, seed 0x12345.
        self.call(0x7662b0, [])
        self.call(0x766320, [256, 0x3f800000, 256, 0x3f7fbe77, 8, 0x12345])

    def _hook(self, u, address, size, data):
        if address not in (0x7631a0, 0x7631b0):
            return
        stack = u.reg_read(UC_X86_REG_ESP)
        ret, arg = struct.unpack('<II', u.mem_read(stack, 8))
        if address == 0x7631a0:
            pointer = self.next_alloc
            self.next_alloc += (arg + 15) & ~15
            if self.next_alloc > OBJ + 0x20000:
                raise MemoryError('probe allocation exhausted')
            u.reg_write(UC_X86_REG_EAX, pointer)
        u.reg_write(UC_X86_REG_ESP, stack + 4)  # cdecl caller cleans argument
        u.reg_write(UC_X86_REG_EIP, ret)

    def call(self, address, args):
        stack = OBJ + 0xf000
        self.u.mem_write(stack, struct.pack('<' + 'I' * (len(args) + 1), END + 0x100, *args))
        self.u.reg_write(UC_X86_REG_ESP, stack)
        self.u.reg_write(UC_X86_REG_ECX, OBJ)
        self.u.emu_start(address, END + 0x100, count=1_000_000)
        if self.u.reg_read(UC_X86_REG_EIP) != END + 0x100:
            raise RuntimeError(f'client routine 0x{address:x} did not return')

    def sample(self, time):
        bits = struct.unpack('<I', struct.pack('<f', time))[0]
        self.call(0x766490, [bits, OBJ + 0x800, 1])
        return struct.unpack('<f', self.u.mem_read(OBJ + 0x800, 4))[0]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('exe', type=Path)
    args = parser.parse_args()
    wind = ClientWind(args.exe)
    for time in [0., .125, 1., 5., 16., 64., 256., 1024., 65536.]:
        print(f'{time:10.3f} {wind.sample(time):.9f}')


if __name__ == '__main__':
    main()
