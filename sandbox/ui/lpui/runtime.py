"""UI runtime: the screen graph (lpui.screens) driven by taps.

The current screen state renders from its recipe (file loads) plus its dynamic content - from screen logic where a logic
module exists (lpui.logic.<name>), else from the capture the state was extracted from. A tap is hit-tested with the game's
rule against the screen's touch areas; the recorded transition for (screen, hit) gives the next screen.
    .venv/bin/python -m lpui.runtime GRAPH.json TRACE_DIR SESSION.jsonl OUT_DIR [SPEC_DIR]
(replays the session's taps, no emulator; with SPEC_DIR each screen renders from its spec: files + widgets)"""
import json, os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))
from lpui import screens  # noqa: E402
from lpui.touch import TouchCtl  # noqa: E402


class Runtime:
    def __init__(self, graph, trace_dir, spec_dir=None):
        self.g = graph
        self.trace_dir = trace_dir
        self.spec_dir = spec_dir          # specs (lpui.spec) per capture: render from files + widgets, no captured memory
        self.by_screen = {}
        for s in graph['states']:
            self.by_screen.setdefault(s['screen'], []).append(s)
        self.edges = {(e['src'], e['hit'] if e['hit'] == 'auto' else tuple(e['hit']) if e['hit'] else None): e['dst']
                      for e in graph['edges']}
        self.ops = {path: ops for f, path, ops, act in screens.walk(trace_dir)}
        self.state = graph['states'][0]

    def goto(self, screen):
        self.state = self.by_screen[screen][0]

    def touch(self):
        return TouchCtl(self.state['touch'])

    def tap(self, x, y):
        """-> (hit, next screen or None)"""
        h = self.touch().hit(x, y)
        dst = self.edges.get((self.state['screen'], tuple(h) if h else None))
        if dst:
            self.goto(dst[0])
        return h, dst[0] if dst else None

    def advance(self):
        """follow the screen's automatic transition (the game moving on by itself); False when there is none"""
        dst = self.edges.get((self.state['screen'], 'auto'))
        if dst:
            self.goto(dst[0])
        return bool(dst)

    def spec(self):
        import json
        return json.load(open(os.path.join(self.spec_dir, 'spec-f%d.json' % self.state['frame'])))

    def vram(self):
        if self.spec_dir:
            from lpui import spec as S
            return S.vram(self.spec())
        v, dyn, sig = screens.build_state(self.state['path'], self.ops[self.state['path']])
        return v

    def render(self):
        (t, tu), (b, bu) = self.vram().render()
        return t, b


def demo(graph_path, trace_dir, session, out, spec_dir=None):
    g = json.load(open(graph_path))
    rt = Runtime(g, trace_dir, spec_dir)
    os.makedirs(out, exist_ok=True)
    from PIL import Image
    path = [rt.state['screen']]
    for f, x, y in screens.taps(session):
        # before the tap: let the game move on by itself until a screen takes this tap (bounded)
        for _ in range(20):
            h = rt.touch().hit(x, y)
            if rt.edges.get((rt.state['screen'], tuple(h) if h else None)):
                break
            if not rt.advance():
                break
            path.append(rt.state['screen'])
        h, to = rt.tap(x, y)
        if to:
            path.append(to)
    ok, seen, worst = 0, [], []
    for sid in path:
        if sid in seen:
            continue
        seen.append(sid)
        rt.goto(sid)
        (t, tu), (b, bu) = rt.vram().render()
        img = (np.concatenate([t, b]) * 255 // 31).astype(np.uint8)
        Image.fromarray(img).save(os.path.join(out, '%03d-%s.png' % (len(seen), sid)))
        import ds2d
        (t2, tu2), (b2, bu2) = ds2d.both(ds2d.Capture(rt.state['path']))     # the capture's own composite (exact colours)
        diff = ((np.abs(np.concatenate([t, b]) - np.concatenate([t2, b2])).max(-1) > 0) & ~np.concatenate([tu, bu]) & ~np.concatenate([tu2, bu2])).sum()
        ok += diff == 0
        worst.append((int(diff), sid, rt.state['frame']))
    print('runtime walked %d steps over %d distinct screens by the session taps; %d render identical to their capture'
          % (len(path), len(seen), ok))
    print('largest differences:', sorted(worst, reverse=True)[:6])
    return path


if __name__ == '__main__':
    demo(*sys.argv[1:6])
