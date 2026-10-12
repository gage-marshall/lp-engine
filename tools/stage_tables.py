#!/usr/bin/env python3
"""Dump the ROM tables the stage handlers index (shapes taken from tools/stage_eval.py) into JSON for the port.
Usage: stage_tables.py ARM9_MAIN_BIN OUT.json   (ROM-derived: write under content/, never commit)"""
import json
import struct
import sys

BASE = 0x02000000
# pool address of the table pointer -> (name, shape [dims...] of u32)
TABLES = {
    0x20f6f9c: ('stage19_slot_stage', [3, 3]), 0x20f7098: ('stage21_slot_stage', [3, 3]), 0x20f6ee8: ('stage10_slot_stage', [3, 3]),
    0x20f6e4c: ('stage20_slot_stage', [3, 3]), 0x20b3b00: ('stage25_slot_stage', [3, 3]), 0x20b34f8: ('stage31_slot_stage', [3, 4]),
    0x20aec84: ('stage35_slot', [3]), 0x20e55a0: ('stage28_slot_stage', [3, 4]), 0x20adb58: ('stage27_lowfive_slot_stage', [3, 3]),
    0x20adb5c: ('stage27_main_default', [3]), 0x20adb60: ('stage27_other_status2', [3]), 0x20adb64: ('stage27_other_status0', [3]),
    0x20adb68: ('stage27_final', [3, 2, 4]), 0x20add9c: ('stage34_lowfive_slot_stage', [3, 3]), 0x20adda0: ('stage34_final', [3, 2, 4]),
    0x20ad23c: ('stage26_main_default', [3]), 0x20ad240: ('stage26_other_default', [3]), 0x20f8830: ('stage3_q', [3, 3, 2]),
    0x20f8834: ('stage3_f_a', [3, 3]), 0x20f8838: ('stage3_f_b', [3, 3]), 0x20d7e24: ('stage16_by_choice', [32, 2]),
    0x20ae48c: ('stage22_const', []), 0x20f8a6c: ('stage4_final', [3, 3, 2]), 0x20aec80: ('stage35_flag', []), 0x20f1a78: ('stage8_flag', []),
    0x20f6f98: ('stage19_flag', []), 0x20af8b4: ('stage24_flag', []), 0x20adb54: ('stage27_flag', []), 0x20ad238: ('stage26_flag', []),
    0x20ad4d4: ('stage33_flag', []), 0x20add98: ('stage34_flag', []),
    # id-producing leaves
    0x20b4130: ('variant_0x25_by_char', [3]), 0x20b4134: ('variant_0x28_by_char', [3]), 0x20b4138: ('variant_0x2a', []),
    0x20b413c: ('variant_0x2b', []), 0x20b4140: ('variant_6_a', []), 0x20b4144: ('variant_6_b', []), 0x20b4148: ('variant_by_code', [48]),
    0x20b45e0: ('pending_0x23', []), 0x20b45e4: ('pending_0x24', []), 0x20b4534: ('today_slot_flag1', [3]), 0x20b4538: ('today_slot_flag0', [3]),
    0x20b87ac: ('uiinput_check_action_id', []), 0x20b873c: ('uiinput_act_codes', [6]), 0x20b8740: ('uiinput_order', [6]),
    0x20b8744: ('uiinput_ids', [3]), 0x20b8748: ('uiinput_codes2', [3]), 0x20d7218: ('statelogic_omicron_lists', [3]),
    0x20d7408: ('statelogic_phi_flags', [3]),
}


def main():
    d = open(sys.argv[1], 'rb').read()

    def r32(a):
        o = a - BASE
        return struct.unpack_from('<I', d, o)[0] if 0 <= o <= len(d) - 4 else 0
    out = {}
    for pool, (name, shape) in TABLES.items():
        if not shape:
            out[name] = r32(pool)                              # the pool word is the constant itself
            continue
        base = r32(pool)
        n = 1
        for s in shape:
            n *= s
        flat = [r32(base + 4 * i) for i in range(n)]
        def build(f, dims):
            if len(dims) == 1:
                return f
            step = len(f) // dims[0]
            return [build(f[i * step:(i + 1) * step], dims[1:]) for i in range(dims[0])]
        out[name] = {'address': base, 'shape': shape, 'data': build(flat, shape)}
    # per-heroine TERM file lists used by the queue stages (stride 8: u8 count, u32 ptr to u16 list)
    lists = {}
    for nm, b in (('map_queue_large', 0x21071d0), ('map_queue_small', 0x2107338), ('text_event', 0x210738c), ('map_event', 0x2107594),
                  ('map_arc_queue', 0x2107678), ('map_selector', 0x2107b00), ('stage18_action_queue', 0x210b308),
                  ('schedule_update_state', 0x210ec18), ('schedule_process_state', 0x210ec30), ('schedule_filter_date', 0x21122ac)):
        lists[nm] = []
        for h in range(3):
            cnt = r32(b + h * 8) & 0xff
            ptr = r32(b + h * 8 + 4)
            lists[nm].append([struct.unpack_from('<H', d, ptr + 2 * i - BASE)[0] for i in range(cnt)])
    out['term_file_lists'] = lists
    json.dump(out, open(sys.argv[2], 'w'))
    print(len(out), 'tables')


if __name__ == '__main__':
    main()
