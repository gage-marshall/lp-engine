#!/usr/bin/env python3
"""Deterministic mock game state for the condition-op oracle.  Both the Unicorn side (real handlers) and the Python
evaluator (tools/cond_eval.py) read leaf values through this class, so equal calls give equal answers."""
import hashlib
import struct

# value ranges for bounded leaves (inclusive); anything else defaults to 0..4 (1 in 8: 0..255)
RANGES = {
    'GameClock_GetYear': (0, 99), 'GameClock_GetMonth': (1, 12), 'GameClock_GetDay': (1, 31), 'GameClock_GetHour': (0, 23),
    'GameClock_GetMinute': (0, 59), 'GameClock_GetSecond': (0, 59), 'GameClock_GetWeekday': (0, 6),
    'GameClock_GetTimePeriod': (0, 5), 'GameClock_GetSeason': (0, 3),
    'GameClock_GetYearFromTimestamp': (0, 99), 'GameClock_GetMonthFromTimestamp': (1, 12), 'GameClock_GetDayFromTimestamp': (1, 31),
    'GameClock_GetHourFromTimestamp': (0, 23), 'GameClock_GetMinuteFromTimestamp': (0, 59), 'GameClock_GetWeekdayFromTimestamp': (0, 6),
    'GameClock_GetSecondFromTimestamp': (0, 59),
    'GameState_GetCurrentSlot': (0, 2), 'GameState_GetField0x98': (-1, 2), 'GameState_GetField0x78': (0, 1), 'Schedule_HasActivePrimaryState': (0, 1), 'Schedule_IsSecondaryState2': (0, 1), 'Schedule_IsSpecialDayActive': (0, 1),
    'GameFlags_Test': (0, 1), 'GameFlags_TestScopedBit': (0, 1), 'GameFlags_GetField0xe0': (0, 1), 'GameFlags_GetField0x4': (0, 40),
    'GameFlags_GetScopedByte': (0, 30), 'GameFlags_CallHookWithConst': (0, 1), 'Calendar_IsEventActive': (0, 1),
    'Calendar_IsWithinDaysAfter': (0, 1), 'Calendar_GetDaysInMonth': (28, 31), 'Calendar_GetSeasonalEventCode': (0, 12),
    'Calendar_MonthToSeason': (0, 3), 'TimePeriod_FromHour': (0, 5),
    'Stats_GetBase_Ofs0x2b4': (0, 250), 'Stats_GetBase_Ofs0x2b5': (0, 250), 'Stats_GetBase_Ofs0x2b6': (0, 250),
    'Stats_GetBase_Ofs0x2b7': (0, 250), 'SaveSlot_GetLevel': (0, 255), 'SaveSlot_GetLevelFromPoints': (0, 16),
    'SaveSlot_ResetFromField8': (1, 20), 'SaveSlot_GetSlotPointerA': (0, 2), 'SaveSlot_GetSlotPointerI': (0, 255),
    'Rand_Next': (0, 99), 'Schedule_ConsumeTodayCachedValue': (0, 2), 'TextEvent_TestDateMapCondition': (-1, 2), 'TextEvent_TestCalendarCondition': (0, 6), 'TextEvent_TestClockEvent': (0, 1), 'Schedule_TestTimePeriodAvailable': (0, 1), 'Schedule_TestAvailability': (0, 1), 'TextEvent_TestWeekdayDateCondition': (0, 1), 'StateLogic_UpdateStateDataRho_d7288': (0, 1), 'SlotArchive_UpdateTimer': (0, 1), 'StateLogic_GetDataRho': (0, 255), 'StateLogic_UpdateStateDataXi_d7e28': (0, 1), 'CalendarRule_RunSaveLogic': (0, 1), 'CharId_IsMatchingCurrent': (0, 1), 'MapEvent_SelectOutcome': (0, 3), 'SaveSlot_ProcessStorageStatus': (0, 3), 'StateLogic_UpdateData_cdb48': (0, 1), 'TextEvent_UpdateMapQueue': (0, 1), 'GameState_GetField0xa0': (0, 7), 'GameFlags_GetSlotValue0x24': (-1, 2), 'Schedule_GetDateEntry': (-1, 1), 'Schedule_UpdateState': (0, 4), 'StateLogic_SelectDataMu_d7b5c': (0, 0x19), 'StateLogic_CalculateData_d7a74': (0, 1), 'MapSelector_GetStatus': (0, 1), 'MapSelector_SelectRandomEntry': (0, 3), 'Schedule_FindRuleById': (0, 1), 'Schedule_GetEligibleCharacter': (-1, 2), 'SaveSlot_ProcessStateRecord': (0, 4), 'EventChoice_GetResult': (0, 4), 'Rand_NextLcg32': (0, 99), 'MapArc_LoadMergedEntryList': (0, 4), 'Schedule_GetPrimaryState': (0, 5), 'Schedule_GetSecondaryState': (0, 5), 'Schedule_GetPrimaryCounterSum': (0, 3), 'TextRes_ProcessSlotState': (0, 3), 'GameFlags_FindSlotValue0x24': (-1, 2), 'GameState_GetField0x9c': (0, 400), 'GameState_GetField0x130': (0, 400), 'Schedule_HasLowFiveFlags': (0, 1), 'Schedule_GetTimer': (0, 12),
    'StatBlock_GetByte_Ofs0x70': (0, 10), 'StatBlock_GetByte_Ofs0x71': (0, 10), 'StatBlock_GetByte_Ofs0x72': (0, 10),
    'StatBlock_GetByte_Ofs0x73': (0, 10), 'StatBlock_GetByte_Ofs0x74': (0, 10), 'Deadline_GetDayCount': (0, 3),
    'Mod2057e7c_GetEntryByte1': (0, 20), 'GameState_GetField0x200': (0, 30), 'GameState_GetField0x134': (0, 30),
    'Counter_GetByte_Ofs0x2e2': (0, 10),
}


