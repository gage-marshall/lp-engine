"""Render a whole recorded heroine scene as video from game data, next to the emulator's own frames.

    .venv/bin/python sandbox/gx/scene_video.py timeline      # observed drivers from the recording (cached json)
    .venv/bin/python sandbox/gx/scene_video.py refs          # emulator frames (replay of the session)
    .venv/bin/python sandbox/gx/scene_video.py render        # our frames (parallel)
    .venv/bin/python sandbox/gx/scene_video.py video         # side-by-side mp4

From game data (no RAM): camera preset (script cmd 6 at the frame it ran), wardrobe (script 12e + hair key from the save
file), time of day (clock rule), expressions (mood value from the script, face tables), models, textures, VRAM placement,
shading tables, joint evaluation + NitroSystem blend, passes/compositing, backdrop picture (game selection rule) and scroll.

Observed from the recording (drivers not decoded yet; labelled in the video):
  motion layers   which motion each part's AnmObj chain plays, its frame and ratio (Motion_UpdateStateMachine sequencing)
  mood timing     when a mood takes effect (Model_SetMotionIdAndInitFlags calls; values match the script's 0x34/0x136)
  mouth           Model_SetMotionModeFromTable(chara, v, 0x14) calls during speech (lip-sync driver), rest otherwise
  pacing          the player's button presses (when each script command ran)
Not drawn: dialogue box / UI layers (BG2, BG3, OBJ), screen fades. Blink: game machine (scene_data.blink_step); glances and
transition eye sequences are not modelled.
"""
import json, os, struct, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'tools')); sys.path.insert(0, os.path.join(ROOT, 'sandbox', 'rec'))

SESSION = os.path.join(ROOT, 'tools/emu_scripts/states/logs/session-20261013-084834.jsonl')
RECORDING = os.path.join(ROOT, 'sandbox/rec/out/s08143402')
SAVE = os.path.join(ROOT, 'tools/emu_scripts/states/logs/backup-session-20261013-110915-f7721.sav')
OUT = os.path.join(ROOT, 'sandbox/rec/out/video_08143402')
EVENT = 0x08143402
F0, F1, STEP = 5520, 10530, 4
CLOCK = __import__('scene_inputs').session_clock(SESSION, F0)    # (month, hour): game clock logged by the session
CALENDAR_78 = __import__('scene_inputs').calendar_state_from_save(SAVE)    # GameState +0x78 (lovers/calendar state)
CHARA = 0x229154c
PART_SLOTS = {0: 'hair', 1: 'body', 4: 'rarm', 6: 'larm', 9: 'leg'}


