"""Minimal x86/x87 interpreter for elementclient's CELGrassLand math.

Recovering the wind spring needed the exact FPU stack order of
`0x651840` (force), `0x651960` (integrate) and `0x651a70` (driver), which is
easy to misread by eye. This runs those routines against a flat virtual memory
image so the formulas can be checked numerically:

    python3 - <<'PY'
    import sys; sys.path[:0] = ["tools", "tools/x87"]
    from interp import Machine
    m = Machine()                       # ecx = CELGrassLand, +0x14 = spring
    m.store(0x30000000 + 0x48, 0.1)     # 0x6517f0 defaults
    m.store(0x30000000 + 0x4c, 2.0)
    m.store(0x30000000 + 0x50, 0.1)
    m.store(0x30000000 + 0x54, 0.1)
    m.reg["esp"] = 0x2fffff00
    m.reg["ecx"] = 0x30000000
    m.push(0)                           # return address
    m.push(1.0)                         # milliseconds for 0x651960
    m.run(0x651960)
    PY

Only the instruction subset those helpers use is implemented (plus the tiny
float helpers `0x401ca0` set3, `0x401cc0` copy3, `0x401ce0` scale3,
`0x4071f0` add3, `0x407220` sub3). Memory words are raw 32-bit patterns so the
integer and x87 views never alias. Addresses are virtual; globals are read
straight out of `google_drive/elementclient.exe` through `tools/pe.py`.

## Results this produced (do not re-derive by eye)

* `0x651960` field roles, confirmed by single-stepping: `+0x00` position,
  `+0x0c` velocity, `+0x18` acceleration, `+0x3c` displacement.
* The integrator is `H = disp / [ecx+0x48]` (= disp * 10), then
  `pos += vel*h + 0.5*h*h*H` and `vel += h*H`, where **`h` is the raw
  argument** (the driver's `ms*0.001` seconds), *not* a `+0x54`-scaled step.
  `0x651960` applies `h` as the raw argument it is handed. Check with
  `pos=(7,8,9)`, `vel=(1,2,3)`, `disp=(0.5,0.6,0.7)`: `h=1 -> (10.5, 6)`,
  `h=16 -> (663, 81)`, `h=33 -> (2762.5, 166)` for `(pos[0], vel[0])`;
  `pos += h*vel + 0.5*h^2*disp/+0x48` and `vel += h*disp/+0x48` reproduce all
  six. `+0x4c`, `+0x50`, `+0x54` are never read by `0x651960`. What the driver
  passes in is still open: `0x651a70` converts ms to seconds for its own vector
  update but hands `0x651960` the unconverted value.
* `0x651840` rewrites `+0x00..+0x08`, `+0x18..+0x2c` and `+0x3c..+0x44` from
  the position, the acceleration and the `+0x30..+0x38` accumulator; the
  shipped lane it stores into `+0x24` lands in `[+0x24, +0x2c)` for lane 0 but
  `[+0x28, +0x30)` and `[+0x2c, +0x34)` for lanes 1 and 2, so its symbolic
  closure is still open. The driver form (`lea edi, [esi+0xc]`) also reads a
  different base for the vector at `+0x00..+0x08`, so do not assume Land
  `+0x00..+0x08` and that vector are the same field.
* `0x6517f0` stores `+0x50 = 0.0`, not 0.1 (`0x651800`); `+0x48` = `+0x54` =
  0.1 and `+0x4c` = 2.0.
* `0x650b40` calls `0x6517f0` with `lea ecx, [esi + 0x14]`, i.e. the spring
  sits at `CELGrassLand + 0x14`; zeroing Land `+0x00..+0x08` is separate.
* `0x651040`: after the clock/history work it computes `noise*+0x6c`,
  `noise*+0x70`, `noise*+0x74` and stores them as the spring *displacement*
  `+0x3c/+0x40/+0x44`, then calls `0x651a70`.
"""
import struct
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pe import PE  # noqa: E402

import capstone  # noqa: E402

try:
    PEIMG = PE()
except FileNotFoundError:
    raise SystemExit(
        "elementclient.exe is not staged. The interpreter reads the client image "
        "for globals and code, so fetch it first (it is git-ignored under "
        "google_drive/); the recovered constants it produced are listed in this "
        "module's docstring."
    )
MD = capstone.Cs(capstone.CS_ARCH_X86, capstone.CS_MODE_32)


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


