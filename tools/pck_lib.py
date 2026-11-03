"""Python port of src/pck.rs (retail v2 PCK reader), for archive surveys."""
import struct, zlib

MASK = 0xa8937462
CHECK = 0x59374231

def directory(path):
    f = open(path, 'rb')
    size = len(f.read())
    def rd(pos, n):
        f.seek(pos); b = f.read(n)
        assert len(b) == n, 'truncated'
        return b
    U = lambda b, p: struct.unpack_from('<I', b, p)[0]
    safe = rd(0, 12)
    assert U(safe, 0) == 0x4dca23ef and U(safe, 8) == 0x56a089b7, 'bad safe header'
    end = U(safe, 4)
    assert 292 <= end <= size, 'bad end %d' % end
    header = rd(end - 280, 280)
    assert U(header, 0) == 0xfdfdfeee and U(header, 268) == 0xf00dbeef, 'bad guard'
    version = U(header, 276)
    assert version in (0x20002, 0x20001), 'version %08x' % version
    assert U(header, 12) & 0x80000000 == 0, 'encrypted'
    count = U(header, 272)
    pos = U(header, 8) ^ MASK
    out = []
    for _ in range(count):
        h = rd(pos, 8)
        ln = U(h, 0) ^ MASK
        assert (U(h, 4) ^ MASK ^ CHECK) == ln, 'record len check'
        raw = rd(pos + 8, ln)
        data = raw if ln == 276 else zlib.decompress(raw)
        assert len(data) == 276, 'decompressed %d' % len(data)
        zero = data[:260].find(b'\0')
        if zero < 0: zero = 260
        name = data[:zero].decode('gbk', 'replace').replace('\\', '/')
        out.append((name, U(data, 260), U(data, 264), U(data, 268)))
        pos += 8 + ln
    f.close()
    return version, out

def read_entry(path, entry):
    name, off, usize, clen = entry
    f = open(path, 'rb'); f.seek(off); b = f.read(clen); f.close()
    return zlib.decompress(b) if clen < usize else b
