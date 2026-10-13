#!/usr/bin/env python3
"""Full BG panorama (NSCR header 384x256, hardware screen-block order) in reading orientation (256 wide x 384 high), and registration of a real frame against it."""
import struct, sys, os
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(__file__))
import bg_decode as b


def pano(t):
    g, p, s = (open(f, 'rb').read() for f in t)
    raw = g[0x30:]
    pal = struct.unpack_from('<%dH' % ((len(p) - 0x28) // 2), p, 0x28)
    ents = struct.unpack_from('<%dH' % ((len(s) - 0x24) // 2), s, 0x24)
    im = Image.new('RGB', (384, 256)); px = im.load()
    for i, e in enumerate(ents):
        blk, j = i // 1024, i % 1024
        cx, cy = (j % 32) + 32 * blk, j // 32
        if cx >= 48: continue
        t_, hf, vf, pl = e & 0x3ff, (e >> 10) & 1, (e >> 11) & 1, e >> 12
        tile = raw[t_ * 32:(t_ + 1) * 32]
        if len(tile) < 32: continue
        for y in range(8):
            for x in range(8):
                sy = 7 - y if vf else y; sx = 7 - x if hf else x
                bb = tile[sy * 4 + sx // 2]; v = (bb >> 4) if sx & 1 else bb & 15
                c = pal[pl * 16 + v]
                px[cx * 8 + x, cy * 8 + y] = ((c & 31) << 3, ((c >> 5) & 31) << 3, ((c >> 10) & 31) << 3)
    return im.rotate(90, expand=True)     # 256 x 384


def register(real, panorama, region=(0, 0, 150, 150)):
    """best (dx, dy) of the 192x256 real frame inside the 256x384 panorama, by sum of absolute differences on a region of the frame"""
    r = np.asarray(real.convert('L')).astype(np.float32)
    P = np.asarray(panorama.convert('L')).astype(np.float32)
    x0, y0, x1, y1 = region
    tpl = r[y0:y1, x0:x1]
    best = (1e18, 0, 0)
    for dy in range(0, 384 - 256 + 1):
        for dx in range(0, 256 - 192 + 1):
            win = P[dy + y0:dy + y1, dx + x0:dx + x1]
            d = np.abs(win - tpl).mean()
            if d < best[0]: best = (d, dx, dy)
    return best
