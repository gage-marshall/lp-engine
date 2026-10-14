"""Choice option text (top screen, sub engine BG2), from the game's option text boxes (read in a choice capture).

Option i (0..2): CharCanvas 2 x 20 tiles, chars from 141 / 181 / 221 (box offsets 4480 / 5760 / 7040), mapped at sub BG2
screen (0x6202000) x = 23 / 15 / 7, y = 3, palette 15; drawn whole in colour 2 at pen x 12, y += advance. Char block
0x6204000. The cards behind them are sub BG3 (mount_menu 0126 + 0125 + 0121) and their touch tabs main BG3 (0124 + 0123),
loaded from the files (recipes); empty options leave their map entries 0 (as the dialogue boxes do)."""
import numpy as np
from lpui import text

from lpui import textlayout

COLOUR = 2


def geometry(capture_3d=True):
    """[(w, h, left, char_no)] of the first three option boxes, from the game's tables (lpui.textlayout)"""
    bx = textlayout.boxes(capture_3d)
    return [(bx[('option', i)]['w'], bx[('option', i)]['h'], textlayout.OPTION_LAYOUT[i][1],
             bx[('option', i)]['char_ofs'] // 32 + 1) for i in range(3)]


BOXES = geometry()
TOP = textlayout.OPTION_TOP


def apply(vram, options, capture_3d=True):
    mem = vram.mem['bg_sub']
    CHAR_BASE, SCR_BASE = textlayout.bg_bases(vram.regs, 1, textlayout.params(textlayout.GROUPS[0][2])[1])
    BOXES = geometry(capture_3d)
    f = text.font()
    for i, (w, h, left, char_no) in enumerate(BOXES):
        s = options[i] if i < len(options) else ''
        c = text.Canvas(w, h)
        y = 0
        for ch in s:
            a = text.advance(ch)
            if a:
                c.draw_char(f, 12, y, COLOUR, ch)
            y += a
        tiles = np.frombuffer(c.tiles(), np.uint8)
        a0 = CHAR_BASE + char_no * 32
        mem[a0:a0 + len(tiles)] = tiles
        n = char_no
        for y in range(TOP, TOP + h):
            for x in range(left, left + w):
                o = SCR_BASE + (y * 32 + x) * 2
                v = (0xf000 | n) if s else 0
                mem[o], mem[o + 1] = v & 0xff, v >> 8
                n += 1
