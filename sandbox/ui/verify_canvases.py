"""Check lpui.canvases: every live text canvas rebuilt from its draw list (Text_DrawString calls since its last clear) and
its box's own string equals its tile buffer in the captured RAM.
    .venv/bin/python sandbox/ui/verify_canvases.py TRACE_DIR"""
import collections, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import ds2d
from lpui import canvases

if __name__ == '__main__':
    res = collections.Counter(); bad = []
    for f, path, draws in canvases.walk(sys.argv[1]):
        ram = open(os.path.join(path, 'ram.bin'), 'rb').read()
        cap = ds2d.Capture(path)
        for sp in canvases.specs(ram, draws):
            bx = sp.box
            mem = np.frombuffer(cap.vram[bx.mem], np.uint8)
            if not sp.on_screen(mem):
                res['not on screen'] += 1
                continue
            m, a, b = sp.tile_range()
            got = mem[a:b]
            best = None
            for cl in (2, 1):
                d = int((np.frombuffer(sp.canvas(cl).tiles(), np.uint8) != got).sum())
                best = d if best is None else min(best, d)
            res[best == 0] += 1
            if best:
                bad.append((f, hex(bx.addr), (bx.w, bx.h), best, [d[0][:10] for d in sp.draws][:4], bx.string[:10]))
    print('live text canvases rebuilt from their draw lists, equal to RAM:', dict(res))
    seen = set()
    for b in bad:
        if b[1] not in seen:
            seen.add(b[1]); print('  differs:', b)
