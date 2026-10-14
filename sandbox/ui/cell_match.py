"""Identify the NNS cells (NCER) a captured OAM is built from: a cell matches where all its OBJs appear with the same
relative position, shape/size, flips and relative tile numbers.
    .venv/bin/python sandbox/ui/cell_match.py CAPTURE_DIR ENGINE ARCHIVE_DIR..."""
import glob, os, struct, sys
sys.path.insert(0, 'tools'); sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns2d, layers  # noqa: E402


def live_objs(d, eng):
    r, pal, bg, obj, oam = layers.load(d, eng)
    _, o = layers.objs(r, pal, obj, oam)
    return [x for x in o if -64 < x['y'] < 192 and not (x['y'] == -64)]


def cell_objs(c):
    out = []
    for a0, a1, a2 in c['oams']:
        o = nns2d.oam_decode(a0, a1, a2)
        out.append(dict(x=o['x'], y=o['y'], w=o['w'], h=o['h'], hf=o['hflip'], vf=o['vflip'], tile=o['tile'], pal=o['pal']))
    return out


def match(live, cobjs):
    """[(anchor x, y, tile base, palette)] where the whole cell is present"""
    if not cobjs:
        return []
    c0 = cobjs[0]
    hits = []
    for s in live:
        if (s['w'], s['h'], s['hf'], s['vf']) != (c0['w'], c0['h'], c0['hf'], c0['vf']):
            continue
        ox, oy, ot = s['x'] - c0['x'], s['y'] - c0['y'], s['tile'] - c0['tile']
        ok = True
        for c in cobjs[1:]:
            if not any(l['x'] == ox + c['x'] and l['y'] == oy + c['y'] and (l['w'], l['h'], l['hf'], l['vf']) == (c['w'], c['h'], c['hf'], c['vf'])
                       and l['tile'] - c['tile'] == ot for l in live):
                ok = False; break
        if ok:
            hits.append((ox, oy, ot, s['pal']))
    return hits


if __name__ == '__main__':
    d, eng = sys.argv[1], sys.argv[2]
    live = live_objs(d, eng)
    print(len(live), 'visible OBJs')
    found = []
    for arch in sys.argv[3:]:
        for f in sorted(glob.glob(os.path.join(arch, '*.ncer'))):
            try:
                ce = nns2d.parse_ncer(open(f, 'rb').read())
            except Exception:
                continue
            for ci, c in enumerate(ce['cells']):
                co = cell_objs(c)
                if len(co) < 2:
                    continue
                for h in match(live, co):
                    found.append((f, ci, len(co), h))
    for f, ci, n, h in found:
        print('%s cell %d (%d objs) at x %d y %d tile+%d pal %d' % (f, ci, n, *h))
