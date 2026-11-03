#!/usr/bin/env python3
"""Emulate A3DSkySphere::Init (0x79a330) and dump the native sky mesh.

The client builds its sky dome procedurally in `A3DSkySphere::Init`; the
geometry is never stored as an asset.  This harness runs the original function
inside Unicorn with the Angelica stream/allocator entry points stubbed, so the
vertex and index buffers are the client's own bytes rather than a reading of
the disassembly.

Usage:
  python3 tools/emulate-sky-sphere.py                 # summary + CSV to stdout
  python3 tools/emulate-sky-sphere.py --csv out.csv   # write vertex table
  python3 tools/emulate-sky-sphere.py --json out.json # write mesh + indices

Requires: unicorn, pefile  (pip install unicorn pefile)
Staged binary: google_drive/elementclient.exe
"""
import argparse
import json
import struct
import sys

import pefile
from unicorn import Uc, UC_ARCH_X86, UC_MODE_32, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED
from unicorn.x86_const import (
    UC_X86_REG_EAX, UC_X86_REG_ECX, UC_X86_REG_EDX, UC_X86_REG_ESP,
    UC_X86_REG_EBP, UC_X86_REG_ESI, UC_X86_REG_EDI, UC_X86_REG_EIP,
    UC_X86_REG_FS_BASE,
)

EXE = "google_drive/elementclient.exe"

# The two geometry builders called from A3DSkySphere::Init (0x7997a0) after
# the texture streams exist.  Registers: ecx = this.
SURROUND = 0x79A330   # horizon curtain -> stream at this+0x584
ROOF = 0x79A810       # polar cap       -> stream at this+0x588
CTOR = 0x799610

STACK = 0x00100000
STACK_SIZE = 0x00200000
TLS = 0x01000000
HEAP = 0x10000000
HEAP_SIZE = 0x00400000
SENTINEL = 0x0BADF00D

# Angelica helpers the real function calls.  Everything here only has to hand
# back storage; the geometry math is untouched.
NEW = 0x763250            # operator new(size)
STREAM_CTOR = 0x784D70    # A3DStream::A3DStream()
STREAM_INIT = 0x784F10    # A3DStream::Init(dev, fvf, fvf2, nverts, nindices, ?, ?)
LOCK_VERT = 0x7855C0      # A3DStream::LockVertexBuffer(?, &ptr, ?, ?)
LOCK_INDEX = 0x785700     # A3DStream::LockIndexBuffer(?, &ptr, ?, ?)
UNLOCK_VERT = 0x7856D0
UNLOCK_INDEX = 0x785810


def load():
    pe = pefile.PE(EXE, fast_load=True)
    base = pe.OPTIONAL_HEADER.ImageBase
    size = 0
    for s in pe.sections:
        size = max(size, s.VirtualAddress + max(s.Misc_VirtualSize, s.SizeOfRawData))
    return pe, base, (size + 0xFFF) & ~0xFFF


