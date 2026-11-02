#!/usr/bin/env python3
"""PE section mapping for the 1.4.2 client: file offset <-> virtual address."""
import struct
import sys

PATH = "google_drive/elementclient.exe"


class PE:
    def __init__(self, path=PATH):
        self.d = open(path, "rb").read()
        d = self.d
        self.e = struct.unpack_from("<I", d, 0x3C)[0]
        nsec = struct.unpack_from("<H", d, self.e + 6)[0]
        optsize = struct.unpack_from("<H", d, self.e + 20)[0]
        off = self.e + 24 + optsize
        self.secs = []
        for _ in range(nsec):
            name = d[off:off + 8].rstrip(b"\0").decode()
            vsize = struct.unpack_from("<I", d, off + 8)[0]
            vaddr = struct.unpack_from("<I", d, off + 12)[0]
            rawsize = struct.unpack_from("<I", d, off + 16)[0]
            rawptr = struct.unpack_from("<I", d, off + 20)[0]
            self.secs.append((name, vaddr, vsize, rawptr, rawsize))
            off += 40
        self.base = struct.unpack_from("<I", d, self.e + 24 + 28)[0]

    def va2off(self, va):
        va -= self.base
        for name, vaddr, vsize, rawptr, rawsize in self.secs:
            if vaddr <= va < vaddr + max(vsize, rawsize):
                return rawptr + (va - vaddr)
        return None

    def off2va(self, o):
        for name, vaddr, vsize, rawptr, rawsize in self.secs:
            if rawptr <= o < rawptr + rawsize:
                return self.base + vaddr + (o - rawptr)
        return None

    def u32(self, va):
        o = self.va2off(va)
        return struct.unpack_from("<I", self.d, o)[0]

    def cstr(self, va, n=200):
        o = self.va2off(va)
        s = self.d[o:o + n].split(b"\0")[0]
        return s.decode("latin1")

    def find_u32(self, value):
        """All virtual addresses whose 4-byte value equals value."""
        pat = struct.pack("<I", value)
        out = []
        start = 0
        while True:
            i = self.d.find(pat, start)
            if i < 0:
                return out
            va = self.off2va(i)
            if va is not None:
                out.append(va)
            start = i + 1


if __name__ == "__main__":
    p = PE()
    for name, vaddr, vsize, rawptr, rawsize in p.secs:
        print(f"{name:10s} va={vaddr:08x} vsize={vsize:08x} rawptr={rawptr:08x} rawsize={rawsize:08x}")
    print("image base", hex(p.base))
