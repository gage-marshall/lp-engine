"""DS 2D engine (the parts LovePlus+ uses): both screens from a capture (sandbox/ui/capture.py).

Text BGs (4/8 bpp, mode 0), OBJs (normal, affine, bitmap; lowest OAM index on top), windows 0/1/OBJ, alpha blending
(BLDCNT/BLDALPHA, brightness up/down with BLDY), master brightness, display mode 2 (an LCDC bank). BG0 = 3D on the main
engine is not ours to draw here: its pixels come back flagged in `unknown`.
    .venv/bin/python sandbox/ui/ds2d.py CAPTURE_DIR... [--out DIR]   (renders, compares with shot.png)"""
import json, os, struct, sys
import numpy as np
from PIL import Image

H, W = 192, 256
SHAPES = {(0, 0): (8, 8), (0, 1): (16, 16), (0, 2): (32, 32), (0, 3): (64, 64), (1, 0): (16, 8), (1, 1): (32, 8), (1, 2): (32, 16),
          (1, 3): (64, 32), (2, 0): (8, 16), (2, 1): (8, 32), (2, 2): (16, 32), (2, 3): (32, 64)}


class Capture:
    def __init__(self, d):
        self.d = d
        rd = lambda n: open(os.path.join(d, n), 'rb').read()
        self.regs = json.load(open(os.path.join(d, 'regs.json')))
        self.pal = np.frombuffer(rd('pal.bin'), '<u2')
        self.oam = rd('oam.bin')
        self.vram = {k: rd('vram_%s.bin' % k) for k in ('bg_main', 'bg_sub', 'obj_main', 'obj_sub')}
        self.dtcm = rd('dtcm.bin') if os.path.exists(os.path.join(d, 'dtcm.bin')) else None
        self.lcdc = {b: rd('lcdc_%s.bin' % b) for b in 'ABCD' if os.path.exists(os.path.join(d, 'lcdc_%s.bin' % b))}
        sp = os.path.join(d, 'scroll.json')
        self.scroll_regs = json.load(open(sp)) if os.path.exists(sp) else None   # mirrored register writes (replay_trace)

    def reg(self, eng, off, size=2):
        sr = getattr(self, 'scroll_regs', None)
        if sr is not None and size == 2 and off in (0x54, 0x6c):            # write-only BLDY / master brightness: mirrored
            k = '%x' % ((0x4000000 if eng == 'main' else 0x4001000) + off)
            if k in sr:
                return sr[k]
        w = self.regs[eng]['%03x' % (off & ~3)]
        return (w >> (8 * (off & 3))) & ((1 << (8 * size)) - 1)

    def scroll(self, eng, n):
        """BG scroll from the game's shadow table (DTCM 0x27e25a0, `Display_ApplyBgScrollRegs`: flag byte per BG, y at
        +8+8i, x at +0xc+8i; main 0-3, sub 4-7) - the registers themselves are write-only"""
        sr = getattr(self, 'scroll_regs', None)
        if sr is not None:
            base = 0x4000010 if eng == 'main' else 0x4001010
            return sr.get('%x' % (base + 4 * n), 0), sr.get('%x' % (base + 4 * n + 2), 0)
        if self.dtcm is None:
            return 0, 0
        i = n + (4 if eng == 'sub' else 0)
        b = 0x25a0
        y, x = struct.unpack_from('<ii', self.dtcm, b + 8 + 8 * i)
        return x & 0x1ff, y & 0x1ff


