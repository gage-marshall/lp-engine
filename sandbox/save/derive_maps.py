"""Derive the save's bit maps in both directions with the game's own code as the oracle, and write sandbox/save/bitmaps.json.

    .venv/bin/python sandbox/save/derive_maps.py STATE.dst [n_random]

unpack: packed block -> struct image (`Ov00Init_UnpackSave{A,B,C}FromBuffer`)
pack:   struct image -> packed block (`Ov00Init_PackSave{A,B,C}`)
Index-coded patterns: input bits j with bit k of j set, and the complement, k = 0..log2(n); an output bit whose value
follows input j in every pattern is mapped to j. The model (bit map + bool bytes + constant bits) is then checked on random
inputs. Fixed inputs: packed section 0 entries 0-3 (byte size, tag, version, struct size) and entry 0 (byte size) of every
section."""
import json, os, random, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import packemu, savefmt as sf  # noqa: E402
from bitmap import Oracle, SIZES, OWN, ranges  # noqa: E402

BASE_B = 0x2142c8c


KINDS = 'ABCDEF'


def struct_base(orc, kind):
    """where the packer reads the struct from (after a reset): the game's struct, or our buffer for D / E / F"""
    if kind in 'DEF':
        orc.pe.uc.mem_map(OWN, 0x10000)
        return OWN
    return BASE_B if kind == 'B' else orc.base[kind]


def empty_block(orc, kind):
    """the game's packing of an all-zero struct (no D / E / F block in the recorded saves)"""
    pe = orc.pe
    pe.reset()
    base = struct_base(orc, kind)
    pe.uc.mem_write(base, bytes(SIZES[kind]))
    return pe.pack(kind, src=base, writers=())[0]


def fixed_packed(kind):
    lay, total = sf.layout(kind)
    fixed = set()
    for s, sec in enumerate(lay):
        for e, (o, size, cnt) in enumerate(sec):
            if e == 0 or (s == 0 and e <= 3):
                fixed.update(range(o, o + size * cnt))
    return fixed, total


def setbits(real, var, on):
    x = int.from_bytes(real, 'little')
    for i in var:
        x &= ~(1 << i)
    for i in on:
        x |= 1 << i
    return x.to_bytes(len(real), 'little')


def derive(f, real, var, nout):
    """bit map out_bit -> in_bit for the function f(bytes) -> bytes, varying the input bits `var` around `real`"""
    K = max(1, (len(var) - 1).bit_length())
    z = int.from_bytes(f(setbits(real, var, [])), 'little')
    one = int.from_bytes(f(setbits(real, var, var)), 'little') ^ z
    code = [0] * (8 * nout)
    alive = one
    for k in range(K):
        a = int.from_bytes(f(setbits(real, var, [v for j, v in enumerate(var) if j >> k & 1])), 'little') ^ z
        b = int.from_bytes(f(setbits(real, var, [v for j, v in enumerate(var) if not j >> k & 1])), 'little') ^ z
        alive &= a ^ b                                   # a mapped bit takes opposite values in a pattern and its complement
        x = a & alive
        while x:
            t = (x & -x).bit_length() - 1; x &= x - 1
            code[t] |= 1 << k
    m = {}
    x = alive
    while x:
        t = (x & -x).bit_length() - 1; x &= x - 1
        if code[t] < len(var):
            m[t] = var[code[t]]
    return m, z


def runs(m):
    """{out: in} -> [[out, in, n]] runs of consecutive bits"""
    out = []
    for t in sorted(m):
        if out and t == out[-1][0] + out[-1][2] and m[t] == out[-1][1] + out[-1][2]:
            out[-1][2] += 1
        else:
            out.append([t, m[t], 1])
    return out


def apply_map(rs, src, base, nout, bools=()):
    s = int.from_bytes(src, 'little')
    o = int.from_bytes(base, 'little')
    for t, i, n in rs:
        mask = (1 << n) - 1
        o = (o & ~(mask << t)) | (((s >> i) & mask) << t)
    out = bytearray(o.to_bytes(nout, 'little'))
    for so, po, n in bools:                              # struct byte = (packed byte != 0)
        for j in range(n):
            out[so + j] = 1 if src[po + j] else 0
    return bytes(out)


def check(f, model, real, var, nr, rng, nonzero_bits=None):
    bad = set()
    for _ in range(nr):
        x = setbits(real, var, [v for v in var if rng.random() < 0.5])
        got, want = f(x), model(x)
        d = int.from_bytes(got, 'little') ^ int.from_bytes(want, 'little')
        while d:
            t = (d & -d).bit_length() - 1; d &= d - 1
            bad.add(t >> 3)
    return bad