class Machine:
    """Flat memory + integer registers + x87 stack."""

    def __init__(self):
        self.mem = {}          # va -> float (4 bytes)
        self.reg = {"eax": 0, "ebx": 0, "ecx": 0, "edx": 0,
                    "esi": 0, "edi": 0, "ebp": 0, "esp": 0}
        self.st = []
        self.trace = []
        self.steps = 0
        self.calls = []
        self.jump = None
        self.depth = 0        # nested `call` depth (helpers do not count)
        self.last_size = 0
        self.eip = 0

    # -- memory ---------------------------------------------------------
    # Words are raw 32-bit patterns; float/int views are chosen per
    # instruction, exactly like the hardware.
    def word(self, va):
        if va in self.mem:
            return self.mem[va]
        off = PEIMG.va2off(va)
        if off is not None and off + 4 <= len(PEIMG.d):
            return struct.unpack_from("<I", PEIMG.d, off)[0]
        return 0

    def load(self, va):
        return struct.unpack("<f", struct.pack("<I", self.word(va)))[0]

    def load_i(self, va):
        w = self.word(va)
        return w - (1 << 32) if w & 0x80000000 else w

    def store(self, va, v):
        self.mem[va] = struct.unpack("<I", struct.pack("<f", f32(v)))[0]

    def store_i(self, va, v):
        self.mem[va] = v & 0xFFFFFFFF

    # -- operand helpers ------------------------------------------------
    def reg_of(self, name):
        name = name.strip()
        if name.startswith("fs:"):
            return 0      # no SEH chain in this sandbox; keeps `mov fs:[0]` harmless
        if name in ("eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"):
            return self.reg[name]
        if name.startswith("st("):
            return self.st[int(name[3:-1])]
        if name.startswith("0x"):
            return int(name, 16)
        if name.lstrip("-").isdigit():
            return int(name)
        if name.startswith("ds:"):
            return self.load(int(name[3:], 16))
        raise ValueError("unknown operand " + name)

    def addr(self, inner):
        """Resolve an effective address inside [ ... ]."""
        inner = inner.strip()
        total = 0
        sign = 1
        for token in inner.replace("-", "+-").split("+"):
            token = token.strip()
            if not token:
                continue
            if token.startswith("-"):
                sign = -1
                token = token[1:]
            if "*" in token:
                a, b = token.split("*")
                total += sign * self.reg_of(a.strip()) * int(b.strip(), 0)
            elif token in self.reg:
                total += sign * self.reg[token]
            else:
                total += sign * int(token, 0)
            sign = 1
        return total

    def src_i(self, op):
        """Integer view of an operand."""
        op = op.strip()
        for pre in ("dword ptr ", "byte ptr ", "word ptr ", "qword ptr "):
            if op.startswith(pre):
                op = op[len(pre):].strip()
                break
        if op.startswith("["):
            return self.load_i(self.addr(op[1:-1]))
        return self.reg_of(op)

    def src_f(self, op):
        """float view (only used by the x87 unit)."""
        op = op.strip()
        if op.startswith("dword ptr "):
            op = op[len("dword ptr "):].strip()
        if op.startswith("["):
            return self.load(self.addr(op[1:-1]))
        if op.startswith("st("):
            return self.st[int(op[3:op.index(")")])]
        return float(op, 0)

    def dst_f(self, op):
        """float store target: returns a callable writer."""
        op = op.strip()
        if op.startswith("dword ptr "):
            op = op[len("dword ptr "):].strip()
        if op.startswith("["):
            va = self.addr(op[1:-1])
            return lambda v: self.store(va, v)
        if op.startswith("st("):
            i = int(op[3:op.index(")")])
            def wr(v):
                while len(self.st) <= i:
                    self.st.append(0.0)
                self.st[i] = f32(v)
            return wr
        raise ValueError("float store " + op)

    def dst_va(self, op):
        op = op.strip()
        if op.startswith("dword ptr "):
            op = op[len("dword ptr "):].strip()
        if op.startswith("["):
            return self.addr(op[1:-1])
        raise ValueError("not a memory operand " + op)

    # -- stack ----------------------------------------------------------
    def push(self, value):
        self.reg["esp"] -= 4
        self.store_i(self.reg["esp"], int(value))

    def push_arg(self, value):
        """Arguments of the float helpers are pushed as float bit patterns."""
        self.reg["esp"] -= 4
        self.store(self.reg["esp"], value)

    def push_f(self, value):
        self.reg["esp"] -= 4
        self.store(self.reg["esp"], value)

    def pop(self):
        v = self.load_i(self.reg["esp"])
        self.reg["esp"] += 4
        return v

    # -- x87 ------------------------------------------------------------
    def push_f(self, v):
        self.st.insert(0, f32(v))

    def pop_f(self):
        return self.st.pop(0) if self.st else 0.0

    def run(self, start, stop_va=None, max_steps=20000):
        """Execute from `start` until `ret` to the caller (stop_va)."""
        va = start
        while self.steps < max_steps:
            self.steps += 1
            off = PEIMG.va2off(va)
            if off is None:
                raise RuntimeError("bad address %#x" % va)
            insn = next(MD.disasm(PEIMG.d[off:off + 16], va))
            mn, op = insn.mnemonic, insn.op_str
            self.last_size = insn.size
            self.trace.append((va, mn, op))
            nxt = va + insn.size
            done = self.exec_insn(mn, op)
            if done:
                return
            j = self.jump
            self.jump = None
            if j is not None:
                va = j
            elif self.eip:
                va, self.eip = self.eip, 0
            else:
                va = nxt
        raise RuntimeError("step limit")

    # returns True when a `ret` was executed
    def exec_insn(self, mn, op):
        if mn == "mov":
            dst, src = [x.strip() for x in op.split(",")]
            v = self.src_i(src)
            if dst.startswith("["):
                # integer move: keep the raw word, do not reinterpret as float
                self.store_i(self.dst_va(dst), v)
            else:
                self.reg[dst] = v & 0xFFFFFFFF
        elif mn == "lea":
            dst, src = [x.strip() for x in op.split(",")]
            self.reg[dst] = self.addr(src[1:-1])
        elif mn == "push":
            self.push(self.src_i(op))
        elif mn == "pop":
            self.reg[op.strip()] = int(self.pop())
        elif mn == "add":
            dst, src = [x.strip() for x in op.split(",")]
            if dst.startswith("["):
                va = self.dst_va(dst)
                self.store_i(va, self.load_i(va) + self.src_i(src))
            else:
                self.reg[dst] = (self.reg[dst] + self.src_i(src)) & 0xFFFFFFFF
        elif mn == "sub":
            dst, src = [x.strip() for x in op.split(",")]
            if dst.startswith("["):
                va = self.dst_va(dst)
                self.store_i(va, self.load_i(va) - self.src_i(src))
            else:
                self.reg[dst] = (self.reg[dst] - self.src_i(src)) & 0xFFFFFFFF
        elif mn == "imul":
            parts = [x.strip() for x in op.split(",")]
            if len(parts) == 2:
                self.reg[parts[0]] = (self.reg[parts[0]] * int(self.src_i(parts[1]))) & 0xFFFFFFFF
            elif "," not in op:                     # imul r32  (eax * r)
                self.reg["eax"] = (self.reg["eax"] * self.src_i(op)) & 0xFFFFFFFF
            else:
                raise RuntimeError("imul form " + op)
        elif mn == "cdq":
            eax = self.reg["eax"]
            self.reg["edx"] = 0xFFFFFFFF if eax & 0x80000000 else 0
        elif mn == "sar":
            dst, src = [x.strip() for x in op.split(",")]
            v = self.reg[dst]
            if v & 0x80000000:
                v -= 1 << 32
            self.reg[dst] = (v >> int(src, 0)) & 0xFFFFFFFF
        elif mn == "and":
            dst, src = [x.strip() for x in op.split(",")]
            self.reg[dst] = self.reg[dst] & int(self.src_i(src))
        elif mn == "xor":
            dst, src = [x.strip() for x in op.split(",")]
            self.reg[dst] = self.reg[dst] ^ int(self.src_i(src))
        elif mn in ("cmp", "test"):
            dst, src = [x.strip() for x in op.split(",")]
            a = self.src_i(dst)
            b = self.src_i(src)
            self.zf = (a == b) if mn == "cmp" else ((a & b) == 0)
        elif mn == "je":
            if self.zf:
                self.jump = int(op, 16)
        elif mn == "jne":
            if not self.zf:
                self.jump = int(op, 16)
        elif mn in ("ret", "retn"):
            # callee pops its own return address (and any `ret N` immediates)
            imm = int(op, 0) if op else 0
            self.eip = self.pop()          # pop first
            self.reg["esp"] += imm         # then drop the arguments
            self.depth -= 1
            return self.depth < 0
        elif mn == "nop" or mn == "int3":
            pass
        elif mn.startswith("f"):
            self.exec_fpu(mn, op)
        elif mn == "call":
            try:
                target = int(op, 16)
            except ValueError:
                target = self.src_i(op) & 0xFFFFFFFF   # `call edi` style
            if target in (0x401ca0, 0x401cc0, 0x401ce0, 0x4071f0, 0x407220):
                self.call_helper(target)
            else:
                # real subroutine: push return address, jump
                assert self.jump is None
                self.jump = target
                self.depth += 1
                ret = self.trace[-1][0] + self.last_size
                self.push(ret & 0xFFFFFFFF)
        else:
            raise RuntimeError("unsupported %s %s @ %#x" % (mn, op, self.trace[-1][0]))
        return False

    def call_helper(self, target):
        esp = self.reg["esp"]
        if target == 0x401ca0:            # set3(dst, x, y, z)
            dst = self.reg["ecx"]
            x, y, z = (self.load(esp + 4), self.load(esp + 8), self.load(esp + 12))
            # set3(dst, x, y, z): the three values were pushed as floats
            self.store(dst, x); self.store(dst + 4, y); self.store(dst + 8, z)
            self.reg["esp"] = esp + 16
        elif target == 0x401cc0:          # copy3(dst, src)
            dst = self.reg["ecx"]
            src = self.load_i(esp + 4)
            self.store(dst, self.load(src))
            self.store(dst + 4, self.load(src + 4))
            self.store(dst + 8, self.load(src + 8))
            self.reg["esp"] = esp + 8
        elif target == 0x401ce0:          # scale3(dst, src, s)
            dst = self.reg["ecx"]
            src = self.load_i(esp + 4)
            s = self.load(esp + 8)
            self.store(dst, self.load(src) * s)
            self.store(dst + 4, self.load(src + 4) * s)
            self.store(dst + 8, self.load(src + 8) * s)
            self.reg["esp"] = esp + 12
        elif target == 0x4071f0:          # add3(dst, a, b)
            dst = self.reg["ecx"]
            a = self.load_i(esp + 4); b = self.load_i(esp + 8)
            for i in range(3):
                self.store(dst + i * 4, self.load(a + i * 4) + self.load(b + i * 4))
            self.reg["esp"] = esp + 12
        elif target == 0x407220:          # sub3(dst, a, b)
            dst = self.reg["ecx"]
            a = self.load_i(esp + 4); b = self.load_i(esp + 8)
            for i in range(3):
                self.store(dst + i * 4, self.load(a + i * 4) - self.load(b + i * 4))
            self.reg["esp"] = esp + 12
        else:
            raise RuntimeError("unsupported call %#x" % target)

    def exec_fpu(self, mn, op):
        if mn == "fld":
            self.push_f(self.src_f(op))
        elif mn == "fild":
            self.push_f(float(self.src_i(op)))
        elif mn == "fldz":
            self.push_f(0.0)
        elif mn == "fld1":
            self.push_f(1.0)
        elif mn == "fst":
            self.dst_f(op)(self.st[0])
        elif mn == "fstp":
            v = self.pop_f()
            self.dst_f(op)(v)
        elif mn == "fadd":
            self.fpu_arith(op, lambda a, b: a + b)
        elif mn == "faddp":
            a = self.pop_f(); b = self.pop_f(); self.push_f(b + a)
        elif mn == "fsub":
            self.fpu_arith(op, lambda a, b: a - b)
        elif mn == "fsubr":
            self.fpu_arith(op, lambda a, b: b - a)
        elif mn == "fsubp":
            a = self.pop_f(); b = self.pop_f(); self.push_f(b - a)
        elif mn == "fmul":
            self.fpu_arith(op, lambda a, b: a * b)
        elif mn == "fdiv":
            self.fpu_arith(op, lambda a, b: a / b)
        elif mn == "fdivr":
            self.fpu_arith(op, lambda a, b: b / a)
        elif mn == "fchs":
            self.st[0] = -self.st[0]
        elif mn == "fabs":
            self.st[0] = abs(self.st[0])
        elif mn == "fxch":
            i = 1
            if op.startswith("st("):
                i = int(op[3:op.index(")")])
            self.st[0], self.st[i] = self.st[i], self.st[0]
        else:
            raise RuntimeError("unsupported fpu %s %s" % (mn, op))

    def fpu_arith(self, op, fn):
        op = op.strip()
        if op.startswith("st(") and "," in op:
            a, b = [x.strip() for x in op.split(",")]
            i = int(a[3:a.index(")")])
            j = int(b[3:b.index(")")]) if b.startswith("st(") else i
            if b.startswith("st("):
                self.st[i] = f32(fn(self.st[i], self.st[j]))
            else:
                self.st[i] = f32(fn(self.st[i], self.src_f(b)))
        else:
            self.st[0] = f32(fn(self.st[0], self.src_f(op)))