def text_bg(cap, eng, n):
    """(colour index into the BG palette, opaque) of text BG n, scrolled"""
    dc = cap.reg(eng, 0, 4)
    cnt = cap.reg(eng, 8 + 2 * n)
    vram = np.frombuffer(cap.vram['bg_' + eng], np.uint8)
    cbase = (cnt >> 2 & 15) * 0x4000 + ((dc >> 24 & 7) * 0x10000 if eng == 'main' else 0)
    sbase = (cnt >> 8 & 31) * 0x800 + ((dc >> 27 & 7) * 0x10000 if eng == 'main' else 0)
    c256 = cnt >> 7 & 1
    lw, lh = [(256, 256), (512, 256), (256, 512), (512, 512)][cnt >> 14]
    sx, sy = cap.scroll(eng, n)
    ys = (np.arange(H)[:, None] + sy) % lh
    xs = (np.arange(W)[None, :] + sx) % lw
    blk = (xs // 256) + (ys // 256) * (lw // 256)
    tx, ty = (xs % 256) // 8, (ys % 256) // 8
    eo = sbase + blk * 0x800 + (ty * 32 + tx) * 2
    ent = vram[eo].astype(np.int32) | (vram[eo + 1].astype(np.int32) << 8)
    tile, hf, vf, pal = ent & 0x3ff, ent >> 10 & 1, ent >> 11 & 1, ent >> 12
    px = np.where(hf == 1, 7 - xs % 8, xs % 8)
    py = np.where(vf == 1, 7 - ys % 8, ys % 8)
    if c256:
        c = vram[(cbase + tile * 64 + py * 8 + px) % len(vram)].astype(np.int32)
        return c, c != 0
    b = vram[(cbase + tile * 32 + py * 4 + px // 2) % len(vram)].astype(np.int32)
    c = np.where(px & 1, b >> 4, b & 15)
    return pal * 16 + c, c != 0


def objs(cap, eng):
    """OBJ layer: (15-bit colour, opaque, priority, semi-transparent, obj-window mask)"""
    dc = cap.reg(eng, 0, 4)
    vram = cap.vram['obj_' + eng]
    oam = cap.oam[0x400:] if eng == 'sub' else cap.oam[:0x400]
    pal = cap.pal[0x300:0x400] if eng == 'sub' else cap.pal[0x100:0x200]
    col = np.zeros((H, W), np.int32); opq = np.zeros((H, W), bool); pri = np.full((H, W), 4, np.int32)
    semi = np.zeros((H, W), bool); win = np.zeros((H, W), bool)
    one_d, bound = dc >> 4 & 1, 32 << (dc >> 20 & 3)
    for i in range(127, -1, -1):
        a0, a1, a2 = struct.unpack_from('<HHH', oam, i * 8)
        affine, flag2 = a0 >> 8 & 1, a0 >> 9 & 1
        if not affine and flag2:
            continue
        mode, c256 = a0 >> 10 & 3, a0 >> 13 & 1
        w, h = SHAPES.get((a0 >> 14, a1 >> 14), (8, 8))
        y, x = a0 & 0xff, a1 & 0x1ff
        if x >= 256: x -= 512
        if y >= 192: y -= 256
        tile, prio, palno = a2 & 0x3ff, a2 >> 10 & 3, a2 >> 12
        bw, bh = (2 * w, 2 * h) if affine and flag2 else (w, h)
        yy, xx = np.mgrid[0:bh, 0:bw]
        if affine:
            g = a1 >> 9 & 31
            pa, pb, pc, pd = (struct.unpack_from('<h', oam, (g * 4 + k) * 8 + 6)[0] for k in range(4))
            cx, cy = xx - bw // 2, yy - bh // 2
            sx = (pa * cx + pb * cy >> 8) + w // 2
            sy = (pc * cx + pd * cy >> 8) + h // 2
            inside = (sx >= 0) & (sx < w) & (sy >= 0) & (sy < h)
        else:
            hf, vf = a1 >> 12 & 1, a1 >> 13 & 1
            sx = w - 1 - xx if hf else xx
            sy = h - 1 - yy if vf else yy
            inside = np.ones_like(xx, bool)
        sx, sy = np.clip(sx, 0, w - 1), np.clip(sy, 0, h - 1)
        if mode == 3:                                           # bitmap OBJ: direct 15-bit colour, bit 15 = opaque
            if dc >> 6 & 1:
                base = tile * (128 << (dc >> 22 & 1))
                stride = w * 2
            else:
                wide = 32 if dc >> 5 & 1 else 16
                base = ((tile % wide) * 0x10 + (tile // wide) * wide * 0x80)
                stride = wide * 16
            o = base + sy * stride + sx * 2
            raw = np.frombuffer(vram, '<u2')[np.clip(o // 2, 0, len(vram) // 2 - 1)].astype(np.int32)
            c, op = raw & 0x7fff, (raw >> 15 & 1).astype(bool) & inside
        else:
            if one_d:
                tb = tile * bound
                tw = w // 8
                to = tb + ((sy // 8) * tw + sx // 8) * (64 if c256 else 32)
            else:
                to = (tile + (sy // 8) * 32 + (sx // 8) * (2 if c256 else 1)) * 32
            v = np.frombuffer(vram, np.uint8)
            if c256:
                ci = v[np.clip(to + (sy % 8) * 8 + sx % 8, 0, len(v) - 1)].astype(np.int32)
                c, op = pal[ci].astype(np.int32) & 0x7fff, (ci != 0) & inside
            else:
                b = v[np.clip(to + (sy % 8) * 4 + (sx % 8) // 2, 0, len(v) - 1)].astype(np.int32)
                ci = np.where(sx & 1, b >> 4, b & 15)
                c, op = pal[palno * 16 + ci].astype(np.int32) & 0x7fff, (ci != 0) & inside
        X, Y = x + xx, y + yy
        if mode != 3:
            Y = Y % 256
            Y = np.where(Y >= 192, Y - 256, Y)
        X = np.where(X >= 256, X - 512, X)
        m = op & (X >= 0) & (X < W) & (Y >= 0) & (Y < H)
        Xs, Ys = X[m], Y[m]
        if mode == 2:
            win[Ys, Xs] = True
            continue
        col[Ys, Xs] = c[m]; opq[Ys, Xs] = True; pri[Ys, Xs] = prio; semi[Ys, Xs] = mode == 1
    return col, opq, pri, semi, win


def windows(cap, eng):
    """per-pixel layer-enable mask bits (0-3 BG, 4 OBJ, 5 colour effect) from WIN0/WIN1/OBJWIN/WINOUT"""
    dc = cap.reg(eng, 0, 4)
    if not dc >> 13 & 7:
        return None
    win0h, win1h = cap.reg(eng, 0x40), cap.reg(eng, 0x42)
    win0v, win1v = cap.reg(eng, 0x44), cap.reg(eng, 0x46)
    winin, winout = cap.reg(eng, 0x48), cap.reg(eng, 0x4a)
    m = np.full((H, W), winout & 0x3f, np.int32)
    ys, xs = np.mgrid[0:H, 0:W]

    def inside(h, v):
        x1, x2, y1, y2 = h >> 8, h & 0xff, v >> 8, v & 0xff
        ix = (xs >= x1) & (xs < x2) if x1 <= x2 else (xs >= x1) | (xs < x2)
        iy = (ys >= y1) & (ys < y2) if y1 <= y2 else (ys >= y1) | (ys < y2)
        return ix & iy
    if dc >> 15 & 1:
        m = np.where(objs(cap, eng)[4], winout >> 8 & 0x3f, m)
    if dc >> 14 & 1:
        m = np.where(inside(win1h, win1v), winin >> 8 & 0x3f, m)
    if dc >> 13 & 1:
        m = np.where(inside(win0h, win0v), winin & 0x3f, m)
    return m


def to_rgb5(c):
    return np.stack([c & 31, c >> 5 & 31, c >> 10 & 31], -1)


def screen(cap, eng):
    """(rgb 5-bit HxWx3, unknown mask) of one engine's output"""
    dc = cap.reg(eng, 0, 4)
    unknown = np.zeros((H, W), bool)
    dmode = dc >> 16 & 3
    if eng == 'main' and dmode == 2:
        bank = 'ABCD'[dc >> 18 & 3]
        if bank in cap.lcdc:
            c = np.frombuffer(cap.lcdc[bank], '<u2')[:H * W].reshape(H, W).astype(np.int32)
            return master_brightness(cap, eng, to_rgb5(c & 0x7fff)), unknown
        return np.zeros((H, W, 3), np.int32), np.ones((H, W), bool)
    if dmode == 0:
        return np.full((H, W, 3), 31, np.int32), unknown
    bgpal = (cap.pal[0x200:0x300] if eng == 'sub' else cap.pal[0:0x100]).astype(np.int32) & 0x7fff
    wm = windows(cap, eng)
    # candidate layers: (rank array, colour, opaque, is_target bits id)
    layers = []
    for n in range(4):
        if not dc >> (8 + n) & 1:
            continue
        prio = cap.reg(eng, 8 + 2 * n) & 3
        if eng == 'main' and n == 0 and dc >> 3 & 1:
            col, opq = np.zeros((H, W), np.int32), np.ones((H, W), bool)
            three_d = True
        else:
            ci, opq = text_bg(cap, eng, n)
            col, three_d = bgpal[ci], False
        if wm is not None:
            opq = opq & ((wm >> n & 1) == 1)
        layers.append((np.full((H, W), prio * 8 + 1 + n), col, opq, n, three_d))
    if dc >> 12 & 1:
        col, opq, pri, semi, _ = objs(cap, eng)
        if wm is not None:
            opq = opq & ((wm >> 4 & 1) == 1)
        layers.append((pri * 8, col, opq, 4, False))
    else:
        semi = np.zeros((H, W), bool)
    BIG = 1000
    ranks = np.stack([np.where(l[2], l[0], BIG) for l in layers] + [np.full((H, W), BIG - 1)])
    cols = np.stack([l[1] for l in layers] + [np.full((H, W), bgpal[0])])
    ids = [l[3] for l in layers] + [5]
    is3d = [l[4] for l in layers] + [False]
    order = np.argsort(ranks, axis=0, kind='stable')
    top, second = order[0], order[1]
    yy, xx = np.mgrid[0:H, 0:W]
    c1, c2 = cols[top, yy, xx], cols[second, yy, xx]
    id1, id2 = np.array(ids)[top], np.array(ids)[second]
    unknown = np.array(is3d)[top] | np.array(is3d)[second]
    rgb1, rgb2 = to_rgb5(c1), to_rgb5(c2)
    bld = cap.reg(eng, 0x50)
    alpha = cap.reg(eng, 0x52)
    eva, evb, evy = min(alpha & 31, 16), min(alpha >> 8 & 31, 16), min(cap.reg(eng, 0x54) & 31, 16)
    mode = bld >> 6 & 3
    t1 = (bld >> id1 & 1) == 1
    t2 = (bld >> (8 + id2) & 1) == 1
    eff = np.ones((H, W), bool) if wm is None else ((wm >> 5 & 1) == 1)
    out = rgb1.copy()
    semi_top = semi & (id1 == 4)
    blend = eff & ((mode == 1) & t1 | semi_top) & t2
    out = np.where(blend[..., None], np.minimum(31, (rgb1 * eva + rgb2 * evb) // 16), out)
    if mode == 2:
        m = eff & t1 & ~blend
        out = np.where(m[..., None], rgb1 + ((31 - rgb1) * evy) // 16, out)
    elif mode == 3:
        m = eff & t1 & ~blend
        out = np.where(m[..., None], rgb1 - (rgb1 * evy) // 16, out)
    return master_brightness(cap, eng, out), unknown


def master_brightness(cap, eng, out):
    """MASTER_BRIGHT (+0x6c): mode 1 up, 2 down, factor 0..16 - applied to the engine's whole output"""
    mb = cap.reg(eng, 0x6c)
    f = min(mb & 31, 16)
    if mb >> 14 == 1:
        out = out + ((31 - out) * f) // 16
    elif mb >> 14 == 2:
        out = out - (out * f) // 16
    return out


def both(cap):
    """(top, bottom) as (rgb5, unknown); POWCNT1 bit 15 = main engine on the top screen"""
    m, s = screen(cap, 'main'), screen(cap, 'sub')
    return (m, s) if cap.regs['powcnt'] >> 15 & 1 else (s, m)


def compare(d):
    cap = Capture(d)
    (t, tu), (b, bu) = both(cap)
    shot = np.asarray(Image.open(os.path.join(d, 'shot.png')).convert('RGB')).astype(np.int32) >> 3
    res = []
    for img, unk, part in ((t, tu, shot[:192]), (b, bu, shot[192:])):
        diff = (np.abs(img - part).max(-1) > 1) & ~unk
        res.append((int(diff.sum()), int((~unk).sum())))
    return res, (t, tu, b, bu)


if __name__ == '__main__':
    args = [a for a in sys.argv[1:] if not a.startswith('--')]
    out = sys.argv[sys.argv.index('--out') + 1] if '--out' in sys.argv else None
    if out:
        args = [a for a in args if a != out]
    tot = [0, 0]
    for d in args:
        try:
            (rt, rb), (t, tu, b, bu) = compare(d)
        except Exception as ex:
            print(os.path.basename(d), 'ERROR', ex); continue
        print('%-50s top %5d/%5d  bottom %5d/%5d px differ' % (os.path.basename(d)[:50], rt[0], rt[1], rb[0], rb[1]))
        tot[0] += rt[0] + rb[0]; tot[1] += rt[1] + rb[1]
        if out:
            os.makedirs(out, exist_ok=True)
            img = np.concatenate([np.where(tu[..., None], [31, 0, 31], t), np.where(bu[..., None], [31, 0, 31], b)]) * 255 // 31
            Image.fromarray(img.astype(np.uint8)).save(os.path.join(out, os.path.basename(d) + '.png'))
    print('total %d / %d known pixels differ (%.2f%%)' % (tot[0], tot[1], 100 * tot[0] / max(1, tot[1])))