def main(dst, nr):
    orc = Oracle(dst)
    card = sf.Card.open(dst[:-4] + '.sav')
    rng = random.Random(7)
    path = os.path.join(HERE, 'bitmaps.json')
    out = json.load(open(path)) if os.path.exists(path) else {}
    for kind in KINDS:
        n = SIZES[kind]
        real = card.read(kind) if kind in 'ABC' else empty_block(orc, kind)
        fixed, total = fixed_packed(kind)
        pvar = [8 * i + b for i in range(total) if i not in fixed for b in range(8)]

        def U(p):
            r = orc.unpack(kind, p)
            assert r is not None, 'unpacker rejected a block'
            return r
        um, uz = derive(U, real, pvar, n)
        urs = runs(um)
        # bytes the map misses but the random check flags: probe for (packed != 0) booleans
        bools = []
        if kind == 'B':
            lay, _ = sf.layout('B')
            o = lay[60][1][0]
            nb = sum(s * c for _, s, c in lay[60][1:6])
            bools = [[0x3958, o, nb]]
            urs = [r for r in urs if not (0x3958 * 8 <= r[0] < (0x3958 + nb) * 8)]
        uconst = bytearray(uz.to_bytes(n, 'little'))
        # struct bytes the unpacker never writes (the oracle clobbers the struct with 0xa5 first): runtime fields that keep
        # their value over a load; a fresh load leaves them 0
        untouched = [i for i in range(n) if uconst[i] == 0xa5 and not any(t >> 3 == i for t in um)] if kind != 'B' else []
        for i in untouched:
            uconst[i] = 0
        uconst = bytes(uconst)
        def Uc(p):
            r = bytearray(U(p))
            for i in untouched:
                r[i] = 0
            return bytes(r)
        bad = check(Uc, lambda p: apply_map(urs, p, uconst, n, bools), real, pvar, nr, rng)
        print(kind, 'unpack: %d runs, %d struct bits mapped, %d bool bytes; random check bad struct bytes: %s'
              % (len(urs), sum(r[2] for r in urs), sum(b[2] for b in bools), ranges(bad)[:20]))

        # pack direction: the game's packer on a struct image
        img = Uc(real)
        def P(s):
            pe = orc.pe
            pe.reset()                                   # a second pack in the same instance fails (ov00 io state)
            base = struct_base(orc, kind)
            pe.uc.mem_write(base, s)
            buf, _, _ = pe.pack(kind, src=base, writers=())
            return buf
        assert P(img) == real, 'pack(unpack(real)) != real'
        svar = [8 * i + b for i in range(n) for b in range(8)]
        pm, pz = derive(P, img, svar, total)
        # on the images the game can hold (canonical: what an unpack produces) the packer inverts the unpacker; random
        # struct images break the pattern decoding where the packer ORs a field into its neighbours or stores (byte == 1),
        # so a packed bit the unpacker reads takes its source from the unpack map, and only the bits the unpacker ignores
        # keep the derived source
        inv = {}
        for t, i, nb in urs:
            for k in range(nb):
                inv.setdefault(i + k, t + k)
        pm = {i: s for i, s in pm.items() if i not in inv and not any(po * 8 <= i < (po + nb) * 8 for _, po, nb in bools)}
        pm.update(inv)
        for so, po, nb in bools:                         # bool bytes: a canonical struct byte is 0 / 1 = the packed byte
            for k in range(8 * nb):
                pm[po * 8 + k] = so * 8 + k
        prs = runs(pm)
        pconst = pz.to_bytes(total, 'little')
        # a struct bit the packer does not read must not matter; the constant packed bits are the section sizes / tag /
        # version, plus the unwritten entries (0)
        bad = set()
        for _ in range(nr):                               # canonical images: unpack of random blocks
            c = Uc(setbits(real, pvar, [v for v in pvar if rng.random() < 0.5]))
            d = int.from_bytes(P(c), 'little') ^ int.from_bytes(apply_map(prs, c, pconst, total), 'little')
            bad |= {t >> 3 for t in range(total * 8) if d >> t & 1}
        print(kind, 'pack:   %d runs, %d packed bits mapped; canonical check bad packed bytes: %s'
              % (len(prs), sum(r[2] for r in prs), ranges(bad)[:20]))
        # inverse consistency: unpack(pack(s)) keeps every struct bit that the packer reads
        out[kind] = dict(struct_size=n, packed_size=total, unpack=urs, unpack_bools=bools, pack=prs,
                         unpack_const=uconst.hex(), pack_const=pconst.hex(),
                         unpack_untouched=untouched)
        orc.pe.reset()
    json.dump(out, open(path, 'w'))
    print('-> bitmaps.json')


if __name__ == '__main__':
    if len(sys.argv) > 3:
        KINDS = sys.argv[3]
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 20)
