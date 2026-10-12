#!/usr/bin/env python3
"""Check MicSession_UpdateTimed / MicSession_Clear natively against tools/pipeline_eval.py.
Clear runs natively with the 36 stage handlers answered by the Python stage port (tests the dispatcher, not the stages)."""
import csv
import json
import os
import random
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
import cond_mock  # noqa: E402
from cond_mock import Mock  # noqa: E402
from cond_oracle import Oracle  # noqa: E402
from pipeline_eval import Pipeline  # noqa: E402
from stage_check import AutoArity  # noqa: E402
from stage_eval import SESS  # noqa: E402

SES = 0x2430000


class PipeMock(Mock):
    """Answers stage-handler calls (addresses in the stage table) with the Python stage port."""

    def __init__(self, seed, handlers, rom):
        super().__init__(seed)
        self.handlers = handlers
        self.rom = rom
        self.addr = None

    def leaf(self, name, args, uc, sp):
        k = self.handlers.get(self.addr)
        if k is None:
            return super().leaf(name, args, uc, sp)
        from stage_eval import Stages
        res_ptr, period, sess = args[0], args[1], args[2]
        rinit = struct.unpack('<I', bytes(uc.mem_read(res_ptr, 4)))[0]
        partner, act = bytes(uc.mem_read(sess + 4, 2))
        ret, res = getattr(Stages(self, self.rom), 's%d' % k)(rinit, period, partner, act)
        uc.mem_write(res_ptr, struct.pack('<I', res & 0xffffffff))
        return ret


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 200
    arm9 = open(os.path.join(ROOT, 'work/arm9_main.bin'), 'rb').read()
    cond_mock.AUTO = AutoArity(arm9, os.path.join(ROOT, 'fid/labels_all.csv'))
    ev = json.load(open(os.path.join(ROOT, 'content/engine/event_selection.json')))
    stages = ev['stages']
    handlers = {}
    for i, s in enumerate(stages):
        handlers[s['handler']] = i
    o = Oracle()
    uc = o.uc
    rng = random.Random(5)
    # ---- UpdateTimed
    bad = tot = 0
    first = None
    for i in range(n):
        seed = rng.randrange(1 << 30)
        period = rng.randrange(0, 7)
        p3, p4 = rng.choice((0, 1)), rng.choice((0, 1))
        uc.mem_write(SES, b'\0' * 0x40)
        mk = Mock(seed)
        ok, tr, err, r0 = o.call(0x20c1dd0, (SES, period, p3, p4), mk, mod=(0x20c1dd0, 0x20c1dd0 + 336))
        nat = (r0 & 0xff, struct.unpack('<I', bytes(uc.mem_read(SES + 0x10, 4)))[0] if r0 & 0xff else None,
               bytes(uc.mem_read(SES + 4, 2)) if r0 & 0xff else None)
        pr = Pipeline(Mock(seed), arm9, stages)
        ret, mask, b4, b5 = pr.update_timed(period, p3, p4)
        py = (ret & 0xff, mask, bytes([b4, b5]) if ret else None)
        tot += 1
        if nat != py:
            bad += 1
            first = first or (seed, period, p3, p4, nat, py, err)
    print('UpdateTimed', tot, 'cases,', bad, 'mismatches', first)
    # ---- Clear
    bad = tot = 0
    first = None
    paths = set()
    for i in range(n * 5):
        seed = rng.randrange(1 << 30)
        period = rng.randrange(0, 6)
        partner, act = rng.choice((0, 1, 2, 3, 4)), rng.choice(list(range(0, 24)) + [0x30, 0x65, 0x66, 0x6b, 0x6c, 0x6e, 0x71, 0x72, 0xff])
        mask = 0
        for sh in rng.sample(range(0, 18), rng.randrange(2, 9)):
            mask |= 1 << sh
        mask |= 1 << period
        idx0 = rng.choice((0, 0, 0, 10, 20))
        uc.mem_write(SES, b'\0' * 0x40)
        uc.mem_write(SES + 4, bytes([partner, act]))
        uc.mem_write(SES + 0x10, struct.pack('<I', mask))
        uc.mem_write(SES + 0x14, struct.pack('<H', idx0))
        uc.mem_write(SES + 0x16, bytes([period]))
        pm = PipeMock(seed, handlers, arm9)
        # leaf hook needs the callee address: patch the oracle call to expose it
        o.pipe = pm
        ok, tr, err, r0 = o.call(0x20c1f24, (SES, 0, 0, 0), pm, mod=(0x20c1f24, 0x20c1f24 + 156))
        paths.add(tuple(t[0] for t in tr))
        nat = (r0 & 0xff, struct.unpack('<I', bytes(uc.mem_read(SES + 8, 4)))[0], struct.unpack('<H', bytes(uc.mem_read(SES + 0xc, 2)))[0],
               struct.unpack('<H', bytes(uc.mem_read(SES + 0x14, 2)))[0])
        pr = Pipeline(Mock(seed), arm9, stages)
        ret, res, kind, idx = pr.clear(mask, idx0, period, partner, act)
        py = (ret & 0xff, res & 0xffffffff, kind, idx)
        tot += 1
        if nat != py:
            bad += 1
            first = first or (seed, period, partner, act, hex(mask), idx0, nat, py, err)
    print('Clear', tot, 'cases,', bad, 'mismatches, distinct stage-call sequences', len(paths), first)


if __name__ == '__main__':
    main()
