#!/usr/bin/env python3
"""Independent 80-digit Decimal oracle for the traced retail gamma ramp.

The float constants and f32 exponent store are from 0x430226 / 0x77b7c0.
No platform pow/powf or the production Rust implementation is used.
Print FNV-1a of all 101 x 256 little-endian u16 entries for the Rust test.
"""
from decimal import Decimal, localcontext
from fractions import Fraction
import struct


def value(bits):
    return Fraction.from_float(struct.unpack("<f", struct.pack("<I", bits))[0])


def round_f32(v):
    lo, hi = 0, 0x7F7FFFFF
    while lo + 1 < hi:
        mid = (lo + hi) // 2
        if value(mid) <= v:
            lo = mid
        else:
            hi = mid
    a, b = v - value(lo), value(hi) - v
    return value(lo if a < b or (a == b and lo % 2 == 0) else hi)


def decimal(v):
    return Decimal(v.numerator) / Decimal(v.denominator)


def main():
    digest = 0xCBF29CE484222325
    words = 0
    with localcontext() as ctx:
        ctx.prec = 80
        for setting in range(101):
            gamma = int(30 + setting * value(0x3FD9999A))
            exponent = decimal(round_f32(1 / (gamma * value(0x3C23D70A))))
            for i in range(256):
                entry = 0 if i == 0 else max(1, int(ctx.power(decimal(i * value(0x3B808081)), exponent) * 65535))
                assert 0 <= entry <= 65535
                for b in struct.pack("<H", entry):
                    digest = ((digest ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
                words += 1
    print(f"{words} native ramp words, 80-digit Decimal oracle FNV-1a: {digest:016x}")


if __name__ == "__main__":
    main()
