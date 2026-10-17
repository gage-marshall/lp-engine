#!/usr/bin/env python3
"""The app icon, drawn from shapes (no artwork from the game): a rose rounded square, a white heart, a plus badge.

    make_icon.py OUTDIR     writes icon_1024.png, the macOS AppIcon.iconset sizes, icon.ico and icon_256.png
"""
import math
import os
import sys

from PIL import Image, ImageDraw, ImageFilter

S = 4096  # drawn at 4x, then reduced


def heart(cx, cy, w):
    """the heart outline: the classic parametric curve, scaled to width w"""
    pts = []
    for i in range(720):
        t = 2 * math.pi * i / 720
        x = 16 * math.sin(t) ** 3
        y = 13 * math.cos(t) - 5 * math.cos(2 * t) - 2 * math.cos(3 * t) - math.cos(4 * t)
        pts.append((cx + x * w / 34, cy - y * w / 34))
    return pts


def draw():
    img = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    # the tile: macOS's grid (824 of 1024, corner radius ~185)
    m = S * 100 // 1024
    r = S * 185 // 1024
    # a soft shadow under the tile
    sh = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(sh).rounded_rectangle((m, m + S // 80, S - m, S - m + S // 80), r, fill=(120, 20, 60, 110))
    img.alpha_composite(sh.filter(ImageFilter.GaussianBlur(S // 60)))
    # the tile's gradient: light rose at the top to deep rose at the bottom
    grad = Image.new('RGBA', (S, S))
    top, bot = (255, 170, 196), (232, 62, 120)
    gd = ImageDraw.Draw(grad)
    for y in range(S):
        k = y / (S - 1)
        gd.line([(0, y), (S, y)], fill=tuple(int(a + (b - a) * k) for a, b in zip(top, bot)) + (255,))
    mask = Image.new('L', (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle((m, m, S - m, S - m), r, fill=255)
    img.paste(grad, (0, 0), mask)
    d = ImageDraw.Draw(img)
    # the heart, a little below centre
    cx, cy, w = S * 0.47, S * 0.53, S * 0.56
    hs = Image.new('RGBA', (S, S), (0, 0, 0, 0))
    ImageDraw.Draw(hs).polygon(heart(cx, cy + S * 0.012, w), fill=(150, 20, 70, 90))
    img.alpha_composite(hs.filter(ImageFilter.GaussianBlur(S // 90)))
    d.polygon(heart(cx, cy, w), fill=(255, 255, 255, 255))
    # the plus badge, upper right of the heart
    bx, by, br = S * 0.70, S * 0.31, S * 0.115
    d.ellipse((bx - br, by - br, bx + br, by + br), fill=(255, 255, 255, 255))
    d.ellipse((bx - br * 0.82, by - br * 0.82, bx + br * 0.82, by + br * 0.82), fill=(236, 72, 128, 255))
    a, t = br * 0.5, br * 0.16
    d.rounded_rectangle((bx - a, by - t, bx + a, by + t), t, fill=(255, 255, 255, 255))
    d.rounded_rectangle((bx - t, by - a, bx + t, by + a), t, fill=(255, 255, 255, 255))
    return img.resize((1024, 1024), Image.LANCZOS)


def main():
    out = sys.argv[1] if len(sys.argv) > 1 else '.'
    os.makedirs(out, exist_ok=True)
    big = draw()
    big.save(os.path.join(out, 'icon_1024.png'))
    big.resize((256, 256), Image.LANCZOS).save(os.path.join(out, 'icon_256.png'))
    iconset = os.path.join(out, 'AppIcon.iconset')
    os.makedirs(iconset, exist_ok=True)
    for s in (16, 32, 128, 256, 512):
        big.resize((s, s), Image.LANCZOS).save(os.path.join(iconset, 'icon_%dx%d.png' % (s, s)))
        big.resize((2 * s, 2 * s), Image.LANCZOS).save(os.path.join(iconset, 'icon_%dx%d@2x.png' % (s, s)))
    big.save(os.path.join(out, 'icon.ico'), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])


if __name__ == '__main__':
    main()
