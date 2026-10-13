#!/usr/bin/env python3
"""Side-by-side of a recorded game frame and the Rust scene player at the same moment, driven from the recorded log alone.

    python3 tools/side_by_side.py SHOT.png [--out OUT.png] [--log LOG.jsonl] [--player NAME] [--swap|--noswap]

SHOT = a screenshot written by play_monitor (t-<script>-f<frame>.png, start-<script>-f<frame>.png or motion-<id>-f<frame>.png).
Steps: find the monitor log and script run the frame belongs to; take the script's id, game date and hour from the log; find the menu choices that make the
host reproduce the recorded command stream; align the frame with the player's timeline through the last recorded voice / motion command before it
(same clip / motion index in both), offset by the real elapsed time; render that scene time at the DS portrait size; write real | ours.
Screenshots taken before the emu_driver colour fix are red/blue swapped; they are corrected automatically (by file time) unless --swap/--noswap is given."""
import argparse, glob, json, os, re, subprocess, sys
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import scene_diff as sd

LOGS = os.path.join(ROOT, 'tools/emu_scripts/states/logs')
PLAYER = os.path.join(ROOT, 'engine/target/release/scene_player')


def session_window(path):
    base = os.path.basename(path)[len('session-'):-len('.jsonl')]
    import time
    start = time.mktime(time.strptime(base, '%Y%m%d-%H%M%S'))
    return start, os.path.getmtime(path)


def find_run(shot):
    name = os.path.basename(shot)
    m = re.match(r'(t|start)-([0-9a-f]{8})-f(\d+)\.png', name) or re.match(r'(motion)-(-?\d+)-f(\d+)\.png', name)
    if not m: sys.exit('unrecognised screenshot name: ' + name)
    kind, ident, frame = m.group(1), m.group(2), int(m.group(3))
    mt = os.path.getmtime(shot)
    cands = []
    for lf in sorted(glob.glob(os.path.join(LOGS, 'session-*.jsonl'))):
        a, b = session_window(lf)
        if not (a - 5 <= mt <= b + 5): continue
        cur = None; evs = []
        for line in open(lf):
            e = json.loads(line)
            if e['ev'] == 'script_start':
                cur = {'id': '%08x' % (int(e['id'], 16) & 0xffffffff), 'f0': e['f'], 'date': e.get('date'), 'hour': e.get('hour'), 'ev': [], 'f1': None}; evs.append(cur)
            elif e['ev'] == 'script_end' and cur:
                cur['f1'] = e['f']; cur = None
            elif cur and e['ev'] == 'cmd':
                cur['ev'].append((e['f'], int(e['cmd'], 16), e['args']))
        for r in evs:
            if kind in ('t', 'start') and r['id'] != ident: continue
            end = r['f1'] if r['f1'] is not None else 10 ** 9
            if r['f0'] <= frame <= end + 5: cands.append((lf, r, frame))
    if not cands: sys.exit('no script run in the logs contains this frame')
    return cands[-1]


def run_player(event, clock, choose, extra):
    cmd = [PLAYER, os.path.join(ROOT, 'content'), event] + extra
    if clock[0]: cmd += ['--month', str(clock[0])]
    if clock[1] is not None: cmd += ['--hour', str(clock[1])]
    if choose: cmd += ['--choose', choose]
    return subprocess.run(cmd, capture_output=True, text=True, cwd=os.path.join(ROOT, 'engine'))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('shot'); ap.add_argument('--out'); ap.add_argument('--log'); ap.add_argument('--player', default='一')
    ap.add_argument('--swap', action='store_true'); ap.add_argument('--noswap', action='store_true')
    a = ap.parse_args()
    lf, run, frame = find_run(a.shot)
    sid = run['id']
    sc = [s for s in sd.real_cmds(lf) if s['id'] == sid and s['clock'] == sd.clock_of({'date': run['date'], 'hour': run['hour']})]
    sc = sc[-1] if sc else None
    clock = sd.clock_of({'date': run['date'], 'hour': run['hour']})
    choose = None
    if sc:
        seen = sd.hooked_ids()
        real = sd.dedupe(sc['seq'])
        import difflib
        h = sd.dedupe([x for x in sd.host_cmds(os.path.join(ROOT, 'content'), sid, clock) if x[0] in seen])
        if difflib.SequenceMatcher(None, real, h, autojunk=False).ratio() < 0.9999:
            br, bc = sd.best_choices(sc, seen)
            if bc: choose = bc
    flags = ['--player', a.player]
    tl = run_player(sid, clock, choose, flags + ['--timeline'])
    ours = []   # (time, kind, id)
    marks = []  # every timeline event time (text boxes, fades)
    for ln in tl.stdout.splitlines():
        m = re.match(r'T ([\d.]+) (voice|motion) (-?\d+)', ln)
        if m: ours.append((float(m.group(1)), m.group(2), int(m.group(3))))
        m = re.match(r'T ([\d.]+) (fade|text) ', ln)
        if m: marks.append(float(m.group(1)))
    # last recorded voice/motion command at or before the frame that our timeline also has
    best = None
    for f, cmd, args in run['ev']:
        if f > frame: break
        kind = 'voice' if cmd == 0x5c else 'motion' if cmd == 0xf else None
        if kind and args:
            for t, k, i in ours:
                if k == kind and i == args[0]: best = (f, t, kind, args[0]); break
    if best is None:
        t = 0.0; note = 'no common voice/motion command before the frame: scene time 0'
    else:
        t = best[1] + (frame - best[0]) / 60.0
        nxt = [m for m in sorted(marks + [x[0] for x in ours]) if m > best[1] + 1e-6]
        if nxt and t > nxt[0] - 0.05: t = max(best[1], nxt[0] - 0.05)   # the game holds a line until the tap; stay inside ours
        note = '%s %d at game frame %d = player t %.2f s' % (best[2], best[3], best[0], best[1])
    tag = os.path.join(ROOT, 'export/scene/sbs')
    flags += ['--size', '384x512', '--shot', '%.3f' % t, '--out', tag]
    r = run_player(sid, clock, choose, flags)
    png = tag + '_%07.3f.png' % t if False else sorted(glob.glob(tag + '_*.png'), key=os.path.getmtime)[-1]
    ours_im = Image.open(png).convert('RGB')
    real = Image.open(a.shot).convert('RGB')
    fix = os.path.getmtime(os.path.join(ROOT, 'tools/emu_driver.py'))
    swap = a.swap or (not a.noswap and session_window(lf)[0] < fix)   # the monitor process loads the driver once at start
    if swap:
        rr, gg, bb = real.split(); real = Image.merge('RGB', (bb, gg, rr))
    real3d = real.crop((192, 0, 384, 256)).resize((384, 512))
    W = 384 * 2
    out = Image.new('RGB', (W, 512 + 36), (24, 24, 28)); out.paste(real3d, (0, 36)); out.paste(ours_im, (384, 36))
    d = ImageDraw.Draw(out)
    d.text((6, 4), 'game  %s  frame %d%s' % (sid, frame, '  (colours corrected)' if swap else ''), fill=(255, 255, 255))
    d.text((390, 4), 'ours  t=%.2fs  choices=%s  %s' % (t, choose or 'default', ''), fill=(255, 255, 255))
    d.text((6, 18), note, fill=(180, 220, 255))
    dest = a.out or os.path.join(ROOT, 'export/scene/sbs_%s_f%d.png' % (sid, frame))
    out.save(dest); print(dest); print(note)


if __name__ == '__main__':
    main()
