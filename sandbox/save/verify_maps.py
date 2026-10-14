"""Check bitmaps.json (derived on one savestate by derive_maps.py) against the game's own pack / unpack code:
  1. every backup: model unpack(card block) == game unpack, model pack(game image) == the card block;
  2. canonical images (game unpack of random blocks): model pack == game pack, and model unpack(model pack(img)) == img.
    .venv/bin/python sandbox/save/verify_maps.py STATE.dst [...]"""
import json, os, random, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
import savefmt as sf  # noqa: E402
import derive_maps as dm  # noqa: E402
from bitmap import Oracle  # noqa: E402

MAPS = json.load(open(os.path.join(HERE, 'bitmaps.json')))


def m_unpack(kind, packed):
    m = MAPS[kind]
    return dm.apply_map(m['unpack'], packed, bytes.fromhex(m['unpack_const']), m['struct_size'], m['unpack_bools'])


def m_pack(kind, img):
    m = MAPS[kind]
    return dm.apply_map(m['pack'], img, bytes.fromhex(m['pack_const']), m['packed_size'])


def game_pack(orc, kind, img):
    pe = orc.pe
    pe.reset()
    base = dm.BASE_B if kind == 'B' else orc.base[kind]
    pe.uc.mem_write(base, img)
    return pe.pack(kind, src=base, writers=())[0]


if __name__ == '__main__':
    for dst in sys.argv[1:]:
        orc = Oracle(dst)
        card = sf.Card.open(dst[:-4] + '.sav')
        rng = random.Random(hash(dst) & 0xffff)
        line = []
        for kind in 'ABC':
            real = card.read(kind)
            g = orc.unpack(kind, real)
            ok1 = m_unpack(kind, real) == g and m_pack(kind, g) == real
            fixed, total = dm.fixed_packed(kind)
            var = [8 * i + b for i in range(total) if i not in fixed for b in range(8)]
            ok2 = 0
            for r in range(8):
                blk = dm.setbits(real, var, [v for v in var if rng.random() < 0.5])
                img = orc.unpack(kind, blk)
                gp = game_pack(orc, kind, img)
                ok2 += gp == m_pack(kind, img) and m_unpack(kind, m_pack(kind, img)) == img
            line.append('%s backup %s, canonical %d/8' % (kind, 'exact' if ok1 else 'MISMATCH', ok2))
        print(os.path.basename(dst), '; '.join(line))
