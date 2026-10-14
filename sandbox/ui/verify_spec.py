"""Render every capture's screen spec (lpui.spec: files + text canvases + sprites from files + registers; no RAM, no
captured memory) and compare with the composite of the capture's real VRAM. Writes the specs (one JSON per capture) and
a sheet of the worst screens.
    .venv/bin/python sandbox/ui/verify_spec.py TRACE_DIR OUT_DIR"""
import collections, json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import ds2d
from lpui import spec as S

if __name__ == '__main__':
    trace, out = sys.argv[1], sys.argv[2]
    os.makedirs(out, exist_ok=True)
    rows = []
    for f, path, sp in S.extract(trace):
        json.dump(sp, open(os.path.join(out, 'spec-f%d.json' % f), 'w'))
        (t, tu), (b, bu) = S.render(sp)
        (t2, tu2), (b2, bu2) = ds2d.both(ds2d.Capture(path))
        dt = int(((np.abs(t - t2).max(-1) > 0) & ~tu & ~tu2).sum())
        db = int(((np.abs(b - b2).max(-1) > 0) & ~bu & ~bu2).sum())
        unk = int((tu | bu).sum())
        dc = sp['regs']['main']['000'] if isinstance(sp['regs']['main'], dict) else 0
        scene = bool(int(str(dc), 0) & 8 if isinstance(dc, str) else dc & 8) or ((int(str(dc), 0) if isinstance(dc, str) else dc) >> 16 & 3) == 2
        rows.append((f, dt, db, unk, scene, S.residual_bytes(sp)))
    for name, sel in (('menu', [r for r in rows if not r[4]]), ('scene', [r for r in rows if r[4]])):
        print('%s screens %d: top identical %d, bottom identical %d, both %d; mean differing pixels %.0f' % (
            name, len(sel), sum(r[1] == 0 for r in sel), sum(r[2] == 0 for r in sel), sum(r[1] == 0 and r[2] == 0 for r in sel),
            np.mean([r[1] + r[2] for r in sel]) if sel else 0))
    import collections
    rt = collections.Counter()
    for r in rows:
        rt.update(r[5])
    print('residual bytes per capture (generated content no file or widget explains yet):', {k: v // len(rows) for k, v in rt.items()})
    tot = len(rows)
    print('%d screen specs rendered; top screen identical %d, bottom identical %d, both %d' % (
        tot, sum(r[1] == 0 for r in rows), sum(r[2] == 0 for r in rows), sum(r[1] == 0 and r[2] == 0 for r in rows)))
    print('pixels differing (of 2 x 49152 per capture): mean %.0f' % np.mean([r[1] + r[2] for r in rows]))
    print('worst menu:', sorted([r for r in rows if not r[4]], key=lambda r: -(r[1] + r[2]))[:15])
    json.dump(rows, open(os.path.join(out, 'result.json'), 'w'))
