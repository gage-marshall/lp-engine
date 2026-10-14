"""File select (overlay 27; decompiled Ov27FileSelect_*, fid/decomp/ov27.c): three save slots on an arc carousel.

State (Sprite_Get_ca0 object): +0x78c screen state, +0x14e8 / +0x14ec / +0x14f0 slot shown left / centre / right,
+0x14f4.. their x, +0x153c + 4 i slot i has data, +0x7c + 0x1a0 i the slot's save date/time.
Layout (PositionFileSelectSlotSprites): slot k's disc sprite (+0x1424 + 4 k) at (x, arc(x)) with sequence 3 k, the
centre disc shown, the side discs hidden; its label sprite (disc + 3) sequence 3 k + 1 in the centre, 3 k + 2 at the sides.
arc(x) = 0x60 - round(((x - 128) / 9)^2) (FX_Div, square with +0x800 rounding, both 12-bit fixed point).
Centre slot at start (SetSlotDisplayOrder): the newest save (FindNewestSaveSlot), none -> slot 0; order (newest - 1,
newest, newest + 1) mod 3.
Touch targets (0x216ec8c): 0 left disc (194, 43, 40 x 40), 1 centre (128, 96), 2 right (62, 43), 3 centre large
(128, 96, 64 x 64), 4 (50, 152, 24 x 24), 5 back (14, 160, 14 x 32); after a selection only 5 is enabled
(SetFileSelectHitListDefault). Tapping a side disc (0 / 2) starts a rotation (state 3 -> 4 / 5): every frame the
three discs move by (2, 4, 3) px (left) / (3, 4, 2) (right); the middle one speeds up to 3 after 10 frames; positions
clamp at the targets (194, 128, 62); a disc whose arc y drops below -59 wraps round to the far end (x 236 / 20) and the
rotation ends when it reaches its target; the order then shifts by one."""
from lpui import gamedata as _G
# read from overlay 27 (through the code's literal pointers): disc x 0x216ec80, touch targets 0x216ec8c, rotation speeds
POS = tuple(_G.pointer_table(0x216bedc, 'i', 3, ov=27))
HITS = [r[:5] for r in _G.pointer_table(0x216c41c, '6i', 6, ov=27)]
SPEED_LEFT = tuple(_G.pointer_table(0x216c9f4, 'i', 3, ov=27))
SPEED_RIGHT = tuple(_G.pointer_table(0x216cebc, 'i', 3, ov=27))


def fx_div(a, b):
    return int((a << 12) / b) if b else 0


def arc(x):
    f = fx_div((x - 0x80) * 0x1000, 0x9000)
    sq = (f * f + 0x800) >> 12
    return 0x60 - (sq >> 12)


def initial_order(newest):
    n = 0 if newest in (None, -1) else newest
    return [(n - 1) % 3, n, (n + 1) % 3]


class FileSelect:
    def __init__(self, slot_has_data, newest):
        self.has = slot_has_data
        self.order = initial_order(newest)
        self.xs = list(POS)
        self.wrapped = False
        self.frames = 0
        self.moving = None

    def sprites(self):
        """{slot: (disc seq, x, y, shown), ...} and label sequences"""
        out = {}
        for pos, k in enumerate(self.order):
            x = self.xs[pos]
            out[k] = dict(disc=(3 * k, x, arc(x), pos == 1), label=(3 * k + (1 if pos == 1 else 2), x, arc(x)))
        return out

    def tap(self, target):
        if target in (0, 2) and self.moving is None:
            self.moving = 'left' if target == 0 else 'right'
            self.frames = 0; self.wrapped = False
            return True
        return False

    def step(self):
        """one frame of the rotation (AnimateSlotCarouselLeft / Right); True when it has finished"""
        if self.moving is None:
            return True
        left = self.moving == 'left'
        sp = list(SPEED_LEFT if left else SPEED_RIGHT)
        if self.wrapped:
            sp[2 if left else 0] = 2
        if self.frames > 10:
            sp[1] = 3
        self.frames += 1
        for i in range(3):
            self.xs[i] += -sp[i] if left else sp[i]
        if left:
            self.xs[0] = max(self.xs[0], POS[1]); self.xs[1] = max(self.xs[1], POS[2])
            if arc(self.xs[2]) < -0x3b:
                self.xs[2] = 0xec; self.wrapped = True
            done = self.wrapped and self.xs[2] <= POS[0]
        else:
            self.xs[1] = min(self.xs[1], POS[0]); self.xs[2] = min(self.xs[2], POS[1])
            if arc(self.xs[0]) < -0x3b:
                self.xs[0] = 0x14; self.wrapped = True
            done = self.wrapped and self.xs[0] >= POS[2]
        if done:
            o = self.order
            self.order = [o[2], o[0], o[1]] if left else [o[1], o[2], o[0]]
            self.xs = list(POS)
            self.moving = None
            self.frames = 0
        return done
