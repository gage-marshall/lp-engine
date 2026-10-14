"""Check lpui.sprites: OAM rebuilt from the sprite objects in each capture's RAM vs the captured OAM (visible entries, in
order). The capture's OAM is what the hardware shows: the OAM manager fills it from the start; unused entries are hidden.
    .venv/bin/python sandbox/ui/verify_sprites.py CAPTURE_DIR..."""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui import sprites


def visible(oam, base):
    out = []
    for i in range(128):
        a0, a1, a2 = struct.unpack_from('<3H', oam, base + 8 * i)
        if (a0 >> 8) & 3 == 2:
            continue
        out.append((a0, a1, a2))
    return out


if __name__ == '__main__':
    tot = {'main': [0, 0], 'sub': [0, 0]}
    full, caps = {'main': 0, 'sub': 0}, {'main': 0, 'sub': 0}
    for d in sys.argv[1:]:
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        oam = open(os.path.join(d, 'oam.bin'), 'rb').read()
        mine = sprites.oam_from_ram(ram)
        for eng, base in (('main', 0), ('sub', 0x400)):
            got = visible(oam, base)
            m = [e for e in mine[eng] if (e[0] >> 8) & 3 != 2]
            same = sum(1 for a, b in zip(m, got) if a == b)
            tot[eng][0] += same; tot[eng][1] += len(m)
            full[eng] += same == len(m)
            caps[eng] += 1
    for eng in tot:
        print('%s: %d / %d sprite OAM entries rebuilt equal the hardware OAM (%.2f%%); %d / %d captures fully equal' % (
            eng, tot[eng][0], tot[eng][1], 100 * tot[eng][0] / max(1, tot[eng][1]), full[eng], caps[eng]))
