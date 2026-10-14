"""Check motion_sim against the recording, update by update.

    .venv/bin/python sandbox/gx/motion_verify.py [--order cmd-first|update-first] [--show N]
"""
import argparse, json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'sandbox', 'rec'))
import motion_sim as ms  # noqa: E402

SESSION = os.path.join(ROOT, 'tools/emu_scripts/states/logs/session-20261013-084834.jsonl')
RECORDING = os.path.join(ROOT, 'sandbox/rec/out/s08143402')
TICK = 0x212ece0
C = 0x229154c
CHAIN_CACHE = os.path.join(ROOT, 'sandbox/rec/out/video_08143402/ram_ticks.json')


def ram_ticks(f0, f1):
    """per logic update: last frame, chara fields and the body AnmObj chain (stem@bundle via the video timeline's matcher)"""
    if os.path.exists(CHAIN_CACHE):
        return {int(k): v for k, v in json.load(open(CHAIN_CACHE)).items()}
    from ramat import Recording
    import scene_video as sv
    r = Recording(RECORDING)
    tl = {row['f']: row for row in json.load(open(os.path.join(sv.OUT, 'timeline.json')))['rows']}
    out = {}
    for f in range(f0, f1):
        b = r.at(f).tobytes(); w = lambda a: struct.unpack_from('<I', b, a - 0x2000000)[0]
        t = w(TICK)
        out[t] = dict(f=f, idx=struct.unpack_from('<i', b, C + 0xc - 0x2000000)[0], rem=struct.unpack_from('<h', b, C + 0x18 - 0x2000000)[0],
                      mood=struct.unpack_from('<h', b, C + 0x454 - 0x2000000)[0])
    json.dump(out, open(CHAIN_CACHE, 'w'))
    return out


def frame_phases(r, f0=5482, f1=10541):
    """Capture3D phase byte (0x213d2e4+0xa3) per recorded frame"""
    return {f: r.at(f).tobytes()[0x213d2e4 + 0xa3 - 0x2000000] for f in range(f0, f1)}


def update_clock(r, f0=5482, f1=10541, phase=None):
    """logic updates of the recording: list of (first phase-0 frame, last phase-0 frame) per Capture3D cycle"""
    phase = phase or frame_phases(r, f0, f1)
    runs = []; f = f0
    while f < f1 - 1:
        if phase[f] == 0 and (f == f0 or phase[f - 1] != 0):
            g = f
            while g + 1 < f1 and phase[g + 1] == 0: g += 1
            runs.append((f, g)); f = g + 1
        else:
            f += 1
    return runs


