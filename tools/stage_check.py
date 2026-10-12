#!/usr/bin/env python3
"""Differential test of the stage handlers: native ARM (Unicorn, only the handler's own code) vs tools/stage_eval.py.
Usage: stage_check.py [seeds=8] [stage ...]"""
import csv
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import cond_mock  # noqa: E402
from arity import Arity  # noqa: E402
from cond_mock import Mock  # noqa: E402
from cond_oracle import Oracle  # noqa: E402
from stage_eval import Stages  # noqa: E402


class AutoArity:
    def __init__(self, arm9, labels):
        self.ar = Arity(arm9)
        self.addr = {r['name']: int(r['addr'], 16) for r in csv.DictReader(open(labels)) if r['module'] == 'static'}

    def get(self, name, default=0):
        a = self.addr.get(name)
        if a is None and name.startswith('sub_'):
            a = int(name[4:], 16)
        return self.ar.of(a) if a is not None else default


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 8
    only = set(int(x) for x in sys.argv[2:])
    arm9 = open(os.path.join(ROOT, 'work/arm9_main.bin'), 'rb').read()
    cond_mock.AUTO = AutoArity(arm9, os.path.join(ROOT, 'fid/labels_all.csv'))
    cond_mock.RANGES.update({'EventLogic_ClearData_cdf80': (-1, 3), 'GameState_GetCurrentSlot': (0, 2)})
    ev = json.load(open(os.path.join(ROOT, 'content/engine/event_selection.json')))
    stages = ev['stages']
    sizes = json.load(open(os.path.join(ROOT, 'fid/stage_sizes.json')))
    o = Oracle()
    out = {}
    for k, st in enumerate(stages):
        if only and k not in only:
            continue
        py = getattr(Stages(Mock(0), arm9), 's%d' % k, None)
        if py is None:
            out[k] = 'missing'
            continue
        bad = tot = 0
        first = None
        paths = set()
        for seed in range(seeds):
            for period in range(0, 6):
                for partner in (0, 1, 2, 3, 4, 0xff):
                    for act in sorted(set(list(range(0, 24)) + [0x1d, 0x1e, 0x30, 0x5d, 0x5e, 0x65, 0x66, 0x6b, 0x6c, 0x6d, 0x6e, 0x71, 0x72, 0xff])):
                        for rinit in (0, 0x1234):
                            h, sz, r3 = (0x20f1750, 456, k == 32) if k in (9, 32) else (st['handler'], sizes[str(k)], 0)
                            ret, res, err, tr = o.stage(h, sz, rinit, period, partner, act, Mock(seed), int(r3))
                            if err == 'timeout':
                                continue
                            pr = Stages(Mock(seed), arm9)
                            pret, pres = getattr(pr, 's%d' % k)(rinit, period, partner, act)
                            tot += 1
                            paths.add(tuple(t[0] for t in tr))
                            if (ret, res & 0xffffffff) != (pret, pres & 0xffffffff):
                                bad += 1
                                if first is None:
                                    first = (seed, period, partner, act, rinit, (ret, res), (pret, pres), [t[0] for t in tr][:6], err)
        out[k] = (bad, tot, len(paths), first)
    for k, v in out.items():
        print(k, v)


if __name__ == '__main__':
    main()
