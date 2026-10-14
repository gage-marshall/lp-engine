"""Top-screen status HUD: date widgets (DateUi_DrawCursorState 0x2080aa8), on the HUD's sprite slots (struct 0x214b9ac).

Weekday sprite (slot at +0x30): sequence from table 0x2126da4 = {weekday: (weekday + 6) % 7} (Sunday = 6).
Day counter: n = GameState +0x138 + 1 (capped at 99999); digit sprites ones / tens / hundreds / thousands / ten-thousands
(slots at +0x58 / +0x5c / +0x60 / +0x64 / +0x68) show sequence 10 + digit (table 0x2126de0); a digit is shown when it or
any higher digit is non-zero (ones always). When hidden, the digits are all set to sequence 10 (zeroed buffer). The whole counter and the weekday are hidden when n > 100 and game flag field
0xe0 is clear, or in map modes 9 / 10 (with all three clock slots occupied for mode 9).
Weekday source (GameClock_GetWeekday): manual weekday (clock +0x54) when the clock is in manual mode, else the RTC's.
Sprites: bank date_ui 447 (cells) / 448 (animations)."""

DIGIT_SLOTS = (0x58, 0x5c, 0x60, 0x64, 0x68)
WEEKDAY_SLOT = 0x30


def weekday_seq(weekday):
    return {0: 6, 1: 0, 2: 1, 3: 2, 4: 3, 5: 4, 6: 5}[weekday]


def date_widgets(day_field, weekday, hide_all=False):
    """{slot offset: (sequence, hidden)}"""
    n = min(day_field + 1, 99999)
    digits = [n % 10, n // 10 % 10, n // 100 % 10, n // 1000 % 10, n // 10000 % 10]
    if hide_all:
        digits = [0] * 5                 # the hide path skips the digit split: the zeroed digit buffer is shown (hidden)
    out = {WEEKDAY_SLOT: (weekday_seq(weekday), hide_all)}
    for i, off in enumerate(DIGIT_SLOTS):
        shown = i == 0 or any(digits[i:])
        out[off] = (10 + digits[i], hide_all or not shown)
    return out


# Time-of-day slot (DateUi_AdvanceTimeSequence 0x2080d70, every frame): the sprite at HUD +0xe8 (date_ui cell bank 31,
# the column of four circles) shows sequence 0x28 + GameClock_GetTimePeriod(). Periods (TimePeriod_FromHour 0x205ebf4,
# tables 0x2101d48 / 0x2101d4d): hour < 5 -> 5, else the first i with hour < (8, 12, 15, 18, 21)[i], else 5.
TIME_SLOT = 0xe8
import os as _os, sys as _sys
_sys.path.insert(0, _os.path.dirname(_os.path.dirname(_os.path.dirname(_os.path.abspath(__file__)))))
from lpui import gamedata as _G
PERIOD_BOUNDS = tuple(_G.table(0x2101d48, 'B', 5))     # TimePeriod_FromHour's hour table
NIGHT_START = _G.table(0x2101d4d, 'B', 1)[0]


def time_period(hour):
    if hour < NIGHT_START:
        return 5
    for i, h in enumerate(PERIOD_BOUNDS):
        if hour < h:
            return i
    return 5


def time_slot_seq(hour):
    return 0x28 + time_period(hour)
