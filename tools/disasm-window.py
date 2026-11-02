#!/usr/bin/env python3
"""Read a window of the disassembled 1.4.2 client by virtual address.

Parses the line format produced by `objdump -d elementclient.exe`:
    ^\\s+([0-9a-f]+):\\t(?:[0-9a-f]{2} )+\\s*\\t(.*)$
and bisects by address so windows can be opened anywhere in .text/.rdata code.

Usage:
    tools/disasm-window.py 0x7ccf60            # 60 instructions from address
    tools/disasm-window.py 0x7ccf60 --n 200
    tools/disasm-window.py 0x7ccf60 --to 0x7cd100
    tools/disasm-window.py --addr 0x7ccf60 --asm /tmp/dis/text.asm
"""
import argparse
import bisect
import re

LINE = re.compile(r"^\s+([0-9a-f]+):\t(?:[0-9a-f]{2} )+\s*\t(.*)$")


class Disasm:
    def __init__(self, path):
        self.addrs = []
        self.lines = []
        with open(path, "r", errors="replace") as f:
            for raw in f:
                m = LINE.match(raw.rstrip("\n"))
                if m:
                    self.addrs.append(int(m.group(1), 16))
                    self.lines.append(m.group(2))

    def window(self, start, count=60, end=None):
        i = bisect.bisect_left(self.addrs, start)
        out = []
        n = 0
        while i < len(self.addrs):
            a = self.addrs[i]
            if end is not None and a > end:
                break
            if end is None and n >= count:
                break
            out.append((a, self.lines[i]))
            i += 1
            n += 1
        return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("start", help="start virtual address, e.g. 0x7ccf60")
    p.add_argument("--n", type=int, default=60)
    p.add_argument("--to", default=None, help="inclusive end virtual address")
    p.add_argument("--addr", default=None, help="address to show (alternative)")
    p.add_argument("--asm", default="/tmp/dis/text.asm")
    a = p.parse_args()
    start = int(a.start, 16) if isinstance(a.start, str) else a.start
    if a.addr:
        start = int(a.addr, 16)
    end = int(a.to, 16) if a.to else None
    d = Disasm(a.asm)
    for addr, text in d.window(start, a.n, end):
        print(f"{addr:08x}: {text}")


if __name__ == "__main__":
    main()
