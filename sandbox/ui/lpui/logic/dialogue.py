"""Scene dialogue window text (main engine BG2), from the game's message boxes (MsgBox / Text_DrawNextChar).

Four text boxes, each a CharCanvas whose tiles are mapped with NNS_G2dMapScrToCharText(screen 5 = 0x6002800, area, left,
top 1, width 32, charNo = boxofs/32 + 1, palette 15) and drawn one character per typewriter step in colour 2:
  name   2 x 6 tiles  at map x 7, chars from 361, colour 2 (dark on the tab), drawn at x 12, y = (48 - 12 n) / 2 (centred)
  line 1 2 x 20 tiles at map x 5, chars from 241, colour 1 (white), x 11, y += advance (12 full-width / 6 ASCII)
  line 2 2 x 20 tiles at map x 3, chars from 281
  line 3 2 x 20 tiles at map x 1, chars from 321
BG2 char block 3 (0x600c000). The BG2 map is cleared for each message; a box is mapped (MapScrToCharText runs in
Text_DrawNextChar) only once it has drawn a character, so empty boxes leave map entries 0. Values read from the boxes' structs in a scene capture (fields +0x28/+0x2c area, +0x30 char
offset, +0x294/+0x298 map left/top, +0x274/+0x278 pen, +0x280 characters drawn)."""
import numpy as np
from lpui import text

from lpui import textlayout
from lpui.logic import msgwin

COLOUR = {'name': 2, 'line1': 1, 'line2': 1, 'line3': 1}     # the colour NNS_G2dCharCanvasDrawChar receives (traced)


def geometry(capture_3d=True, lines=3):
    """{kind: (w, h, left, top, char_no)} derived from the game's tables (lpui.textlayout)"""
    bx = textlayout.boxes(capture_3d)
    lay = msgwin.LAYOUT3 if lines >= 3 else msgwin.LAYOUT2
    out = {}
    for i in range(3):
        b = bx[('line', i)]
        out['line%d' % (i + 1)] = (b['w'], b['h'], lay[i][1], 1, b['char_ofs'] // 32 + 1)
    b = bx[('name', 0)]
    out['name'] = (b['w'], b['h'], textlayout.NAME_LAYOUT[1], textlayout.NAME_LAYOUT[2], b['char_ofs'] // 32 + 1)
    return out


BOXES = {k: (w, h, left, cn) for k, (w, h, left, top, cn) in geometry().items()}     # 3D on, 3+ lines (scenes)


def box_canvas(kind, s, typed=None):
    w, h, left, char_no = BOXES[kind]
    c = text.Canvas(w, h)
    f = text.font()
    chars = list(s)[:typed] if typed is not None else list(s)
    if kind == 'name':
        y = (h * 8 - sum(text.advance(ch) for ch in s)) // 2
        x = 12
    else:
        x, y = 11, 0
    for ch in chars:
        a = text.advance(ch)
        if a:
            c.draw_char(f, x, y, COLOUR[kind], ch)
        y += a
    return c


def apply(vram, name, lines, typed=None, capture_3d=True):
    """write the four boxes into main BG2 (tiles + map entries); typed = characters shown per line (None = all).
    Char / screen bases come from the BG2 control register (vram.regs); box geometry from lpui.textlayout"""
    mem = vram.mem['bg_main']
    CHAR_BASE, SCR_BASE = textlayout.bg_bases(vram.regs, 0, textlayout.params(textlayout.GROUPS[2][2])[1])
    geo = geometry(capture_3d, len([l for l in lines if l]) if lines else 0)
    typed = typed or [None] * 3
    for kind, s, t in [('name', name, None)] + [('line%d' % (i + 1), lines[i] if i < len(lines) else '', typed[i] if i < len(typed) else None) for i in range(3)]:
        w, h, left, top, char_no = geo[kind]
        c = box_canvas(kind, s or '', t)
        tiles = np.frombuffer(c.tiles(), np.uint8)
        a = CHAR_BASE + char_no * 32
        mem[a:a + len(tiles)] = tiles
        shown = len(list(s or '')[:t]) if t is not None else len(s or '')
        n = char_no
        if shown == 0:
            for y in range(top, top + h):
                for x in range(left, left + w):
                    o = SCR_BASE + (y * 32 + x) * 2
                    mem[o] = mem[o + 1] = 0
            continue
        for y in range(top, top + h):
            for x in range(left, left + w):
                o = SCR_BASE + (y * 32 + x) * 2
                v = 0xf000 | n
                mem[o], mem[o + 1] = v & 0xff, v >> 8
                n += 1