def main():
    ap = argparse.ArgumentParser(); ap.add_argument('--order', default='cmd-first'); ap.add_argument('--show', type=int, default=25); ap.add_argument('--skip', type=int, default=0); ap.add_argument('--eye', default='before', help='before (game: the eye step sees the mood before this update motion start, traced) or after')
    a = ap.parse_args()
    from ramat import Recording
    log = [json.loads(l) for l in open(SESSION)]
    cmds = [(x['f'], int(x['cmd'], 16), x['args']) for x in log if x['ev'] == 'cmd' and 5480 <= x['f'] <= 10540]
    r = Recording(RECORDING)
    # frame -> logic update; RAM per update (state at the update's last frame)
    # logic update = once per Capture3D cycle, at the end of the cycle's run of phase-0 frames (it may spill one frame);
    # a command at frame f is handled by the first update whose phase-0 run ends at or after f; RAM is compared at the
    # last frame of the cycle (before the next phase-0 run)
    phase = {}
    for f in range(5482, 10541):
        phase[f] = r.at(f).tobytes()[0x213d2e4 + 0xa3 - 0x2000000]
    runs = []; f = 5482
    while f < 10540:
        if phase[f] == 0 and (f == 5482 or phase[f - 1] != 0):
            g = f
            while g + 1 < 10541 and phase[g + 1] == 0: g += 1
            runs.append((f, g)); f = g + 1
        else:
            f += 1
    tick_of = {}; ram = {}
    k = 0
    for f in range(5482, 10540):
        while k < len(runs) and runs[k][1] < f: k += 1
        tick_of[f] = k
    def snap(f):
        b = r.at(f).tobytes()
        obj = struct.unpack_from('<I', b, C + 0x208 + 16 - 0x2000000)[0]
        a_ = struct.unpack_from('<I', b, obj + 0x2cc + 16 - 0x2000000)[0] if obj else 0; ch = []
        while a_ and len(ch) < 4:
            ch.append((struct.unpack_from('<I', b, a_ + 8 - 0x2000000)[0], struct.unpack_from('<i', b, a_ - 0x2000000)[0] >> 12,
                       struct.unpack_from('<i', b, a_ + 4 - 0x2000000)[0]))
            a_ = struct.unpack_from('<I', b, a_ + 16 - 0x2000000)[0]
        return dict(f=f, idx=struct.unpack_from('<i', b, C + 0xc - 0x2000000)[0], rem=struct.unpack_from('<h', b, C + 0x18 - 0x2000000)[0],
                      mood=struct.unpack_from('<h', b, C + 0x454 - 0x2000000)[0], frames=[c[1] for c in ch], ratios=[c[2] for c in ch],
                      eye=struct.unpack_from('<b', b, C + 0x48b - 0x2000000)[0], eye_mode=struct.unpack_from('<i', b, C + 0x4b0 - 0x2000000)[0],
                      hands=[struct.unpack_from('<i', b, C + 0xd9c + 0x14 * k + 0xc - 0x2000000)[0] for k in (0, 1)])
    alt = {}
    for t in range(len(runs) - 1):
        ram[t] = snap(runs[t + 1][0] - 1); alt[t] = snap(runs[t + 1][0])     # end of cycle, and one frame later (spill)
    by_tick = {}
    for f, c, args in cmds:
        by_tick.setdefault(tick_of.get(f, 0), []).append((c, args))
    import scene_data as sd_, scene_video as sv_
    sim = ms.MotionSim(2, sd_.label_class(0xfc)); pre = [sv_.prev_event_preset(0x08143402)]; eye = ms.EyeSim(); sim.on_mood = eye.mood_set; hands = [None, None]
    eye_stats = [0, 0]; hand_stats = [0, 0]; eye_bad = []
    t0 = min(ram); ok = 0; n = 0; shown = 0
    for t in range(t0, max(ram) + 1):
        def run_cmds():
            nonlocal_preset = None
            for c, args in by_tick.get(t, []):
                if c == 0x6: pre[0] = args[0]
                if c == 0x2: sim.cmd_set_actor(pre[0])
                if c == 0xf: sim.cmd_f(*args)
                elif c == 0x5d: sim.cmd_5d(*args)
                elif c == 0x34: sim.cmd_34(*args)
                elif c == 0x136: sim.cmd_136(*args)
        if a.order == 'cmd-first':
            if a.eye == 'before': eye.update(sim.mood)
            # Model_UpdateCharaScene: eye step (UiAsset_UpdateSceneVariants) sees the mood before this update's motion start
            # (Model_SetMotionIdAndInitFlags runs later in Gfx3D_UpdateActor), then Gfx3D_DrawActor -> UpdateActorState
            # issues requests from the current frame's tracks; the frame advance (Mod2045ee8_ProcessessGraphics_10) follows
            run_cmds(); sim.update()
            if a.eye == 'after': eye.update(sim.mood)
            h = ms.actor_state(sim, eye, 2)
            sim.advance()
            if h is not None:
                for k in (0, 1):
                    if h[k] is not None: hands[k] = h[k]
        else:
            sim.update(); sim.advance(); run_cmds()
        if t not in ram: continue
        R = ram[t]; ch = sim.chain()
        got = dict(idx=sim.idx, rem=sim.rem, mood=sim.mood, frames=[x[1] for x in ch], ratios=[x[2] for x in ch])
        exp = {k: R[k] for k in got}; exp2 = {k: alt[t][k] for k in got}
        n += 1
        for RR in (R, alt[t]):
            pass
        e_ok = eye.st['eye'] in (R['eye'], alt[t]['eye']) and eye.st['mode'] in (R['eye_mode'], alt[t]['eye_mode'])
        eye_stats[e_ok] += 1
        if not e_ok and len(eye_bad) < 12: eye_bad.append((t, R['f'], dict(eye=eye.st['eye'], mode=eye.st['mode'], trans=eye.trans, sub=eye.sub, ph=eye.st['phase']), (R['eye'], R['eye_mode'])))
        hb = [ms.HAND_BASE['l'][2] + hands[0] if hands[0] is not None else None, ms.HAND_BASE['r'][2] + hands[1] if hands[1] is not None else None]
        h_ok = hb == R['hands'] or hb == alt[t]['hands']
        hand_stats[h_ok] += 1
        if got == exp or got == exp2: ok += 1
        elif shown < a.show and t >= a.skip:
            shown += 1; print('tick %d f%d  sim %s\n              ram %s' % (t, R['f'], got, exp))
    print('updates %d, exact %d' % (n, ok))
    print('eye texture+mode exact %d / %d' % (eye_stats[1], sum(eye_stats)))
    for x in eye_bad: print('   eye', x)
    print('hands exact %d / %d' % (hand_stats[1], sum(hand_stats)))


if __name__ == '__main__':
    main()
