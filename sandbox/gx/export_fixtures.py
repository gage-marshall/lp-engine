"""Export parity fixtures for engine/tests/scene_parity.rs from the verified sandbox drivers and the recording.

    .venv/bin/python sandbox/gx/export_fixtures.py

Writes engine/tests/data/s08143402/{inputs,sched,sim,ram}.json. Only derived numbers: no ROM bytes, no script text.
  inputs  what the engine replay takes: event, frame range, Capture3D phase per frame, tap releases, script start frame,
          the save's file name (local, not committed), the game clock
  sched   the sandbox schedule (update of every command / text, typing start and end frames) and the recorded command
          updates (RAM truth for the 70 / 70 check)
  sim     the sandbox SceneSim state after every update (exact target for the engine sim)
  ram     the recorded state per update (motion chain, mood, eye, hands, brightness) for the parity counts
"""
import bisect, copy, json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'sandbox', 'rec'))
import scene_video as sv, script_sim, motion_sim as ms, motion_verify as mv, scene_data as sd, lipsync_sim as lsm  # noqa: E402
from ramat import Recording  # noqa: E402

OUT = os.path.join(ROOT, 'engine', 'tests', 'data', 's08143402')
F0, F1 = 5482, 10541


def main():
    os.makedirs(OUT, exist_ok=True)
    r = Recording(sv.RECORDING)
    log = [json.loads(l) for l in open(sv.SESSION)]
    phase = mv.frame_phases(r, F0, F1)
    runs = mv.update_clock(r, F0, F1, phase=phase)
    starts = [a for a, b in runs]; ends = [b for a, b in runs]
    rel = []; prev = None
    for x in log:
        if x['ev'] == 'in':
            tt = x.get('touch')
            if prev and not tt and F0 - 2 <= x['f'] <= sv.F1: rel.append(x['f'])
            prev = tt
    first = next(i for i, (s0, e0) in enumerate(runs) if e0 >= 5517)      # as scene_video.script_schedule
    inputs = dict(event=sv.EVENT, frames=[F0, F1], phases=''.join(str(phase[f]) for f in range(F0, F1)), releases=rel,
                  script_start_update=first, save=os.path.basename(sv.SAVE), month=sv.CLOCK[0], hour=sv.CLOCK[1],
                  prev_preset=sv.prev_event_preset(sv.EVENT), heroine=2)
    json.dump(inputs, open(os.path.join(OUT, 'inputs.json'), 'w'), indent=1)

    # schedule (sandbox) + typing frames + recorded command updates
    typing = []; orig = script_sim.typing_end
    script_sim.typing_end = lambda f0, n, s, p: (typing.append((f0, n, orig(f0, n, s, p))), typing[-1][2])[1]
    sched, _ = sv.script_schedule(r, log)
    script_sim.typing_end = orig
    real = [(bisect.bisect_left(ends, x['f']), int(x['cmd'], 16)) for x in log if x['ev'] == 'cmd' and 5480 <= x['f'] <= 10540]
    json.dump(dict(cmds=[[u, p[0], p[1]] for u, k, p in sched if k == 'cmd'],
                   texts=[[u, len([c for c in (p or '') if c != '\n'])] for u, k, p in sched if k == 'text'],
                   typing=[[f0, n, e] for f0, n, e in typing], recorded_cmds=real),
              open(os.path.join(OUT, 'sched.json'), 'w'))

    # sandbox scene sim per update (same loop as scene_video.sim_timeline, every update kept)
    by_tick = {}
    for u, kind, payload in sched:
        if kind == 'cmd':
            by_tick.setdefault(u, []).append(payload)
    sim = ms.MotionSim(2, sd.label_class(sv._scene_base()['outfit_label'])); eye = ms.EyeSim(); sim.on_mood = eye.mood_set
    hands = [None, None]; preset_now = inputs['prev_preset']
    lip = lsm.LipSync(); voice_at = {}; mouth_v = 5; mouth_hold = 0; prev_f = None
    fade = script_sim.ScreenFade(); rows = []
    for t in range(len(runs) - 1):
        for c, a in by_tick.get(t, []):
            if c == 0x1b3:
                lip.set_param(a[1], a[0]) if len(a) > 1 else lip.set_param(a[0], 0)
            if c == 0x5c and a and a[0] >= 0:
                voice_at[t + script_sim.VOICE_START] = a[0]
        for _ in range(3):
            fade.process()
        for c, a in by_tick.get(t, []):
            if c == 0xd: fade.to_black(a[0] if a else 10)
            elif c == 0xe: fade.to_clear(a[0] if a else 10, 6)
        fu = runs[t][1]
        if t in voice_at:
            lip.play(voice_at[t], fu - 1)
        if prev_f is not None:
            lip.frames(prev_f, fu)
        prev_f = fu
        calls = lip.update(fu)
        if calls:
            mouth_v = calls[-1]; mouth_hold = 20
        elif mouth_hold > 0:
            mouth_hold -= 1
        else:
            mouth_v = 5
        eye.update(sim.mood)
        for c, a in by_tick.get(t, []):
            if c == 0x6: preset_now = a[0]
            if c == 0x2: sim.cmd_set_actor(preset_now)
            {0xf: sim.cmd_f, 0x5d: sim.cmd_5d, 0x34: sim.cmd_34, 0x136: sim.cmd_136}.get(c, lambda *x: None)(*a)
        sim.update()
        h = ms.actor_state(sim, eye, 2)
        sim.advance()
        if h is not None:
            hands = [h[k] if h[k] is not None else hands[k] for k in (0, 1)]
        seen = copy.copy(fade); seen.process(); seen.process()
        chain = [[ms.mtab()[L.idx]['stem'], ms.mtab()[L.idx]['bundle'], L.frame >> 12,
                  sim.n[k] if sim.rem > 0 else (4096 if k == 0 else 0)] for k, L in enumerate(sim.layers) if L is not None]
        rows.append(dict(t=t, frame=fu, chain=chain, mood=sim.mood, eye=eye.st['eye'], eye_mode=eye.st['mode'], trans=eye.trans,
                         hands=hands, mouth=mouth_v, fade_cur=fade.cur, seen_cur=seen.cur, preset=preset_now,
                         calls=calls))
    json.dump(dict(rows=rows), open(os.path.join(OUT, 'sim.json'), 'w'))

    # recorded truth per update (motion_verify's sample: end of the cycle and one frame later)
    C = 0x229154c; ram = []
    for t in range(len(runs) - 1):
        out = []
        for f in (runs[t + 1][0] - 1, runs[t + 1][0]):
            b = r.at(f).tobytes(); rd = lambda a, fmt='<i': struct.unpack_from(fmt, b, a - 0x2000000)[0]
            obj = rd(C + 0x208 + 16, '<I'); a_ = rd(obj + 0x2cc + 16, '<I') if obj else 0; ch = []
            while a_ and len(ch) < 4:
                ch.append([rd(a_ + 8, '<I'), rd(a_) >> 12, rd(a_ + 4)]); a_ = rd(a_ + 16, '<I')
            out.append(dict(f=f, idx=rd(C + 0xc), mood=rd(C + 0x454, '<h'), frames=[c[1] for c in ch], ratios=[c[2] for c in ch],
                            eye=rd(C + 0x48b, '<b'), eye_mode=rd(C + 0x4b0), hands=[rd(C + 0xd9c + 0x14 * k + 0xc) for k in (0, 1)],
                            bright=sv._brightness(rd(0x213e4cc))))
        ram.append(out)
    json.dump(dict(hand_base=[ms.HAND_BASE['l'][2], ms.HAND_BASE['r'][2]], rows=ram), open(os.path.join(OUT, 'ram.json'), 'w'))
    print('fixtures:', OUT, 'updates', len(rows), 'cmds', sum(1 for u, k, p in sched if k == 'cmd'), 'releases', len(rel))


if __name__ == '__main__':
    main()