# number of leading register arguments that are part of the key (everything else is ignored)
ARITY = {
    'Schedule_ConsumeTodayCachedValue': 1, 'TextEvent_TestDateMapCondition': 1, 'TextEvent_TestCalendarCondition': 1,
    'TextEvent_TestClockEvent': 1, 'Schedule_TestTimePeriodAvailable': 1, 'TextEvent_TestWeekdayDateCondition': 1,
    'StateLogic_UpdateStateDataRho_d7288': 1, 'SlotArchive_UpdateTimer': 1, 'TextEvent_GetSlotTableValue': 1,
    'TextEvent_GetCurrentTextIndex': 1, 'StateLogic_UpdateStateDataXi_d7e28': 1, 'GameFlags_InvokeCallbackSlot6': 2,
    'StateLogic_UpdateData_cbc8c': 1, 'SaveSlot_GetLevel': 1, 'SaveSlot_ProcessStorageStatus': 1,
    'CalendarRule_RunDailyEventLogic': 1, 'CalendarRule_RunSaveLogic': 2, 'CalendarRule_RunSaveStateMachine': 2, 'MapEvent_SelectOutcome': 4,
    'MapEvent_GetVariantValue': 3, 'GameFlags_RegisterCallbackState': 3, 'Sprite_SetContextValue': 2, 'CharId_IsMatchingCurrent': 1,
    'EventLogic_CalculateData_cea28': 1, 'TextEvent_UpdateMapQueue': 3, 'StateLogic_UpdateData_cdb48': 2, 'GameFlags_GetSlotValue0x24': 1,
    'GameState_GetField0xa0': 1,
    'GameFlags_Test': 2, 'GameFlags_TestScopedBit': 1, 'GameFlags_GetScopedByte': 1, 'Calendar_IsEventActive': 4,
    'Calendar_GetDaysInMonth': 2, 'Calendar_GetSeasonalEventCode': 3, 'Calendar_MonthToSeason': 1, 'TimePeriod_FromHour': 1,
    'GameClock_GetCurrentSlot': 2, 'SaveSlot_GetLevel': 1, 'SaveSlot_GetSlotPointerA': 1, 'SaveSlot_GetSlotPointerI': 1,
    'SaveSlot_ResetFromField8': 1, 'SaveSlot_GetLevelFromPoints': 1, 'SaveSlot_CallStateHook': 1, 'Schedule_GetEntryFlag': 1,
    'Schedule_GetUpcomingEntry': 2, 'Deadline_GetDayCount': 2, 'Mod2057e7c_GetEntryByte1': 1, 'EventLogic_ResetRecord': 1,
    'EventLogic_ProcessRecord': 1, 'StateLogic_GetData_cc764': 1, 'StateLogic_SelectDataUpsilon_cf8e4': 2, 'Rand_Next': 1,
    'GameState_GetSlotPtr': 1, 'SlotTable_ForwardProc20b7cfc_Kind0': 1, 'SlotTable_ForwardProc20b7cfc_Kind1': 1,
    'SlotTable_ForwardProc20b7cfc_Kind2': 1, 'SlotTable_ForwardProc20b7cfc_Kind3': 1, 'SlotTable_ForwardGet4a_Kind2': 1,
    'SlotTable_ForwardGet57_Kind2': 1, 'SlotTable_ForwardCalc20b8048_Kind2': 1, 'Script_ReadVariable': 1,
    'GameState_ProcessField0x70': 3, 'Calendar_IsWithinDaysAfter': 3, 'RTC_ConvertDateToDay': 1,
    'SaveSlot_GetUnpackedDate_Ofs0x4e': 1, 'SaveSlot_GetDateLeapDayAdjusted_Ofs0x4e': 1, 'StateLogic_UpdateTimeDataChi_cdcf0': 1, 'GameState_GetSlotIndexWrapper': 0,
    'CalendarRule_ResolveDateTimeForClock': 7, 'Calendar_GetCurrentWeekTableValue': 0, 'GameFlags_FindSlotValue0x24': 1,
    'SlotTable_ForwardGet4a_Kind3': 1, 'SlotTable_ForwardGet57_Kind3': 1, 'SlotTable_ForwardCalc20b8048_Kind3': 1,
    'SlotTable_ForwardProc20b83d4_Kind0': 1, 'SlotTable_ForwardProc20b83d4_Kind3': 1,
}
SKIP = {'MapSelector_SelectRandomEntry': {0}, 'Schedule_UpdateState': {0}, 'MapArc_LoadMergedEntryList': {2}, 'GameHeap_AllocZeroed': {0}, 'GameHeap_Free': {0}, 'MIi_CpuClearFast': {0, 1, 2}}
DATE_OUT = {'Schedule_CopyPrimaryFields0x18': 12, 'Schedule_CopySecondaryFields0x40': 12, 'GameClock_GetDate': 16}
HEAP = 0x2440000
# percent chance that a guard predicate reads 0 (so handlers get past their guards often enough to be tested)
PZERO = {'Schedule_TestTimePeriodAvailable': 80, 'Schedule_TestAvailability': 80, 'TextEvent_TestClockEvent': 80,
         'StateLogic_UpdateStateDataXi_d7e28': 80, 'StateLogic_UpdateStateDataRho_d7288': 40, 'Schedule_HasActivePrimaryState': 85, 'Schedule_IsSecondaryState2': 85, 'Schedule_IsSpecialDayActive': 85,
         'Schedule_HasLowFiveFlags': 80, 'GameState_GetField0xe0': 50}
