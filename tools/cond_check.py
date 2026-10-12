#!/usr/bin/env python3
"""Differential test: Python evaluator (tools/cond_eval.py) vs the real ARM handlers (tools/cond_oracle.py), over every
(op, a, b) that occurs in tmx_tbin.arc and N mocked states.  Usage: cond_check.py [N=20] [op ...]"""
import collections
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
from cond_eval import Ev  # noqa: E402
from cond_mock import Mock  # noqa: E402
from cond_oracle import Oracle  # noqa: E402
from term import parse  # noqa: E402


def combos():
    c = collections.defaultdict(set)
    tdir = os.path.join(ROOT, 'work/assets/arc/tmx_tbin.arc')
    for f in sorted(os.listdir(tdir)):
        for r in parse(open(os.path.join(tdir, f), 'rb').read())[0]:
            for g in r['groups']:
                for op, a, b in g:
                    c[op].add((a, b))
    return c


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 20
    only = set(int(x) for x in sys.argv[2:])
    o = Oracle()
    rom = open(os.path.join(ROOT, 'work/arm9_main.bin'), 'rb').read()
    cs = combos()
    res = {}
    for op in sorted(cs):
        if only and op not in only:
            continue
        mism = 0
        total = 0
        missing = False
        first = None
        for (a, b) in sorted(cs[op]):
            for seed in range(n):
                nat, tr, err, r0 = o.run(op, a, b, Mock(seed))
                try:
                    py = Ev(Mock(seed), rom).evaluate(op, a, b)
                except NotImplementedError:
                    missing = True
                    break
                total += 1
                if py != nat:
                    mism += 1
                    if first is None:
                        first = (a, b, seed, py, nat, [t[0] for t in tr][:6], err)
            if missing:
                break
        res[op] = ('missing' if missing else (mism, total, first))
    bad = {k: v for k, v in res.items() if v == 'missing' or v[0]}
    print('ops checked', len(res), '; clean', sum(1 for v in res.values() if v != 'missing' and not v[0]), '; missing', sum(1 for v in res.values() if v == 'missing'), '; mismatching', sum(1 for v in res.values() if v != 'missing' and v[0]))
    for k, v in sorted(bad.items()):
        print(k, v)


if __name__ == '__main__':
    main()
