"""Find BL/BLX (ARM and Thumb) and literal-pool references to addresses across static + overlays.
    python3 sandbox/save/xref.py ADDR ..."""
import bisect, csv, json, os, struct, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MM = json.load(open(os.path.join(ROOT, 'content/engine/memory_map.json')))
LAB = {}
for r in csv.DictReader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
    LAB.setdefault(r['module'], {})[int(r['addr'], 16)] = r['name']


def fn(mod, a):
    d = LAB.get(mod, {})
    ks = sorted(d)
    i = bisect.bisect_right(ks, a) - 1
    return d[ks[i]] if i >= 0 else '?'


def modules():
    for name, m in MM.items():
        if name in ('itcm', 'dtcm'):
            continue
        p = os.path.join(ROOT, 'content/engine', ('static' if name == 'static' else name) + '.bin')
        if os.path.exists(p):
            yield name, m['base'], open(p, 'rb').read()


def xrefs(targets):
    targets = set(targets)
    out = []
    for mod, base, data in modules():
        for o in range(0, len(data) - 3, 2):
            a = base + o
            if o % 4 == 0:
                w = struct.unpack_from('<I', data, o)[0]
                if (w >> 24) & 0xf == 0xb and (w >> 28) != 0xf:            # ARM BL
                    off = w & 0xffffff
                    if off & 0x800000: off -= 0x1000000
                    t = a + 8 + off * 4
                    if t in targets: out.append((mod, a, t, 'bl'))
                elif (w >> 25) == 0x7d:                                       # ARM BLX imm
                    off = w & 0xffffff
                    if off & 0x800000: off -= 0x1000000
                    t = a + 8 + off * 4 + ((w >> 24) & 1) * 2
                    if t in targets: out.append((mod, a, t, 'blx'))
                if w in targets or (w & ~1) in targets:
                    out.append((mod, a, w & ~1, 'ptr'))
            h1, h2 = struct.unpack_from('<HH', data, o)
            if (h1 >> 11) == 0x1e and (h2 >> 11) in (0x1f, 0x1d):          # Thumb BL/BLX
                off = ((h1 & 0x7ff) << 12) | ((h2 & 0x7ff) << 1)
                if off & 0x400000: off -= 0x800000
                t = a + 4 + off
                if (h2 >> 11) == 0x1d: t &= ~3
                if t in targets: out.append((mod, a, t, 'tbl'))
    return out


if __name__ == '__main__':
    ts = [int(x, 16) for x in sys.argv[1:]]
    for mod, a, t, k in xrefs(ts):
        print('%-6s %#x %-4s -> %#x  in %s' % (mod, a, k, t, fn(mod, a)))