AUTO = None      # name -> estimated arity, installed by tools/stage_check.py (tools/arity.py)
PTR_IN = {'CalendarRule_RunSaveLogic': {0: 2}, 'CalendarRule_RunSaveStateMachine': {0: 2}, 'MapEvent_SelectOutcome': {0: 2}, 'Schedule_GetDateEntry': {0: 4}, 'StateLogic_UpdateDataXi_d70e0': {0: 3}, 'sub_20f8250': {0: 4}, 'Calendar_IsWithinDaysAfter': {0: 4, 2: 4}, 'RTC_ConvertDateToDay': {0: 4}}
PTR_RET = {'CalendarRule_ResolveDateTimeForClock'}
BLOB = {'GameState_GetBitFlagArray': (0x2360000, 0x800)}   # leaf returns a pointer to this many deterministic bytes
PTR_OUTIDX = {'Schedule_ConsumeTodayCachedValue': (0,), 'CalendarRule_RunDailyEventLogic': (0,), 'MapEvent_SelectOutcome': (1,), 'StateLogic_UpdateData_cdb48': (0,), 'TextEvent_UpdateMapQueue': (0,), 'StateLogic_UpdateDataXi_d70e0': (0,), 'Schedule_CopyPrimaryFields0x18': (0,), 'Schedule_CopySecondaryFields0x40': (0,), 'GameClock_GetDate': (0,), 'SaveSlot_GetUnpackedDate_Ofs0x4e': (0,), 'SaveSlot_GetDateLeapDayAdjusted_Ofs0x4e': (0,), 'GameState_ProcessField0x70': (0, 1), 'StateLogic_UpdateTimeDataChi_cdcf0': (0,)}

