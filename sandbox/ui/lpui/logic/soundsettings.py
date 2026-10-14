"""Sound settings (room menu; static MicUi_* 0x20c0430.., fid/decomp/micui.c): three volume bars and the microphone gain.

State (Sprite_GetFieldca0 object): +0x18 / +0x1c / +0x20 levels 0..15 of bars 1..3 (saved as sound level bytes 5 / 6 / 7;
bar 1 also sets the BGM channel volume level * 127 / 15), +0x28 microphone gain (0..; reset = 2), +0x8..+0x14 the values
on entry (MicUi_SetSelectedSprite: anything changed?).
Bars: sprite slots 2 / 3 / 4 (MicUi_HitTestLevelBar): a touch with x in [bar x - 8, bar x + 30] and y in
[bar y - 80, bar y + 80] sets the level to the 10-px band it falls in, (y - (bar y - 80)) // 10 (0..15).
Knob of bar i: sprite slot 10 + i at (KNOB_X[i], level * 10 + 11); bar fill: cell slot 6 + i shows cell level + 1
(+16 while the bar is being dragged).
Reset (MicUi_SetSpriteState): all three levels 15, gain 2. Leaving saves the levels and the gain (MicUi_ClampLevel)."""
KNOB_X = (0xc6, 0x99, 0x6c)
LEVELS = 16


class SoundSettings:
    def __init__(self, levels=(15, 15, 15), gain=2):
        self.levels = list(levels)
        self.gain = gain
        self.entry = (tuple(levels), gain)
        self.dragging = None

    @staticmethod
    def hit_bar(bar_x, bar_y, x, y):
        """the level a touch at (x, y) selects on a bar at (bar_x, bar_y), or None"""
        top = bar_y - 80
        if bar_x - 8 <= x <= bar_x + 30 and top <= y <= bar_y + 80:
            for i in range(LEVELS):
                if y <= (i + 1) * 10 + top:
                    return i
        return None

    def knobs(self):
        return [(KNOB_X[i], self.levels[i] * 10 + 11) for i in range(3)]

    def fill_cells(self):
        return [self.levels[i] + 1 + (16 if self.dragging == i else 0) for i in range(3)]

    def reset(self):
        self.levels = [15, 15, 15]
        self.gain = 2

    def changed(self):
        return (tuple(self.levels), self.gain) != self.entry


# Screen layout from the game's layout file (menu/mount_layout.bin 0003, lpui.layoutfile): sprite slots in file order;
# slots 2..4 are the three bars (anchors for the hit test), 10..12 the knobs (moved by the code), 0 / 1 the back and
# reset buttons (touch rectangles in the file).
def layout():
    from lpui import layoutfile
    return layoutfile.load('sound')


def bar_anchors():
    s = layout()['slots']
    return [(s[k]['x'], s[k]['y']) for k in (2, 3, 4)]


def buttons():
    """{slot id: (x, y, half w, half h)} touch rectangles from the layout file"""
    return {s['slot']: s['hit'] for s in layout()['slots'] if s['hit']}
