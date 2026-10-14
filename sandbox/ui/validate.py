"""Compositor check against the screenshots, fitting what captures cannot record: the scroll of layers the game animates
itself (the top screen's 「LIVE」 pattern: sub BG1, diagonal, period 32). Writes per-capture diff images.
    .venv/bin/python sandbox/ui/validate.py CAPTURE_DIR... --out DIR"""
import os, sys
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import ds2d  # noqa: E402


def fit_live(cap, top):
    if not cap.reg('sub', 0, 4) >> 9 & 1:
        return None
    orig = cap.scroll; bgpal = cap.pal[0x200:0x300].astype(np.int32) & 0x7fff
    best = (-1, 0)
    for s in range(32):
        cap.scroll = lambda e, k, s=s: (s, s) if (e, k) == ('sub', 1) else orig(e, k)
        ci, op = ds2d.text_bg(cap, 'sub', 1)
        sc = (np.abs(ds2d.to_rgb5(bgpal[ci]) - top).max(-1) <= 1)[op].mean() if op.any() else 0
        if sc > best[0]: best = (sc, s)
    cap.scroll = lambda e, k, s=best[1]: (s, s) if (e, k) == ('sub', 1) else orig(e, k)
    return best[1]


if __name__ == '__main__':
    out = sys.argv[sys.argv.index('--out') + 1]
    ds = [a for a in sys.argv[1:] if a not in ('--out', out)]
    os.makedirs(out, exist_ok=True)
    tot = [0, 0]
    for d in ds:
        cap = ds2d.Capture(d)
        shot = np.asarray(Image.open(os.path.join(d, 'shot.png')).convert('RGB')).astype(np.int32) >> 3
        sub_is_top = not (cap.regs['powcnt'] >> 15 & 1)
        live = 'reg' if cap.scroll_regs is not None else fit_live(cap, shot[:192] if sub_is_top else shot[192:])
        (t, tu), (b, bu) = ds2d.both(cap)
        res = []
        rows = []
        for img, unk, part in ((t, tu, shot[:192]), (b, bu, shot[192:])):
            diff = (np.abs(img - part).max(-1) > 1) & ~unk
            res.append('%5d/%5d' % (diff.sum(), (~unk).sum())); tot[0] += diff.sum(); tot[1] += (~unk).sum()
            rows.append(np.concatenate([img * 8, part * 8, np.where(diff[..., None], [255, 0, 0], np.where(unk[..., None], [80, 0, 80], [0, 0, 0]))], 1))
        Image.fromarray(np.concatenate(rows).astype(np.uint8)).save(os.path.join(out, os.path.basename(d) + '.png'))
        print('%-24s live %-4s top %s  bottom %s' % (os.path.basename(d)[:24], live, *res))
    print('total %d / %d (%.2f%%)' % (tot[0], tot[1], 100 * tot[0] / max(1, tot[1])))
