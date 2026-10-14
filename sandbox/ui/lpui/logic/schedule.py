"""Schedule input (overlay 3; decompiled Ov03ScheduleInput_*, fid/decomp/ov03.c): the day's plan, one activity per time
period (4 columns), each column a wheel picker.

Main touch targets (0x215ea80; x, y, half w, half h, id): 0 back (14, 160, 14, 32), 1 OK (14, 32, 14, 32), 2..5 the
columns' lower ends (203 / 154 / 102 / 49, 173, 20 x 16), 6..9 their upper ends (y 19), 10..13 the columns' centres
(203 / 152 / 101 / 50, 96, 24 x 24).
Column pickers (HandleScheduleInputPressB 0x215cdd8): column i (x 203 / 152 / 101 / 50) with n candidate activities
(+0xe54 + 0xc4 i) gets touch layout n - 1 below: target 0 = the current choice (y 96, half size 24), 11..13 the next
choices below it, 21..23 the previous ones above (half size 16). A press on target t moves the choice by t % 10 (t < 21)
or -(t % 10) (HandleScheduleInputPressD), one step per wheel animation (PollScheduleInput state 10:
ProcessScheduleInputStateP / Q -> RunScheduleInputLoopJ 0x215c360): the choice (+0xe58) goes +1 wrapping to 0 at n, or
-1 wrapping to n - 1; a column with a single candidate does not turn (GetScheduleInputFieldE54A / B: n != 1).
Which activities a period offers (and the random encounters a choice can roll) are game logic (Schedule_* / MapArc_*,
GameClock periods 1..4), not part of this screen."""

import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))))
from lpui import gamedata as _G
MAIN = [r[:5] for r in _G.table(0x215ea80, '6i', 14, ov=3)]                    # overlay 3 0x215ea80
COLUMN_X = tuple(_G.pointer_table(0x215cf38, 'i', 4, ov=3))                    # 0x215e880
_Y = _G.pointer_table(0x215cf34, '10i', 10, ov=3)                              # 0x215e6f0: y per option, layout n - 1
_ID = _G.pointer_table(0x215cf3c, '10i', 10, ov=3)                             # 0x215e890: target id per option
LAYOUT_Y = [tuple(r) for r in _Y]                                              # full rows: unused entries have id -1
LAYOUT_ID = [tuple(r) for r in _ID]


def offset_of(target):
    """HandleScheduleInputPressD: how far a picker target moves the choice"""
    return target % 10 if target < 0x15 else -(target % 10)


class Column:
    def __init__(self, x, options, choice=0):
        self.x, self.options, self.choice = x, list(options), choice

    def targets(self):
        """[(x, y, half w, half h, id)] of the enabled picker targets"""
        n = max(1, len(self.options))
        out = []
        for y, t in zip(LAYOUT_Y[n - 1], LAYOUT_ID[n - 1]):
            if t < 0:
                continue
            h = 0x18 if t == 0 else 0x10
            out.append((self.x, y, h, h, t))
        return out

    def press(self, target):
        n = len(self.options)
        if n > 1:
            k = offset_of(target)
            step = 1 if k > 0 else -1
            for _ in range(abs(k)):                       # one wheel step at a time, each wrapping
                self.choice += step
                if self.choice >= n:
                    self.choice = 0
                elif self.choice < 0:
                    self.choice = n - 1
        return self.current()

    def current(self):
        return self.options[self.choice] if self.options else None


class ScheduleInput:
    def __init__(self, options_per_period, choices=None):
        choices = choices or [0] * 4
        self.columns = [Column(COLUMN_X[i], options_per_period[i], choices[i]) for i in range(4)]

    def hit_main(self, x, y):
        for tx, ty, hw, hh, i in MAIN:
            if tx - hw <= x <= tx + hw and ty - hh <= y <= ty + hh:
                return i
        return None

    def plan(self):
        return [c.current() for c in self.columns]
