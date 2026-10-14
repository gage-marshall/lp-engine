"""The game's text drawing: NitroSystem CharCanvas (BG layout) + the game's Text_DrawString rules, written from the
NitroSystem library behaviour (read-only reference) and the game code.

CharCanvas (NNS_G2dCharCanvasInitForBG): areaWidth x areaHeight 4bpp tiles, row-major (tile = y * areaWidth + x).
DrawGlyphLine/LetterChar: glyph bits are read MSB-first per row (row = cellWidth * bpp bits); a non-zero value v is written
as cl + v - 1 into the 4-bit pixel. NNS_G2dCharCanvasDrawChar (no direction support in this build: see `ROT`) adds the
glyph's `left` to x. Text_DrawString 0x2084328 (canvas, str, x, y, ..., colour): per character DrawChar(x, y, colour) then
y += advance (6 for ASCII, 12 otherwise in UTF-8 mode, from Text_GetGlyphWidth); '\\n' -> x -= 12, y = y0 - 6; '\\\\'
draws nothing and toggles the colour between the given one and 3. TextCanvas_InitForBG 0x2084024 (obj, screen, bg, a, b,
.., .., .., flag): with flag 0 the canvas is b tiles wide and a tiles high; its tiles are transferred to the BG's char
block at byte offset obj+0x30 + 0x20 (VramTransfer type 4..7 main / 20..23 sub)."""
import os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import nftr  # noqa: E402

FONTS = {}
ROT = 'rot90'         # this build has NNS_G2D_FONT_ENABLE_DIRECTION_SUPPORT: the R90 font draws at x - cellW, y + left (verified: canvas == RAM)


def font(name='LC12_UTF8_R90'):
    if name not in FONTS:
        FONTS[name] = nftr.Font(open(os.path.join(ROOT, 'content/assets/test_font/%s.NFTR' % name), 'rb').read())
    return FONTS[name]


class Canvas:
    def __init__(self, area_w, area_h):
        self.w, self.h = area_w, area_h
        self.px = np.zeros((area_h * 8, area_w * 8), np.uint8)       # one 4-bit value per pixel

    def tiles(self):
        """the canvas as 4bpp tile bytes (row-major tiles, low nibble = left pixel)"""
        t = self.px.reshape(self.h, 8, self.w, 4, 2).transpose(0, 2, 1, 3, 4)
        return (t[..., 0] | (t[..., 1] << 4)).astype(np.uint8).tobytes()

    def set_tiles(self, b):
        a = np.frombuffer(b, np.uint8).reshape(self.h, self.w, 8, 4)
        self.px = np.stack([a & 15, a >> 4], -1).reshape(self.h, self.w, 8, 8).transpose(0, 2, 1, 3).reshape(self.h * 8, self.w * 8).copy()

    def glyph(self, f, x, y, cl, gi):
        left, gw, cw = f.metrics(gi)
        if gw <= 0:
            return
        o = f.glyph_off + gi * f.cell_size
        raw = f.d[o:o + f.cell_size]
        bits = np.unpackbits(np.frombuffer(raw, np.uint8))           # MSB first
        bpp, cwid = f.bpp, f.cell_w
        for r in range(f.cell_h):
            for c in range(gw):
                p = (r * cwid + c) * bpp
                v = int(''.join(map(str, bits[p:p + bpp])), 2) if bpp > 1 else int(bits[p])
                if v:
                    X, Y = x + c, y + r
                    if 0 <= X < self.w * 8 and 0 <= Y < self.h * 8:
                        self.px[Y, X] = (cl + v - 1) & 15

    def draw_char(self, f, x, y, cl, ch):
        gi = f.glyph_index(ch)
        left, gw, cw = f.metrics(gi)
        if ROT == 'rot90':
            x, y = x - f.cell_w, y + left
        else:
            x = x + left
        self.glyph(f, x, y, cl, gi)


def advance(ch, mode=1):
    if ch == '\\':
        return 0
    if mode == 0:
        return 12
    half, full = (6, 12) if mode == 1 else (5, 10)
    return half if ord(ch) < 0x80 else full


def draw_string(canvas, text, x, y, colour, f=None, mode=1):
    """Text_DrawString"""
    f = f or font()
    y0, cl = y, colour
    for ch in text:
        if ch == '\n':
            x, y = x - 12, y0 - 6
            continue
        a = advance(ch, mode)
        if a == 0:
            cl = colour if cl == 3 else 3
            continue
        canvas.draw_char(f, x, y, cl, ch)
        y += a
