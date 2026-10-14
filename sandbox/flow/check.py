"""Replay a recorded call log (calllog.py) through the Python port and report the first divergence.
    .venv/bin/python sandbox/flow/check.py LOG [--only Name,Name]"""
import os, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpflow.ctx import LogCtx, Mismatch
from lpflow.ports import ports
from lpflow.top import scoped_fade_check

if __name__ == '__main__':
    only = sys.argv[sys.argv.index('--only') + 1].split(',') if '--only' in sys.argv else None
    P = ports(only)
    cx = LogCtx(sys.argv[1], P)
    print('ported:', ', '.join(sorted(v[0] for v in P.values())))
    t = time.time()
    n = 0
    try:
        for fr in cx.frames():
            # the frame's entry: the log holds no call record for it (the hook is the function itself)
            scoped_fade_check(cx)
            n += 1
        print('OK: %d frames' % n)
    except EOFError as e:
        print('OK: %d frames (the log ends inside frame %s: %s)' % (n, cx.frame, e))
    except Mismatch as e:
        print('MISMATCH after %d frames: %s' % (n, e))
        print('next records:', cx.recs[cx.i:cx.i + 8])
    print(cx.counts, '%.1fs' % (time.time() - t))
    by = {}
    for (m, st), k in sorted(cx.coverage.items()):
        by.setdefault(m, []).append('%x' % st if st >= 0 else str(st))
    print('states seen:')
    for m, sts in by.items():
        print('  %-18s %s' % (m, ' '.join(sts)))
    if '--cov' in sys.argv:
        import json
        json.dump({'%s %d' % k: v for k, v in cx.coverage.items()}, open(sys.argv[sys.argv.index('--cov') + 1], 'w'))
    ws = cx.warnings
    print(len(ws), 'warnings')
    from collections import Counter
    for w, k in Counter(w.split(': ', 1)[1] for w in ws).most_common(15):
        print('  %5d  %s' % (k, w))
