#!/usr/bin/env python3
"""Compare the real game's scripted presentation commands (play_monitor log) with the Rust host's effects, per script.

    python3 tools/scene_diff.py LOG.jsonl [--only ID] [--choose N,N]

Real side: cmd 0xf motion(idx,blend,hold), 0x6/0xb camera preset, 0x34 face mood, 0x5c voice clip, 0xe/0xd fades, taken between a script_start and its script_end.
Host side: engine host_run effects mapped to the same tuples.  Prints the first divergence per script."""
import json, re, subprocess, sys, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def clock_of(e):
    """(month, hour) the game clock showed when the script started"""
    d = e.get('date')
    try: return (int(d.split('-')[1]), e.get('hour'))
    except Exception: return (None, None)


def clock_args(clock):
    m, h = clock
    return (['--month', str(m)] if m else []) + (['--hour', str(h)] if h is not None else [])


def real_scenes(path):
    cur, out = None, []
    for l in open(path):
        e = json.loads(l)
        if e['ev'] == 'script_start':
            cur = {'id': '%08x' % (int(e['id'], 16) & 0xffffffff), 'seq': [], 'sess': e.get('sess'), 'clock': clock_of(e)}
            out.append(cur)
        elif e['ev'] == 'script_end':
            cur = None
        elif e['ev'] == 'cmd' and cur is not None:
            c, a = int(e['cmd'], 16), e['args']
            if c == 0xf and a: cur['seq'].append(('motion', a[0]))
            elif c in (0x6, 0xb) and a: cur['seq'].append(('camera', a[0]))
            elif c == 0x34 and a: cur['seq'].append(('face', a[0]))
            elif c == 0x5c and a: cur['seq'].append(('voice', a[0]))
            elif c == 0xe: cur['seq'].append(('fade', 'in'))
            elif c == 0xd: cur['seq'].append(('fade', 'out'))
    return out


def host_seq(content, sid, choose, clock=(None, None)):
    cmd = [os.path.join(ROOT, 'engine/target/release/host_run'), content, sid] + clock_args(clock)
    if choose: cmd += ['--choose', choose]
    txt = subprocess.run(cmd, capture_output=True, text=True).stdout
    seq = []
    for ln in txt.splitlines():
        m = re.match(r'\s*[\d.]+\s+(.*)', ln)
        if not m: continue
        s = m.group(1)
        if (x := re.match(r'motion (\d+)', s)): seq.append(('motion', int(x[1])))
        elif (x := re.match(r'camera preset (\d+)', s)): seq.append(('camera', int(x[1])))
        elif (x := re.match(r'face mood (\d+)', s)): seq.append(('face', int(x[1])))
        elif (x := re.match(r'voice clip (-?\d+)', s)): seq.append(('voice', int(x[1])))
        elif s.startswith('fade in'): seq.append(('fade', 'in'))
        elif s.startswith('fade out'): seq.append(('fade', 'out'))
    return seq


def dedupe(seq):
    out = []
    for x in seq:
        if not out or out[-1] != x: out.append(x)
    return out


def real_cmds(path):
    cur, out = None, []
    for l in open(path):
        e = json.loads(l)
        if e['ev'] == 'script_start':
            cur = {'id': '%08x' % (int(e['id'], 16) & 0xffffffff), 'seq': [], 'clock': clock_of(e)}
            out.append(cur)
        elif e['ev'] == 'script_end':
            cur = None
        elif e['ev'] == 'cmd' and cur is not None:
            cur['seq'].append((int(e['cmd'], 16), tuple(a & 0xffffffff for a in e['args'])))
    return out


def host_cmds(content, sid, clock=(None, None)):
    cmd = [os.path.join(ROOT, 'engine/target/release/host_run'), content, sid, '--cmds'] + clock_args(clock)
    out = []
    for ln in subprocess.run(cmd, capture_output=True, text=True).stdout.splitlines():
        m = re.match(r'\s*[\d.]+\s+cmd ([0-9a-f]+)\s*(.*)', ln)
        if m: out.append((int(m[1], 16), tuple(int(x) for x in m[2].split())))
    return out


def hooked_ids():
    """command ids the monitor can see (fid/script_commands.csv); anything else executes without a native handler"""
    import csv
    return {int(r['cmd'], 16) for r in csv.DictReader(open(os.path.join(ROOT, 'fid', 'script_commands.csv')))}


def full(path, only):
    import difflib
    seen = hooked_ids()
    for sc in real_cmds(path):
        if only and sc['id'] != only: continue
        r, h = dedupe(sc['seq']), dedupe([x for x in host_cmds(os.path.join(ROOT, 'content'), sc['id'], sc['clock']) if x[0] in seen])
        sm = difflib.SequenceMatcher(None, r, h, autojunk=False)
        miss = [x for tag, i1, i2, j1, j2 in sm.get_opcodes() if tag in ('delete', 'replace') for x in r[i1:i2]]
        extra = [x for tag, i1, i2, j1, j2 in sm.get_opcodes() if tag in ('insert', 'replace') for x in h[j1:j2]]
        print('%s  real %d host %d  ratio %.3f  only-real=%s  only-host=%s' % (sc['id'], len(r), len(h), sm.ratio(), sorted({'%x' % c for c, _ in miss}), sorted({'%x' % c for c, _ in extra})))
        if only:
            for tag, i1, i2, j1, j2 in sm.get_opcodes():
                if tag != 'equal': print(' ', tag, [(hex(c), a) for c, a in r[i1:i2]][:6], '|', [(hex(c), a) for c, a in h[j1:j2]][:6])


def main():
    if '--full' in sys.argv:
        full(sys.argv[1], sys.argv[sys.argv.index('--only') + 1] if '--only' in sys.argv else None)
        return
    a = sys.argv[1:]
    path = a[0]
    only = a[a.index('--only') + 1] if '--only' in a else None
    choose = a[a.index('--choose') + 1] if '--choose' in a else None
    content = os.path.join(ROOT, 'content')
    for sc in real_scenes(path):
        if only and sc['id'] != only: continue
        r, h = dedupe(sc['seq']), dedupe(host_seq(content, sc['id'], choose, sc['clock']))
        n = next((i for i in range(min(len(r), len(h))) if r[i] != h[i]), None)
        if n is None and len(r) == len(h):
            print('%s  MATCH  (%d commands)' % (sc['id'], len(r)))
        else:
            n = min(len(r), len(h)) if n is None else n
            print('%s  DIVERGE at #%d  real=%s  host=%s   (real %d, host %d)' % (sc['id'], n, r[n] if n < len(r) else None, h[n] if n < len(h) else None, len(r), len(h)))


if __name__ == '__main__':
    main()
