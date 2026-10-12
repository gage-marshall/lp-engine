#!/usr/bin/env python3
"""Probe the two 'work-state ladder' stages natively: stage 7 (0x20b4814: side effect StateLogic_GetDataMu_cbdfc(0|5) for matching
work-state keys, result = key) and stage 11 (0x20b4ad0: result = 0 for excluded keys else the key).
Candidate keys are the pool constants +- multiples of 0x80 (every comparison in the ladders is against such values).
Usage: stage_ladders.py OUT.json"""
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
from cond_mock import Mock  # noqa: E402
from cond_oracle import Oracle  # noqa: E402


def main():
    arm9 = open(os.path.join(ROOT, 'work/arm9_main.bin'), 'rb').read()
    r32 = lambda a: struct.unpack_from('<I', arm9, a - 0x2000000)[0]
    ev = json.load(open(os.path.join(ROOT, 'content/engine/event_selection.json')))
    sizes = json.load(open(os.path.join(ROOT, 'fid/stage_sizes.json')))
    o = Oracle()
    consts7 = [r32(a) for a in range(0x20b4a48, 0x20b4a60, 4)]
    consts11 = [r32(a) for a in range(0x20b4bdc, 0x20b4bec, 4)]
    cand = set()
    for K in consts7 + consts11:
        for m in range(-700, 701):
            for base in (0, 0xb0000000, -0xc700, 0xc700):
                cand.add((K + base + m * 0x80) & 0xffffffff)
    set5, set0, excl = [], [], []
    for v in sorted(cand):
        fx = {'MapEvent_CheckWorkState': v, 'GameState_GetField0x78': 0}
        ret, res, err, tr = o.stage(ev['stages'][7]['handler'], sizes['7'], 0, 1, 0, 0x66, Mock(0, fx))
        for t in tr:
            if t[0] == 'StateLogic_GetDataMu_cbdfc':
                (set5 if t[1] == 5 else set0).append(v)
        ret, res, err, tr = o.stage(ev['stages'][11]['handler'], sizes['11'], 0x77, 1, 0, 0x6c, Mock(0, fx))
        if res == 0:
            excl.append(v)
    out = {'stage7_pool': consts7, 'stage7_call5': set5, 'stage7_call0': set0, 'stage11_pool': consts11, 'stage11_excluded': excl}
    json.dump(out, open(sys.argv[1], 'w'))
    print({k: (len(v) if isinstance(v, list) else v) for k, v in out.items()})


if __name__ == '__main__':
    main()
