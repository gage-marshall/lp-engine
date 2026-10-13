#!/usr/bin/env python3
"""Model of the game's backdrop selection (ScheduleText_UpdateState @0x209dc34 + helpers), validated against play_monitor logs.

record table (20 bytes per bg id) at the pointer in the literal pool 0x209e064: {id, base file number, flags[3] at +8, mode at +0xc, param at +0x10}
season index (ScheduleText_SetState of GameClock_GetSeason): spring 0, summer 1, autumn 2, winter 3
tod index (ScheduleText_GetStateValue): day 0, evening 1, night 2
file number -> list entry in arc/bg_file_no_0N.bin (N = archive); the picture is that entry's (NCGR, NCLR, NSCR) triple."""
import struct, os
ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
B = 0x2000000
_static = open(os.path.join(ROOT, 'content/engine/static.bin'), 'rb').read()
TABLE = struct.unpack_from('<I', _static, 0x209e064 - B)[0] - B


def record(i):
    r = _static[TABLE + i * 20:TABLE + i * 20 + 20]
    w = struct.unpack_from('<5i', r)
    return {'id': w[0], 'base': w[1], 'flags': list(r[8:11]), 'mode': w[3], 'param': r[16]}


def field(cond, param, p3):
    """ScheduleText_GetStateField"""
    if param == 2: return 30 if not cond else 0
    if param == 3:
        if cond: return 0
        return ((1 if p3 == 2 else p3) + 1) * 30
    if param == 4:
        p = 1 if p3 == 2 else p3
        return (p + (2 if not cond else 0)) * 30
    if param == 6: return (p3 + (3 if not cond else 0)) * 30
    return 0


def file_number(i, season_idx, tod):
    """season_idx 0..3 (spring..winter), tod 0 day / 1 evening / 2 night; returns the file number the game computes for generic ids"""
    r = record(i); b = r['base']; f = r['flags']; mode = r['mode']; p = r['param']
    s = season_idx
    if not any(f):
        if mode == -1: return b
        if mode == 5: return b + field(s == 1, p, tod)
        if mode == 6: return b + field(s == 2, p, tod)
        if mode == 7: return b + field(s == 0, p, tod)
        return b
    if not f[tod]:
        if tod == 2 and f[1]:
            if mode == -1: return b + 0x1e
            if mode == 8: return (s * 2 + 1) * 0x1e + b
            if mode == 4: return b + field(s < 2, p, 2)
            if mode == 5: return b + field(s == 1, p, 2)
            if mode == 6: return b + field(s == 2, p, 2)
            if mode == 7: return b + field(s == 0, p, 2)
        return b
    if mode == -1:
        if p != 1 and p < 4: return tod * 0x1e + b
        return b
    if mode == 8:
        if p == 12: return (tod + s * 3) * 0x1e + b
        if p == 8: return (tod + s * 2) * 0x1e + b
        if p == 4: return s * 0x1e + b
        return b
    if mode == 4: return b + field(s < 2, p, tod)
    if mode == 5: return b + field(s == 1, p, tod)
    if mode == 6: return b + field(s == 2, p, tod)
    if mode == 7: return b + field(s == 0, p, tod)
    return b


def entry_list(archive_no):
    b = open(os.path.join(ROOT, 'content/assets/arc/bg_file_no_0%d.bin' % archive_no), 'rb').read()
    c = struct.unpack_from('<I', b, 0)[0]
    return list(struct.unpack_from('<%dI' % c, b, 4))


def list_index(lst, fn):
    """index of the entry for a file number (exact, else the largest entry not above it)"""
    if fn in lst: return lst.index(fn)
    c = [i for i, v in enumerate(lst) if v <= fn]
    return c[-1] if c else None


def month_season_idx(month):
    sea = next((i for i, m in enumerate((2, 5, 8, 11)) if month <= m), 0)   # GameClock season: 0 winter,1 spring,2 summer,3 autumn
    return {0: 3, 1: 0, 2: 1, 3: 2}[sea]
