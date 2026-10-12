#!/usr/bin/env python3
"""Differential test of whole TERM files: native CondList_CollectPassingRecords (Unicorn) vs the Python evaluator.
Usage: cond_check_files.py [seeds=3] [threshold values...]"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
from cond_eval import Ev  # noqa: E402
from cond_mock import Mock  # noqa: E402
from cond_oracle import Oracle  # noqa: E402
from term import parse  # noqa: E402


def main():
    seeds = int(sys.argv[1]) if len(sys.argv) > 1 else 3
    thrs = [int(x) for x in sys.argv[2:]] or [0xffffff81 - (1 << 32), 0, 50]
    o = Oracle()
    rom = open(os.path.join(ROOT, 'work/arm9_main.bin'), 'rb').read()
    tdir = os.path.join(ROOT, 'work/assets/arc/tmx_tbin.arc')
    handlers = o.handlers
    tot = bad = 0
    firstbad = None
    for f in sorted(os.listdir(tdir)):
        data = open(os.path.join(tdir, f), 'rb').read()
        recs = parse(data)[0]
        if not recs:
            continue
        for seed in range(seeds):
            for thr in thrs:
                nat, nfin, err = o.collect(data, thr, Mock(seed))
                py, pfin = Ev(Mock(seed), rom).collect(recs, thr, handlers)
                tot += 1
                same = [tuple(x) for x in nat] == [tuple(x) for x in py] and (nfin & 0xffffffff) == (pfin & 0xffffffff)
                if not same:
                    bad += 1
                    if firstbad is None:
                        firstbad = (f, seed, thr, nat[:4], py[:4], nfin, pfin, err)
    print('file evaluations', tot, 'mismatches', bad, firstbad)


if __name__ == '__main__':
    main()
