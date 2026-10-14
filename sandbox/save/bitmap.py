"""Derive the save's packed-bit -> struct-bit map with the game's own unpacker as the oracle (index-coded patterns), then
check the model on random blocks.
    .venv/bin/python sandbox/save/bitmap.py STATE.dst KIND [n_random]
Fixed (not varied): all of section 0 (count, tag, version, struct size, ...) and entry 0 (byte size) of every section."""
import os, random, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import packemu, savefmt as sf  # noqa: E402
from unicorn.arm_const import UC_ARM_REG_R0  # noqa: E402

SIZES = {'A': 0x1f8, 'B': 0x3c58, 'C': 0xf4, 'D': 0x1002, 'E': 0xc4, 'F': 0x14c8}   # D/E/F: MI_CpuFill8 in UnpackSave*
OWN = 0x3300000           # our own struct buffer for D / E / F (their game structs are not allocated in the recorded RAM)


class Oracle:
    def __init__(self, dst):
        self.pe = packemu.PackEmu(dst)
        self.base = {'A': self.pe.w32(0x21544e8 + 4), 'C': self.pe.w32(0x21544e8 + 8)}
        self.ok = None

    def unpack(self, kind, packed):
        """struct image produced by the game's unpacker, or None if it rejected the block"""
        pe = self.pe
        pe.reset()                                       # ov00 io state left by a previous pack / unpack
        io = pe._alloc(len(packed) + 0x20)
        pe.uc.mem_write(io, packed)
        pe.uc.mem_write(0x2166a84 + 8, struct.pack('<I', io))
        n = SIZES[kind]
        if kind == 'B':
            got = {}

            def commit(uc):
                got['t'] = pe.rd(uc.reg_read(UC_ARM_REG_R0), n)
                pe._ret(uc, 0)
            pe.stub(0x2063274, commit)
            pe.call('Ov00Init_UnpackSaveBFromBuffer')
            return got.get('t')
        if kind in 'DEF':                               # the FromBuffer wrappers' steps, into our buffer
            pe.uc.mem_map(OWN, 0x10000)
            pe.uc.mem_write(OWN, b'\xa5' * n)
            k = packemu.KINDS[kind]
            pe.call(k['init'])
            size = pe.call(k['size'])
            assert size == len(packed)
            buf = pe._alloc(size)
            pe.call(k['setbuf'], buf)
            pe.uc.mem_write(buf, packed)
            r = pe.call('Ov00Init_UnpackSave%s' % kind, OWN)
            return pe.rd(OWN, n) if r else None
        pe.uc.mem_write(self.base[kind], b'\xa5' * n)
        pe.call('Ov00Init_UnpackSave%sFromBuffer' % kind)
        out = pe.rd(self.base[kind], n)
        return None if out == b'\xa5' * n else out


def variable_bits(kind):
    lay, total = sf.layout(kind)
    fixed = set()
    for s, sec in enumerate(lay):
        for e, (o, size, cnt) in enumerate(sec):
            if s == 0 or e == 0:
                fixed.update(range(o, o + size * cnt))
    return [8 * i + b for i in range(total) if i not in fixed for b in range(8)], total


def apply(real, var, bits_on):
    out = bytearray(real)
    for i in var:
        out[i >> 3] &= ~(1 << (i & 7))
    for i in bits_on:
        out[i >> 3] |= 1 << (i & 7)
    return bytes(out)


def bitsof(img):
    return int.from_bytes(img, 'little')


def derive(orc, kind, real):
    var, total = variable_bits(kind)
    n = SIZES[kind] * 8
    K = max(1, (len(var) - 1).bit_length())
    z = orc.unpack(kind, apply(real, var, []))
    one = orc.unpack(kind, apply(real, var, var))
    assert z is not None and one is not None, 'all-zero / all-one block rejected'
    z, one = bitsof(z), bitsof(one)
    codes = [0] * n
    for k in range(K):
        on = [v for j, v in enumerate(var) if j >> k & 1]
        img = orc.unpack(kind, apply(real, var, on))
        assert img is not None, 'pattern %d rejected' % k
        d = bitsof(img) ^ z
        # also the complement pattern: a mapped bit must read the opposite way
        img2 = orc.unpack(kind, apply(real, var, [v for j, v in enumerate(var) if not j >> k & 1]))
        d2 = bitsof(img2) ^ z
        for t in range(n):
            a, b = d >> t & 1, d2 >> t & 1
            if a == b:
                codes[t] = None if codes[t] is None or a != (one ^ z) >> t & 1 else codes[t]
                if a != (one ^ z) >> t & 1 or True:
                    if not (a == 0 and (one ^ z) >> t & 1 == 0):
                        codes[t] = None
            elif codes[t] is not None:
                codes[t] |= a << k
    smap = {}
    for t in range(n):
        if (one ^ z) >> t & 1 and codes[t] is not None and codes[t] < len(var):
            smap[t] = var[codes[t]]                      # struct bit t <- packed bit var[code] (xor its zero value)
    return smap, z, var


def predict(smap, z, packed, n):
    p = bitsof(packed)
    s = z
    for t, i in smap.items():
        s = (s & ~(1 << t)) | (((p >> i & 1) ^ (z >> t & 1) ^ 1 if False else (p >> i & 1)) << t)
    return s.to_bytes(n, 'little')


if __name__ == '__main__':
    dst, kind = sys.argv[1], sys.argv[2]
    nr = int(sys.argv[3]) if len(sys.argv) > 3 else 20
    orc = Oracle(dst)
    card = sf.Card.open(dst[:-4] + '.sav')
    real = card.read(kind)
    smap, z, var = derive(orc, kind, real)
    n = SIZES[kind]
    # polarity: a mapped struct bit equals its packed bit when the zero pattern gives 0
    inv = [t for t in smap if z >> t & 1]
    used = set(smap.values())
    print(kind, 'variable packed bits', len(var), 'mapped struct bits', len(smap), 'distinct sources', len(used), 'inverted', len(inv))
    rng = random.Random(1)
    bad = {}
    for r in range(nr):
        on = [v for v in var if rng.random() < 0.5]
        blk = apply(real, var, on)
        got = orc.unpack(kind, blk)
        if got is None:
            print('random', r, 'rejected'); continue
        pb, gb = bitsof(blk), bitsof(got)
        want = z
        for t, i in smap.items():
            want = (want & ~(1 << t)) | ((pb >> i & 1) << t)
        x = want ^ gb
        while x:
            t = (x & -x).bit_length() - 1; x &= x - 1
            bad[t] = bad.get(t, 0) + 1
    byts = sorted(set(t >> 3 for t in bad))
    print('random check: struct bytes where the bit-map model fails:', len(byts), [hex(b) for b in byts[:60]])
    unm = sorted(set(var) - used)
    print('packed bits not feeding any struct bit:', len(unm), 'bytes', sorted(set(i >> 3 for i in unm))[:60])


def ranges(xs):
    out = []
    for x in sorted(xs):
        if out and x == out[-1][1] + 1:
            out[-1][1] = x
        else:
            out.append([x, x])
    return ['%#x-%#x' % (a, b) if a != b else '%#x' % a for a, b in out]
