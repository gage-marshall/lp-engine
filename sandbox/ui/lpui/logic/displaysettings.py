"""Display settings (room menu; static module 0x20d0378.., fid/decomp/dispui.c).

Values (loaded from save data and range-checked, StateLogic_UpdateDataPhi 0x20d0760): flag d (0/1, else 0), field c
(0..3, else 1: the slider), flag f (0/1, else 1), field e (0..5, else 0); the screen edits copies at +0x18..+0x24.
Slider: sprite slot 4 at (0x9e, c * 40 + 0x1c), fill cell slot 3 = c + 1 (+4 while dragged); a touch with x in
[bar x - 8, bar x + 16], y within bar y +- 80 picks the 40-px band (0..3) (StateLogic_ClearData 0x20d0a78).
Touch ids (StateLogic_UpdateData 0x20d0bc0): 0 OK (saves when anything changed), 1 reset, 5 / 6 flag d = 1 / 0,
7 / 8 flag f = 1 / 0, 9..14 field e = id - 9 (only while flag f is 0)."""


def clamp_loaded(d, c, f, e):
    return (d if d in (0, 1) else 0, c if 0 <= c <= 3 else 1, f if f in (0, 1) else 1, e if 0 <= e <= 5 else 0)


class DisplaySettings:
    def __init__(self, d=0, c=1, f=1, e=0):
        self.saved = clamp_loaded(d, c, f, e)
        self.d, self.c, self.f, self.e = self.saved
        self.dragging = False

    def knob(self):
        return (0x9e, self.c * 40 + 0x1c)

    def fill_cell(self):
        return self.c + 1 + (4 if self.dragging else 0)

    @staticmethod
    def slider_band(bar_x, bar_y, x, y):
        top = bar_y - 80
        if bar_x - 8 <= x <= bar_x + 16 and top <= y <= bar_y + 80:
            for i in range(4):
                if y <= (i + 1) * 40 + top:
                    return i
        return None

    def changed(self):
        """StateLogic_UpdateData 0x20d0f54: d or c differ; with the saved f = 0: f now set or e differs; else f differs"""
        d0, c0, f0, e0 = self.saved
        ch = self.c != c0 or self.d != d0
        if f0 == 0:
            return ch or self.f != 0 or self.e != e0
        return ch or self.f != f0

    def touch(self, ident):
        """-> 'save' / 'unchanged' / 'reset' / None"""
        if ident == 0:
            return 'save' if self.changed() else 'unchanged'
        if ident == 1:                       # StateLogic_RunSequenceData 0x20d08a8: d 0, c 1, f 1 (e kept)
            self.d, self.c, self.f = 0, 1, 1
            return 'reset'
        if ident in (5, 6):
            self.d = 1 if ident == 5 else 0
        elif ident in (7, 8):
            self.f = 1 if ident == 7 else 0
        elif 9 <= ident <= 14 and self.f == 0:
            self.e = ident - 9
        return None


# Screen layout from the game's layout file (menu/mount_layout.bin 0004, lpui.layoutfile): sprite slot k = touch id k;
# the touch rectangles (x, y, half w, half h) are the file's, the slider bar is slot 2 (anchor for the band hit test).
def layout():
    from lpui import layoutfile
    return layoutfile.load('display')


def touch_targets():
    """{touch id: (x, y, half w, half h)}"""
    return {s['slot']: s['hit'] for s in layout()['slots'] if s['hit']}


def slider_anchor():
    s = layout()['slots'][2]
    return s['x'], s['y']


def hit(x, y):
    """the touch id under (x, y): the file's rectangles, then the slider bar"""
    for i, (tx, ty, hw, hh) in sorted(touch_targets().items()):
        if tx - hw <= x <= tx + hw and ty - hh <= y <= ty + hh:
            return i
    bx, by = slider_anchor()
    if DisplaySettings.slider_band(bx, by, x, y) is not None:
        return 2
    return None
