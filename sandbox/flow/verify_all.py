"""Check every recorded call log against the port and merge the state coverage.
    .venv/bin/python sandbox/flow/verify_all.py LOG_DIR... > report.txt
Logs: calls*.txt files and */calls.txt under the given directories."""
import glob, os, subprocess, sys
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from lpflow.ctx import LogCtx, Mismatch
from lpflow.ports import ports
from lpflow.top import scoped_fade_check

logs = []
for d in sys.argv[1:]:
    logs += sorted(glob.glob(os.path.join(d, 'calls*.txt'))) + sorted(glob.glob(os.path.join(d, '*', 'calls.txt')))
cov = {}
totals = dict(frames=0, calls=0, loads=0, stores=0)
bad = 0
for path in logs:
    cx = LogCtx(path, ports())
    n = 0
    status = 'OK'
    try:
        for _ in cx.frames():
            scoped_fade_check(cx)
            n += 1
    except EOFError:
        pass
    except Mismatch as e:
        status = 'MISMATCH: %s' % e
        bad += 1
    if cx.warnings:
        status += ' (%d warnings)' % len(cx.warnings)
    totals['frames'] += n
    for k in ('calls', 'loads', 'stores'):
        totals[k] += cx.counts[k]
    for k, v in cx.coverage.items():
        cov[k] = cov.get(k, 0) + v
    print('%-60s %7d frames  %s' % (os.path.relpath(path, sys.argv[1]), n, status))
print()
print('%d logs, %d diverged; %d frames, %d calls, %d loads, %d stores checked' %
      (len(logs), bad, totals['frames'], totals['calls'], totals['loads'], totals['stores']))
by = {}
for (m, st), k in sorted(cov.items()):
    by.setdefault(m, []).append('%x' % st if st >= 0 else str(st))
print('states seen (each ran at least one frame):')
for m, sts in sorted(by.items()):
    print('  %-18s %s' % (m, ' '.join(sts)))
