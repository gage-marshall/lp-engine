"""Render the main engine's text BG layers and OBJs from a capture (sandbox/ui/capture.py) separately.
    .venv/bin/python sandbox/ui/layers.py CAPTURE_DIR"""
import json, os, struct, sys
from PIL import Image


def rgb555(c):
    return ((c & 31) * 255 // 31, (c >> 5 & 31) * 255 // 31, (c >> 10 & 31) * 255 // 31, 255)


ENG = 'main'


def load(d, eng='main'):
    """engine 'main' or 'sub': (regs with 'main' = that engine's registers, palettes (BG at 0, OBJ at 0x200), BG VRAM, OBJ
    VRAM, OAM)"""
    r = json.load(open(os.path.join(d, 'regs.json')))
    if eng == 'sub':
        r = dict(r, main=r['sub'])
    pal = open(os.path.join(d, 'pal.bin'), 'rb').read()
    pal = pal[0x400:0x800] if eng == 'sub' else pal[:0x400]
    bg = open(os.path.join(d, 'vram_bg_%s.bin' % eng), 'rb').read()
    obj = open(os.path.join(d, 'vram_obj_%s.bin' % eng), 'rb').read()
    oam = open(os.path.join(d, 'oam.bin'), 'rb').read()
    oam = oam[0x400:] if eng == 'sub' else oam[:0x400]
    return r, pal, bg, obj, oam


def color(pal, base, i):
    return rgb555(struct.unpack_from('<H', pal, base + 2 * i)[0])


def bg_layer(r, pal, vram, n):
    d = r['main']['000']
    cnt = (r['main']['008' if n < 2 else '00c'] >> (16 * (n & 1))) & 0xffff
    char = (cnt >> 2 & 15) * 0x4000 + (d >> 24 & 7) * 0x10000
    scr = (cnt >> 8 & 31) * 0x800 + (d >> 27 & 7) * 0x10000
    col256 = cnt >> 7 & 1
    size = cnt >> 14
    w, h = [(256, 256), (512, 256), (256, 512), (512, 512)][size]
    im = Image.new('RGBA', (w, h), (0, 0, 0, 0))
    px = im.load()
    for ty in range(h // 8):
        for tx in range(w // 8):
            blk = (tx // 32) + (ty // 32) * (w // 256)
            o = scr + blk * 0x800 + ((ty % 32) * 32 + tx % 32) * 2
            e = struct.unpack_from('<H', vram, o)[0]
            t, hf, vf, p = e & 0x3ff, e >> 10 & 1, e >> 11 & 1, e >> 12
            for y in range(8):
                for x in range(8):
                    sx, sy = (7 - x if hf else x), (7 - y if vf else y)
                    if col256:
                        c = vram[char + t * 64 + sy * 8 + sx]
                        if c: px[tx * 8 + x, ty * 8 + y] = color(pal, 0, c)
                    else:
                        b = vram[char + t * 32 + sy * 4 + sx // 2]
                        c = b >> 4 if sx & 1 else b & 15
                        if c: px[tx * 8 + x, ty * 8 + y] = color(pal, 0, p * 16 + c)
    return im, dict(cnt=hex(cnt), char=hex(char), screen=hex(scr), size=(w, h), col256=col256, prio=cnt & 3)


SHAPES = {(0, 0): (8, 8), (0, 1): (16, 16), (0, 2): (32, 32), (0, 3): (64, 64), (1, 0): (16, 8), (1, 1): (32, 8), (1, 2): (32, 16),
          (1, 3): (64, 32), (2, 0): (8, 16), (2, 1): (8, 32), (2, 2): (16, 32), (2, 3): (32, 64)}


def objs(r, pal, vram, oam):
    d = r['main']['000']
    bound = 32 << (d >> 20 & 3)
    im = Image.new('RGBA', (256, 192), (0, 0, 0, 0))
    px = im.load()
    out = []
    for i in range(127, -1, -1):
        a0, a1, a2 = struct.unpack_from('<HHH', oam, i * 8)
        rs, dis = a0 >> 8 & 1, a0 >> 9 & 1
        if not rs and dis:
            continue
        y, x = a0 & 0xff, a1 & 0x1ff
        if x >= 256: x -= 512
        if y >= 192: y -= 256
        shape, size = a0 >> 14, a1 >> 14
        w, h = SHAPES[(shape, size)]
        col256, mode = a0 >> 13 & 1, a0 >> 10 & 3
        hf, vf = (a1 >> 12 & 1, a1 >> 13 & 1) if not rs else (0, 0)
        tile, prio, palno = a2 & 0x3ff, a2 >> 10 & 3, a2 >> 12
        out.append(dict(i=i, x=x, y=y, w=w, h=h, tile=tile, prio=prio, pal=palno, mode=mode, col256=col256, rs=rs, hf=hf, vf=vf))
        base = tile * bound
        tw = w // 8
        for py in range(h):
            for pxx in range(w):
                sx, sy = (w - 1 - pxx if hf else pxx), (h - 1 - py if vf else py)
                t = (sy // 8) * tw + sx // 8
                if col256:
                    c = vram[base + t * 64 + (sy % 8) * 8 + sx % 8]
                    col = color(pal, 0x200, c) if c else None
                else:
                    b = vram[base + t * 32 + (sy % 8) * 4 + (sx % 8) // 2]
                    c = b >> 4 if sx & 1 else b & 15
                    col = color(pal, 0x200, palno * 16 + c) if c else None
                X, Y = x + pxx, y + py
                if col and 0 <= X < 256 and 0 <= Y < 192:
                    px[X, Y] = col
    return im, out


if __name__ == '__main__':
    d = sys.argv[1]
    eng = sys.argv[2] if len(sys.argv) > 2 else 'main'
    r, pal, bg, obj, oam = load(d, eng)
    p = '' if eng == 'main' else 'sub_'
    for n in (0, 1, 2, 3):
        if not (r['main']['000'] >> (8 + n) & 1) or (n == 0 and r['main']['000'] & 8):
            continue
        im, info = bg_layer(r, pal, bg, n)
        im.save(os.path.join(d, '%sbg%d.png' % (p, n))); print('BG%d' % n, info)
    im, o = objs(r, pal, obj, oam)
    im.save(os.path.join(d, '%sobj.png' % p))
    print(len(o), 'objs'); [print(' ', x) for x in o[:40]]
