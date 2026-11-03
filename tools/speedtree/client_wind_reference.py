#!/usr/bin/env python3
"""Reference translation of the client's tree wind-strength noise, not leaf motion.

Based on elementclient.exe 64fcbe, 6506b0, 766320, 766490, 7668f0,
766910 and 766940. Intermediate floating-point behavior is approximated, not a
bit-exact x87 implementation. See probe_client_wind.py for the emulator.
"""
import math
import struct


def f32(value):
    return struct.unpack('<f', struct.pack('<f', value))[0]


class WindNoise:
    def __init__(self):
        self.seed = 0x12345
        raw = []
        # Three components are generated in separate passes; tree wind uses
        # only component zero, but all three passes advance the same PRNG.
        for _ in range(3):
            raw.append([f32(((self.random() % 2001) - 1000) * f32(.001)) for _ in range(256)])
        self.table = [f32((raw[0][(i-1) % 256] + raw[0][(i+1) % 256]) * .25 + raw[0][i] * .5) for i in range(256)]
        persistence = struct.unpack('<f', struct.pack('<I', 0x3f7fbe77))[0]
        weights, total, value = [], 0., 1.
        for _ in range(8):
            weights.append(value)
            total = f32(total + value)
            value = f32(value * persistence)
        self.offsets = [self.random() % 1023 for _ in range(8)]
        self.weights = [f32(w / total) for w in weights]
        self.periods = [256 >> i for i in range(8)]

    def random(self):
        self.seed = (self.seed * 16807) % 2147483647
        return self.seed

    def sample(self, time):
        if not math.isfinite(time) or time < 0:
            raise ValueError('only finite nonnegative time is supported')
        time = f32(time)
        result = 0.
        for period, offset, weight in zip(self.periods, self.offsets, self.weights):
            coordinate = time / period + offset
            index = math.trunc(coordinate)
            fraction = f32(coordinate - index)
            a, b = self.table[index % 256], self.table[(index + 1) % 256]
            result = f32(result + abs(a * (1. - fraction) + b * fraction) * weight)
        return result