# ------------------------------------------------------------------ backdrop (engine/src/backdrop.rs port; tools/bg_model.py)
def backdrop(event_id, month, tod):
    arc = os.path.join(ROOT, 'content/assets/arc')
    sel = json.load(open(os.path.join(ROOT, 'content/engine/bg_select.json')))
    bg = None
    for f in sorted(os.listdir(os.path.join(arc, 'tmx_bbin.arc'))):
        b = open(os.path.join(arc, 'tmx_bbin.arc', f), 'rb').read()
        for i in range(struct.unpack_from('<I', b)[0]):
            ev, g = struct.unpack_from('<IH', b, 4 + 8 * i)
            if ev == event_id and bg is None:
                bg = g
    assert bg is not None, 'no backdrop record'

    def archive_of(i):
        if i >= 10001: return i // 10000
        for lo, hi, a in ((0, 0x2e, 1), (0xa7, 0xa9, 1), (0x2f, 0x47, 2), (0x48, 0x60, 3), (0x61, 0x77, 4), (0xaa, 0xaa, 4),
                          (0x78, 0x84, 5), (0x85, 0xa6, 6), (0xab, 0xaf, 6), (0xb0, 0xd4, 7), (0xd5, 0xd7, 8)):
            if lo <= i <= hi: return a

    def sf(cond, p, p3):
        return {2: 0 if cond else 30, 3: 0 if cond else ((1 if p3 == 2 else p3) + 1) * 30,
                4: ((1 if p3 == 2 else p3) + (0 if cond else 2)) * 30, 6: (p3 + (0 if cond else 3)) * 30}.get(p, 0)
    sea = next((i for i, m in enumerate((2, 5, 8, 11)) if month <= m), 0); s = [3, 0, 1, 2][sea]
    r = sel['records'][bg]; b, fl, mode, p = r['base'], r['flags'], r['mode'], r['param']
    if bg >= 10001:
        fnum = bg
    elif not any(fl):
        fnum = b + {5: sf(s == 1, p, tod), 6: sf(s == 2, p, tod), 7: sf(s == 0, p, tod)}.get(mode, 0)
    elif fl[tod] == 0:
        fnum = b if not (tod == 2 and fl[1]) else b + {-1: 0x1e, 8: (s * 2 + 1) * 0x1e, 4: sf(s < 2, p, 2), 5: sf(s == 1, p, 2),
                                                     6: sf(s == 2, p, 2), 7: sf(s == 0, p, 2)}.get(mode, 0)
    else:
        if mode == -1: fnum = tod * 0x1e + b if (p != 1 and p < 4) else b
        elif mode == 8: fnum = {12: (tod + s * 3) * 0x1e + b, 8: (tod + s * 2) * 0x1e + b, 4: s * 0x1e + b}.get(p, b)
        else: fnum = b + {4: sf(s < 2, p, tod), 5: sf(s == 1, p, tod), 6: sf(s == 2, p, tod), 7: sf(s == 0, p, tod)}.get(mode, 0)
    an = archive_of(bg)
    lb = open(os.path.join(arc, 'bg_file_no_%02d.bin' % an), 'rb').read()
    lst = struct.unpack_from('<%dI' % struct.unpack_from('<I', lb)[0], lb, 4)
    entry = next(i for i, v in enumerate(lst) if v >= fnum)
    d = os.path.join(arc, sel['archives'][an - 1] + '.bin'); files = sorted(os.listdir(d))
    import nns2d
    g, pl, sc = (open(os.path.join(d, files[3 * entry + k]), 'rb').read() for k in range(3))
    ch, pal, scr = nns2d.parse_ncgr(g), nns2d.parse_nclr(pl), nns2d.parse_nscr(sc)
    ents = struct.unpack('<%dH' % (len(scr['raw']) // 2), scr['raw'])
    # BG1 picture: 48x32 tiles. The character data is the picture in linear tile order (tile = ty*48 + tx); the screen
    # map (two blocks: 32 columns, then the remaining 16 at 1024 + ty*16 + tx-32) holds that same index in 10 bits, so
    # every tile from 1024 on is stored as 0 - only its palette nibble is usable. Tile from the position, palette from the map.
    img = np.zeros((256, 384, 3), np.uint8)
    for ty in range(32):
        for tx in range(48):
            i = ty * 32 + tx if tx < 32 else 1024 + ty * 16 + tx - 32
            e = ents[i]; px = nns2d.tile_pixels(ch, ty * 48 + tx)
            if px is None: continue
            pn = e >> 12 if ch['bpp'] == 4 else 0
            for y in range(8):
                for x in range(8):
                    v = px[(7 - y if e & 0x800 else y) * 8 + (7 - x if e & 0x400 else x)]
                    c = pal['colors'][(pn * 16 if ch['bpp'] == 4 else 0) + v]
                    img[ty * 8 + y, tx * 8 + x] = c
    return img, dict(bg_id=bg, archive=sel['archives'][an - 1], entry=entry, file_number=fnum)


def bg_window(pano, hofs, vofs):
    ys = (np.arange(192) + vofs) % 256; xs = (np.arange(256) + hofs) % 512
    out = np.zeros((192, 256, 3), np.uint8); ok = xs < 384
    out[:, ok] = pano[ys][:, xs[ok]]
    return out


# ------------------------------------------------------------------ timeline
def timeline():
    from ramat import Recording
    import nnsgen, scene_data as sd
    log = [json.loads(l) for l in open(SESSION)]
    cmds = [(x['f'], int(x['cmd'], 16), x['args']) for x in log if x['ev'] == 'cmd' and F0 - 200 <= x['f'] <= F1]
    sig = {}
    adir = os.path.join(ROOT, 'content/assets/arc/async3d.arc')
    for fn in sorted(os.listdir(adir)):                  # every motion clip in the game, keyed by stem
        bundle = int(fn[:4])
        try:
            ents = nnsgen.bundle_entries(open(os.path.join(adir, fn), 'rb').read())
        except struct.error:
            continue                                     # not a bundle (models / textures)
        for name, d in ents.items():
            if not name.endswith('.nsbca'): continue
            stem = name[:-6]
            for nm, an in nnsgen.load_bca(d).items():
                sig.setdefault(d[an.ja:an.ja + 48], []).append(('%s@%d' % (stem, bundle), nm[len(stem) + 1:], d[an.ja:an.ja + 1024]))
    r = Recording(RECORDING)
    moods = [(x['f'], int(x['args'][1], 16)) for x in log if x['ev'] == 'mdl' and x['fn'] == 'Model_SetMotionIdAndInitFlags']
    mouth = [(x['f'], int(x['args'][1], 16)) for x in log if x['ev'] == 'mdl' and x['fn'] == 'Model_SetMotionModeFromTable']
    presets = [(f, a[0]) for f, c, a in cmds if c == 6]
    rows = []
    for f in range(F0, F1 + 1, STEP):
        ram = r.at(f).tobytes()
        w = lambda a: struct.unpack_from('<I', ram, a - 0x2000000)[0]
        layers = {}
        for slot, part in PART_SLOTS.items():
            obj = w(CHARA + 0x208 + 16 * slot)
            if not obj: continue
            a = w(obj + 0x2cc + 16); chain = []
            while a and len(chain) < 4:
                res = w(a + 8)
                key = None
                if 0x2000000 <= res < 0x2400000:
                    o = res - 0x2000000
                    for k_, nm_, body in sig.get(ram[o:o + 48], []):
                        if ram[o:o + len(body)] == body:
                            key = (k_, nm_); break
                chain.append([key[0] if key else None, struct.unpack_from('<i', ram, a - 0x2000000)[0] >> 12, struct.unpack_from('<i', ram, a + 4 - 0x2000000)[0]])
                a = w(a + 16)
            layers[part] = chain
        mood = next((m for ff, m in reversed(moods) if ff <= f), None)
        mv = next(((ff, v) for ff, v in reversed(mouth) if ff <= f), None)
        preset = next((p for ff, p in reversed(presets) if ff <= f), None)
        rows.append(dict(f=f, layers=layers, mood=mood, mouth=mv, preset=preset))
    os.makedirs(OUT, exist_ok=True)
    json.dump(dict(rows=rows, cmds=cmds), open(os.path.join(OUT, 'timeline.json'), 'w'))
    unk = sum(1 for row in rows for ch in row['layers'].values() for k, _, _ in ch if k is None)
    print('timeline: %d samples, %d unidentified AnmObj resources' % (len(rows), unk))


# ------------------------------------------------------------------ emulator reference frames
def refs():
    from emu_driver import Emu
    from record import load_session, apply_in
    anc, f0, init, ins, recs = load_session(SESSION)
    emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), anc['path']); emu.pin_rtc(*anc['rtc'])
    apply_in(emu, init); qi = 0; f = f0; frames = {}
    while f < F1:
        while qi < len(ins) and ins[qi]['f'] <= f:
            apply_in(emu, ins[qi]); qi += 1
        emu.step(1); f += 1
        if f >= F0 and (f - F0) % STEP == 0:
            frames[f] = np.asarray(emu.image())[192:384].copy()          # lower screen (engine A: the 3D scene)
    np.savez_compressed(os.path.join(OUT, 'refs.npz'), frames=np.array(sorted(frames)), imgs=np.stack([frames[k] for k in sorted(frames)]))
    print('refs: %d frames' % len(frames))


