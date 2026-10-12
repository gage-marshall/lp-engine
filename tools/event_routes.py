#!/usr/bin/env python3
"""Dump the 'text route' script-id tables used by SceneRoute_SelectTextRoute (0x209f020).

Usage: event_routes.py ARM9_MAIN_BIN [OUT.json]   (ARM9 image loaded at 0x02000000, e.g. work/arm9_main.bin)

Native logic being reproduced:
    kind = GameFlow state field +4            (1..0x10, see ROUTE_KIND_POOL)
    table = pool[kind]                        (word pool at 0x209f118..0x209f150)
    stage = TextRes_UpdateSlotState(slot, SaveSlot_GetSlotPointerA(slot))   (0..2; 3 when not in slot mode)
    id    = table[slot * 4 + stage]           (u32 script id; slot = current heroine 0..2)
    no table (kinds 0, 0xa, 0x11, 0x12) -> default id at 0x209f150
"""
import json
import struct
import sys

# kind -> address of the literal-pool word holding the table base (kind 0xe reuses kind 4's table)
ROUTE_KIND_POOL = {1: 0x209f118, 2: 0x209f11c, 3: 0x209f120, 4: 0x209f124, 5: 0x209f128, 6: 0x209f12c,
                   7: 0x209f130, 8: 0x209f134, 9: 0x209f138, 0xb: 0x209f13c, 0xc: 0x209f140, 0xd: 0x209f144,
                   0xe: 0x209f124, 0xf: 0x209f148, 0x10: 0x209f14c}
DEFAULT_ID = 0x209f150
BASE = 0x02000000


def stage_for(slot, save_value):
    """TextRes_UpdateSlotState (0x209ef54): relationship-stage column from the heroine's save value."""
    if slot in (1, 2):
        return {0: 2, 1: 1}.get(save_value, 0)
    return {0: 2, 2: 1}.get(save_value, 0)


def dump(arm9):
    def r32(a):
        return struct.unpack_from('<I', arm9, a - BASE)[0]
    out = {'default': r32(DEFAULT_ID), 'kinds': {}}
    for kind, pool in ROUTE_KIND_POOL.items():
        base = r32(pool)
        out['kinds'][kind] = [[r32(base + s * 16 + c * 4) for c in range(4)] for s in range(3)]
    return out


if __name__ == '__main__':
    data = open(sys.argv[1], 'rb').read()
    res = dump(data)
    text = json.dumps({'default': res['default'], 'kinds': {str(k): v for k, v in res['kinds'].items()}}, indent=1)
    if len(sys.argv) > 2:
        open(sys.argv[2], 'w').write(text)
    for k, v in res['kinds'].items():
        print('kind %#x:' % k, ' '.join('%08x' % x for x in v[0]))
