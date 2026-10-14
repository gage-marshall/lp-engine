"""The common message dialog (static MsgBox_Open 0x20be4a8 / Mod20bef50_* / MsgBox_RunStateMachine 0x20bf210): a window
(mount_adhoc gfx set, frame sprite at (128, 96)) with lines of text and buttons; used for confirmations everywhere
(clock setting, profile, sleep, save, file select).

Text: MsgBox_PrintLine(index, row, centred): text resource line `index` (of the resource set given to MsgBox_Open)
drawn in row `row` of the window's canvas (32 wide), centred in 0x90 px when asked.
Buttons (touch targets x, y, half w, half h, id): OK (80, 96, 16, 28, 1) (MsgBox_Open's default); with
MsgBox_ProcInput(2) the yes / no pair (80, 64, 16, 28, 2) and (80, 128, 16, 28, 3) (checked against the sleep and save
dialogs). Multi-page dialogs add page arrows: next (80, 158, 18, 18, 4), previous (80, 34, 18, 18, 5); on the last page
the OK button joins the previous arrow (tables 0x20bf594..0x20bf5a0). Mod20bef50_ProcInput returns the button released
on (press and release on the same target), else 0."""

import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))))
from lpui import gamedata as _G
OK = [r[:5] for r in _G.pointer_table(0x20be650, '6i', 1)]                      # MsgBox_Open's target, 0x210b690
NEXT, PREV = [r[:5] for r in _G.pointer_table(0x20bf594, '6i', 2)]              # page arrows, 0x210b7a4
YES_NO = [r[:5] for r in _G.pointer_table(0x20bee24, '6i', 2)]                  # MsgBox_HandleInput(2) 0x20bec40


class Dialog:
    def __init__(self, lines, buttons='ok', pages=1):
        self.lines = lines                # [(text, row, centred)] per page
        self.kind = buttons
        self.pages = pages
        self.page = 0
        self.pressed = None

    def targets(self):
        if self.pages > 1:
            if self.page == 0:
                return [NEXT]
            if self.page == self.pages - 1:
                return [PREV] + (OK if self.kind == 'ok' else YES_NO)
            return [NEXT, PREV]
        return OK if self.kind == 'ok' else YES_NO

    def hit(self, x, y):
        for tx, ty, hw, hh, i in self.targets():
            if tx - hw <= x <= tx + hw and ty - hh <= y <= ty + hh:
                return i
        return None

    def press(self, x, y):
        self.pressed = self.hit(x, y)

    def release(self, x, y):
        """-> 'ok' / 'yes' / 'no' / None (page arrows turn the page)"""
        b = self.pressed if self.pressed is not None and self.hit(x, y) == self.pressed else None
        self.pressed = None
        if b == 4:
            self.page += 1
        elif b == 5:
            self.page -= 1
        return {1: 'ok', 2: 'yes', 3: 'no'}.get(b)
