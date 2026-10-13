"""Extract the backdrop-selection tables of the game (L0): record table of ScheduleText_UpdateState (20 bytes per bg id at the pointer in
the literal pool 0x209e064), time-of-day thresholds (GameClock_ClearTimeState, 0x2101d4e), the period -> time-of-day mapping, and the special-id
file numbers of ScheduleText_RefreshDateState.  Consumed by engine/src/backdrop.rs; see tools/bg_model.py for the reference model."""
import struct

B = 0x2000000


def extract(arm9):
    u32 = lambda a: struct.unpack_from('<I', arm9, a - B)[0]
    table = u32(0x209e064) - B
    recs = []
    for i in range(0, 0xd8):
        r = arm9[table + i * 20:table + i * 20 + 20]
        w = struct.unpack_from('<5i', r)
        recs.append({'id': w[0], 'base': w[1], 'flags': list(r[8:11]), 'mode': w[3], 'param': r[16]})
    tod = u32(0x205eca4) - B
    thresholds = [list(arm9[tod + s * 3:tod + s * 3 + 3]) for s in range(4)]
    special = {'%x' % a: u32(a) for a in range(0x209e50c, 0x209e574, 4)}
    return {
        'note': 'bg id -> {base file number, flags[day,evening,night] has variants, mode, param}; file number -> entry of arc/bg_file_no_0N.bin (N = id range archive); '
                'time_state[season 0 winter..3 autumn] = hour thresholds (ClearTimeState); period_tod = TextRes_SelectTimePeriodText per time period; special = ScheduleText_RefreshDateState constants',
        'records': recs,
        'time_state': thresholds,
        'period_tod': [1, 1, 1, 2, 0, 0],
        'special': special,
        'archives': ['bg_center', 'bg_town', 'bg_suburb', 'bg_sea_side', 'bg_mountain', 'bg_school', 'bg_trip_atami', 'bg_room'],
    }
