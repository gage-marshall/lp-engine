"""Profile entry (overlay 6, onboarding; decompiled Ov06ProfileDlt_* UI part, fid/decomp/ov06ui.c). The handwriting
recogniser in the same overlay (the "DLT" engine: 340 functions) is not ported here; the kana grid is the other input.

State (pointer at *(0x21855a0 + 500)):
  +0x26cc family name, +0x26ec given name, +0x2840 called-as: 4 characters each as (item, index) pairs into text
          resource bank 3 group 0 (0xc0) (index -1 = empty)
  +0x277c / +0x2792 the two free strings (nickname / handle), +0x270c / +0x2744 their 7-character pair forms
  +0x27d4 birth month  +0x27d8 birth day  +0x27dc blood type (1 A, 2 B, 3 O, 4 AB)  +0x27e0 hometown (1..47)
  form canvas (TextBox) at +0x145c; message canvas at +0x11b4
Text: text resource bank 0 group 1 items 1 (days １日..), 2 (months １月..), 5 (Ａ型 Ｂ型 Ｏ型 ＡＢ型), 6 (prefectures).

DrawProfileSelectionValues (the summary form): Text_DrawString on the form canvas, colour 2, pen (x, y):
  family name chars at x 0xdc, y 0x4c + 15 i        handle string at x 0xc6, y 0x4c + 14 i
  given name chars at x 0xaf, y 0x4c + 15 i         nickname string at x 0x99, y 0x4c + 14 i
  called-as chars at x 0x83, y 0x4c + 14 i
  month name at (0x6b, 0x4c); day name at (0x6b, 0x4c + 12 * (2, or 3 when month > 9))
  blood type at (0x54, 0x4c); hometown at (0x3e, 0x4c)
(the y step is the pair table's own: DrawProfileTextAt(.., count, x, y0, step))."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(HERE))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))

STATE_HOLDER = 0x21855a0
_T = {}


def textres(bank, item, index):
    """TextRes_Load(bank * 0x40 + group, item, index) for group 0 / 1 as the profile uses them"""
    import txrc
    if bank not in _T:
        name = 'gd_textres_jpn.txrc' if bank == 0 else 'gd_textres%02d_jpn.txrc' % bank
        _T[bank] = txrc.parse(open(os.path.join(ROOT, 'content/assets/text', name), 'rb').read(), decrypted=True)
    return _T[bank]


def res(a, item, index):
    g = textres(a >> 6, item, index)[a & 0x3f]
    if item >= len(g) or index >= len(g[item]):
        return ''
    return g[item][index].decode('utf8')


class Profile:
    def __init__(self, family, given, called, handle='', nickname='', month=0, day=0, blood=0, hometown=0):
        """family / given / called: lists of up to 4 (item, index) character pairs (bank 3 group 0)"""
        self.family, self.given, self.called = family, given, called
        self.handle, self.nickname = handle, nickname
        self.month, self.day, self.blood, self.hometown = month, day, blood, hometown

    @staticmethod
    def chars(pairs):
        return ''.join(res(0xc0, it & 0xff, ix) for it, ix in pairs if ix >= 0)

    def summary_draws(self):
        """[(string, x, y)] of DrawProfileSelectionValues, in call order"""
        out = []

        def at(pairs, x, step):
            for i, (it, ix) in enumerate(pairs):
                if ix >= 0:
                    out.append((res(0xc0, it & 0xff, ix), x, 0x4c + i * step))

        def entry(s, x, step):
            for i, ch in enumerate(s):
                out.append((ch, x, 0x4c + i * step))
        at(self.family, 0xdc, 0xf)
        entry(self.handle, 0xc6, 0xe)
        at(self.given, 0xaf, 0xf)
        entry(self.nickname, 0x99, 0xe)
        at(self.called, 0x83, 0xe)
        out.append((res(1, 2, self.month - 1), 0x6b, 0x4c))
        out.append((res(1, 1, self.day - 1), 0x6b, ((self.month > 9) + 2) * 12 + 0x4c))
        out.append((res(1, 5, self.blood - 1), 0x54, 0x4c))
        out.append((res(1, 6, self.hometown - 1), 0x3e, 0x4c))
        return out


# ---- editor pages (DrawProfileEditDialog 0x217ee74, mode = state +0x50) -------------------------------------------
# A grid page (modes 1 / 7) shows bank 3 group 0 item `page` (+0x6c): cell (r, c) holds an index into that item, drawn at
# x = 0xf2 - 20 r, y = 0xc + 20 c. The grid is built by ClearProfileEditSelection 0x217f4d4 (cells cleared to -1):
#   page types 0, 1, 6 (kana): 10 rows x 5 columns, plus ゛ (50) at row 0 column 5 and ゜ (51) at row 1 column 5
#   page types 2, 3, 7: 10 x 5;   4, 5: 10 x 6;   8 and up (kanji by reading): 9 rows x 6 starting one row down
#   cell index = columns * (rows * sub-page + r) + c; sub-pages per page type (SetProfileEditMode 0x217f468): type 4 -> 2,
#   types 0..7 otherwise 1, types 8+ -> the byte table 0x2184058 (overlay 6).
KANA_ROWS = 10


def grid(page_type=6, subpage=0):
    """10 x 9 cell indices (-1 = empty), as ClearProfileEditSelection builds them"""
    g = [[-1] * 9 for _ in range(10)]
    rows, cols, first = 10, 6, 0
    if page_type in (0, 1, 6):
        g[0][5], g[1][5] = 0x32, 0x33
        cols = 5
    elif page_type in (2, 3, 7):
        cols = 5
    elif page_type in (4, 5):
        cols = 6
    else:
        rows, first = 9, 1
    for r in range(rows):
        for c in range(cols):
            g[r + first][c] = cols * (rows * subpage + r) + c
    return g


def subpages(page_type):
    if page_type == 4:
        return 2
    if page_type < 8:
        return 1
    return PAGE_COUNTS[page_type]


def kana_grid(page_type=6, subpage=0):
    return [row[:6] for row in grid(page_type, subpage)]


def kana_grid_draws(page=6, subpage=0):
    """[(string, x, y)] of a grid page (checked: 52 / 52 draws at the traced kana page)"""
    out = []
    for r, row in enumerate(grid(page, subpage)):
        for c, ix in enumerate(row):
            if ix >= 0:
                out.append((res(0xc0, page, ix), 0xf2 - 20 * r, 0xc + 20 * c))
    return out


DAKUTEN = 50
HANDAKUTEN = 51
# BuildProfileEditMenu 0x2181364 tables (overlay 6 0x2184224 / 0x2184248 / 0x218426c / 0x2184278): an index in
# [from, from + 5) of a kana page becomes index + (to - from)
sys.path.insert(0, os.path.dirname(os.path.dirname(HERE)))
from lpui import gamedata as _G
DAKUTEN_MAP = list(zip(_G.table(0x2184224, 'i', 9, ov=6), _G.table(0x2184248, 'i', 9, ov=6)))      # か→が .. ぱ→ば, が→か ..
HANDAKUTEN_MAP = list(zip(_G.table(0x218426c, 'i', 3, ov=6), _G.table(0x2184278, 'i', 3, ov=6)))   # は→ぱ, ば→ぱ, ぱ→は


class NameInput:
    """a name being typed: up to `size` (page, index) characters (BuildProfileEditMenu / GetProfileEditMenuValue)"""
    def __init__(self, size=4):
        self.size = size
        self.pairs = [(0, -1)] * size
        self.cursor = 0
        self.count = 0

    @staticmethod
    def _kana_page(p):
        return p < 2 or p == 6

    def press(self, page, index):
        """a grid cell; False when the press does nothing (a voicing mark with nothing to voice)"""
        if self._kana_page(page):
            if self.cursor > 0:
                k = self.cursor - 1 if self.count < self.size else self.cursor
                pp, pi = self.pairs[k]
                if self._kana_page(pp):
                    table = DAKUTEN_MAP if index == DAKUTEN else HANDAKUTEN_MAP if index == HANDAKUTEN else None
                    if table:
                        for f, t in table:
                            if f <= pi < f + 5:
                                self.pairs[k] = (pp, pi + t - f)
                                return True
                        return False
                elif pi == 0:                       # a kanji page's reading character (item = its kana)
                    row = 0 if 0xd <= pp <= 0x1b else 1 if 0x21 <= pp <= 0x25 else -1
                    if row >= 0 and index == DAKUTEN:
                        self.pairs[k] = (0, pp + 0x27 - 5 * row); return True
                    if row == 1 and index == HANDAKUTEN:
                        self.pairs[k] = (0, pp + 0x27); return True
            if index in (DAKUTEN, HANDAKUTEN):
                return False
        self.pairs[self.cursor] = (page, index)
        if self.cursor < self.size - 1:
            self.cursor += 1
        if self.count < self.size:
            self.count += 1
        return True

    def back(self):
        if self.count == self.size:
            self.count -= 1
            self.pairs[self.cursor] = (self.pairs[self.cursor][0], -1)
            return True
        if self.cursor > 0:
            self.cursor -= 1; self.count -= 1
            self.pairs[self.cursor] = (self.pairs[self.cursor][0], -1)
            return True
        return False

    def text(self):
        return Profile.chars(self.pairs)


# ---- value pages (DrawProfileEditDialog cases 3..6; positions from the disassembly, row stride 20) -------------------
def _entry(s, x, y, step, right=True):
    """DrawProfileTextEntry: one character per Text_DrawString, `step` apart; right-aligned ends at y"""
    n = len(s)
    off = -step * (n - 1) if right else 0
    return [(ch, x, y + step * i + off) for i, ch in enumerate(s)]


def month_page_draws():
    """case 3: 6 rows x 2 columns of month names (text resource 1/2), x = 0xdb - 20 row, y = 0x40 + 64 col (12 higher
    for two-digit months) - drawn whole (DrawProfileTextById)"""
    out = []
    for r in range(6):
        for c in range(2):
            i = r * 2 + c
            out.append((res(1, 2, i), 0xdb - 20 * r, 0x40 + 64 * c - (12 if i >= 9 else 0)))
    return out


def day_page_draws(days):
    """case 4: 8 rows x 4 columns of day names (1/1), x = 0xdb - 20 row, y = 0x32 + 33 col, per character 10 apart,
    right-aligned; stops after `days`"""
    out = []
    for r in range(8):
        for c in range(4):
            i = r * 4 + c
            if i + 1 > days:
                return out
            out += _entry(res(1, 1, i), 0xdb - 20 * r, 0x32 + 33 * c, 10)
    return out


def blood_page_draws():
    """case 5: the four blood types (1/5), x = 0xd3 - 20 i, y = 0x66, 12 apart, right-aligned"""
    out = []
    for i in range(4):
        out += _entry(res(1, 5, i), 0xd3 - 20 * i, 0x66, 12)
    return out


PAGE_COUNTS = tuple(_G.table(0x2184058, 'B', 52, ov=6))
# one table, two readers: sub-pages per grid page type (SetProfileEditMode, types 8+) and entries per prefecture page
# (DrawProfileEditDialog case 6, by +0x5c). (Checked: the traced hometown page = 10 entries from 0, 30 / 30 positions.)
PREFECTURE_PAGE_COUNTS = PAGE_COUNTS[:12]


def prefecture_page_draws(start, count, reverse=False):
    """case 6: prefectures (1/6) `start` .. `start + count` of the region page, x = 0xea - 20 k, y = 0x36, 12 apart,
    right-aligned; region page 1 lists them in reverse (index 0x2e - i)"""
    out = []
    for k in range(count):
        i = start + k
        out += _entry(res(1, 6, 0x2e - i if reverse else i), 0xea - 20 * k, 0x36, 12)
    return out
