#!/usr/bin/env python3
"""Decode LovePlus+ BG archives (NCGR 4bpp + NCLR + NSCR triples) into portrait pictures.
The NSCR header claims 384x256 but the picture is the first 32x24 tiles (256x192) and is drawn rotated for the sideways-held DS;
`bg(arc_dir, n)` returns an upright-for-reading 192x256 PIL image.  Usage: bg_decode.py ARC_DIR OUT.png [first count]"""
import os, struct, sys
from PIL import Image, ImageDraw


def col(v): return ((v & 31) << 3, ((v >> 5) & 31) << 3, ((v >> 10) & 31) << 3)


def triples(d):
    fs = sorted(os.listdir(d)); out = []; i = 0
    while i + 2 < len(fs):
        if fs[i].endswith('.ncgr') and fs[i + 1].endswith('.nclr') and fs[i + 2].endswith('.nscr'):
            out.append(tuple(os.path.join(d, f) for f in fs[i:i + 3])); i += 3
        else: i += 1
    return out


def bg(t):
    g, p, s = (open(f, 'rb').read() for f in t)
    depth = struct.unpack_from('<I', g, 0x1c)[0] if False else struct.unpack_from('<I', g, 0x1c - 4)[0]
    bpp4 = struct.unpack_from('<I', g, 0x18 + 4)[0] == 3
    raw = g[0x30:]
    pal = struct.unpack_from('<%dH' % ((len(p) - 0x28) // 2), p, 0x28)
    n = (len(s) - 0x24) // 2
    ents = struct.unpack_from('<%dH' % n, s, 0x24)
    im = Image.new('RGB', (256, 192)); px = im.load()
    for i, e in enumerate(ents[:768]):
        t_, hf, vf, pl = e & 0x3ff, (e >> 10) & 1, (e >> 11) & 1, e >> 12
        tx, ty = (i % 32) * 8, (i // 32) * 8
        if bpp4: tile = raw[t_ * 32:(t_ + 1) * 32]
        else: tile = raw[t_ * 64:(t_ + 1) * 64]
        if len(tile) < (32 if bpp4 else 64): continue
        for y in range(8):
            for x in range(8):
                sy = 7 - y if vf else y; sx = 7 - x if hf else x
                if bpp4:
                    b = tile[sy * 4 + sx // 2]; v = (b >> 4) if sx & 1 else b & 15; c = pal[pl * 16 + v] if pl * 16 + v < len(pal) else 0
                else:
                    v = tile[sy * 8 + sx]; c = pal[v] if v < len(pal) else 0
                px[tx + x, ty + y] = col(c)
    return im.rotate(90, expand=True)   # 192 x 256, reading orientation


def main():
    d, out = sys.argv[1], sys.argv[2]
    first = int(sys.argv[3]) if len(sys.argv) > 3 else 0
    count = int(sys.argv[4]) if len(sys.argv) > 4 else 40
    ts = triples(d)[first:first + count]
    cols, tw, th = 10, 96, 128
    sheet = Image.new('RGB', (cols * tw, ((len(ts) + cols - 1) // cols) * th)); dr = ImageDraw.Draw(sheet)
    for n, t in enumerate(ts):
        try: im = bg(t).resize((tw, th))
        except Exception as e: continue
        sheet.paste(im, ((n % cols) * tw, (n // cols) * th)); dr.text(((n % cols) * tw + 2, (n // cols) * th + 2), str(first + n), fill=(255, 255, 0))
    sheet.save(out); print(len(ts), 'triples')


if __name__ == '__main__':
    main()
