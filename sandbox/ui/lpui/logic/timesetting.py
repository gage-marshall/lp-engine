"""Clock setting screen (overlay 42, onboarding; decompiled Ov42TimeSetting_*). State struct (pointer at 0x216c8a0):
+0x04 year (0..99, shown + 2000) +0x08 month +0x0c day +0x10 weekday +0x14 hour +0x18 minute +0x1c second
+0x24 hold counter  +0x28 state  +0x2c message step  sprites: +0x38.. buttons, +0x54 weekday, +0x58 AM/PM,
+0x38 + 4 * slot digit sprites (slots 9 year, 13 month, 15 day, 17 hour, 19 minute).

Display (RenderEditedDateTime / RenderTimeDigits): each field is printed with "%02d"; character c shows animation
sequence c - '-' (digits 3..12) at x = column, y = centre - 16 * len / 2 + 16 * i:
  year + 2000 @ (191, 96)   month @ (130, 55)   day @ (123, 137)   hour % 12 @ (71, 55)   minute @ (71, 137)
weekday sprite sequence = 6 for Sunday else weekday - 1; AM / PM sprite sequence 7 / 8 (hour < 12).
Until edited the fields follow the RTC (PollRtcForDisplay re-renders whenever the minute changes).

Editing (state 6): touch targets (x, y, half w, half h, id) below; ids 0..9 = year +/-, month +/-, day +/-, hour +/-,
minute +/-; 10 = OK, 11 = cancel. A held button repeats: first on press, then every 15 frames until 60 frames held,
then every 5. Year wraps 0..99; month 1..12 carries into the year; day 1..days-in-month carries into the month (day
decrement from 1 goes to 31 of the previous month, then clamps); changing month or year clamps the day; hour wraps
0..23, minute 0..59."""
import calendar

from lpui import gamedata as _G
# touch targets (x, y, half w, half h, id) read from the game: overlay 42 tables 0x216c6e0 (12, edit mode), 0x216c6c8 (OK)
EDIT_TARGETS = [r[:5] for r in _G.table(0x216c6e0, '6i', 12, ov=42)]
OK_TARGET = _G.table(0x216c6c8, '6i', 1, ov=42)[0][:5]
FIELDS = [('year', 9, 191, 96), ('month', 13, 130, 55), ('day', 15, 123, 137), ('hour', 17, 71, 55), ('minute', 19, 71, 137)]


def digits(value, slot, x, yc):
    """[(sprite slot, sequence, x, y)] of one printed field"""
    s = '%02d' % value
    return [(slot + i, (ord(c) - 0x30) + 3, x, yc - (len(s) * 16) // 2 + i * 16) for i, c in enumerate(s)]   # sub #0x30; add #3


class TimeSetting:
    def __init__(self, year, month, day, hour, minute, weekday=None):
        self.year, self.month, self.day, self.hour, self.minute = year % 100, month, day, hour, minute
        self.weekday = weekday
        self.held = 0

    def days_in_month(self):
        return calendar.monthrange(2000 + self.year, self.month)[1]

    def wd(self):
        return (calendar.weekday(2000 + self.year, self.month, self.day) + 1) % 7      # RTC_GetDayOfWeek: 0 = Sunday

    def sprites(self):
        """{sprite slot: (sequence, x, y)} plus 'weekday' / 'ampm' sequences"""
        out = {}
        vals = {'year': self.year + 2000, 'month': self.month, 'day': self.day, 'hour': self.hour % 12, 'minute': self.minute}
        for name, slot, x, yc in FIELDS:
            for sl, seq, xx, yy in digits(vals[name], slot, x, yc):
                out[sl] = (seq, xx, yy)
        w = self.wd()
        out['weekday'] = 6 if w == 0 else w - 1
        out['ampm'] = 7 if self.hour < 12 else 8
        return out

    # --- editing (Increment/DecrementEdited*)
    def _clamp_day(self):
        self.day = min(self.day, self.days_in_month())

    def year_up(self):
        self.year = 0 if self.year + 1 > 99 else self.year + 1

    def year_down(self):
        self.year = 99 if self.year == 0 else self.year - 1

    def month_up(self):
        self.month += 1
        if self.month > 12:
            self.month = 1; self.year_up()
        self._clamp_day()

    def month_down(self):
        self.month -= 1
        if self.month < 1:
            self.month = 12; self.year_down()
        self._clamp_day()

    def day_up(self):
        self.day += 1
        if self.day > self.days_in_month():
            self.day = 1; self.month_up()

    def day_down(self):
        self.day -= 1
        if self.day == 0:
            self.day = 31; self.month_down()

    def press(self, target):
        """apply one step of button `target` (0..9)"""
        [lambda: (self.year_up(), self._clamp_day()), lambda: (self.year_down(), self._clamp_day()), self.month_up,
         self.month_down, self.day_up, self.day_down,
         lambda: setattr(self, 'hour', 0 if self.hour + 1 > 23 else self.hour + 1),
         lambda: setattr(self, 'hour', 23 if self.hour == 0 else self.hour - 1),
         lambda: setattr(self, 'minute', 0 if self.minute + 1 > 59 else self.minute + 1),
         lambda: setattr(self, 'minute', 59 if self.minute == 0 else self.minute - 1)][target]()

    @staticmethod
    def repeat_fires(held):
        """does the held-button counter value `held` (1 on the press frame) apply a step"""
        if held < 1:
            return False
        if held < 60:
            return held % 15 == 1
        return held % 5 == 0