class Mock:
    """Deterministic leaf provider.  key = (name, masked args up to ARITY[name], bytes for pointer-in args)."""

    def __init__(self, seed, fixed=None):
        self.seed = seed
        self.calls = 0
        self.fixed = fixed or {}

    def h(self, key):
        return int.from_bytes(hashlib.blake2b(repr((self.seed, key)).encode(), digest_size=8).digest(), 'little')

    def scalar(self, name, key):
        if name in self.fixed:
            return self.fixed[name]
        x = self.h((name, key))
        if name in PZERO and (x >> 48) % 100 < PZERO[name]:
            return 0
        lo, hi = RANGES.get(name, (0, 4))
        if name not in RANGES and (x >> 40) % 8 == 0:
            hi = 255
        return lo + (x % (hi - lo + 1))

    def make_key(self, name, args):
        n = ARITY.get(name)
        if n is None:
            n = AUTO.get(name, 0) if AUTO else 0
        out = []
        for i in range(n):
            if i in SKIP.get(name, ()):
                continue
            v = args[i] if i < len(args) else 0
            if v is None:
                v = 'out'
            out.append(v if isinstance(v, (bytes, str)) else v & 0xffffffff)
        return tuple(out)

    # ---- outputs / special returns (shared by both sides) ----
    def outputs(self, name, key):
        """dict argindex -> bytes written through out-pointers."""
        o = {}
        if name in ('SaveSlot_GetUnpackedDate_Ofs0x4e', 'SaveSlot_GetDateLeapDayAdjusted_Ofs0x4e'):
            x = [self.h((name, key, i)) for i in range(4)]
            o[0] = struct.pack('<4I', x[0] % 100, 1 + x[1] % 12, 1 + x[2] % 31, x[3] % 7)
        elif name in ('Schedule_CopyPrimaryFields0x18', 'Schedule_CopySecondaryFields0x40'):
            x = self.h((name, key, 'same'))
            if x % 2 == 0:       # half of the time equal to today so the date-equality branches get exercised
                w = (self.scalar('GameClock_GetYear', ()), self.scalar('GameClock_GetMonth', ()), self.scalar('GameClock_GetDay', ()))
            else:
                w = (x % 100, 1 + (x >> 8) % 12, 1 + (x >> 16) % 31)
            o[0] = struct.pack('<3I', *w)
        elif name == 'CalendarRule_RunDailyEventLogic':
            x = self.h((name, key, 'd'))
            o[0] = struct.pack('<Ii', x % 4, ((x >> 8) % 0x34) - 2)
        elif name == 'MapEvent_SelectOutcome':
            o[1] = bytes([self.h((name, key, 'b')) % 2])
        elif name == 'StateLogic_UpdateData_cdb48':
            x = self.h((name, key, 'b'))
            o[0] = ('cdb48', x % 2, (x >> 8) % 5)            # (nonzero?, byte at +4)
        elif name == 'Schedule_ConsumeTodayCachedValue':
            o[0] = struct.pack('<i', (self.h((name, key, 'v')) % 4) - 1)
        elif name == 'TextEvent_UpdateMapQueue':
            x = self.h((name, key, 'q'))
            o[0] = struct.pack('<I', 0 if x % 3 == 0 else (x >> 8) & 0xffff)
        elif name == 'StateLogic_UpdateDataXi_d70e0':
            o[0] = struct.pack('<3i', *[(self.h((name, key, i)) % 4) - 1 for i in range(3)])
        elif name == 'GameClock_GetDate':
            o[0] = struct.pack('<4I', self.scalar('GameClock_GetYear', ()), self.scalar('GameClock_GetMonth', ()),
                               self.scalar('GameClock_GetDay', ()), self.scalar('GameClock_GetWeekday', ()))
        elif name == 'GameState_ProcessField0x70':
            o[0] = bytes([self.h((name, key, 0)) % 32])
            o[1] = bytes([self.h((name, key, 1)) % 32])
        elif name == 'StateLogic_UpdateTimeDataChi_cdcf0':
            b4 = self.h((name, key, 4)) % 9
            b5 = self.h((name, key, 5)) % 0x74
            o[0] = ('rho', 0xff if b4 == 8 else b4, 0xff if b5 == 0x73 else b5)
        return o

    def struct_ret(self, name, key):
        if name == 'CalendarRule_ResolveDateTimeForClock':
            x = self.h((name, key))
            if x % 4 == 0:
                return None
            return struct.pack('<4I', *[self.h((name, key, i)) & 0xffffffff for i in range(4)])
        return None

    def merged_list(self, key, count_scalar):
        """MapArc_LoadMergedEntryList: n pairs (id, used) written to the array at outstruct[1]."""
        n = count_scalar
        return [(1 + self.h(('ml', key, i)) % 0xffff, self.h(('mlu', key, i)) % 2) for i in range(n)]

    def rand(self, n):
        self.calls += 1
        r = self.scalar('Rand_Next', (n & 0xffffffff, self.calls))
        return r % n if n else r

    def blob_byte(self, name, off):
        return self.h((name, 'blob', off)) & 0xff

    # ---- Python-evaluator side: leaf('Name', args...) -> scalar; pointer-in args passed as bytes, out args as None ----
    def py(self, name, *args):
        if name == 'Rand_Next':
            return self.rand(args[0])
        key = self.make_key(name, args)
        return self.scalar(name, key) & 0xffffffff

    def py_list(self, name, *args):
        key = self.make_key(name, args)
        n = self.scalar(name, key)
        return n, self.merged_list(key, n)

    def py_list_direct(self, name, *args):
        key = self.make_key(name, args)
        n = self.scalar(name, key)
        return n, self.merged_list(key, n)

    def py_out(self, name, *args):
        key = self.make_key(name, [('out' if a is None else a) for a in args])
        return self.outputs(name, key)

    def py_struct(self, name, *args):
        return self.struct_ret(name, self.make_key(name, args))

    # ---- Unicorn side ----
    def leaf(self, name, args, uc, sp):
        n = ARITY.get(name)
        if n is None:
            n = AUTO.get(name, 0) if AUTO else 0
        kargs = []
        for i in range(n):
            v = args[i] if i < 4 else int.from_bytes(bytes(uc.mem_read(sp + 4 * (i - 4), 4)), 'little')
            if i in PTR_IN.get(name, {}):
                kargs.append(bytes(uc.mem_read(v, 4 * PTR_IN[name][i])))
            elif i in PTR_OUTIDX.get(name, ()):
                kargs.append('out')
            else:
                kargs.append(v)
        if name == 'Rand_Next':
            return self.rand(args[0])
        key = self.make_key(name, kargs)
        for i, val in self.outputs(name, key).items():
            if isinstance(val, tuple) and val[0] == 'rho':          # bytes at +4 and +5 only
                uc.mem_write(args[i] + 4, bytes([val[1], val[2]]))
            elif isinstance(val, tuple):                              # cdb48: byte at +4 (struct returned through arg 0)
                uc.mem_write(args[i] + 4, bytes([val[2]]))
            else:
                uc.mem_write(args[i], val)
        if name == 'GameHeap_AllocZeroed':
            self.heap_n = getattr(self, 'heap_n', 0) + 1
            addr = HEAP + 0x1000 * self.heap_n
            uc.mem_write(addr, b'\0' * 0x400)
            return addr
        if name == 'Schedule_UpdateState':
            n = self.scalar(name, key)
            uc.mem_write(args[0], b''.join(struct.pack('<2I', i, u) for i, u in self.merged_list(key, n)))
            return n
        if name == 'MapArc_LoadMergedEntryList':
            n = self.scalar(name, key)
            arr = int.from_bytes(bytes(uc.mem_read(args[2] + 4, 4)), 'little')
            uc.mem_write(arr, b''.join(struct.pack('<2I', i, u) for i, u in self.merged_list(key, n)))
            return n
        if name in BLOB:
            base, n = BLOB[name]
            uc.mem_write(base, bytes(self.blob_byte(name, i) for i in range(n)))
            return base
        if name in PTR_RET:
            blob = self.struct_ret(name, key)
            if blob is None:
                return 0
            addr = 0x2350000 + (self.h((name, key, 'a')) % 64) * 0x20
            uc.mem_write(addr, blob)
            return addr
        return self.scalar(name, key) & 0xffffffff


