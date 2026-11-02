#!/usr/bin/env python3
"""Audit captured x86 MSVCRT words and expose the rejected Decimal substitution.

The real reference is capture-native-gamma-x87.c (the client's CRT exports at
PC24), not platform pow/powf or Decimal power. Even rounding every surrounding
instruction does not reproduce the CRT's internal reduced-precision behavior.
"""
from decimal import Decimal, localcontext
from fractions import Fraction
import struct
from pathlib import Path
import hashlib


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
    native = (Path(__file__).resolve().parents[1] / "src/data/display-gamma-pc24.bin").read_bytes()
    assert len(native) == 51712
    native_hash = 0xCBF29CE484222325
    for byte in native:
        native_hash = ((native_hash ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    assert native_hash == 0x9F351CDDFEC7875F
    mismatches = 0
    samples = []
    with localcontext() as ctx:
        ctx.prec = 80
        for setting in range(101):
            gamma = int(round_f32(30 + round_f32(setting * value(0x3FD9999A))))
            exponent = decimal(round_f32(1 / round_f32(gamma * value(0x3C23D70A))))
            for i in range(256):
                entry = 0 if i == 0 else max(1, int(round_f32(Fraction(ctx.power(decimal(round_f32(i * value(0x3B808081))), exponent)) * 65535)))
                assert 0 <= entry <= 65535
                for b in struct.pack("<H", entry):
                    digest = ((digest ^ b) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
                reference = struct.unpack_from("<H", native, words * 2)[0]
                if entry != reference:
                    mismatches += 1
                    samples.append((setting, i, reference, entry))
                words += 1
    assert digest == 0x694D2D717A85A339
    assert mismatches > 0
    print(f"{words} captured x86 MSVCRT PC24 words FNV={native_hash:016x}")
    print(f"SHA256={hashlib.sha256(native).hexdigest()}")
    print(f"Rejected Decimal-power substitution: {mismatches} mismatches; FNV={digest:016x}")
    print("(setting, index, native, rejected):", samples)


if __name__ == "__main__":
    main()
