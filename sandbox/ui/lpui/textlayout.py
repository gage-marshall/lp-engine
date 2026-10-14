"""Text box geometry of the message window set (TextLayout_BuildLine 0x208add0 + TextCanvas_InitForBG 0x2084024),
derived the game's way instead of read from RAM: each group of boxes is initialised from a parameter table
(screen, bg, characters, lines, character spacing, line spacing) and its tiles are allocated one after another per
screen.

  groups (init order, object offset): 6 option boxes +0x7fc (table 0x210318c), 3 guide boxes +0x1fe4 (0x2103144),
  3 dialogue lines +0x17ec (0x210315c), the name box +0x2ac (0x2103174)
  area (vertical text, NNS canvas): height = (chars * 12 + (chars - 1) * char spacing + 7) >> 3 tiles,
                                    width  = (lines * font glyph width + (lines - 1) * line spacing + 7) >> 3 tiles
  tiles: a per-screen allocator (Text_GetContext) starting at 0, or while the 3D capture is on at 0x1180 (sub) /
  0x1e00 (main); each canvas takes width * height * 32 bytes."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from lpui import gamedata as G, text

GROUPS = [('option', 0x7fc, 0x208af90, 6), ('guide', 0x1fe4, 0x208af94, 3), ('line', 0x17ec, 0x208af98, 3),
          ('name', 0x2ac, 0x208af9c, 1)]                 # (name, object offset, literal holding the parameter table, count)
BASE_3D = {1: 0x1180, 0: 0x1e00}                         # TextCanvas_InitForBG: allocator start while Capture3D is active


def params(lit):
    """(screen, bg, chars, lines, char spacing, line spacing)"""
    return tuple(G.pointer_table(lit, 'i', 6))


def area(chars, lines, hspace, vspace, font=None):
    f = font or text.font()
    glyph_w = f.default_width[1]                        # NNSG2dFontInformation.defaultWidth.glyphWidth (font info +5)
    height = (chars * 12 + (chars - 1) * hspace + 7) >> 3
    width = (lines * glyph_w + (lines - 1) * vspace + 7) >> 3
    return width, height


def boxes(capture_3d):
    """{(group, i): dict(screen, bg, w, h, char_ofs, obj_offset)} as TextLayout_BuildLine sets them up"""
    alloc = {0: BASE_3D[0] if capture_3d else 0, 1: BASE_3D[1] if capture_3d else 0}
    out = {}
    for name, off, lit, n in GROUPS:
        screen, bg, chars, lines, hs, vs = params(lit)
        w, h = area(chars, lines, hs, vs)
        for i in range(n):
            out[(name, i)] = dict(screen=screen, bg=bg, w=w, h=h, char_ofs=alloc[screen], obj_offset=off + i * 0x2a8)
            alloc[screen] += w * h * 32
    return out


def bg_bases(regs, screen, bg):
    """(char base, screen base) as byte offsets in the engine's BG VRAM for BG number `bg` (1-based, as the parameter
    tables give it: Bg_GetCharPtr / Bg_GetScrPtr 1..4 -> BG0..3) of `screen` (0 main, 1 sub) - from the BG control
    register (and, on the main engine, DISPCNT's char / screen base)"""
    eng = 'sub' if screen == 1 else 'main'
    n = bg - 1
    r = regs[eng]
    cnt = (r['008'] if n < 2 else r['00c']) >> (16 * (n % 2)) & 0xffff
    dc = r['000']
    cb = ((cnt >> 2) & 15) * 0x4000 + (((dc >> 24) & 7) * 0x10000 if eng == 'main' else 0)
    sb = ((cnt >> 8) & 31) * 0x800 + (((dc >> 27) & 7) * 0x10000 if eng == 'main' else 0)
    return cb, sb


# map positions (TextBox_SetLayoutParams): dialogue lines = the message window layouts (lpui.logic.msgwin), option
# boxes = TextLayout_BuildParagraph's table (row k = option k: (pen x, left) for line 1, line 2), top 3;
# the name box: SetLayoutParams(0xc, 0, left 7, top 1, mode 2 centred) (instruction constants, 0x208b504..0x208b528)
OPTION_LAYOUT = [tuple(r) for r in G.pointer_table(0x208b41c, '4i', 3)]
OPTION_TOP = 3
NAME_LAYOUT = (0xc, 7, 1)        # (pen x, left, top)