class Sky:
    def __init__(self):
        self.pe, self.base, self.size = load()
        self.uc = Uc(UC_ARCH_X86, UC_MODE_32)
        self.uc.mem_map(self.base, self.size)
        self.uc.mem_write(self.base, bytes(self.pe.__data__[: self.size]))
        self.uc.mem_map(STACK, STACK_SIZE)
        # This Unicorn build has no writable FS base (register 250 is a no-op),
        # so page 0 stands in for fs:[0]: the client's SEH prologue reads and
        # rewrites fs:[0] and nothing else in these functions touches it.
        self.uc.mem_map(0, 0x1000, 7)
        self.uc.mem_map(HEAP, HEAP_SIZE, 7)
        self.heap = HEAP
        self.streams = []
        self.trace = __import__("collections").deque(maxlen=64)
        self.debug = bool(__import__("os").environ.get("SKY_DEBUG"))
        self.uc.hook_add(UC_HOOK_CODE, self.hook_code)
        self.uc.hook_add(UC_HOOK_MEM_UNMAPPED, self.hook_unmapped)

    # ---- stubs ---------------------------------------------------------
    def alloc(self, n, align=16):
        self.heap = (self.heap + align - 1) & ~(align - 1)
        p = self.heap
        self.heap += max(n, 16) + 64
        assert self.heap < HEAP + HEAP_SIZE, "heap exhausted"
        return p

    def ret(self, value=0, nargs=0, tag=""):
        """Return from a stub, honouring the callee's `ret N` stack cleanup."""
        esp = self.uc.reg_read(UC_X86_REG_ESP)
        ret = struct.unpack("<I", self.uc.mem_read(esp, 4))[0]
        self.uc.reg_write(UC_X86_REG_EIP, ret)
        self.uc.reg_write(UC_X86_REG_ESP, esp + 4 + nargs * 4)
        self.uc.reg_write(UC_X86_REG_EAX, value & 0xFFFFFFFF)
        if self.debug:
            print("stub %-12s esp=%#x ret=%#x nargs=%d" % (tag, esp, ret, nargs),
                  file=sys.stderr)

    def hook_code(self, uc, address, size, _):
        self.trace.append(address)
        if address == NEW:
            n = struct.unpack("<I", uc.mem_read(uc.reg_read(UC_X86_REG_ESP) + 4, 4))[0]
            self.ret(self.alloc(n), tag="new")
        elif address == STREAM_CTOR:
            # __thiscall constructors return `this` in eax.
            self.ret(uc.reg_read(UC_X86_REG_ECX), tag="stream_ctor")
        elif address == STREAM_INIT:
            # (dev, fvf, ?, nverts, nindices, ?, ?) -> bool
            esp = uc.reg_read(UC_X86_REG_ESP)
            nv, ni = struct.unpack("<II", uc.mem_read(esp + 16, 8))
            self.streams.append({"verts": self.alloc(nv * 32, 32),
                                 "indices": self.alloc(ni * 2, 2),
                                 "nv": nv, "ni": ni})
            self.ret(1, 7, tag="stream_init")
        elif address in (LOCK_VERT, LOCK_INDEX):
            esp = uc.reg_read(UC_X86_REG_ESP)
            slot = struct.unpack("<I", uc.mem_read(esp + 12, 4))[0]
            s = self.streams[-1]
            uc.mem_write(slot, struct.pack("<I", s["verts" if address == LOCK_VERT else "indices"]))
            self.ret(1, 4, tag="lock")
        elif address in (UNLOCK_VERT, UNLOCK_INDEX):
            self.ret(1, tag="unlock")
        elif address == SENTINEL:
            uc.emu_stop()

    def hook_unmapped(self, uc, access, address, size, value, _):
        raise SystemExit("unmapped %#x (access %d)" % (address, access))

    # ---- run -----------------------------------------------------------
    def run(self):
        obj = self.alloc(0x900, 16)
        self.uc.mem_write(obj, b"\0" * 0x900)
        dev = self.alloc(0x100, 16)
        self.uc.mem_write(obj + 0xC, struct.pack("<I", dev))
        esp = STACK + STACK_SIZE - 0x1000
        # [esp] is the callee's return address; the function's own pushes land
        # below it, so the frame unwinds back onto the sentinel.
        self.uc.mem_write(esp, struct.pack("<I", SENTINEL))
        self.uc.reg_write(UC_X86_REG_ESP, esp)
        self.uc.reg_write(UC_X86_REG_EBP, esp - 0x8000)
        self.uc.reg_write(UC_X86_REG_ECX, obj)
        # A3DSkySphere ctor (0x799610) fields that Init reads: the band count
        # word 0x4c0 = 6, the angular step 0x4c4 = 0x3e9c61aa and the radius
        # 0x4c8 = 50.0f.  (The ctor itself also walks engine vtables, so the
        # three constants are installed directly.)
        self.uc.mem_write(obj + 0x4C0, struct.pack("<H", 6))
        self.uc.mem_write(obj + 0x4C4, struct.pack("<I", 0x3E9C61AA))
        self.uc.mem_write(obj + 0x4C8, struct.pack("<I", 0x42480000))
        try:
            for start in (SURROUND, ROOF):
                self.uc.emu_start(start, SENTINEL, count=4000000)
                self.uc.reg_write(UC_X86_REG_ESP, esp)
                self.uc.reg_write(UC_X86_REG_ECX, obj)
        except Exception as exc:
            eip = self.uc.reg_read(UC_X86_REG_EIP)
            esp = self.uc.reg_read(UC_X86_REG_ESP)
            print("fault %s at eip=%#x esp=%#x" % (exc, eip, esp), file=sys.stderr)
            print("trace:", " ".join("%#x" % a for a in list(self.trace)[-24:]), file=sys.stderr)
            try:
                print("bytes", bytes(self.uc.mem_read(eip - 16, 32)).hex(), file=sys.stderr)
            except Exception:
                pass
            raise
        return obj

    def mesh(self, which=-1):
        """Return (vertices, indices, nverts, nindices) for one stream.

        Stream creation order is the client's: 0 = surround curtain (this+0x584,
        built by A3DSkySphere::Init at 0x79a330), 1 = polar cap (this+0x588,
        built at 0x79a810).
        """
        s = self.streams[which]
        raw = bytes(self.uc.mem_read(s["verts"], s["nv"] * 32))
        verts = [struct.unpack_from("<3fI2f2f", raw, i * 32) for i in range(s["nv"])]
        idx = struct.unpack("<%dH" % s["ni"], bytes(self.uc.mem_read(s["indices"], s["ni"] * 2)))
        return verts, idx, s["nv"], s["ni"]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--csv")
    ap.add_argument("--json")
    a = ap.parse_args()
    sky = Sky()
    sky.run()
    verts, idx, nv, ni = sky.mesh()
    rows = [{"i": i, "x": v[0], "y": v[1], "z": v[2], "diffuse": (v[3],),
             "u": v[4], "v": v[5]} for i, v in enumerate(verts)]
    print("vertices %d indices %d triangles %d" % (nv, ni, ni // 3))
    for r in rows:
        print("%3d dir(%8.3f %8.3f) r=%6.2f y=%8.3f uv=(%.4f %.4f) a=%d"
              % (r["i"], r["x"], r["z"], (r["x"] ** 2 + r["z"] ** 2) ** 0.5, r["y"],
                 r["u"], r["v"], r["diffuse"][0] >> 24))
    print("first 24 indices", idx[:24])
    if a.csv:
        with open(a.csv, "w") as f:
            f.write("index,x,y,z,diffuse,u,v\n")
            for r in rows:
                f.write("%d,%.6f,%.6f,%.6f,%d,%.6f,%.6f\n"
                        % (r["i"], r["x"], r["y"], r["z"], r["diffuse"][0] >> 24, r["u"], r["v"]))
    if a.json:
        with open(a.json, "w") as f:
            json.dump({"vertices": rows, "indices": list(idx)}, f, indent=1)


if __name__ == "__main__":
    sys.exit(main())
