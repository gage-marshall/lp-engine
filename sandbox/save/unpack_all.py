"""Loader proof on every recorded backup: the game's own `Ov00Init_UnpackSave{A,B,C}FromBuffer` run on the block read by
savefmt.Card must restore the RAM struct the block was packed from (the struct is clobbered with 0xa5 first).
    .venv/bin/python sandbox/save/unpack_all.py STATE.dst ..."""
import os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import packemu, savefmt as sf  # noqa: E402
from unpack_check import unpack_b, GS, GS_SIZE  # noqa: E402


def unpack_into(pe, kind, packed, base, size):
    pe.uc.mem_write(base, b'\xa5' * size)
    io = pe._alloc(len(packed) + 0x20)
    pe.uc.mem_write(io, packed)
    pe.uc.mem_write(0x2166a84 + 8, struct.pack('<I', io))
    r = pe.call('Ov00Init_UnpackSave%sFromBuffer' % kind)
    return r, pe.rd(base, size)


for dst in sys.argv[1:]:
    sav = dst[:-4] + '.sav'
    pe = packemu.PackEmu(dst)
    card = sf.Card.open(sav)
    res = []
    for kind, base, size in (('A', pe.w32(0x21544e8 + 4), 0x1f8), ('C', pe.w32(0x21544e8 + 8), 0xf4)):
        want = pe.rd(base, size)
        r, got = unpack_into(pe, kind, card.read(kind), base, size)
        res.append('%s %d/%d (ret %d)' % (kind, sum(a == b for a, b in zip(got, want)), size, r))
        pe.reset()
    tmp = unpack_b(pe, card.read('B'))
    ram = pe.rd(GS, GS_SIZE)
    res.append('B %d/%d' % (sum(a == b for a, b in zip(tmp, ram)), GS_SIZE))
    print(os.path.basename(dst), ' '.join(res))
