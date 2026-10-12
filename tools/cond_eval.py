#!/usr/bin/env python3
"""Pure-Python evaluator of the condition ops used by tmx_tbin.arc, transcribed from the native handlers and verified
against the real ARM code by tools/cond_oracle.py + tools/cond_check.py (differential test over mocked game states).

State is read only through `S.py(name, *args)` (see tools/cond_mock.py for the leaf names) so a port can bind the same
leaf names to its own game state.
"""
import struct

M32 = 0xffffffff


def s8(v):
    v &= 0xff
    return v - 256 if v & 0x80 else v


class Ev:
    def __init__(self, mock, rom=None):
        self.S = mock
        self.rom = rom          # arm9_main.bin bytes (tables are read from the ROM image, base 0x02000000)

    def r32(self, a):
        return struct.unpack_from('<I', self.rom, a - 0x2000000)[0]

    def r16s(self, a):
        return struct.unpack_from('<h', self.rom, a - 0x2000000)[0]

    def r8(self, a):
        return self.rom[a - 0x2000000]

    def L(self, name, *args):
        return self.S.py(name, *args)

    # ---- helpers -------------------------------------------------------------------------------------------------
    def cmp3(self, mode, v, b):
        """a: 0 >=, 1 ==, 2 <=  (script-style compare, value v against b)"""
        if mode == 0:
            return b <= v
        if mode == 1:
            return v == b
        if mode == 2:
            return v <= b
        return False

    def evaluate(self, op, a, b):
        f = getattr(self, 'op_%d' % op, None)
        if f is None:
            raise NotImplementedError(op)
        return bool(f(op, a & M32, b & M32))

    # ---- flags and scoped bytes ----------------------------------------------------------------------------------
    def op_1(self, op, a, b):
        return True                                   # handler is `mov r0,#1; bx lr`

    def _in_set(self, op, a, b):
        t = self.L('GameFlags_TestScopedBit', a)
        return t != 0 if op in (2, 13, 24, 107) else t == 0

    op_2 = op_3 = op_13 = op_14 = op_24 = op_25 = op_107 = op_108 = _in_set

    def _scoped_cmp(self, op, a, b):
        v = self.L('GameFlags_GetScopedByte', a)
        k = {4: 'eq', 5: 'le', 6: 'ge', 15: 'eq', 16: 'le', 17: 'ge', 26: 'eq', 27: 'le', 28: 'ge'}[op]
        return v == b if k == 'eq' else (v <= b if k == 'le' else b <= v)

    op_4 = op_5 = op_6 = op_15 = op_16 = op_17 = op_26 = op_27 = op_28 = _scoped_cmp

    # ---- heroine phase (record +4) of a fixed heroine -------------------------------------------------------------
    def _phase(self, op, a, b):
        slot = (0, 2, 1)[(op - 40) % 3]
        ph = self.L('SaveSlot_GetSlotPointerA', slot)
        ph = s8(ph)
        g = (op - 40) // 3
        if g == 0:
            return ph == s8(a)
        if g == 1:
            if a == 0:
                return ph in (1, 3)
            if a == 1:
                return ph in (0, 2)
            return False
        if a == 0:
            return (ph & 0xff) <= 1
        if a == 1:
            return ((ph - 2) & 0xff) <= 1
        return False

    op_40 = op_41 = op_42 = op_43 = op_44 = op_45 = op_46 = op_47 = op_48 = _phase

    def op_74(self, op, a, b):
        return (self.L('GameClock_GetCurrentSlot', 0x20, M32) & 1) == 1

    def op_75(self, op, a, b):
        return (self.L('GameClock_GetCurrentSlot', 4, M32) & 1) == 1

    # ---- clock ----------------------------------------------------------------------------------------------------
    def op_49(self, op, a, b):
        return self._season(op, a)

    op_50 = op_49

    def _season(self, op, a):
        s = self.L('GameClock_GetSeason')
        if a in (0, 1, 2, 3):
            return s == a if op == 49 else s != a
        if a == 4:
            return (s in (1, 3)) if op == 49 else (s not in (1, 3))
        if a == 5:
            return (s in (2, 0)) if op == 49 else (s not in (2, 0))
        return False

    def op_51(self, op, a, b):
        return self._period(op, a)

    op_52 = op_51

    def _period(self, op, a):
        p = self.L('GameClock_GetTimePeriod')
        r = False
        if 0 <= a <= 5:
            r = p == a
        elif a == 6:
            h = self.L('GameClock_GetHour')
            r = not (5 < h)
        elif a == 7:
            h = self.L('GameClock_GetHour')
            if not (h < 3):
                r = not (5 < self.L('GameClock_GetHour'))
        elif a == 8:
            h = self.L('GameClock_GetHour')
            if not (h < 6):
                r = not (8 < self.L('GameClock_GetHour'))
        elif a == 9:
            r = p < 2
        elif a == 10:
            if p == 5:
                r = 0x14 < self.L('GameClock_GetHour')
        elif a == 11:
            if p == 5:
                self.L('GameClock_GetHour')
                r = not (2 < self.L('GameClock_GetHour'))
        elif a == 12:
            if p == 5 and not (self.L('GameClock_GetHour') < 3):
                r = not (4 < self.L('GameClock_GetHour'))
        return (not r) if op == 0x34 else r

    def op_53(self, op, a, b):
        return self.L('GameClock_GetMonth') == a

    def op_54(self, op, a, b):
        return self.L('GameClock_GetMonth') != a

    def op_116(self, op, a, b):
        return self._weekday(op, a)

    op_117 = op_116

    def _weekday(self, op, a):
        wd = self.L('GameClock_GetWeekday')
        if self.L('GameState_GetField0x78') == 0:
            ev = 1 if wd == 0 else 0
        else:
            ev = self.L('Calendar_IsEventActive', 0x51, self.L('GameClock_GetYear'), self.L('GameClock_GetMonth'),
                        self.L('GameClock_GetDay'))
        r = False
        if 0 <= a <= 5:
            r = wd == a + 1
        elif a == 6:
            r = wd == 0
        elif a == 7:
            if ev == 0:
                r = wd != 6
        elif a == 8:
            r = ev == 0
        elif a == 9:
            r = (ev != 0) or wd == 6
        elif a == 10:
            r = ev != 0
        return (not r) if op == 0x75 else r

    # ---- save-record comparisons ---------------------------------------------------------------------------------
    def op_71(self, op, a, b):
        v = self.L('SaveSlot_CallStateHook', M32)
        return a == v

    def op_72(self, op, a, b):
        return self.L('SaveSlot_CallStateHook', M32) <= a

    def op_73(self, op, a, b):
        return a <= self.L('SaveSlot_CallStateHook', M32)

    def op_77(self, op, a, b):
        return b <= (self.L('SaveSlot_GetLevelFromPoints', M32) + 1) & M32

    def op_114(self, op, a, b):
        return ((self.L('SaveSlot_GetLevelFromPoints', M32) + 1) & M32) <= b

    def op_85(self, op, a, b):
        slot = self.L('GameState_GetCurrentSlot')
        return a == self.L('GameClock_GetCurrentSlot', 1, slot)

    def op_179(self, op, a, b):
        if self.L('GameState_GetField0x78') == 0:
            return False
        slot = self.L('GameState_GetField0x98')
        return a == self.L('GameClock_GetCurrentSlot', 2, slot)

    def op_118(self, op, a, b):
        slot = self.L('GameState_GetCurrentSlot')
        return (a + 1) & M32 <= self.L('SaveSlot_ResetFromField8', slot)

    def op_156(self, op, a, b):
        return b <= self.L('SaveSlot_GetLevel', M32)

    def op_158(self, op, a, b):
        out = self.S.py_out('SaveSlot_GetUnpackedDate_Ofs0x4e', None)
        year = struct.unpack('<4I', out[0])[0]
        r = 0
        if self.L('GameState_GetField0x78') != 0 and year == self.L('GameClock_GetYear'):
            r = 1
        if a == 0:
            r = 0 if r else 1
        return r != 0

    # ---- player stats ---------------------------------------------------------------------------------------------
    STAT = ('Stats_GetBase_Ofs0x2b4', 'Stats_GetBase_Ofs0x2b5', 'Stats_GetBase_Ofs0x2b6', 'Stats_GetBase_Ofs0x2b7')

    RANK = ('Stats_GetRank_Ofs0x2b4', 'Stats_GetRank_Ofs0x2b5', 'Stats_GetRank_Ofs0x2b6', 'Stats_GetRank_Ofs0x2b7')

    def _stat(self, op, a, b):
        # ops 59..70: rank of stat (op-59)//3 compared with `a` (eq, le, ge); ops 135..146: base value of the stat vs `b`
        if op <= 70:
            i = op - 59
            v = self.L(self.RANK[i // 3])
            k = i % 3
            return v == a if k == 0 else (v <= a if k == 1 else a <= v)
        i = op - 135
        v = self.L(self.STAT[i // 3])
        k = i % 3
        return v == b if k == 0 else (v <= b if k == 1 else b <= v)

    op_59 = op_60 = op_61 = op_62 = op_63 = op_64 = op_65 = op_66 = op_67 = op_68 = op_69 = op_70 = _stat
    op_135 = op_136 = op_137 = op_138 = op_139 = op_140 = op_141 = op_142 = op_143 = op_144 = op_145 = op_146 = _stat

    def op_178(self, op, a, b):
        t = sum(self.L(n) for n in self.STAT) & M32
        avg = ((t + (((t >> 1) & M32) >> 30)) & M32)
        avg = (s32(avg) >> 2) & M32
        return b <= avg

    def op_82(self, op, a, b):
        return (b * 7) & M32 <= (self.L('GameState_GetField0x9c') - self.L('GameState_GetField0x130')) & M32

    def op_92(self, op, a, b):
        return b <= self.L('GameState_GetSlotIndexWrapper')

    def op_94(self, op, a, b):
        return b <= self.L('SlotTable_ForwardGet5e_Kind3')

    def op_96(self, op, a, b):
        return b <= self.L('StatBlock_GetByte_Ofs0x73')

    def op_97(self, op, a, b):
        return b <= self.L('GameState_GetField0x3a1b')

    def op_112(self, op, a, b):
        return self.L('GameState_IsCounterOfs0xa0Above98') != 0

    def op_131(self, op, a, b):
        if a == 0:
            v = self.L('StatBlock_GetByte_Ofs0x70')
        elif a == 1:
            v = self.L('StatBlock_GetByte_Ofs0x71')
        elif a == 2:
            v = self.L('StatBlock_GetByte_Ofs0x72')
        else:
            return False
        return b <= v

    def op_132(self, op, a, b):
        if a != 0:
            return False
        return b <= self.L('StatBlock_GetByte_Ofs0x74')

    def op_133(self, op, a, b):
        t = (self.L('SaveSlot_GetSignedByte_Ofs0x49') + self.L('SaveSlot_GetSignedByte_Ofs0x4a') + self.L('SaveSlot_GetSignedByte_Ofs0x4b')) & M32
        return b <= t

    def op_134(self, op, a, b):
        return b <= self.S.blob_byte('GameState_GetBitFlagArray', 0x600)

    def op_188(self, op, a, b):
        return self.L('GameState_GetField0x134') <= b

    def op_189(self, op, a, b):
        return b <= self.L('GameState_GetField0x134')

    def op_170(self, op, a, b):
        if self.L('GameState_GetField0x78') == 0:
            u = 0
        else:
            u = 1 if self.L('GameFlags_GetField0xe0') == 0 else 0
        return a == u

    # ---- schedule / counters (a = comparator 0 >=, 1 ==, 2 <=; b = value) ------------------------------------------------
    def op_210(self, op, a, b):
        return 4 < s32(self.L('SlotTable_ForwardGet170'))

    def op_211(self, op, a, b):
        if a == 0:
            return b <= self.L('StateLogic_GetData_cc834')
        if a == 1:
            return b <= self.L('StateLogic_GetData_cc88c')
        return False

    def op_213(self, op, a, b):
        e = self.L('Schedule_CommitDateEntries')
        r = 0
        if self.L('Schedule_HasLowFiveFlags') == 0 and e != M32:
            r = 1
        if a == 0:
            r = 0 if r else 1
        return r != 0

    def op_214(self, op, a, b):
        v = self.L('Schedule_HasLowFiveFlags')
        return v == 0 if a == 0 else v != 0

    def op_215(self, op, a, b):
        return self.cmp3(a, self.L('Schedule_GetTimer'), b)

    def op_216(self, op, a, b):
        return self.cmp3(a, self.L('Schedule_GetEntryFlag', 0), b)

    def op_217(self, op, a, b):
        return self.cmp3(a, self.L('Stats_GetValue_Ofs0x2cd'), b)

    def op_218(self, op, a, b):
        return self.cmp3(a, self.L('Stats_GetValue_Ofs0x2ce'), b)

    def op_219(self, op, a, b):
        return self.cmp3(a, self.L('Stats_GetValue_Ofs0x2cf'), b)

    def op_220(self, op, a, b):
        return self.cmp3(a, self.L('Stats_GetValue_Ofs0x2d0'), b)

    def _cc764(self, op, a, b):
        idx = {221: 0, 222: 1, 223: 2, 224: 3}[op]
        return self.cmp3(a, self.L('StateLogic_GetData_cc764', idx), b)

    op_221 = op_222 = op_223 = op_224 = _cc764

    def op_225(self, op, a, b):
        r = self.L('GameFlags_CallHookWithConst')
        if a == 0:
            return r == 0
        if a == 1:
            return r != 0
        return False

    def op_226(self, op, a, b):
        r = self.L('Schedule_GetUpcomingEntry', 0, 0)
        if a == 0:
            return r == 0xa7
        if a == 1:
            return r != 0xa7
        return False

    def op_229(self, op, a, b):
        r = self.L('GameFlags_TestScopedBit', 0xed0)
        if a == 0:
            return r == 0
        if a == 1:
            return r != 0
        return False

    def op_230(self, op, a, b):
        return self.cmp3(a, self.L('Counter_GetByte_Ofs0x2e3'), b)

    def op_232(self, op, a, b):
        v = self.L('StateLogic_GetData_cc95c')
        u = 1 if v == 1 else (2 if v == 2 else 0)
        return self.cmp3(a, u, b)

    # ---- batch 2 -------------------------------------------------------------------------------------------------
    def op_78(self, op, a, b):
        R = lambda n: self.S.rand(n)
        if a == 0:
            return self.L('GameState_GetField0x78') != 0 and R(100) < 10
        if a == 1:
            return self.L('GameState_GetField0x78') != 0 and R(100) < 0x23
        if a == 2:
            if self.L('GameState_GetField0x78') != 0 and self.L('GameFlags_GetField0xe0') == 0:
                return True
            return R(100) < 0x46
        if a == 3:
            return True
        if a in (4, 5):
            return False
        if a == 6:
            if self.L('GameState_GetField0x78') != 0:
                return True
            slot = self.L('GameState_GetCurrentSlot')
            idx = s32(self.L('GameFlags_FindSlotValue0x24', slot))
            if idx < 0:
                return False
            return R(100) < self.r8(self.r32(0x2069070) + idx)
        if a == 7:
            return self.L('GameState_GetField0x78') != 0 and R(100) < 10
        if a == 8:
            return R(100) < b
        if a == 9:
            return R(100) < 0x1e
        if a in (10, 11):
            return R(100) < b
        return False

    MAPID = {0: 2, 1: 0x13, 2: 0x57, 3: 0x65, 4: 100, 5: 0x38, 6: 0x40, 7: 0x46, 8: 0x42, 9: 0x44}

    def mapid(self, a):
        return self.MAPID.get(a, M32)

    def op_79(self, op, a, b):
        return b <= self.L('SlotTable_ForwardProc20b7cfc_Kind0', self.mapid(a))

    def op_147(self, op, a, b):
        return b <= self.L('SlotTable_ForwardProc20b7cfc_Kind1', self.mapid(a))

    def op_80(self, op, a, b):
        v1 = self.L('Mod2057e7c_GetEntryByte1', self.L('GameFlags_GetField0x4'))
        return v1 == self.L('Mod2057e7c_GetEntryByte1', self.mapid(a))

    def op_83(self, op, a, b):
        rho = self.S.py_out('StateLogic_UpdateTimeDataChi_cdcf0', None)[0]
        v = self.L('StateLogic_SelectDataUpsilon_cf8e4', rho[2], a)
        return (op == 0x53) if v != 0 else (op != 0x53)

    def op_111(self, op, a, b):
        rho = self.S.py_out('StateLogic_UpdateTimeDataChi_cdcf0', None)[0]
        return rho[2] == a

    def op_91(self, op, a, b):
        m = self.L('GameClock_GetMonth')
        d = self.L('GameClock_GetDay')
        return (m, d) in ((5, 27), (10, 13), (2, 9), (7, 19), (12, 4), (2, 11), (3, 9))

    def op_148(self, op, a, b):
        slot = self.L('GameState_GetCurrentSlot')
        arr = [self.r32(self.r32(0x2069f7c) + 4 * i) for i in range(4)]
        date = struct.pack('<4I', self.L('GameClock_GetYear'), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay'),
                           self.L('GameClock_GetWeekday'))
        st = self.L('RTC_ConvertDateToDay', date)
        mult, add = self.r32(0x2069f80), self.r32(0x2069f84)
        for i in range(4):
            st = (mult * st + add) & M32
            n = (4 - i) & 0xffff
            x = st >> 16
            if n:
                x = (x * n) >> 16
            j = i + x
            arr[i], arr[j] = arr[j], arr[i]
        loc48 = [self.r32(self.r32(0x2069f88) + 4 * i) for i in range(3)]
        idx = 0
        for k in range(4):
            idx = k
            if loc48[slot] == arr[k]:
                break
            idx = 0
        loc58 = [self.r32(self.r32(0x2069f8c) + 4 * i) for i in range(4)]
        return a == loc58[idx]

    def op_152(self, op, a, b):
        rec = {0: 0, 1: 0, 2: 1, 3: 1, 4: 2, 5: 2, 6: 3, 7: 3}.get(a)
        if rec is None:
            return False
        r = self.L('EventLogic_ResetRecord', rec) != M32
        return (not r) if a % 2 == 1 else r

    def op_157(self, op, a, b):
        best = -1
        lvl_best = 0
        for i in range(3):
            lv = self.L('SaveSlot_GetLevel', i)
            nb, nl = best, lvl_best
            if best == -1:
                nb, nl = i, lv
            elif lvl_best < lv:
                nb, nl = i, lv
            else:
                nb, nl = best, lvl_best
                if lvl_best == lv:
                    if self.L('SaveSlot_GetSlotPointerI', best) < self.L('SaveSlot_GetSlotPointerI', i):
                        nb, nl = i, lv
            best, lvl_best = nb, nl
        return best == self.L('GameState_GetCurrentSlot')

    def op_155(self, op, a, b):
        y, m, d, h = (self.L('GameClock_GetYear'), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay'), self.L('GameClock_GetHour'))
        T = lambda i: self.r16s(0x2124ade + 2 * i)
        p6 = p7 = 0
        rule = 2
        if 0 <= a <= 0x14 or 0x28 <= a <= 0x3e:
            rule = T(a)
        elif 0x15 <= a <= 0x26:
            p6 = T(a)
            rule = 0x13 if a <= 0x1a else (4 if a <= 0x20 else 10)
            p7 = M32 if p6 < 0 else 0
        elif a == 0x27:
            rule = 0
        blob = self.S.py_struct('CalendarRule_ResolveDateTimeForClock', rule & M32, y, m, d, h, p6 & M32, p7 & M32)
        if blob is None:
            return False
        return (struct.unpack('<4I', blob)[3] & 0x80) != 0

    HOLIDAY = [0, 1, 2, 3, 4, 5, 0, 1, 2, 3, 4, 5, 6, 0, 1, 2, 3, 4, 6]

    def op_159(self, op, a, b):
        v = -1
        if 0 <= a <= 5:
            v = self.L('Calendar_GetMapDependentWeekValue')
        elif 6 <= a <= 12:
            v = self.L('Calendar_GetCurrentWeekAndDayIndex')
        elif 13 <= a <= 18:
            v = self.L('Calendar_GetCurrentWeekTableValue')
            if v == 5:
                v = 4
        if not 0 <= a < 19:
            return False
        return (v & M32) == self.r32(0x2124b5c + 4 * a)

    CAT = {}

    def op_180(self, op, a, b):
        v = self.L('Mod2057e7c_GetEntryByte1', self.L('GameFlags_GetField0x4'))
        A = (0x0, 0x2, 0x4, 0xd, 0x18, 0x19, 0x1b, 0x26, 0x27, 0x2e, 0x2f, 0x30, 0x37, 0x38, 0x39, 0x3a)
        B = (0x1, 0x5, 0xe, 0xf, 0x15, 0x17, 0x1a, 0x28, 0x29, 0x2a)
        C = (0x6, 0x10, 0x1c)
        D = (0x21, 0x22, 0x35, 0x3e)
        flag = 0xea4 if v in A else 0xea6 if v in B else 0xea7 if v in C else 0xea5 if v in D else None
        return flag is not None and self.L('GameFlags_TestScopedBit', flag) != 0

    def op_185(self, op, a, b):
        v = self.L('GameFlags_GetField0x4')
        ok = {300: (0,), 301: (1,), 302: (2, 4), 303: (3, 4), 304: (5,), 305: (6, 8), 306: (7, 8), 307: (9, 11), 308: (10, 11)}
        r = a in ok.get(v, ())
        return (not r) if op == 0xb9 else r

    def _slot_table(self, op, a, b):
        T0 = 0x2124ade
        k3 = op == 0x58
        sh = lambda i: self.r16s(T0 + 2 * i) & M32
        if 0 <= a <= 0x14 or 0x28 <= a <= 0x3e:
            v = self.L('SlotTable_ForwardProc20b7cfc_Kind3' if k3 else 'SlotTable_ForwardProc20b7cfc_Kind2', sh(a))
        elif 0x15 <= a <= 0x1a:
            v = self.L('SlotTable_ForwardGet4a_Kind3' if k3 else 'SlotTable_ForwardGet4a_Kind2', sh(a))
        elif 0x1b <= a <= 0x20:
            v = self.L('SlotTable_ForwardGet57_Kind3' if k3 else 'SlotTable_ForwardGet57_Kind2', sh(a))
        elif 0x21 <= a <= 0x26:
            v = self.L('SlotTable_ForwardCalc20b8048_Kind3' if k3 else 'SlotTable_ForwardCalc20b8048_Kind2', sh(a))
        elif a == 0x27:
            T1 = 0x2124ac8
            f = 'SlotTable_ForwardProc20b7cfc_Kind3' if k3 else 'SlotTable_ForwardProc20b7cfc_Kind2'
            v = (self.L(f, self.r16s(T1 + 0x16) & M32) + self.L(f, self.r16s(T1 + 0x32) & M32)) & M32
        elif a == 0x3f:
            v = self.L('SlotTable_ForwardProc20b83d4_Kind3' if k3 else 'SlotTable_ForwardProc20b83d4_Kind0', sh(a))
        else:
            return False
        return b <= v

    op_87 = op_88 = op_89 = _slot_table

    # ---- op 90: all ids of a fixed list are non-zero ---------------------------------------------------------------
    def op_90(self, op, a, b):
        ids = [self.r32(0x21024c4 + 4 * i) for i in range(21)]
        for v in ids:
            v = s32(v)
            if v < 0:
                continue
            if self.L('SlotTable_ForwardProc20b7cfc_Kind0', v & M32) == 0:
                return False
        return True

    # ---- calendar events (ops 55-58, 153, 154): EventCond_TestCalendarEvent(op, a, b, clock mode 0) -----------------
    CAL_GROUPS = {7: (4, 5, 6), 0x13: (0xe, 0xf, 0x10, 0x11, 0x12), 0x2e: (8, 9, 10), 0x2f: (0xb, 0xc, 0xd), 0x37: (0xe, 0x10),
                  0x38: (0xf, 0x11, 0x12), 0x48: (0x49, 0x4e), 0x49: (0x4a, 0x4b, 0x4c, 0x4d), 0x4e: (0x4f, 0x50, 0x51, 0x52),
                  0x53: (0x54, 0x59), 0x54: (0x55, 0x56, 0x57, 0x58), 0x59: (0x5a, 0x5b, 0x5c, 0x5d)}

    def _cal(self, op, a, b):
        return self.cal(op, a)

    op_55 = op_56 = op_57 = op_58 = op_153 = op_154 = _cal

    def dim(self, y, m):
        return self.L('Calendar_GetDaysInMonth', y & M32, m & M32)

    def cal(self, op, a):
        y, m, d = self.L('GameClock_GetYear'), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay')
        code = self.r8(0x2124a80 + a) if a < 0x60 else 0xff
        if code != 0xff:
            w = 30 if op in (0x37, 0x39) else 7 if op == 0x99 else 14 if op == 0x9a else 1
            rem = w
            while rem > 0:
                if ((a - 0x39) & M32) < 6:
                    u = code == self.L('Calendar_GetSeasonalEventCode', y, m, d)
                else:
                    u = self.L('Calendar_IsEventActive', code, y, m, d) != 0
                if u:
                    if rem == w and op not in (0x38, 0x3a):
                        return 0
                    return 1 if op != 0x3a else 0
                if op == 0x37 or ((op - 0x99) & M32) < 2:
                    d += 1
                    if self.dim(y, m) < d:
                        m += 1
                        if m > 12:
                            m = 1
                            y = y + 1
                            if y > 99:
                                y = 0
                        d = 1
                else:
                    d -= 1
                    if d == 0:
                        m -= 1
                        if m == 0:
                            y = 99 if y == 0 else y - 1
                            m = 12
                        d = self.dim(y, m)
                rem -= 1
            return 1 if op == 0x3a else 0
        if a in self.CAL_GROUPS:
            rs = [self.cal(op, k) for k in self.CAL_GROUPS[a]]
            return int(all(rs)) if op == 0x3a else int(any(rs))
        if a not in (3, 0x15, 0x2d):
            return 0
        # special dates: B = event date struct (year, month, day, w), A = today
        Bm, Bd, Bw, By = 1, 1, 0, 0
        if a == 3:
            o = self.S.py_out('GameState_ProcessField0x70', None, None, y)
            By, Bm, Bd = y, o[0][0], o[1][0]
        elif a == 0x15:
            By, Bm = y, 1
            if op in (0x37, 0x99, 0x9a):
                Bd = 1
            elif op == 0x39:
                Bd = 3
            elif m == 1 and d < 4:
                return 1 if op != 0x3a else 0
        else:
            o = self.S.py_out('SaveSlot_GetDateLeapDayAdjusted_Ofs0x4e', None, y)[0]
            By, Bm, Bd, Bw = struct.unpack('<4I', o)
            if By == y:
                if op == 0x3a:
                    return 1
                if op == 0x38:
                    return 0
        Am, Ad = m, d
        uBm, uBd, uBw = Bm, Bd, Bw
        if op == 0x39:
            nby = y
            if Am == 1 and Bm == 12:
                nby = 99 if y == 0 else y - 1
        else:
            nby = y
            if Am == 12 and Bm == 1:
                nby = y + 1
                if nby > 99:
                    nby = 0
        By = nby
        n = 1
        if op > 0x99:
            n = 0xe if op == 0x9a else 1
        elif op > 0x98:
            n = 7
        else:
            if op in (0x38, 0x3a):
                same = (Bm == Am)
                r = 1 if (same and Bd == Ad) else 0
                if op != 0x3a:
                    return r
                return 1 if r == 0 else 0
            if op in (0x37, 0x39):
                n = 0x1e
        Astruct = struct.pack('<4I', y, m, d, 0)
        Bstruct = struct.pack('<4I', By & M32, uBm & M32, uBd & M32, uBw & M32)
        if op == 0x39:
            return self.L('Calendar_IsWithinDaysAfter', Bstruct, n, Astruct)
        return self.L('Calendar_IsWithinDaysAfter', Astruct, n, Bstruct)

    # ---- record / file level (CondList_EvaluateGroups / CollectPassingRecords) -------------------------------------
    def eval_record(self, groups, handlers):
        res = False
        for g in groups:
            counted = False
            acc = True
            for op, a, b in g:
                if op >= len(handlers) or handlers[op] == 0:
                    continue
                counted = True
                r = self.evaluate(op, a, b)      # every handler runs (no short circuit): matters for the RNG call order
                acc = acc and r
            if counted:
                res = res or acc
        return res

    def collect(self, recs, thr, handlers):
        """recs: parsed TERM records.  Returns ([(id, used_flag_value)], final_priority_threshold)."""
        out = []
        thr = s32(thr)
        for r in recs:
            pr = s32(r['priority'])
            if pr < thr:                      # signed compare, like the native `(int)priority < threshold`
                continue
            if self.eval_record(r['groups'], handlers):
                if thr < pr:
                    out = []
                    thr = pr
                out.append((r['id'], self.L('GameFlags_TestScopedBit', r['flag'])))
        return out, thr


def s32(v):
    v &= M32
    return v - (1 << 32) if v & 0x80000000 else v
