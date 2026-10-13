#!/usr/bin/env python3
"""Probe Calendar_IsEventActive (0x20a90c0) natively over every (code, year, month, day).
The whole calendar module (0x20a8700-0x20ac000: Holiday_*, Calendar_*) runs natively; calls leaving it are mocked.
Usage: calendar_probe.py OUT.json  -> {code: {year: [[month, day]...]}}  (year = game year 0..99)"""
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
from cond_mock import Mock  # noqa: E402
from cond_oracle import Oracle  # noqa: E402

LO, HI = 0x20a8700, 0x20ac000


def main():
    o = Oracle()
    fixed = {'GameState_GetField0x78': 0, 'GameFlags_GetField0xe0': 0}
    codes = range(int(sys.argv[2]) if len(sys.argv) > 2 else 0, int(sys.argv[3]) if len(sys.argv) > 3 else 175)
    out = {}
    leaves = {}
    for code in codes:
        days = {}
        for y in range(0, 28):
            act = []
            for m in range(1, 13):
                for d in range(1, 32):
                    ok, tr, err, r0 = o.call(0x20a90c0, (code, y, m, d), Mock(0, fixed), mod=(LO, HI))
                    for t in tr:
                        leaves[t[0]] = leaves.get(t[0], 0) + 1
                    if ok:
                        act.append((m, d))
            days[y] = act
        out[code] = days
        print(code, sum(len(v) for v in days.values()), flush=True)
    json.dump({'codes': out, 'leaves': leaves}, open(sys.argv[1], 'w'))


if __name__ == '__main__':
    main()
