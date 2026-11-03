"""Small ABI checks for the experimental emulator's x87 substitutes.

These tests cover only ordinary finite inputs; they do not establish Windows
MSVCRT equivalence, special-value behavior, or full DLL/client fidelity.
"""
import math
import struct
import unittest

try:
    import unicorn
    from unicorn.x86_const import UC_X86_REG_ESP
    from probe_materials_emulated import FTOL, TRIG, POW
except ImportError:
    unicorn = None


@unittest.skipIf(unicorn is None, 'requires pefile, speakeasy-emulator and unicorn')
class X87Stubs(unittest.TestCase):
    def setUp(self):
        self.u = unicorn.Uc(unicorn.UC_ARCH_X86, unicorn.UC_MODE_32)
        self.u.mem_map(0x1000, 0x2000)
        self.u.mem_write(0x1800, FTOL)
        self.u.mem_write(0x1900, TRIG)
        self.u.mem_write(0x1a00, POW)
        self.u.reg_write(UC_X86_REG_ESP, 0x2ff0)

    def eval(self, stub, *values):
        self.u.mem_write(0x2000, struct.pack('<' + 'd' * len(values), *values))
        code = b''.join(b'\xdd\x05' + struct.pack('<I', 0x2000 + 8*i) for i in range(len(values)))
        ret = 0x1000 + len(code) + 5
        code += b'\xe8' + struct.pack('<i', stub - ret)
        code += b'\xdd\x1d' + struct.pack('<I', 0x2040)
        self.u.mem_write(0x1000, code)
        self.u.emu_start(0x1000, 0x1000 + len(code), count=100)
        return struct.unpack('<d', self.u.mem_read(0x2040, 8))[0]

    def test_acos_and_asin(self):
        for x in (-1.0, -0.5, 0.0, 0.2, 0.5, 1.0):
            self.assertAlmostEqual(self.eval(0x1900, x), math.acos(x), places=12)
            self.assertAlmostEqual(self.eval(0x190f, x), math.asin(x), places=12)

    def test_pow_positive_bases(self):
        for base, power in ((0, 1.5), (2, 3), (2, 2.5), (4, .2), (.4, 2.5)):
            self.assertAlmostEqual(self.eval(0x1a00, base, power), base ** power, places=12)

    def test_ftol_truncates_toward_zero(self):
        # FLD double; CALL _ftol stub; store EAX. EDX is the sign extension.
        code = bytes.fromhex('dd0500200000e8f5070000a340200000891544200000')
        self.u.mem_write(0x1000, code)
        for value in (-2.9, -0.9, 0, 0.9, 2.9):
            self.u.mem_write(0x2000, struct.pack('<d', value))
            self.u.emu_start(0x1000, 0x1000 + len(code), count=100)
            self.assertEqual(struct.unpack('<q', self.u.mem_read(0x2040, 8))[0], int(value))


if __name__ == '__main__':
    unittest.main()
