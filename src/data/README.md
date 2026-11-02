# Native display-gamma numeric reference

`display-gamma-pc24.bin` is 101 × 256 little-endian u16 ramp words, ordered by
integer UI setting 0–100, then input index 0–255 (51,712 bytes).

- Source: recovered elementclient instructions at `0x430226` and
  `0x77b7ed–0x77b859`, executed by `tools/capture-native-gamma-x87.c` against
  the same x86 MSVCRT `_CIpow` / `_ftol` exports imported by the client.
- Floating-point context: PC24, round-to-nearest; native game flags 1/5 omit
  D3D8 FPU_PRESERVE. Both entry and exit control words are verified.
- Capture: GitHub Actions run **41473598574**, Windows job **125373256487**.
- SHA256: `6fae72f31f9fd16b926482b306a0c7cd0fce1cacbf11eaf92b242b61f11ef2b3`.
- FNV-1a: `9f351cddfec7875f`.

This small numeric table is intentional: substituting double/Decimal power
and merely rounding surrounding instructions is not equivalent to the CRT's
internal reduced-precision calculation (eight words differ). The PC53 control
capture reproduces the earlier `1988df3e411fa151` model, not the live PC24 mode.

Windows CI regenerates the reference independently and compares **every word**
with production `gamma_curve::entry`, using `tools/check-native-gamma-words.rs`.
Do not update this file merely to satisfy a hash. Investigate any CRT/precision
change first. The Python diagnostic retains the rejected Decimal substitution
to demonstrate the distinction, not as a replacement native oracle.

These are captured arithmetic results, not screenshots or original-client
scene execution. Display hardware interpolation/compositor behavior and the
remaining upstream material color paths are separate acceptance questions.
