#!/usr/bin/env python3
"""Python port of the 36 event-resolver stage handlers (table 0x210b8f0), checked against the real ARM code by
tools/stage_check.py.  Every call that leaves the handler is a mocked leaf read through `S.py(name, *args)`.

Handler signature (native): int handler(u32 *result, u32 period, Session *s) -> r0 (0 = abort, 1 = ok), *result = script id.
Session bytes used: s[4] partner type (signed), s[5] activity code (signed)."""
import struct

M32 = 0xffffffff
SESS = 0x2430000          # session pointer value seen by the leaves in the oracle


def s8(v):
    v &= 0xff
    return v - 256 if v & 0x80 else v


def s32(v):
    v &= M32
    return v - (1 << 32) if v & 0x80000000 else v


class Stages:
    def __init__(self, mock, rom):
        self.S = mock
        self.rom = rom

    def L(self, name, *args):
        return self.S.py(name, *args)

    def r32(self, a):
        o = a - 0x2000000
        if o < 0 or o + 4 > len(self.rom):
            return 0                       # outside the static image the oracle reads zero-filled RAM
        return struct.unpack_from('<I', self.rom, o)[0]

    def r8(self, a):
        o = a - 0x2000000
        return self.rom[o] if 0 <= o < len(self.rom) else 0

    def guard3(self):
        """HasActivePrimaryState / IsSecondaryState2(0) / IsSpecialDayActive(0) prologue -> True when the stage must stop (return 1)."""
        return (self.L('Schedule_HasActivePrimaryState') != 0 or self.L('Schedule_IsSecondaryState2', 0) != 0 or
                self.L('Schedule_IsSpecialDayActive', 0) != 0)

    # stage handlers: (result_init, period, partner, act) -> (ret, result)
    def s0(self, res, period, partner, act):
        if self.L('Schedule_HasActivePrimaryState') == 0:
            self.L('Schedule_IsSecondaryState2', 0)
        return 1, 0

    def s1(self, res, period, partner, act):
        return 1, self.L('UiInput_CheckAction', period, SESS)

    def s2(self, res, period, partner, act):
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) == 0:
            return 1, self.L('UiInput_Process', period, SESS)
        return 1, 0

    def s6(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            if self.L('Schedule_HasActivePrimaryState') != 0:
                return 1, 0
            if self.L('Schedule_IsSecondaryState2', 0) != 0:
                return 1, 0
            if self.L('Schedule_HasLowFiveFlags', 0) != 0:
                return 1, 0
        if s8(act) in (0x66, 0x0d, 0x0e, 0x6e, 0x65):
            return 1, self.L('MapEvent_CheckWorkState', 3)
        return 1, 0

    def s13(self, res, period, partner, act):
        if self.L('Schedule_IsSecondaryState2') == 0:
            return 1, self.L('MapEvent_ConsumePendingSpecialValue', period)
        return 1, 0

    def s14(self, res, period, partner, act):
        if self.L('Schedule_IsSecondaryState2') == 0:
            return 1, self.L('MapEvent_GetTodaySlotValue', 0)
        return 1, 0

    def s15(self, res, period, partner, act):
        if self.L('Schedule_IsSecondaryState2') == 0:
            return 1, self.L('sub_20b43ac', 0)
        return 1, 0

    def s17(self, res, period, partner, act):
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) == 0:
            c = s8(act)
            if 0 <= c < 13 and 9 < c:
                u = 0
                if c != 10:
                    if c == 11:
                        u = 1
                    elif c == 12:
                        u = 2
                    else:
                        return 1, 0
                r = self.L('SlotCmd_ProcessSlotCommand', u, period)
                if r != 0:
                    self.L('SlotWorkflow_ProcessPhase', u)
                return 1, r
        return 1, 0

    def s18(self, res, period, partner, act):
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        queue = 0                                         # *0x2156780 is 0 in the oracle
        if self.L('sub_20b8a68', 0) != 0 and queue == 0:
            self.L('sub_20b8904')
        self.L('sub_20b892c', queue)
        if self.L('sub_20b8a68') != 0:
            r = self.L('sub_20b8a84')
            self.L('sub_20b8af8')
            return 1, r
        return 0, 0

    def s19(self, res, period, partner, act):
        if self.guard3():
            return 1, 0
        if period == 3:
            slot = self.L('GameState_GetCurrentSlot', 0)
            if self.L('GameFlags_TestScopedBit', self.r32(0x20f6f98)) == 0:
                return 1, 0
            st = self.L('TextRes_ProcessSlotState', slot)
            if s32(st) < 3:
                self.L('SaveSlot_GetSelectedIndexPtr', st)
                return 1, self.r32(self.r32(0x20f6f9c) + slot * 0xc + st * 4)
        return 1, 0

    def s21(self, res, period, partner, act):
        if period == 0 or period > 3:
            return 1, 0
        if self.guard3():
            return 1, 0
        slot = self.L('GameState_GetCurrentSlot', 0)
        st = self.L('TextRes_ProcessSlotState', slot)         # r0 still holds the slot
        if s32(st) > 2:
            return 1, 0
        c = s8(act)
        if c == -1:
            return 1, 0
        if 0x1d < c < 0x5e:
            if self.L('Schedule_TestSpecialDateCode', M32) != 0:
                return 1, 0
            av = self.L('Schedule_TestDateEventAvailability', 0, slot, SESS)
            if av != 0:
                self.L('SaveSlot_GetSelectedIndexPtr', av)
                return 1, self.r32(self.r32(0x20f7098) + slot * 0xc + st * 4)
        return 1, 0

    def s22(self, res, period, partner, act):
        v = s32(self.L('EventLogic_ClearData_cdf80', s8(act) & M32))
        if v == -1:
            return 1, 0
        r = self.r32(0x20ae48c)
        self.L('GameFlags_InvokeCallbackSlot5', r, period)
        if self.L('GameState_GetField0x78') != 0:
            if self.L('Schedule_HasActivePrimaryState') == 0 and self.L('Schedule_IsSecondaryState2') == 0:
                if self.L('Schedule_IsSpecialDayActive') == 0:
                    return 1, r
                self.L('GameState_GetField0x98')
                u = self.L('MapEvent_SelectRandomText')
            else:
                u = 0x20000000
            self.L('GameFlags_InvokeCallbackSlot5', u, period)
        return 1, r

    def s23(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            if self.guard3():
                return 1, res
        if s8(act) != -1:
            return 1, self.L('MapEvent_CheckWorkState', 1)
        return 1, res

    def s3(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') == 0:
            return 0, 0
        slot = s32(self.L('GameState_GetField0x98'))
        if slot < 0 or slot > 2:
            return 0, 0
        r = 0
        if self.L('Schedule_GetPrimaryState') == 2:
            if s8(act) == 0x71:
                cs = self.L('Schedule_GetPrimaryCounterSum')
                st = self.L('TextRes_ProcessSlotState', slot)
                if st < 3:
                    r = self.r32(self.r32(0x20f8830) + slot * 0x18 + st * 8 + (4 if cs != 1 else 0))
        elif s8(act) == 0x66 and period == 1:
            P = struct.unpack('<3I', self.S.py_out('Schedule_CopyPrimaryFields0x18', None)[0])
            S2 = struct.unpack('<3I', self.S.py_out('Schedule_CopySecondaryFields0x40', None)[0])
            if self.L('GameFlags_GetField0xe0') == 0:
                b8 = self.L('Schedule_GetSecondaryState') == 4
                b9 = self.L('Schedule_GetPrimaryState') == 4
            else:
                b8 = self.L('Schedule_GetSecondaryState') in (4, 5)
                b9 = self.L('Schedule_GetPrimaryState') in (4, 5)
            today = (self.L('GameClock_GetYear'), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay'))
            b2 = S2 == today
            b1 = P == today
            base = None
            if b9 or b1:
                st = self.L('TextRes_ProcessSlotState', slot)
                bad = st > 2
                if not bad:
                    base = self.r32(0x20f8834)
            else:
                if not b8 and not b2:
                    return 1, 0
                st = self.L('TextRes_ProcessSlotState', slot)
                bad = st > 2
                if not bad:
                    base = self.r32(0x20f8838)
            if not bad:
                r = self.r32(base + slot * 0xc + st * 4)
        return 1, r

    def s4(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') == 0:
            return 0, 0
        slot = s32(self.L('GameState_GetField0x98'))
        if slot < 0 or slot > 2:
            return 0, 0
        r = 0
        if s8(act) == 0x72 and self.L('Schedule_IsSecondaryState2') != 0:
            flag = False
            if period in (1, 3):
                acc = 0
                idx = 0
                tA = self.r32(0x20f8a60)
                tB = self.r32(0x20f8a64)
                tC = self.r32(0x20f8a68)
                sv = struct.unpack_from('<h', self.rom, tA + 0x10 - 0x2000000)[0]
                while sv != 0:
                    if s32(self.L('SaveSlot_GetSlotPointerI', slot)) < struct.unpack_from('<H', self.rom, tB + idx * 4 - 0x2000000)[0]:
                        acc = struct.unpack_from('<H', self.rom, tC + idx * 4 - 0x2000000)[0]
                        break
                    idx += 1
                    sv = struct.unpack_from('<h', self.rom, tB + idx * 4 - 0x2000000)[0]
                if self.L('sub_20f8200') != 0:
                    acc += 10
                if self.L('Rand_NextLcg32', 100) < acc:
                    flag = True
            if flag:
                v = self.L('sub_20f81b4')
                st = self.L('TextRes_ProcessSlotState', slot)
                date = self.S.py_out('GameClock_GetDate', None)[0]
                r = self.r32(self.r32(0x20f8a6c) + (4 if v == 0 else 0) + st * 8 + slot * 0x18)
                self.L('sub_20f8250', date, period)
        return 1, r

    def s5(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') == 0:
            return 0, 0
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        low = self.L('Schedule_HasLowFiveFlags', 0)
        c = s8(act)
        if low == 0:
            if c != 0x6b:
                return 1, 0
            low2 = self.L('Schedule_HasLowFiveFlags')
        else:
            low2 = self.L('Schedule_HasLowFiveFlags')
        c1 = 0
        if low2 != 0:
            c1 = c
        if low2 != 0 and c1 != 0x66:
            return 1, 0
        slot = s32(self.L('GameState_GetField0x98'))
        if slot < 0 or slot > 2:
            return 1, 0
        T = self.r32(0x20f5b54)
        cnt = s8(self.r8(T + slot * 8))
        lp = self.r32(T + slot * 8 + 4)
        self.L('GameHeap_AllocZeroed', 0xf0)
        n, pairs = self.S.py_list('MapArc_LoadMergedEntryList', lp, cnt & M32, 0, 0xffffff81)
        r = 0
        if n == 1:
            r = pairs[0][0]
        elif n > 1:
            for i in range(n):
                if pairs[i][1] == 0:
                    r = pairs[i][0]
                    break
            if r == 0:
                k = self.S.rand(n)
                r = pairs[k][0]
        self.L('GameHeap_Free', 0)
        return 1, r

    def s10(self, res, period, partner, act):
        if period != 3 and period != 2:
            return 1, 0
        if self.L('Schedule_HasActivePrimaryState', 1) != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        if self.L('Schedule_IsSpecialDayActive', 0) == 0:
            slot = self.L('GameState_GetCurrentSlot', 0)
            st = self.L('TextRes_ProcessSlotState', slot)
            if st > 2:
                return 1, 0
            if s8(act) == 0x10:
                return 1, self.r32(self.r32(0x20f6ee8) + slot * 0xc + st * 4)
        return 1, 0

    def s25(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') == 0:
            return 1, 0
        slot = s32(self.L('GameState_GetField0x98'))
        if slot < 0 or slot > 2:
            return 1, 0
        if self.guard3():
            return 1, 0
        v = s32(self.L('SaveSlot_ProcessStateRecord', slot))
        if v < 2:
            return 1, 0
        u = self.L('EventChoice_GetResult', v & M32)
        if u == 0 or u > 3:
            return 1, 0
        c = s8(act)
        ok = (c == 10 and u == 1) or (c == 11 and u == 2) or (c == 12 and u == 3)
        if not ok:
            return 1, 0
        st = self.L('TextRes_ProcessSlotState', slot)
        return 1, self.r32(self.r32(0x20b3b00) + slot * 0xc + st * 4)

    def s30(self, res, period, partner, act):
        queue = 0
        if self.L('MapArcQueue_GetStatus') != 0 and queue == 0:
            self.L('MapArcQueue_InitializeInput')
        self.L('MapArcQueue_LoadEntries', queue)
        if self.L('MapArcQueue_GetStatus') != 0:
            r = self.L('MapArcQueue_SelectRandomEntry')
            self.L('MapArcQueue_Destroy')
            return 1, r
        return 0, 0

    def s31(self, res, period, partner, act):
        f = self.L('GameState_GetField0x78')
        if self.L('Schedule_FindRuleById', s8(act) & M32, 1 if f != 0 else 0) == 0:
            return 1, 0
        if self.L('GameState_GetField0x78') == 0:
            p = s8(partner)
            if (p & M32) > 1:
                return 1, 0
            slot = s32(self.L('Schedule_GetEligibleCharacter', p & M32))
        else:
            slot = s32(self.L('GameState_GetField0x98'))
        if slot == -1:
            slot = 0
        st = self.L('TextRes_ProcessSlotState', slot & M32)
        r = self.r32(self.r32(0x20b34f8) + slot * 0x10 + st * 4)
        self.L('Schedule_ApplyCurrentRule', slot & M32, period)
        return 1, r

    def s35(self, res, period, partner, act):
        slot = self.L('GameState_GetCurrentSlot')
        r = 0
        if self.L('GameFlags_TestScopedBit', self.r32(0x20aec80)) == 0:
            v = s32(self.L('SaveSlot_ProcessStateRecord', slot))
            if v >= 3:
                r = self.r32(self.r32(0x20aec84) + slot * 4)
        return 1, r

    def s28(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') == 0:
            return 1, 0
        if self.L('GameFlags_GetField0xe0') != 0:
            return 1, 0
        if self.guard3():
            return 1, 0
        c = s8(act)
        if c > 0x6b:
            return 1, 0
        if 5 <= c <= 9:
            slot = s32(self.L('GameState_GetField0x98'))
            v = s32(self.L('SaveSlot_CallStateHook', slot & M32))
            if v > 1:
                st = self.L('TextRes_ProcessSlotState', slot & M32)
                r = self.r32(self.r32(0x20e55a0) + slot * 0x10 + st * 4)
                self.L('GameFlags_InvokeCallbackSlot4', r, period)
                return 1, r
        return 1, 0

    def s20(self, res, period, partner, act):
        if period != 2 and period != 3:
            return 1, 0
        if self.guard3():
            return 1, 0
        slot = self.L('GameState_GetCurrentSlot', 0)
        st = self.L('TextRes_ProcessSlotState', slot)
        if st > 2:
            return 1, 0
        if period == 2:
            if self.L('GameClock_GetWeekday') == 6:
                return 1, 0
        elif period == 3 and self.L('TextEvent_TestWeekdayDateCondition', st) == 0:
            return 1, 0
        c = act & 0xff
        if not (5 <= c <= 0xe):
            return 1, 0
        if self.L('Schedule_TestSpecialDateCode', c) != 0:
            return 1, 0
        av = self.L('Schedule_TestDateEventAvailability', 1, slot, SESS)
        if av != 0:
            if period == 3:
                self.L('SaveSlot_GetSelectedIndexPtr', av)
            return 1, self.r32(self.r32(0x20f6e4c) + slot * 0xc + st * 4)
        return 1, 0

    # work-state ladders (derived by tools/stage_ladders.py from the pool constants): job-event key families
    WORK5 = {p + d for p in (0xae6c880, 0x5ae6c880, 0xa2e6c880) for d in (0, 0x80, 0x100, 0x180)}
    WORK0 = {p + d for p in (0xae60080, 0x5ae60080, 0xa2e60080) for d in (0, 0x80, 0x100, 0x180)}
    EXCL11 = {p + d for p in (0x5cd86480, 0xcd86480, 0xa4d86480) for d in (0, 0x80, 0x100)}

    def s7(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            if self.L('Schedule_HasActivePrimaryState') != 0:
                return 1, 0
            if self.L('Schedule_IsSecondaryState2', 0) != 0:
                return 1, 0
            if self.L('Schedule_HasLowFiveFlags', 0) != 0:
                return 1, 0
        if s8(act) not in (0x66, 0x0d, 0x0e, 0x6e, 0x65):
            return 1, 0
        v = self.L('MapEvent_CheckWorkState', 0)
        if v in self.WORK5:
            self.L('StateLogic_GetDataMu_cbdfc', 5)
        elif v in self.WORK0:
            self.L('StateLogic_GetDataMu_cbdfc', 0)
        return 1, v

    def s11(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            if self.L('Schedule_HasActivePrimaryState') != 0:
                return 1, 0
            if self.L('Schedule_IsSecondaryState2', 0) != 0:
                return 1, 0
            if self.L('Schedule_HasLowFiveFlags', 0) != 0:
                return 1, 0
        if s8(act) != 0x6c:
            return 1, 0
        v = self.L('MapEvent_CheckWorkState', 2)
        return 1, (0 if v in self.EXCL11 else v)

    def s8(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') == 0:
            return 1, 0
        slot = s32(self.L('GameState_GetField0x98'))
        if slot < 0 or slot > 2:
            return 1, 0
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        f = self.r32(0x20f1a78)
        if self.L('GameFlags_TestScopedBit', f) == 0 and self.L('GameFlags_TestScopedBit', (f + 0xb) & M32) == 0:
            return 1, 0
        if s8(act) != 0x66:
            return 1, 0
        T = self.r32(0x20f1a7c) + slot * 8
        self.L('GameHeap_AllocZeroed', 0x300)
        n, pairs = self.S.py_list('MapArc_LoadMergedEntryList', self.r32(T + 4), s8(self.r8(T)) & M32, 0, 0xffffff81)
        r = 0
        if n == 1:
            r = pairs[0][0]
        elif n > 1:
            for i in range(n):
                if pairs[i][1] == 0:
                    r = pairs[i][0]
                    break
        self.L('GameHeap_Free', 0)
        return 1, r

    def _sched_body(self, res, period, flag):
        if self.L('GameState_GetField0x78') == 0:
            return 1, 0
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        slot = s32(self.L('GameState_GetField0x98', 0))
        self.L('GameHeap_AllocZeroed', 0x300)
        n, pairs = self.S.py_list_direct('Schedule_UpdateState', 0, 0x60)
        date = self.S.py_out('GameClock_GetDate', None)[0]
        di = s32(self.L('Schedule_GetDateEntry', date, 0))
        minute = self.L('GameClock_GetMinute')
        hour = self.L('GameClock_GetHour')
        mod = (hour * 0x3c + minute) & M32
        r = 0
        if di >= 0:
            p = self.r32(self.r32(self.r32(0x20f1918) + slot * 4) + di * 4)
            while s8(self.r8(p)) != -1:
                for k in range(n):
                    if self.r32(p + 8) == pairs[k][0]:
                        up = self.L('Schedule_GetUpcomingEntry', 0, 0)
                        if self.L('Schedule_SwitchWrapper', s8(self.r8(p + 0xe)) & M32, up) != 0:
                            ef = self.L('Schedule_GetEntryFlag', 0)
                            st = self.L('Schedule_GetState', s8(self.r8(p + 0x10)) & M32, ef)
                            u16 = self.r8(p + 0xc) | (self.r8(p + 0xd) << 8)
                            if st != 0 and period == self.r32(p + 4) and (flag == 0 or (u16 <= mod < u16 + 0x3c)):
                                r = self.r32(p + 8)
                                break
                if r != 0:
                    break
                p += 0x14
        self.L('GameHeap_Free', 0)
        return 1, r

    def s9(self, res, period, partner, act):
        return self._sched_body(res, period, 0)

    def s32(self, res, period, partner, act):
        return self._sched_body(res, period, 1)

    def s16(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            return 1, 0
        c = act & 0xff
        if c in (0, 10, 11, 12):
            for h in range(3):
                i1 = self.L('StateLogic_SelectDataMu_d7b5c', h, period, s8(act) & M32)
                if i1 != 0x19:
                    r = self.r32(self.r32(0x20d7e24) + i1 * 8)
                    self.L('StateLogic_UpdateData_d79ec')
                    i2 = self.L('StateLogic_CalculateData_d7a74', i1)
                    self.L('StateLogic_UpdateStateDataOmicron_d7e84', h, 3 if i2 != 0 else 1)
                    self.L('SaveSlot_CallOptionalHook4', 1, h)
                    u = self.L('StateLogic_SelectData_d7ad0', i1)
                    self.L('SaveSlot_CallOptionalHook4', 3, u)
                    return 1, r
        return 1, 0

    def s24(self, res, period, partner, act):
        if self.L('Schedule_HasActivePrimaryState') != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        if self.L('Schedule_IsSpecialDayActive', 0) != 0:
            return 1, 0
        c = s8(act)
        go = False
        if c > 0x6c:
            if c > 0x6e or c < 0x6e:
                return 1, 0
            if period == 2:
                return 1, 0
            go = True
        elif c > 0x6b:
            return 1, 0
        elif 5 <= c <= 9:
            go = True
        if not go:
            return 1, 0
        arg = c
        if period == 1 and self.L('GameFlags_GetField0xe0', c & M32) == 0:
            if self.L('GameFlags_TestScopedBit', self.r32(0x20af8b4)) != 0:
                return 1, 0
            arg = 0
        queue = 0
        st = self.L('MapSelector_GetStatus', arg & M32)
        if st != 0 and queue == 0:
            self.L('MapSelector_InitializeInput')
        self.L('MapSelector_LoadEntries', queue)
        if self.L('MapSelector_GetStatus') != 0:
            e = self.L('MapSelector_SelectRandomEntry', 0)
            self.L('GameFlags_InvokeCallbackSlot3', e, period)
            r = 0
            if e != 0:
                r = self.L('MapSelector_RunTextStateMachine', e, period)
            self.L('MapSelector_Destroy')
            return 1, r
        return 0, 0

    def s29(self, res, period, partner, act):
        self.L('StateLogic_UpdateData_d72b4')
        c = act & 0xff
        if c in (1, 2, 3, 4, 0xd, 0xe, 0xf, 0x10, 0xff):       # the other codes (incl. default) proceed
            return 1, 0
        arr = struct.pack('<3I', *[self.r32(self.r32(0x20d7540) + 4 * i) for i in range(3)])
        arr = self.S.py_out('StateLogic_UpdateDataXi_d70e0', arr)[0]
        ids = struct.unpack('<3i', arr)
        r4 = 0
        v = 0
        for i in range(3):
            v = ids[i]
            if v != -1 and self.L('StateLogic_UpdateStateDataXi_d7e28', v & M32) == 0:
                if self.L('StateLogic_UpdateDataPhi_d73a0', v & M32, period, SESS) != 0:
                    r4 = self.L('StateLogic_ClearDataOmicron_d7164', v & M32)
                if r4 != 0:
                    self.L('StateLogic_UpdateData_cbc8c', v & M32)
                    break
        if r4 != 0:
            self.L('StateLogic_UpdateData_cbc8c', v & M32)
            self.L('StateLogic_UpdateStateDataPi_d721c', v & M32)
        return 1, r4

    def s12(self, res, period, partner, act):
        o = self.S.py_out('CalendarRule_RunDailyEventLogic', None)[0]
        idv, code = struct.unpack('<Ii', o)
        if code < 0 or code > 0x2d:
            return 1, 0
        if code in (0x23, 0x24):
            return 1, 0
        if self.L('CharId_IsMatchingCurrent', idv & M32) == 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2') != 0:
            return 1, 0
        c = s8(act)
        if c < 0x66:
            if c < 0x65 and not (6 <= c + 1 <= 0x10):
                return 1, 0
        elif c != 0x66:
            return 1, 0
        st = struct.pack('<2I', idv & M32, code & M32)
        if self.L('GameState_GetField0x78') != 0:
            r1 = self.L('CalendarRule_RunSaveLogic', st, 0)
            if r1 == 0:
                self.L('CalendarRule_RunSaveStateMachine', st, 0)
                u = 4
            else:
                self.L('CalendarRule_RunSaveStateMachine', st, 1)
                u = 0
            self.L('EventLogic_CalculateData_cea28', u)
            v = self.L('MapEvent_GetVariantValue', code & M32, idv & M32, period)
            r = v
            if v != 0:
                r = code
                if r1 != 0:
                    r = code | 0x80000000
                if self.L('Schedule_HasActivePrimaryState') == 0:
                    if self.L('Schedule_HasLowFiveFlags') != 0:
                        r |= 0x40000000
                else:
                    r |= 0x20000000
            return 1, r
        u = self.L('MapEvent_SelectOutcome', st, None, 0, 0)
        flagbyte = self.S.py_out('MapEvent_SelectOutcome', st, None, 0, 0)[1][0]
        if self.L('CharId_IsMatchingCurrent', idv & M32) == 0 or s32(self.L('SaveSlot_ProcessStorageStatus', idv & M32)) < 1:
            u = 0
        self.L('GameFlags_RegisterCallbackState', u, flagbyte, period)
        self.L('Sprite_SetContextValue', 2, idv & M32)
        return 1, self.L('MapEvent_GetVariantValue', code & M32, idv & M32, period)

    def _text_slot_loop(self):
        """the common loop: first slot whose GetSlotValue0x24 is valid, matching the current char, not blocked, status > 1"""
        iv1 = 0
        iv2 = 0
        iv6 = 0
        while True:
            iv2 = s32(self.L('GameFlags_GetSlotValue0x24', iv1))
            iv6 = iv2
            if iv2 != -1:
                m = self.L('CharId_IsMatchingCurrent', iv2 & M32)
                iv6 = 0
                if m != 0:
                    iv6 = self.L('StateLogic_UpdateStateDataXi_d7e28', iv2 & M32)
                    if iv6 == 0:
                        iv6 = s32(self.L('SaveSlot_ProcessStorageStatus', iv2 & M32))
                        if 1 < iv6:
                            return iv2, iv6, True
            iv1 += 1
            iv2 = -1
            if iv1 >= 3:
                return iv2, iv6, False

    def s26(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            if self.guard3():
                return 1, 0
        c = s8(act)
        iv8 = 1
        if c < 0x6e:
            if c < 0x6d:
                if c >= 0x6b:
                    return 1, 0
                if not (6 <= c + 1 <= 0x10):
                    return 1, 0
            # c == 0x6d falls through without a check
        elif c < 0x70:
            if c < 0x6f and c != 0x6e:
                return 1, 0
        else:
            if c > 0x70 or c != 0x70:
                return 1, 0
        if self.L('GameState_GetField0x78') == 0:
            iv2, iv6, _ = self._text_slot_loop()
        else:
            if self.L('GameFlags_GetField0xe0') != 0:
                if 0 != 0:                # *(DAT_020ad234 + 1) is zero in the oracle
                    return 1, 0
                iv8 = 4
                u4 = 0
            else:
                if self.L('GameFlags_TestScopedBit', self.r32(0x20ad238)) != 0:
                    return 1, 0
                if self.L('Schedule_TestTimePeriodAvailable', 0) != 0:
                    return 1, 0
                u4 = self.L('TextEvent_TestCalendarCondition', 0)
                if u4 in (2, 4, 6):
                    v = s32(self.L('SaveSlot_CallStateHook', M32))
                    if v < 2:
                        return 1, 0
                    u4 = self.L('GameState_GetField0xa0', v & M32)
                    if (u4 & 1) != 0:
                        return 1, 0
            iv6 = s32(self.L('GameState_GetField0x98', u4))
            iv2 = iv6
        if iv2 == -1:
            return 1, 0
        if self.L('GameState_GetField0x78', iv6 & M32) == 0:
            iv6 = 0
            if self.L('GameState_GetField0xa0', 0) == 0:
                return 1, 0
            for i in range(4):
                o = self.S.py_out('StateLogic_UpdateData_cdb48', None, i + 1)[0]
                ok = self.L('StateLogic_UpdateData_cdb48', None, i + 1)
                if ok != 0 and o[2] < 3:
                    iv6 += 1
            if iv6 < 3:
                return 1, 0
        else:
            if s32(self.L('SaveSlot_CallStateHook', iv2 & M32)) < 2:
                return 1, 0
        status = self.L('TextEvent_UpdateMapQueue', None, iv2 & M32, period)
        q = struct.unpack('<I', self.S.py_out('TextEvent_UpdateMapQueue', None, iv2 & M32, period)[0])[0]
        if status == 0:
            return 0, 0
        if iv8 == 4 and self.S.rand(100) > 0x1d:
            return 1, 0
        if q == 0:
            if self.L('GameState_GetField0x78') == 0:
                jv2, jv6, found = self._text_slot_loop()
                if jv2 == -1 and not found:
                    return 1, 0
                if jv2 == -1:
                    return 1, 0
                q = self.r32(self.r32(0x20ad240) + jv2 * 4)
            else:
                q = self.r32(self.r32(0x20ad23c) + iv2 * 4)
        self.L('GameFlags_InvokeCallbackSlot6', q, period)
        idx = self.L('TextEvent_GetCurrentTextIndex', iv2 & M32)
        self.L('StateLogic_UpdateData_cbc8c', iv2 & M32)
        return 1, idx

    def s33(self, res, period, partner, act):
        if period != 1:
            return 1, 0
        if self.L('GameState_GetField0x78') == 0:
            return 1, 0
        if self.L('GameFlags_GetField0xe0') != 0:
            return 1, 0
        if self.L('Schedule_IsSpecialDayActive', 0) != 0:
            return 1, 0
        if self.L('GameFlags_TestScopedBit', self.r32(0x20ad4d4)) != 0:
            return 1, 0
        if self.L('TextEvent_TestClockEvent', 1) != 0:
            return 1, 0
        if self.L('Schedule_HasActivePrimaryState', 0) != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        if self.L('Schedule_TestTimePeriodAvailable', 0) != 0:
            return 1, 0
        y, m, d = self.L('GameClock_GetYear', 0), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay')
        if self.L('Calendar_IsEventActive', 0x17, y, m, d) != 0:
            return 1, 0
        if self.L('Calendar_IsEventActive', 0x1e, y, m, d) != 0:
            return 1, 0
        slot = s32(self.L('GameState_GetField0x98', 0))
        if slot == -1:
            return 1, 0
        u = self.L('TextEvent_TestCalendarCondition', M32)
        if u in (2, 4, 6):
            v = s32(self.L('SaveSlot_CallStateHook', slot & M32))
            if v < 2:
                return 1, 0
            w = self.L('GameState_GetField0xa0', v & M32)
            if (w & 1) != 0:
                return 1, 0
        status = self.L('TextEvent_UpdateMapQueue', None, slot & M32, 1)
        q = struct.unpack('<I', self.S.py_out('TextEvent_UpdateMapQueue', None, slot & M32, 1)[0])[0]
        if status == 0:
            return 0, 0
        if q == 0:
            return 1, 0
        self.L('GameFlags_InvokeCallbackSlot6', q, 1)
        idx = self.L('TextEvent_GetCurrentTextIndex', slot & M32)
        self.L('StateLogic_UpdateData_cbc8c', slot & M32)
        return 1, idx

    def s34(self, res, period, partner, act):
        if period != 3:
            return 1, 0
        if self.L('GameState_GetField0x78') == 0:
            return 1, 0
        if self.L('GameFlags_GetField0xe0') != 0:
            return 1, 0
        if self.L('GameFlags_TestScopedBit', self.r32(0x20add98)) != 0:
            return 1, 0
        if self.L('TextEvent_TestWeekdayDateCondition', 0) == 0:
            return 1, 0
        if self.L('TextEvent_TestClockEvent', 3) != 0:
            return 1, 0
        if self.L('Schedule_HasActivePrimaryState', 0) != 0:
            return 1, 0
        if self.L('Schedule_IsSecondaryState2', 0) != 0:
            return 1, 0
        if self.L('Schedule_TestTimePeriodAvailable', 0) != 0:
            return 1, 0
        y, m, d = self.L('GameClock_GetYear', 0), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay')
        if self.L('Calendar_IsEventActive', 0x17, y, m, d) != 0:
            return 1, 0
        if self.L('Schedule_HasLowFiveFlags', 0) == 0:
            if self.L('Schedule_TestAvailability') != 0:
                return 1, 0
        c = s8(act)
        if c in (10, 11, 12):
            slot = s32(self.L('GameState_GetField0x98', 0))
            u7 = self.L('StateLogic_GetDataRho')
            if self.L('Schedule_HasLowFiveFlags') != 0:
                st = self.L('TextRes_ProcessSlotState', slot & M32)
                r = self.r32(self.r32(0x20add9c) + slot * 0xc + st * 4)
                self.L('GameFlags_InvokeCallbackSlot6', r, 3)
                self.L('StateLogic_UpdateData_cbc8c', slot & M32)
                return 1, r
            status = self.L('TextEvent_UpdateMapQueue', None, slot & M32, 3)
            q = struct.unpack('<I', self.S.py_out('TextEvent_UpdateMapQueue', None, slot & M32, 3)[0])[0]
            if status != 0:
                if q != 0:
                    self.L('GameFlags_InvokeCallbackSlot6', q, 3)
                    st = self.L('TextRes_ProcessSlotState', slot & M32)
                    r = self.r32(self.r32(0x20adda0) + slot * 0x20 + (0x10 if u7 > 0x7f else 0) + st * 4)
                    self.L('StateLogic_UpdateData_cbc8c', slot & M32)
                    return 1, r
                return 1, 0
            return 0, 0
        return 1, 0

    def s27(self, res, period, partner, act):
        if self.L('GameState_GetField0x78') != 0:
            if self.L('Schedule_HasActivePrimaryState') != 0:
                return 1, 0
            if self.L('Schedule_IsSecondaryState2', 0) != 0:
                return 1, 0
        if period != 3:
            return 1, 0
        if self.L('TextEvent_TestWeekdayDateCondition', 0) == 0:
            return 1, 0
        iv8 = 0
        iv5 = 0
        iv4 = 0
        if self.L('GameState_GetField0x78') == 0:
            c = s8(act)
            if c < 0 or c > 12 or c < 10:
                return 1, 0
            iv4 = {10: 0, 11: 1, 12: 2}[c]
            ist24 = -1
            iv8 = 0
            n = s32(self.L('Schedule_ConsumeTodayCachedValue', None))
            cached = struct.unpack('<i', self.S.py_out('Schedule_ConsumeTodayCachedValue', None)[0])[0]
            if n < 1:
                ist24 = -1
            else:
                ist24 = cached
                iv8 = self.L('SaveSlot_GetLevel', ist24 & M32)
            for i in range(3):
                if ist24 != i and self.L('StateLogic_UpdateStateDataRho_d7288', i) != 0:
                    if ist24 == -1:
                        ist24 = i
                        iv8 = self.L('SaveSlot_GetLevel', i)
                    else:
                        v = self.L('SaveSlot_GetLevel', i)
                        if iv8 < v:
                            iv8 = v
                            ist24 = i
            if ist24 != -1:
                iv4 = ist24
            if self.L('CharId_IsMatchingCurrent', iv4 & M32) == 0:
                return 1, 0
            st = s32(self.L('SaveSlot_ProcessStorageStatus', iv4 & M32))
            if st < 3 and self.L('SaveSlot_GetLevel', iv4 & M32) < 10:
                return 1, 0
            x = self.L('StateLogic_UpdateStateDataXi_d7e28', iv4 & M32)
            if x != 0:
                return 1, x
            iv8 = self.L('SlotArchive_UpdateTimer', iv4 & M32)
            if iv8 == 0:
                if ist24 == -1:
                    iv5 = s32(self.L('TextEvent_TestDateMapCondition', iv4 & M32))
                    if iv5 < 0 or iv5 > 1:
                        return 1, 0
                else:
                    iv5 = 0
            else:
                iv5 = 1
        else:
            if self.L('GameFlags_GetField0xe0') == 0:
                if self.L('GameFlags_TestScopedBit', self.r32(0x20adb54)) != 0:
                    return 1, 0
            # else: *DAT_020adb50 != 0 -> the byte is zero in the oracle
            if self.L('Schedule_TestTimePeriodAvailable', 0) != 0:
                return 1, 0
            c = s8(act)
            if c > 0x6e:
                return 1, 0
            iv5 = 0
            go = False
            if c > 0x6d:
                go = True
            elif c > 0xf:
                if c != 0x6d:
                    return 1, 0
                go = True
            else:
                iv5 = c + 1
                if 10 <= c <= 0xf:
                    go = True
            if not go:
                return 1, 0
            iv4 = s32(self.L('GameState_GetField0x98', iv5 & M32))
            if self.L('GameFlags_GetField0xe0') == 0:
                v = s32(self.L('SaveSlot_CallStateHook', iv4 & M32))
                iv5 = 0 if v in (2, 3) else 1 if v == 4 else 3
            else:
                iv5 = s32(self.L('TextEvent_TestDateMapCondition', 0))
                if iv5 < 0 or iv5 > 1:
                    return 1, 0
            if self.L('Schedule_HasLowFiveFlags') != 0:
                st = self.L('TextRes_ProcessSlotState', iv4 & M32)
                r = self.r32(self.r32(0x20adb58) + iv4 * 0xc + st * 4)
                self.L('GameFlags_InvokeCallbackSlot6', r, 3)
                self.L('StateLogic_UpdateData_cbc8c', iv4 & M32)
                return 1, r
            if self.L('Schedule_TestAvailability') != 0:
                return 1, 0
        # LAB_020ada38
        if iv8 == 0:
            status = self.L('TextEvent_UpdateMapQueue', None, iv4 & M32, 3)
            q = struct.unpack('<I', self.S.py_out('TextEvent_UpdateMapQueue', None, iv4 & M32, 3)[0])[0]
            if status == 0:
                return 0, 0
        else:
            q = self.L('TextEvent_GetSlotTableValue', iv4 & M32)
        if iv5 == 3:
            if self.S.rand(100) > 0x1d:
                return 1, 0
            iv5 = 0
        if q == 0:
            if self.L('GameState_GetField0x78') == 0:
                if s32(self.L('SaveSlot_ProcessStorageStatus', iv4 & M32)) < 2:
                    q = self.r32(self.r32(0x20adb64) + iv4 * 4)
                else:
                    q = self.r32(self.r32(0x20adb60) + iv4 * 4)
            else:
                q = self.r32(self.r32(0x20adb5c) + iv4 * 4)
        self.L('GameFlags_InvokeCallbackSlot6', q, 3)
        st = self.L('TextRes_ProcessSlotState', iv4 & M32)
        r = self.r32(self.r32(0x20adb68) + iv4 * 0x20 + iv5 * 0x10 + st * 4)
        self.L('StateLogic_UpdateData_cbc8c', iv4 & M32)
        return 1, r
