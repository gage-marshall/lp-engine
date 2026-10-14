"""Title menu (overlay 21; decompiled Ov21TitleMenu_*, fid/decomp/ov21.c).

Pages (state +0x2ac) and their button count (SetTitleSpriteSlot): 0 main 5, 1 settings 5, 2 communication 6, 3 gallery 6,
4 ご当地 3, 5 card exchange 3, 6 save transfer 4. Button layout (static table 0x21705c4, page with n buttons at
(n - 3) * 0x54, 12 bytes each): hit rectangle (x, y, half w, half h) and sprite position. The button under the touch
shows its description in the text box (DrawTitleMenuText: text resource bank 1 group 0 item 1, index = button + page
offset (0, 5, 10, 16, 22, 25, 28), layout x 0xd, centred).
Selecting (UpdateTitleMenuGfxTransition): on the main page buttons 1 / 2 / 3 / 4 open pages 2 / 4 / 3 / 1 and button 0
leaves with code 0 (start: file select); elsewhere the last button goes back (to page 0; pages 5 and 6 to page 2),
page 2 buttons 3 / 4 open pages 5 / 6, and any other button leaves the title menu with code button + page offset (the
same index as its description: 5 display settings, 6 sound settings, 7 delete data, 8 staff roll, ...)."""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(HERE))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))

COUNT = {0: 5, 1: 5, 2: 6, 3: 6, 4: 3, 5: 3, 6: 4}
OFFSET = {0: 0, 1: 5, 2: 10, 3: 16, 4: 22, 5: 25, 6: 28}
PARENT = {1: 0, 2: 0, 3: 0, 4: 0, 5: 2, 6: 2}
MAIN_OPENS = {1: 2, 2: 4, 3: 3, 4: 1}
COMM_OPENS = {3: 5, 4: 6}
# layout table rows (x, y, half w, half h, sprite x, sprite y) per button count n, read from overlay 21 0x21705c4 + (n-3)*0x54
from lpui import gamedata as _G
LAYOUT = {n: _G.table(0x21705c4 + (n - 3) * 0x54, '6h', n, ov=21) for n in (3, 4, 5, 6)}
_T = None


def description(index):
    global _T
    if _T is None:
        import txrc
        _T = txrc.parse(open(os.path.join(ROOT, 'content/assets/text/gd_textres01_jpn.txrc'), 'rb').read(), decrypted=True)
    return _T[0][1][index].decode('utf8')


class TitleMenu:
    def __init__(self):
        self.page = 0
        self.hover = None

    def buttons(self):
        """[(button, (x, y, half w, half h), description)]"""
        n = COUNT[self.page]
        return [(i, LAYOUT[n][i][:4], description(OFFSET[self.page] + i)) for i in range(n)]

    def hit(self, x, y):
        for i, (bx, by, hw, hh), _ in self.buttons():
            if bx - hw <= x <= bx + hw and by - hh <= y <= by + hh:
                return i
        return None

    def select(self, button):
        """-> ('page', p) or ('exit', code)"""
        p = self.page
        if p == 0:
            if button in MAIN_OPENS:
                self.page = MAIN_OPENS[button]; return ('page', self.page)
            return ('exit', button)
        if p == 2 and button in COMM_OPENS:
            self.page = COMM_OPENS[button]; return ('page', self.page)
        if button == COUNT[p] - 1:
            self.page = PARENT[p]; return ('page', self.page)
        return ('exit', button + OFFSET[p])

    def tap(self, x, y):
        """UpdateTitleMenu: the first touch on a button highlights it and shows its description; touching the highlighted
        button again selects it. -> None, ('hover', button, description) or select()'s result"""
        b = self.hit(x, y)
        if b is None:
            return None
        if self.hover == b:
            self.hover = None
            return self.select(b)
        self.hover = b
        return ('hover', b, description(OFFSET[self.page] + b))
