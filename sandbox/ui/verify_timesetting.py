"""Check lpui.logic.timesetting: on every capture with the clock setting screen (overlay 42 state in RAM), the digit,
weekday and AM/PM sprites computed from the state's date / time equal the sprite objects' sequences and positions.
    .venv/bin/python sandbox/ui/verify_timesetting.py TRACE_DIR"""
import collections, glob, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui import sprites
from lpui.logic import timesetting
B = 0x2000000

if __name__ == '__main__':
    res = collections.Counter(); bad = []
    for d in sorted(glob.glob(os.path.join(sys.argv[1], 'cap-f*'))):
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        rd = lambda a: struct.unpack_from('<I', ram, a - B)[0]
        st = rd(0x216c8a0)
        if not B <= st < B + 0x400000 or not 2000 <= rd(st + 4) + 2000 <= 2099 or not 1 <= rd(st + 8) <= 12:
            continue
        ts = timesetting.TimeSetting(rd(st + 4), rd(st + 8), rd(st + 0xc), rd(st + 0x14), rd(st + 0x18))
        want = ts.sprites()

        def obj(off):
            h = rd(st + off)
            p = rd(0x2150c80 + 0x38 * h) if h < 0x9c else h
            return sprites.SprObj(ram, p) if B <= p < B + 0x400000 else None
        got = {}
        for name, slot, x, yc in timesetting.FIELDS:
            for i in range(4 if name == 'year' else 2):
                o = obj(0x38 + 4 * (slot + i))
                if o:
                    got[slot + i] = (o.seq, o.x >> 16, o.y >> 16)
        for k, off in (('weekday', 0x54), ('ampm', 0x58)):
            o = obj(off)
            got[k] = o.seq if o else None
        ok = all(got.get(k) == v for k, v in want.items())
        res[ok] += 1
        if not ok:
            bad.append((os.path.basename(d), {k: (want[k], got.get(k)) for k in want if got.get(k) != want[k]}))
    print('clock setting captures, sprites computed from the date / time equal RAM:', dict(res))
    for b in bad[:5]:
        print('  ', b)
