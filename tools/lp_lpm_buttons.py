"""The LovePlus mode buttons in English: loveplusmode/loveplusmode.bin holds them as 32x64 8bpp textures (entry: a
16-byte header (format 4, width, height, ?, palette bytes, texel bytes), the palette, the texels), blue (normal) and
orange (pressed). Each button's inside is filled with its darker ground tone and the English drawn in white with a
one-pixel ring in that tone, read top to bottom like the Japanese.
    python3 tools/lp_lpm_buttons.py CONTENT [PREVIEW.png]   -> patch/fs/loveplusmode/loveplusmode.bin"""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import lp_gfx as G

ARCHIVE = 'loveplusmode/loveplusmode.bin'
# entry -> English (normal / pressed pairs)
LABELS = {0: 'Back', 1: 'Back', 3: 'End Talk', 4: 'End Talk', 5: 'Menu', 6: 'Menu', 7: 'OK', 8: 'OK',
          9: 'Cancel', 10: 'Cancel', 55: 'Yes', 56: 'Yes', 57: 'No', 58: 'No'}
BOX = [8, 8, 16, 46]  # the button's inside
BOXES = {55: [11, 9, 15, 45], 56: [11, 9, 15, 45], 57: [11, 9, 15, 45], 58: [11, 9, 15, 45]}  # the Yes / No buttons sit further in


class Tex:
    def __init__(self, e):
        self.w, self.h, _, self.pb, self.tb = struct.unpack_from('<HHHHI', e, 4)
        self.pal = [struct.unpack_from('<H', e, 0x10 + 2 * k)[0] for k in range(self.pb // 2)]
        tx = e[0x10 + self.pb:0x10 + self.pb + self.tb]
        self.px = [list(tx[y * self.w:(y + 1) * self.w]) for y in range(self.h)]
        self.head = e[:0x10 + self.pb]
        self.tail = e[0x10 + self.pb + self.tb:]

    def rgb(self, v):
        c = self.pal[v]
        return ((c & 31) * 8, (c >> 5 & 31) * 8, (c >> 10 & 31) * 8)

    def build(self):
        return self.head + bytes(v for r in self.px for v in r) + self.tail


def relabel(t, en, content, box=BOX):
    """the inside filled with the ground's darker tone (the saturated colours in the box, darker half, its
    commonest shade; white over the lighter half was hard to read), the English in white ringed in that tone"""
    from collections import Counter
    bx, by, bw, bh = box
    cnt = Counter(t.px[y][x] for y in range(by, by + bh) for x in range(bx, bx + bw))
    sat = lambda v: max(t.rgb(v)) - min(t.rgb(v))
    ground = [v for v, n in cnt.most_common() if v and sat(v) > 100]
    lum = lambda v: sum(t.rgb(v))
    ground.sort(key=lum)
    mid = (lum(ground[0]) + lum(ground[-1])) / 2
    dark = [v for v in ground if lum(v) < mid]
    light = [v for v in ground if lum(v) >= mid]
    white = max((v for v in range(1, len(t.pal))), key=lum)
    main = lambda tone: max(tone, key=lambda v: cnt[v])
    errs = G.relabel_fill(t, en, 90, content, box=box, ink=white, ground=main(dark), outline=main(dark))
    return errs


def main(content, preview=None):
    raw = open(os.path.join(content, 'fs', ARCHIVE), 'rb').read()
    arc = G.Arc(raw)
    shots = []
    for i, en in LABELS.items():
        t = Tex(arc.entries[i])
        before = [r[:] for r in t.px]
        errs = relabel(t, en, content, BOXES.get(i, BOX))
        if errs:
            print(i, en, errs)
            continue
        arc.entries[i] = t.build()
        arc.changed.add(i)
        shots.append((t, before))
    out = os.path.join(os.path.dirname(os.path.abspath(content)), 'patch', 'fs', ARCHIVE)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, 'wb').write(arc.build())
    print('%d buttons -> %s' % (len(shots), out))
    if preview:
        from PIL import Image
        im = Image.new('RGB', (len(shots) * 36, 132), (255, 0, 255))
        for k, (t, before) in enumerate(shots):
            for row, px in ((0, before), (1, t.px)):
                for y in range(t.h):
                    for x in range(t.w):
                        if px[y][x]:
                            im.putpixel((k * 36 + x, row * 68 + y), t.rgb(px[y][x]))
        im.resize((im.width * 3, im.height * 3), Image.NEAREST).save(preview)


if __name__ == '__main__':
    main(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else None)
