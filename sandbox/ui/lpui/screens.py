"""Screens and the transition graph, extracted from a replay trace (sandbox/ui/replay_trace.py) and its session's inputs.

A screen state = the file loads still in VRAM (under the capture's bank mapping), the dynamic bytes the files do not give
(generated: text, built maps - kept from the capture until logic replaces them), display registers, OAM, scroll shadow and
the touch areas of the controls the game polled in the frames before the capture. Screens are identified by their static
recipe + registers + touch areas; transitions come from the session's taps: (screen, hit) -> next distinct screen.
    .venv/bin/python -m lpui.screens TRACE_DIR SESSION.jsonl OUT.json   (from sandbox/ui)"""
import hashlib, json, os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
import ds2d  # noqa: E402
from lpui.vram import Vram, SIZES  # noqa: E402
from lpui.touch import TouchCtl  # noqa: E402

POLL_WINDOW = 8          # frames: a control polled this recently belongs to the screen on show


def walk(trace_dir):
    """yield per capture: (frame, path, effective load ops, touch lists of the recently polled controls)"""
    ops, lists, polls = [], {}, {}
    for line in open(os.path.join(trace_dir, 'trace.jsonl')):
        r = json.loads(line)
        ev = r['ev']
        if ev == 'load':
            ops.append(r)
        elif ev == 'hits':
            lists[r['ctl']] = r['lists']
        elif ev == 'poll':
            polls[r['ctl']] = r['f']
        elif ev == 'capture':
            f = r['f']
            active = [lists[c] for c, pf in sorted(polls.items(), key=lambda x: -x[1]) if f - pf <= POLL_WINDOW and c in lists]
            yield f, r['path'], ops, active


def build_state(path, ops):
    """(Vram rebuilt from files + capture fallback, from-file mask per memory, static signature)"""
    cap = ds2d.Capture(path)
    vc = bytes(cap.regs['vramcnt']).hex()
    actual = {k: cap.vram[k] for k in ('bg_main', 'bg_sub', 'obj_main', 'obj_sub')}
    actual['pal'] = cap.pal.tobytes()
    v = Vram(regs=cap.regs, oam=cap.oam, dtcm=cap.dtcm, scroll_regs=cap.scroll_regs)
    sig = []
    for op in ops:
        if op.get('vc') not in (None, vc):
            continue
        if v.apply(op):
            o = op.get('origin') or {}
            sig.append((op['mem'], op['at'], op['size'], o.get('arc'), o.get('entry'), o.get('off')))
    # dynamic = whatever still differs from the capture (generated content, not yet derived)
    dyn = {}
    for k in SIZES:
        act = np.frombuffer(actual[k], np.uint8)
        d = v.mem[k] != act
        dyn[k] = int(d.sum())
        v.mem[k][d] = act[d]
    return v, dyn, sig


def signature(cap_regs, sig, touch):
    h = hashlib.sha1()
    keep = {e: {o: cap_regs[e][o] for o in ('000', '008', '00c')} for e in ('main', 'sub')}
    h.update(json.dumps(keep, sort_keys=True).encode())
    h.update(json.dumps(sorted(set(str(tuple(x)) for x in sig))).encode())
    h.update(json.dumps(touch.areas(False)).encode())
    return h.hexdigest()[:12]


def taps(session):
    """[(frame, x, y)] touch presses of a play_monitor session"""
    out, down = [], False
    for line in open(session):
        r = json.loads(line)
        if r['ev'] != 'in':
            continue
        t = r['touch']
        if t and not down:
            out.append((r['f'], t[0], t[1]))
        down = bool(t)
    return out


def extract(trace_dir, session):
    states = []
    for f, path, ops, active in walk(trace_dir):
        v, dyn, sig = build_state(path, ops)
        touch = TouchCtl(active[0] if active else [])
        sid = signature(v.regs, sig, touch)
        states.append(dict(frame=f, path=path, screen=sid, dynamic=dyn, touch=touch.lists, n_static=len(sig)))
    edges = {}
    tp = taps(session)
    for i, s in enumerate(states):
        nxt = states[i + 1] if i + 1 < len(states) else None
        for f, x, y in tp:
            if s['frame'] <= f < (nxt['frame'] if nxt else 1 << 30):
                h = TouchCtl(s['touch']).hit(x, y)
                # the next state whose screen differs is where this tap led
                to = next((t['screen'] for t in states[i + 1:] if t['screen'] != s['screen']), None)
                key = '%s|%s' % (s['screen'], h)
                edges.setdefault(key, dict(src=s['screen'], hit=h, dst=set(), taps=[]))
                if to:
                    edges[key]['dst'].add(to)
                edges[key]['taps'].append([f, x, y])
    # screen changes with no tap in between: the game moved on by itself (time, script, fade)
    tf = [f for f, x, y in tp]
    for i in range(len(states) - 1):
        a, b = states[i], states[i + 1]
        if a['screen'] != b['screen'] and not any(a['frame'] <= f < b['frame'] for f in tf):
            key = '%s|auto' % a['screen']
            edges.setdefault(key, dict(src=a['screen'], hit='auto', dst=set(), taps=[], dwell=[]))
            edges[key]['dst'].add(b['screen']); edges[key]['dwell'].append(b['frame'] - a['frame'])
    for e in edges.values():
        e['dst'] = sorted(e['dst'])
    return states, list(edges.values())


if __name__ == '__main__':
    states, edges = extract(sys.argv[1], sys.argv[2])
    screens = {}
    for s in states:
        screens.setdefault(s['screen'], []).append(s['frame'])
    json.dump(dict(states=states, edges=edges, screens=screens), open(sys.argv[3], 'w'), indent=0)
    dyn = sum(sum(s['dynamic'].values()) for s in states)
    print('%d states, %d distinct screens, %d tap edges (%d with a hit); dynamic bytes per state %.0f' % (
        len(states), len(screens), len(edges), sum(1 for e in edges if e['hit']), dyn / max(1, len(states))))
