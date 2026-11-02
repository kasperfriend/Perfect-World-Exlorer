#!/usr/bin/env python3
"""Compare the two candidate controller-108 table constructions.

The 1.4.2 render thread samples the 128-entry table with PC24 arithmetic
(0x6ec190..0x6ec22e), but the table itself is built while the GFX asset loads
(0x6ec5dd..0x6ec73d), and no traced FPU control word covers that thread. This
tool measures how much the choice matters before any mode is assumed:

  reference   exact integer table over the stored f32 reciprocal 0x3c010204:
              n = i * 2113665, value = (a * (2^28 - n) + b * n) / 2^28
              (this is `Precision::Extended` in src/gfx_color_track.rs)
  pc24_a      every operation rounded to f32: a * (1 - f) + b * f
              with f = f32(i * reciprocal) (this is `Precision::Pc24`)
  pc24_b      the other common order: a + (b - a) * f

All three are truncated to an integer byte channel exactly like the native
store. The counts are over every (from, to) byte pair and every interior index
1..=126, mirroring `channel()`/`build()` in src/gfx_color_track.rs.

Usage: python3 tools/analyze-color-track-construction.py [--pairs LIMIT]
"""

import argparse
import struct


def f32(value):
    return struct.unpack("f", struct.pack("f", value))[0]


RECIPROCAL = struct.unpack("f", struct.pack("I", 0x3C010204))[0]
DENOMINATOR = 1 << 28
NUMERATOR = 2113665


def extended(a, b, i):
    n = i * NUMERATOR
    return (a * (DENOMINATOR - n) + b * n) // DENOMINATOR


def pc24_a(a, b, i):
    f = f32(float(i) * RECIPROCAL)
    return int(f32(f32(float(a) * f32(1.0 - f)) + f32(float(b) * f)))


def pc24_b(a, b, i):
    f = f32(float(i) * RECIPROCAL)
    return int(f32(float(a) + f32(f32(float(b) - float(a)) * f)))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pairs", type=int, default=256,
                        help="endpoint byte values per channel (default 256)")
    args = parser.parse_args()
    limit = max(2, min(256, args.pairs))
    total = 0
    divergent_a = 0
    divergent_b = 0
    worst = (0, 0, 0)
    for a in range(limit):
        for b in range(limit):
            count_a = 0
            count_b = 0
            for i in range(1, 127):
                exact = extended(a, b, i)
                total += 1
                if pc24_a(a, b, i) != exact:
                    count_a += 1
                if pc24_b(a, b, i) != exact:
                    count_b += 1
            divergent_a += count_a
            divergent_b += count_b
            if count_a > worst[0]:
                worst = (count_a, a, b)
    print(f"stored reciprocal 0x3c010204 = {RECIPROCAL!r} (exact 1/127 = "
          f"{1/127!r})")
    print(f"interior comparisons: {total}")
    print(f"a*(1-f)+b*f       : {divergent_a} differ ({100.0*divergent_a/total:.2f}%)")
    print(f"a+(b-a)*f         : {divergent_b} differ ({100.0*divergent_b/total:.2f}%)")
    print(f"worst endpoint pair: from={worst[1]} to={worst[2]} with "
          f"{worst[0]}/126 interior entries differing")
    print("example from=0 to=127: "
          f"index 1 -> extended {extended(0, 127, 1)}, "
          f"a*(1-f)+b*f {pc24_a(0, 127, 1)}, "
          f"a+(b-a)*f {pc24_b(0, 127, 1)}")
    print("Both float orders therefore disagree with the integer table; the "
          "loading-thread precision must be decided from evidence, not chosen "
          "by whichever expression happens to be written.")


if __name__ == "__main__":
    main()
