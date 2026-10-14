"""Static option screen Menu_* 0x20952c0..0x20961a0 (fid/decomp/options.c). NOT the title menu's settings: the explorer
showed the title menu's 表示設定 runs the display-settings module (lpui.logic.displaysettings) and these functions never
execute there; its caller is unknown (no reachable screen found yet). Written from the code, unchecked.

Values (Menu_LoadSavedOptions): a = (save field 8 == 0) (0 / 1), level = save field c (0..3), flag = save flag f,
choice = save flag e (0..5); the screen keeps the values on entry for reset.
Sprites (table 0x2103690: resource, id, -, x, y, ...): touch rectangles are built around the sprite of each id
(Menu_HandleTouchSelection, ids checked in order 6 7 8 11 12 13..18 20 21 22):
  6 / 7 at (206, 66) / (206, 147): x +-12, y +-24 -> a = 0 / 1
  8 the level bar at (160, 92): x -8..+16, y +-80 -> level = the 40-px band (Menu_UpdateTouchRowHover)
  11 / 12 at (128, 97): x -16..+8, y -72..0 / 0..+72 -> flag = 1 / 0
  13..18 at (88, 36 + 24 k): x +-16, y +-12 (only while flag is 0) -> choice = k
  20 / 21 / 22 at (40, 32 / 96 / 160): x +-16, y +-32 -> save / reset / back; each needs a second touch (the first arms
  it, Menu_ToggleOption*): save writes the values and leaves (code 4), reset restores the entry values, back leaves
  (code 3). A touch acts on release over the target it was pressed on."""
RECTS = {6: (206 - 12, 206 + 12, 66 - 24, 66 + 24), 7: (206 - 12, 206 + 12, 147 - 24, 147 + 24),
         8: (160 - 8, 160 + 16, 92 - 80, 92 + 80), 11: (128 - 16, 128 + 8, 97 - 72, 97), 12: (128 - 16, 128 + 8, 97, 97 + 72),
         20: (40 - 16, 40 + 16, 0, 64), 21: (40 - 16, 40 + 16, 64, 128), 22: (40 - 16, 40 + 16, 128, 192)}
for _k in range(6):
    RECTS[13 + _k] = (88 - 16, 88 + 16, 36 + 24 * _k - 12, 36 + 24 * _k + 12)
ORDER = (6, 7, 8, 11, 12, 13, 14, 15, 16, 17, 18, 20, 21, 22)


class OptionScreen:
    def __init__(self, field8=0, level=1, flag=1, choice=0):
        self.a, self.level, self.flag, self.choice = int(field8 == 0), level, flag, choice
        self.entry = (self.a, self.level, self.flag, self.choice)
        self.armed = None

    def hit(self, x, y):
        for i in ORDER:
            if 13 <= i <= 18 and self.flag:
                continue
            x0, x1, y0, y1 = RECTS[i]
            if x0 <= x <= x1 and y0 <= y <= y1:
                return i
        return None

    def release(self, x, y):
        """apply a touch released at (x, y); -> 'save' / 'back' / None"""
        i = self.hit(x, y)
        if i in (6, 7):
            self.a = i - 6
        elif i == 8:
            band = (y - (92 - 80)) // 40
            self.level = max(0, min(3, band))
        elif i in (11, 12):
            self.flag = 1 if i == 11 else 0
        elif i is not None and 13 <= i <= 18:
            self.choice = i - 13
        elif i in (20, 21, 22):
            if self.armed != i:
                self.armed = i
                return None
            self.armed = None
            if i == 20:
                return 'save'
            if i == 21:
                self.a, self.level, self.flag, self.choice = self.entry
                return None
            return 'back'
        return None

    def save_fields(self):
        """(field 8, field c, flag f, flag e) as Menu_SaveOptionsAndLoadOverlay writes them"""
        return (int(self.a != 1), self.level, self.flag, self.choice)
