#!/usr/bin/env python3
"""Python port of the event-session layer: MicSession_UpdateTimed (0x20c1dd0, builds the context mask / schedule bytes) and
MicSession_Clear (0x20c1f24, runs the 36 resolver stages in priority order).  Verified by tools/pipeline_check.py."""
import struct

from stage_eval import Stages, s8

M32 = 0xffffffff


class Pipeline:
    def __init__(self, mock, rom, stages):
        self.S = mock
        self.rom = rom
        self.st = Stages(mock, rom)
        self.table = stages          # [{'mask', 'kind', 'handler'}]

    def L(self, name, *args):
        return self.S.py(name, *args)

    def update_timed(self, period, p3, p4):
        """-> (ret, mask, partner_byte, act_byte); session +8/+0xc/+0x14 are cleared, +0x16 = period"""
        if period >= 6:
            return 0, None, None, None
        mask = 1 << (period & 0xff)
        mask |= 0x800 if p3 == 0 else 0x400        # p3 = main-game flag: nonzero -> 0x400
        y, m, d = self.L('GameClock_GetYear'), self.L('GameClock_GetMonth'), self.L('GameClock_GetDay')
        if self.L('Calendar_IsAnySixEventActive', y, m, d, 0) == 0:
            if self.L('Calendar_IsAnySpecialEventActive', y, m, d, 0) == 0:
                mask |= 0x100
            else:
                mask |= 0x200
        else:
            mask |= 0x10000
        dat = struct.unpack_from('<I', self.rom, 0x20c1f20 - 0x2000000)[0]
        if (mask & dat) == 0:
            mask |= 0x2000
        elif period == 3:
            mask |= 0x1000
        else:
            mask |= 0x2000
        if self.L('GameFlags_CallHookWithConst') == 0:
            mask |= 0x40
        else:
            mask |= 0x80
        rho = self.S.py_out('StateLogic_UpdateTimeDataChi_cdcf0', None)[0]
        ok = self.L('StateLogic_UpdateTimeDataChi_cdcf0', None)
        b4, b5 = rho[1], rho[2]
        if ok == 0 and p4 == 0:
            b4, b5 = 2, 0xff
        mask |= 0x4000 if p4 == 0 else 0x8000
        self.L('WorkBuf_InitPool')
        self.L('GameFlags_ComputeSlotAvailability')
        self.L('GameFlags_ComputeSlotRanking')
        return 1, mask, b4, b5

    def clear(self, mask, idx, period, partner, act):
        """-> (ret, result, kind, idx)"""
        res = 0
        kind = 0
        while idx < 0x24:
            t = self.table[idx]
            if mask == (mask & t['mask']) and t['handler'] != 0:
                fn = getattr(self.st, 's%d' % idx)
                ret, res = fn(res, period, partner, act)
                if ret == 0:
                    return 0, res, kind, idx
                if res != 0:
                    return 1, res, t['kind'], idx
                kind = 0
            idx += 1
        return 1, res, kind, idx