# ------------------------------------------------------------------ our frames
_ctx = {}


def _init_worker():
    import scene_inputs as si, scene_data as sd
    base, _ = si.resolve(EVENT, 26, record_0d=si.record_0d_from_save(SAVE, 2), random_state=si.random_event_state(CALENDAR_78, si.slot_byte_107_from_save(SAVE, 2)), month=CLOCK[0], hour=CLOCK[1], updates=0, voice_t=99)
    _ctx['base'] = base
    pano, info = backdrop(EVENT, CLOCK[0], si.backdrop_tod(*CLOCK, calendar_78=CALENDAR_78))
    _ctx['pano'] = pano


def mouth_viseme(mv, f):
    """observed mouth call (frame, v) -> viseme: v while the calls run (held 8 frames), else rest (5)"""
    if mv is None or f - mv[0] > 8:
        return 5
    return mv[1]


def render_row(row):
    import scene_gen, nnsgen, scene_data as sd
    from gx import Geo
    from render import render as rnd, to_rgb8
    sc = dict(_ctx['base'])
    if row['preset']: sc['preset'] = row['preset']
    if row['mood'] is not None: sc['mood'] = row['mood']
    sc['viseme'] = row['mouth_v'] if 'mouth_v' in row else mouth_viseme(row['mouth'], row['f'])
    sc['eye_tex'] = row.get('eye_tex')
    if row.get('hands') is not None: sc['hands'] = row['hands']
    layers = {p: [(k, fr, ra) for k, fr, ra in ch if k is not None] for p, ch in row['layers'].items()}
    sc['anim_layers'] = layers; sc['updates'] = 0          # motion_key (from the script) only names the default clip
    S = scene_gen.SceneRenderer(sc)
    outs = {}
    for cyc in range(2):
        for phase in range(3):
            o = S.object_loop(phase, phase - 1)
            if cyc == 1: outs[phase] = o
    toon, edge = sd.shading_tables(S.h, sc['hair_label'], sc['outfit_label'], sc['time_mode'], bool(S.facing))
    tex = np.frombuffer(bytes(S.vram.tex), np.uint8); pal = np.frombuffer(bytes(S.vram.pal), np.uint8)
    hofs, vofs = sd.bg_scroll(S.proj, S.cam, [S.preset[4], S.preset[5]])
    img = bg_window(_ctx['pano'], hofs, vofs).astype(np.int32)
    for phase in (1, 2):                        # leg pass (captured, drawn behind), then the main pass
        g = Geo(); g.run([(c, ps, 0, i) for i, (c, ps) in enumerate(outs[phase] + [(0x50, [0])])])
        polys = g.lists[-1][2] if g.lists else []
        if not polys: continue
        col, alpha, _, _ = rnd(polys, tex, pal, toon, edge, 0x39)
        rgb = to_rgb8(col).astype(np.int32); a = (alpha.astype(np.int32) * 255 // 31)[..., None]
        img = (rgb * a + img * (255 - a)) // 255
    if row.get('fade') is not None:                      # master brightness toward black: level = |fade| * 16
        lvl = int(abs(row['fade']) * 16 + 1e-9)
        img = img - (img * lvl) // 16
    return row['f'], img.astype(np.uint8)


def add_blink(rows):
    """eye texture per update from the game's blink machine (scene_data.blink_step, 0x2094aa4), one step per rendered
    frame (= one game update); eye mode 9 (idle default), state from rest. Not modelled: the idle eye-request timer
    (glances, mode 0/1) and the eye sequences played during motion/mood transitions."""
    import scene_data as sd
    st = dict(counter=0, idx=0, eye=-1, phase=0, hold=0, extra=0, mode=9)
    for row in rows:
        sd.blink_step(st, row['mood'] if row['mood'] is not None else 0, rand=lambda n: n // 2)
        row['eye_tex'] = st['eye'] if st['eye'] >= 0 else None
        row['blink_phase'] = st['phase']
    return rows


TL = os.environ.get('LP_TIMELINE', 'timeline.json')      # timeline_sim.json = motion/mood/eyes/hands from motion_sim


def load_rows():
    rows = json.load(open(os.path.join(OUT, TL)))['rows']
    return rows if TL != 'timeline.json' else add_blink(rows)


def render_all():
    from multiprocessing import Pool
    rows = load_rows()
    res = {}
    with Pool(10, initializer=_init_worker) as pool:
        for k, (f, img) in enumerate(pool.imap_unordered(render_row, rows, chunksize=4)):
            res[f] = img
            if k % 100 == 0: print('rendered', k, '/', len(rows), flush=True)
    fs = sorted(res)
    np.savez_compressed(os.path.join(OUT, 'ours.npz'), frames=np.array(fs), imgs=np.stack([res[f] for f in fs]))
    print('rendered', len(fs))


def video():
    from PIL import Image, ImageDraw
    import subprocess
    ours = np.load(os.path.join(OUT, 'ours.npz')); ref = np.load(os.path.join(OUT, 'refs.npz'))
    if TL != 'timeline.json':                            # refs are every 4th frame; pick the nearest emulator frame per update
        pass
    rmap = {int(f): i for i, f in enumerate(ref['frames'])}
    tl = {r['f']: r for r in load_rows()}
    fr = os.path.join(OUT, 'frames'); os.makedirs(fr, exist_ok=True)
    for k, f in enumerate(ours['frames']):
        f = int(f)
        fr_ = f if f in rmap else min(rmap, key=lambda x: abs(x - f))
        a = Image.fromarray(ref['imgs'][rmap[fr_]]).rotate(90, expand=True)
        b = Image.fromarray(ours['imgs'][k]).rotate(90, expand=True)
        page = Image.new('RGB', (192 * 2 + 12, 256 + 40), (24, 24, 24)); page.paste(a, (0, 0)); page.paste(b, (204, 0))
        dr = ImageDraw.Draw(page); row = tl[f]
        body = row['layers'].get('body') or []
        dr.text((4, 258), 'emulator   |   game data  f%d' % f, fill=(220, 220, 220))
        dr.text((4, 272), 'motion %s  mood %s  mouth %s  eye %s%s' % (' + '.join('%s@%d/%d' % (k_, fr_, ra) for k_, fr_, ra in body),
                                                             row['mood'], row['mouth_v'] if 'mouth_v' in row else mouth_viseme(row['mouth'], f), row.get('eye_tex'),
                                                             ' (blink)' if row.get('blink_phase') in (1, 2, 10) else ''), fill=(160, 160, 160))
        if row.get('hands'): dr.text((4, 286 - 0), '', fill=(0, 0, 0))
        page.resize((page.width * 2, page.height * 2), Image.NEAREST).save(os.path.join(fr, '%05d.png' % k))
    out = os.path.join(OUT, 'scene_08143402_side_by_side%s.mp4' % ('' if TL == 'timeline.json' else '_sim'))
    snd = os.path.join(OUT, 'soundtrack.wav')
    if TL != 'timeline.json' and os.path.exists(snd):
        # each frame lasts its update's real game time (frames / 59.8261 Hz), so picture and soundtrack share one clock
        fl = [int(x) for x in ours['frames']]; lst = os.path.join(OUT, 'frames.txt')
        with open(lst, 'w') as fh:
            for k, f in enumerate(fl):
                d = ((fl[k + 1] - f) if k + 1 < len(fl) else 4) / 59.8261
                fh.write("file '%s'\nduration %.6f\n" % (os.path.join(fr, '%05d.png' % k), d))
            fh.write("file '%s'\n" % os.path.join(fr, '%05d.png' % (len(fl) - 1)))
        subprocess.run(['ffmpeg', '-y', '-loglevel', 'error', '-f', 'concat', '-safe', '0', '-i', lst, '-i', snd,
                        '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '18', '-fps_mode', 'vfr',
                        '-c:a', 'aac', '-b:a', '192k', '-shortest', out], check=True)
    else:
        subprocess.run(['ffmpeg', '-y', '-loglevel', 'error', '-framerate', '15', '-i', os.path.join(fr, '%05d.png'),
                        '-c:v', 'libx264', '-pix_fmt', 'yuv420p', '-crf', '18', out], check=True)
    print('wrote', out)




# ------------------------------------------------------------------ data-driven timeline (motion_sim instead of RAM)
def prev_event_preset(event_id):
    """camera preset in force when an event starts: the last cmd 6 of the previous event of the chain (id - 1)"""
    import script_sim
    ev = script_sim.stream(event_id - 1)
    p = [e['args'][0] for e in ev if e['kind'] == 'command' and int(e['id'], 16) == 0x6 and e['args']]
    return p[-1] if p else -1


def _scene_base():
    import scene_inputs as si
    return si.resolve(EVENT, 26, record_0d=si.record_0d_from_save(SAVE, 2), random_state=si.random_event_state(CALENDAR_78, si.slot_byte_107_from_save(SAVE, 2)), month=CLOCK[0], hour=CLOCK[1], updates=0, voice_t=99)[0]


def save_text_speed():
    """text speed setting from the save (settings record +0xc = save block 0 byte 0x41; script_sim.text_speed)"""
    import scene_inputs as si, script_sim
    return script_sim.text_speed(si.save_block(open(SAVE, 'rb').read(), 0xa20))


def _brightness(cur):
    """ScreenFade_Process master brightness: cur (16.16) / 0x1000 rounded toward zero, -16..0"""
    return -((-cur) >> 12) if cur < 0 else cur >> 12


def script_schedule(r, log):
    """script_sim schedule for the recorded scene: taps (release frames) from the session log, frame clock (Capture3D phase
    per frame) from the recording, text speed from the save. Returns (schedule, update runs)."""
    import bisect, motion_verify as mv, script_sim
    phase = mv.frame_phases(r)
    runs = mv.update_clock(r, phase=phase)
    rel = []; prev = None
    for x in log:
        if x['ev'] == 'in':
            tt = x.get('touch')
            if prev and not tt and 5480 <= x['f'] <= F1: rel.append(x['f'])
            prev = tt
    starts = [s0 for s0, e0 in runs]; ends = [e0 for s0, e0 in runs]
    first = next(i for i, (s0, e0) in enumerate(runs) if e0 >= 5517)
    sched = script_sim.schedule(EVENT, ends, rel, start_update=first, speed=save_text_speed(),
                                cycle_of=lambda f: bisect.bisect_right(starts, f) - 1, phase_of=lambda f: phase.get(f, 0))
    return sched, runs


def sim_timeline():
    """one row per logic update: motion layers + mood from motion_sim fed with the script commands; eye from the blink
    machine (scene_data.blink_step) with the simulated mood. Uses the recording only for (a) the frame at which each script
    command ran (player pacing) and (b) the reference frame of each update for the side-by-side."""
    from ramat import Recording
    import copy, motion_sim as ms, motion_verify as mv, scene_data as sd, script_sim
    log = [json.loads(l) for l in open(SESSION)]
    r = Recording(RECORDING)
    # script pacing from the VM stream + the player's tap releases (script_sim); the recording only supplies the taps and
    # the frame clock (Capture3D phases) used to place them
    sched, runs = script_schedule(r, log)
    ends = [e0 for s0, e0 in runs]
    by_tick = {}
    for u, kind, payload in sched:
        if kind == 'cmd':
            by_tick.setdefault(u, []).append(payload)
    cmds = [(ends[u] if u < len(ends) else F1, c, a) for u, kind, (c, a) in [(u, k, p) for u, k, p in sched if k == 'cmd']]
    presets = [(f, a[0]) for f, c, a in cmds if c == 6]
    import lipsync_sim as lsm
    import scene_data as sd_
    sim = ms.MotionSim(2, sd_.label_class(_scene_base()['outfit_label'])); eye = ms.EyeSim(); sim.on_mood = eye.mood_set; hands = [0, 0]
    preset_now = prev_event_preset(EVENT)                 # camera preset when set_actor runs (the previous event's last cmd 6)
    lip = lsm.LipSync(); voice_at = {}; mouth_v = 5; mouth_hold = 0; prev_f = None
    fade = script_sim.ScreenFade()                     # black at the start
    rows = []
    for t in range(len(runs) - 1):
        for c, a in by_tick.get(t, []):
            if c == 0x1b3:                               # ScriptOp_CallMod209aeb0Set: (value, which) or (which) -> 0
                lip.set_param(a[1], a[0]) if len(a) > 1 else lip.set_param(a[0], 0)
            if c == 0x5c and a and a[0] >= 0:
                voice_at[t + script_sim.VOICE_START] = a[0]   # stream start pipeline (script_sim: PlayEntry after c + 3)
        for _ in range(3):                               # ScreenFade_Process: 3 passes per update, before this update's
            fade.process()                               # commands (a fade starts stepping in the update after its command)
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
            mouth_v = calls[-1]; mouth_hold = 20          # Model_SetMotionModeFromTable hold
        elif mouth_hold > 0:
            mouth_hold -= 1
        else:
            mouth_v = 5                                    # hold over: the mood's default mouth
        eye.update(sim.mood)
        for c, a in by_tick.get(t, []):
            if c == 0x6: preset_now = a[0]
            if c == 0x2: sim.cmd_set_actor(preset_now)
            {0xf: sim.cmd_f, 0x5d: sim.cmd_5d, 0x34: sim.cmd_34, 0x136: sim.cmd_136}.get(c, lambda *x: None)(*a)
        sim.update()
        h = ms.actor_state(sim, eye, 2)                  # UpdateActorState reads the tracks before the frame advance
        sim.advance()
        if h is not None:
            hands = [h[k] if h[k] is not None else hands[k] for k in (0, 1)]
        st = eye.st
        f = runs[t + 1][0] - 1
        if f < F0: continue
        chain = sim.chain()
        seen = copy.copy(fade)                           # the row's frame (end of the cycle) has seen the next cycle's
        seen.process(); seen.process()                   # phase-1 and phase-2 passes too
        rows.append(dict(f=f, tick=t, layers={p: chain for p in ms.PARTS}, mood=sim.mood,
                         mouth_v=mouth_v,
                         preset=next((p for ff, p in reversed(presets) if ff <= f), None),
                         eye_tex=st['eye'] if st['eye'] >= 0 else None, blink_phase=st['phase'] if eye.trans == 0 else 1,
                         hands=list(hands), fade=_brightness(seen.cur) / 16))
    json.dump(dict(rows=rows), open(os.path.join(OUT, 'timeline_sim.json'), 'w'))
    print('sim timeline: %d updates' % len(rows))




# ------------------------------------------------------------------ soundtrack from the script (game data only)
def soundtrack():
    """Audio for the sim video. Every event comes from the script scheduler (script_sim): 0x68 bgm id [fade] = SDAT
    sequence, 0x6a arc idx fade = sequence-archive sub-sequence (ambient slot), 0x6c / 0x6e frames = fade out bgm / ambient,
    0x5c clip = Siren14 voice starting 4 updates later (stream pipeline). Each sound is rendered by the engine's SDAT driver
    (engine/target/release/audio_render, content/ only); fades are the gain ramps the driver applies (frames at 60 Hz).
    Times: the update's frame / 59.8261 s, the same clock as the video frames."""
    import subprocess, wave, bisect
    from ramat import Recording
    import motion_verify as mv, script_sim
    rate = 48000
    rows = load_rows(); f_first = rows[0]['f']
    log = [json.loads(l) for l in open(SESSION)]
    r = Recording(RECORDING)
    sched, runs = script_schedule(r, log)
    ends = [e0 for s0, e0 in runs]
    sec = lambda u: (ends[min(u, len(ends) - 1)] - f_first) / 59.8261
    total = (rows[-1]['f'] - f_first) / 59.8261 + 2.0
    n = int(total * rate); mix = np.zeros((n, 2), np.float64)
    exe = os.path.join(ROOT, 'engine', 'target', 'release', 'audio_render')
    tmp = os.path.join(OUT, 'audio'); os.makedirs(tmp, exist_ok=True)

    def render(args, seconds, name):
        p = os.path.join(tmp, name + '.wav')
        if not os.path.exists(p):
            subprocess.run([exe, os.path.join(ROOT, 'content'), p] + args + ['--seconds', '%.2f' % seconds, '--rate', str(rate)],
                           check=True, capture_output=True)
        w = wave.open(p); a = np.frombuffer(w.readframes(w.getnframes()), '<i2').reshape(-1, 2).astype(np.float64) / 32768
        return a

    def place(a, t0, fade_in=0.0, t_stop=None, fade_out=0.0):
        i0 = int(max(t0, 0) * rate); m = min(len(a), n - i0)
        if m <= 0: return
        g = np.ones(m)
        if fade_in > 0:
            k = min(m, int(fade_in * rate)); g[:k] = np.linspace(0, 1, k, endpoint=False)
        if t_stop is not None:
            s = int((t_stop - t0) * rate); k = int(fade_out * rate)
            if s < m:
                e = min(m, s + k)
                if e > s: g[s:e] *= np.linspace(1, 0, e - s, endpoint=False)
                g[e:] = 0
        mix[i0:i0 + m] += a[:m] * g[:, None]

    # sound levels (music, sound, voice) from the save: settings record, save block 0 at 0x29..0x2b (= RAM record +5..+7)
    import scene_inputs as si
    levels = list(si.save_block(open(SAVE, 'rb').read(), 0xa20)[0x29:0x2c])
    seq_events = []; log_ev = []
    for u, kind, payload in sched:
        if kind != 'cmd': continue
        c, a = payload; t = sec(u)
        if c == 0x68:      # GameSnd_PlaySeq(slot 0, seq, vol 0x69, fade)
            seq_events.append(dict(t=t, op='start', slot=0, seq=a[0], fade=a[1] if len(a) > 1 else 0)); log_ev.append((round(t, 2), 'bgm', a))
        elif c == 0x6a:    # ambient: sequence-archive sub-sequence, third argument = fade-in frames
            seq_events.append(dict(t=t, op='start', slot=1, arc=[a[0], a[1]], fade=a[2] if len(a) > 2 else 0)); log_ev.append((round(t, 2), 'ambient', a))
        elif c == 0x6c:
            seq_events.append(dict(t=t, op='stop', slot=0, fade=a[0] if a else 0)); log_ev.append((round(t, 2), 'stop_seq', a))
        elif c == 0x6e:
            seq_events.append(dict(t=t, op='stop', slot=1, fade=a[0] if a else 0)); log_ev.append((round(t, 2), 'stop_ambient', a))
        elif c == 0x5c and a and a[0] >= 0 and a[0] != 40553:
            log_ev.append((round(sec(u + 4), 2), 'voice', a))
    evp = os.path.join(tmp, 'seq_events.json')
    json.dump(dict(rate=rate, seconds=total, frame_hz=59.8261, levels=levels, events=seq_events), open(evp, 'w'))
    seqw = os.path.join(tmp, 'sequences.wav')
    mixer = os.path.join(ROOT, 'sandbox', 'audio_mix', 'target', 'release', 'audio_mix')
    if not os.path.exists(mixer):
        subprocess.run(['cargo', 'build', '--release'], cwd=os.path.join(ROOT, 'sandbox', 'audio_mix'), check=True, capture_output=True)
    subprocess.run([mixer, os.path.join(ROOT, 'content'), evp, seqw], check=True, capture_output=True)
    w = wave.open(seqw); sq = np.frombuffer(w.readframes(w.getnframes()), '<i2').reshape(-1, 2).astype(np.float64) / 32768
    mix[:min(n, len(sq))] += sq[:min(n, len(sq))]
    # voices: NNS stream, 2 hardware channels (8, 9) carrying the same mono PCM, volume 0 cB (level 15), pan 0x40 each;
    # per side 2 * (127/128) * pan factor ((127 - 64)/127 left, 64/127 right, the driver's pan law)
    import lipsync_sim as lsm
    # voice level (save, settings byte 7): GameSnd_ApplyChannelVolume 0x20724f8 slot 3 -> value = trunc(500 * (level + 15)
    # / 30), level 0 -> 0; GameSnd_ConvertVolumeToAttenuation 0x2071970 -> cB = trunc((500 - value) * -640 / 500);
    # GameSnd_StreamSetup 0x2077e70 / StreamStartIfReady 0x2077f18: cB <= -640 -> -1024; NNS_SndStrmSetVolume(cB) ->
    # amplitude 10^(cB / 200), silent at or below -723 (SND_CalcChannelVolume's floor)
    vlev = levels[2]
    value = int(500 * (vlev + 15) / 30) if vlev != 0 else 0
    vcb = int((500 - min(max(value, 0), 500)) * -640 / 500)
    if vcb <= -640: vcb = -1024
    g = np.array([2 * 127 / 128 * 63 / 127, 2 * 127 / 128 * 64 / 127]) * (10 ** (vcb / 200) if vcb > -723 else 0.0)
    for t0, kind, a in log_ev:
        if kind != 'voice': continue
        x = lsm.pcm(a[0]).astype(np.float64) / 32768                 # 32 kHz
        y = np.interp(np.arange(int(len(x) * rate / 32000)) * 32000 / rate, np.arange(len(x)), x)
        i0 = int(t0 * rate); m = min(len(y), n - i0)
        if m > 0: mix[i0:i0 + m] += y[:m, None] * g[None, :]
    print('levels (music, sound, voice) from the save:', levels)
    pk = np.abs(mix).max()
    out = np.clip(mix, -1, 1)
    p = os.path.join(OUT, 'soundtrack.wav'); w = wave.open(p, 'wb'); w.setnchannels(2); w.setsampwidth(2); w.setframerate(rate)
    w.writeframes((out * 32767).astype('<i2').tobytes()); w.close()
    json.dump(log_ev, open(os.path.join(OUT, 'soundtrack_events.json'), 'w'))
    print('soundtrack: %d events, %.1f s, peak %.2f -> %s' % (len(log_ev), total, pk, p))
    for e in log_ev: print('  ', e)


if __name__ == '__main__':
    {'timeline': timeline, 'sim': sim_timeline, 'refs': refs, 'render': render_all, 'video': video, 'audio': soundtrack}[sys.argv[1]]()
