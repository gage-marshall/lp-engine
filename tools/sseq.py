"""NITRO-Composer sequence (SSEQ) interpreter, MIDI exporter and software synthesizer.

    sseq.py SEQ.sseq OUT.mid [--bank BANK.json] [--loops N] [--max-seconds S]
    sseq.py SEQ.sseq OUT.wav --sound-dir content/sound --bank-name BANK_NAME [--loops N]

The sequence player here is written from the sequence/bank data manuals (read-only reference): command semantics, 48 ticks per quarter note,
variables/random/conditionals, loops/calls, envelope behaviour (decay/release speeds and the squared volume law are fitted to the manuals' tables).
Pipeline: parse() -> run() (event list in ticks) -> to_midi() / render() (WAV).
"""
import struct, sys, json, os, math, random

PPQ = 48


# --------------------------------------------------------------------------------------------- file
def parse(data):
    """-> the sequence data (offsets inside the sequence are relative to its start). SSEQ: 16-byte header, 'DATA' block: size, then the
    file offset of the data."""
    if data[:4] != b'SSEQ' or data[16:20] != b'DATA': raise ValueError('not an SSEQ file')
    return data[struct.unpack_from('<I', data, 24)[0]:]


# --------------------------------------------------------------------------------------------- interpreter
class Track:
    def __init__(self, no, pc):
        self.no, self.pc, self.wait = no, pc, 0
        self.alive = True; self.stack = []                              # call/loop stack: ('call', ret) or ('loop', start, count)
        self.prg = 0; self.vol = 127; self.expr = 127; self.pan = 64; self.bend = 0; self.bend_range = 2; self.transpose = 0
        self.prio = 64; self.notewait = True; self.tie = False; self.mute = 0
        self.porta_on = False; self.porta_key = 60; self.porta_time = 0; self.sweep = 0
        self.mod_depth = 0; self.mod_range = 1; self.mod_speed = 16; self.mod_delay = 0; self.mod_type = 0
        self.env = [-1, -1, -1, -1]; self.cond = True; self.last_key = None; self.loops_done = 0; self.tied = None
        self.mono_note = None


class Result:
    def __init__(self):
        self.notes = []          # dict(track, tick, dur, key, vel, prg, + state snapshot)
        self.ctrl = []           # (tick, track, name, value)
        self.tempo = [(0, 120)]
        self.loop = None         # (start_tick, end_tick) first loop found
        self.end_tick = 0
        self.tracks_used = set()
        self.warnings = []


def run(seq, loops=1, max_seconds=600.0, max_events=400000, gvars=None, rng=None):
    """Interpret the sequence. loops = number of times the first looping track's backward jump is taken before stopping (the intro plus
    `loops` passes of the looped part are produced); returns Result."""
    rng = rng or random.Random(1)
    d = seq; res = Result()
    lvars = [-1] * 16; gv = gvars if gvars is not None else [-1] * 16
    tracks = {0: Track(0, 0)}; tempo = 120; tick = 0; seconds = 0.0; loop_owner = None; nevents = 0
    alloc = 0xffff

    def u8(t): v = d[t.pc]; t.pc += 1; return v
    def s8(t): v = u8(t); return v - 256 if v > 127 else v
    def u16(t): v = struct.unpack_from('<H', d, t.pc)[0]; t.pc += 2; return v
    def s16(t): v = struct.unpack_from('<h', d, t.pc)[0]; t.pc += 2; return v
    def u24(t): v = d[t.pc] | d[t.pc + 1] << 8 | d[t.pc + 2] << 16; t.pc += 3; return v
    def varlen(t):
        v = 0
        while True:
            b = u8(t); v = v << 7 | (b & 0x7f)
            if not b & 0x80: return v
    def var(i): return lvars[i] if i < 16 else gv[i - 16]
    def setvar(i, v):
        v = max(-32768, min(32767, int(v)))
        if i < 16: lvars[i] = v
        else: gv[i - 16] = v

    def emit_ctrl(t, name, v): res.ctrl.append((tick, t.no, name, v))

    def close_tied(t):
        if t.tied is not None:
            n = t.tied; n['dur'] = max(1, tick - n['tick']); t.tied = None

    def do_note(t, key, vel, length):
        key = max(0, min(127, key + 0))
        start_key = t.porta_key if (t.porta_on and t.last_key is None) else (t.last_key if t.porta_on else None)
        note = {'track': t.no, 'tick': tick, 'dur': length, 'key': key, 'vel': vel, 'prg': t.prg, 'transpose': t.transpose, 'env': list(t.env),
                'glide_from': start_key, 'glide_time': t.porta_time, 'sweep': t.sweep, 'mute': t.mute}
        if t.tie:
            if t.tied is not None:                                       # legato: previous voice changes pitch/velocity
                t.tied['dur'] = max(1, tick - t.tied['tick']); note['legato'] = True
            note['dur'] = 0; t.tied = note
        res.notes.append(note); t.last_key = key; res.tracks_used.add(t.no)
        if t.notewait: t.wait = length if length > 0 else 1

    def execute(t, op, mode, execute_it):
        """mode: None normal, 'r' random, 'v' variable. execute_it False = parse and skip."""
        nonlocal tempo, alloc
        def arg(kind):
            """read one parameter of the given kind according to mode (normal/random/var)"""
            if mode == 'r':
                lo, hi = s16(t), s16(t); return rng.randint(min(lo, hi), max(lo, hi))
            if mode == 'v': return var(u8(t))
            return {'u8': u8, 's8': s8, 'u16': u16, 's16': s16, 'var': varlen}[kind](t)
        if op < 0x80:
            vel = u8(t); length = arg('var')
            if execute_it: do_note(t, op + t.transpose, vel, length)
            return
        if op == 0x80:
            v = arg('var')
            if execute_it: t.wait = v
            return
        if op == 0x81:
            v = arg('var')
            if execute_it: t.prg = v; emit_ctrl(t, 'prg', v)
            return
        if op == 0x93:
            no = u8(t); off = u24(t)
            if execute_it and no not in tracks and no < 16: tracks[no] = Track(no, off); tracks[no].wait = 0
            return
        if op in (0x94, 0x95):
            off = u24(t)
            if execute_it:
                if op == 0x95: t.stack.append(('call', t.pc))
                if off < t.pc - 4 and op == 0x94: backward(t, off)
                t.pc = off
            return
        if 0xb0 <= op <= 0xbd:
            vi = u8(t)
            if mode == 'r': lo, hi = s16(t), s16(t); x = rng.randint(min(lo, hi), max(lo, hi))
            elif mode == 'v': x = var(u8(t))
            else: x = s16(t)
            if not execute_it: return
            cur = var(vi)
            if op == 0xb0: setvar(vi, x)
            elif op == 0xb1: setvar(vi, cur + x)
            elif op == 0xb2: setvar(vi, cur - x)
            elif op == 0xb3: setvar(vi, cur * x)
            elif op == 0xb4: setvar(vi, int(cur / x) if x else cur)
            elif op == 0xb5: setvar(vi, cur << x if x >= 0 else cur >> -x)
            elif op == 0xb6:
                setvar(vi, rng.randint(min(0, x), max(0, x)))
            elif op >= 0xb8:
                t.cond = {0xb8: cur == x, 0xb9: cur >= x, 0xba: cur > x, 0xbb: cur <= x, 0xbc: cur < x, 0xbd: cur != x}.get(op, True)
            return
        if op in (0xc0, 0xc1, 0xc2, 0xc3, 0xc4, 0xc5, 0xc6, 0xc7, 0xc8, 0xc9, 0xca, 0xcb, 0xcc, 0xcd, 0xce, 0xcf, 0xd0, 0xd1, 0xd2, 0xd3, 0xd4, 0xd5, 0xd6, 0xd7):
            if op in (0xc3, 0xc4): v = arg('s8')
            else: v = arg('u8')
            if not execute_it: return
            if op == 0xc0: t.pan = v; emit_ctrl(t, 'pan', v)
            elif op == 0xc1: t.vol = v; emit_ctrl(t, 'vol', v)
            elif op == 0xc2: res.ctrl.append((tick, -1, 'main', v))
            elif op == 0xc3: t.transpose = v; emit_ctrl(t, 'transpose', v)
            elif op == 0xc4: t.bend = v; emit_ctrl(t, 'bend', v)
            elif op == 0xc5: t.bend_range = v; emit_ctrl(t, 'bendrange', v)
            elif op == 0xc6: t.prio = v
            elif op == 0xc7: t.notewait = bool(v)
            elif op == 0xc8:
                t.tie = bool(v)
                if not v: close_tied(t)
            elif op == 0xc9: t.porta_key = v; t.porta_on = True; t.last_key = v
            elif op == 0xca: t.mod_depth = v; emit_ctrl(t, 'moddepth', v)
            elif op == 0xcb: t.mod_speed = v; emit_ctrl(t, 'modspeed', v)
            elif op == 0xcc: t.mod_type = v; emit_ctrl(t, 'modtype', v)
            elif op == 0xcd: t.mod_range = v; emit_ctrl(t, 'modrange', v)
            elif op == 0xce:
                t.porta_on = bool(v)
                if not v: t.last_key = None
            elif op == 0xcf: t.porta_time = v
            elif op in (0xd0, 0xd1, 0xd2, 0xd3): t.env[op - 0xd0] = v
            elif op == 0xd4:
                t.stack.append(('loop', t.pc, v))
            elif op == 0xd5: t.expr = v; emit_ctrl(t, 'expr', v)
            elif op == 0xd7: t.mute = v
            return
        if op in (0xe0, 0xe1, 0xe3):
            v = arg('s16' if op == 0xe3 else 'u16')
            if not execute_it: return
            if op == 0xe0: t.mod_delay = v; emit_ctrl(t, 'moddelay', v)
            elif op == 0xe1:
                tempo = max(1, v); res.tempo.append((tick, tempo))
            elif op == 0xe3: t.sweep = v
            return
        if op == 0xfc:
            if not execute_it: return
            for k in range(len(t.stack) - 1, -1, -1):
                if t.stack[k][0] == 'loop': break
            else: return
            kind, start, count = t.stack[k]
            if count == 0:
                backward(t, start); t.pc = start                         # infinite loop
            else:
                count -= 1
                if count > 0: t.stack[k] = ('loop', start, count); t.pc = start
                else: del t.stack[k]
            return
        if op == 0xfd:
            if execute_it:
                while t.stack and t.stack[-1][0] != 'call': t.stack.pop()
                if t.stack: t.pc = t.stack.pop()[1]
            return
        if op == 0xfe:
            alloc = u16(t); return
        if op == 0xff:
            if execute_it: close_tied(t); t.alive = False
            return
        raise ValueError('unknown opcode 0x%02x at 0x%x (track %d)' % (op, t.pc - 1, t.no))

    visit = {}

    def backward(t, target):
        nonlocal loop_owner
        t.loops_done += 1
        if res.loop is None: res.loop = (visit.get((t.no, target), 0), tick)
        if loop_owner is None: loop_owner = t.no

    first_back_target = {}
    done = False
    while not done:
        # run every track that is due at this tick in ascending track order; a track opened with a higher number runs in the same pass,
        # one with a lower number on the next tick (as in the real player)
        for no in range(16):
            t = tracks.get(no)
            if t is None or not t.alive or t.wait > 0: continue
            while t.alive and t.wait <= 0:
                visit.setdefault((t.no, t.pc), tick)
                op = d[t.pc]; t.pc += 1; mode = None; ex = True
                if op == 0xa2:                                       # conditional prefix
                    op = d[t.pc]; t.pc += 1; ex = t.cond
                if op == 0xa0: mode = 'r'; op = d[t.pc]; t.pc += 1
                elif op == 0xa1: mode = 'v'; op = d[t.pc]; t.pc += 1
                if op == 0xa2: op = d[t.pc]; t.pc += 1; ex = t.cond
                jump_back_before = t.loops_done
                execute(t, op, mode, ex)
                nevents += 1
                if nevents > max_events: res.warnings.append('event limit'); done = True; break
                if loop_owner is not None and t.no == loop_owner and t.loops_done >= loops and t.loops_done > jump_back_before: done = True; break
            if done: break
        if done: break
        alive = [t for t in tracks.values() if t.alive]
        if not alive: break
        # advance to the next tick at which a track wakes (at least 1 tick; wait counters are in ticks)
        step = min(t.wait for t in alive); step = max(1, step)
        for t in alive: t.wait -= step
        tick += step; seconds += step * 60.0 / (tempo * PPQ)
        if seconds > max_seconds: res.warnings.append('time limit'); break
    res.end_tick = tick
    for t in tracks.values(): close_tied(t)
    # fill in note durations of infinite (0) notes that were never tied
    for n in res.notes:
        if n['dur'] == 0: n['dur'] = max(1, res.end_tick - n['tick'])
    res.tracks = {no: {'alive': t.alive} for no, t in tracks.items()}
    return res


def tick_seconds(res):
    """tempo map -> function tick -> seconds"""
    tm = sorted(res.tempo, key=lambda e: e[0]); pts = []; sec = 0.0; last_t, last_bpm = 0, tm[0][1]
    for tk, bpm in tm:
        sec += (tk - last_t) * 60.0 / (last_bpm * PPQ); pts.append((tk, sec, bpm)); last_t, last_bpm = tk, bpm
    def f(tick):
        base = pts[0]
        for p in pts:
            if p[0] <= tick: base = p
            else: break
        return base[1] + (tick - base[0]) * 60.0 / (base[2] * PPQ)
    return f


# --------------------------------------------------------------------------------------------- MIDI
def _vlq(n):
    out = [n & 0x7f]; n >>= 7
    while n: out.append(0x80 | (n & 0x7f)); n >>= 7
    return bytes(reversed(out))


def to_midi(res, path, bank=None, name='sequence', main_volume=127, gm_drums=False):
    """Standard MIDI file (format 1): track 0 = tempo map + markers, one MIDI track per sequence track.
    Channels: track n -> channel n (channel 9 is skipped for melodic tracks; drum-set instruments go to channel 9). Volume = track volume * main volume,
    expression, pan, pitch bend (+RPN range) and modulation depth are written as controllers; program numbers are the bank program number & 127."""
    drum_progs = set()
    if bank and gm_drums:
        for i, ins in enumerate(bank.get('instruments', [])):
            if isinstance(ins, dict) and ins.get('type') == 'drumset': drum_progs.add(i)
    used = sorted(res.tracks_used | {c[1] for c in res.ctrl if c[1] >= 0})
    chan = {}; nxt = 0
    for no in used:
        if nxt == 9: nxt = 10
        chan[no] = min(nxt, 15); nxt += 1
    events = {no: [] for no in used}                                    # (tick, order, bytes)
    def add(no, tick, order, b):
        if no in events: events[no].append((tick, order, b))
    mv = main_volume
    cur_prg = {no: 0 for no in used}
    for tick, no, nm, v in sorted(res.ctrl, key=lambda c: c[0]):
        if no == -1 and nm == 'main':
            mv = v
            for t2 in used: add(t2, tick, 1, bytes([0xb0 | chan[t2], 7, min(127, v)]))   # approximated below by vol (kept simple: separate update)
            continue
        if no not in events: continue
        ch = chan[no]
        if nm == 'vol': add(no, tick, 1, bytes([0xb0 | ch, 7, v & 127]))
        elif nm == 'expr': add(no, tick, 1, bytes([0xb0 | ch, 11, v & 127]))
        elif nm == 'pan': add(no, tick, 1, bytes([0xb0 | ch, 10, v & 127]))
        elif nm == 'bend':
            pb = max(0, min(16383, 8192 + v * 64)); add(no, tick, 1, bytes([0xe0 | ch, pb & 127, pb >> 7]))
        elif nm == 'bendrange':
            for cc, val in ((101, 0), (100, 0), (6, v & 127), (38, 0), (101, 127), (100, 127)): add(no, tick, 1, bytes([0xb0 | ch, cc, val]))
        elif nm == 'moddepth': add(no, tick, 1, bytes([0xb0 | ch, 1, v & 127]))
        elif nm == 'modspeed': add(no, tick, 1, bytes([0xb0 | ch, 21, v & 127]))
        elif nm == 'modrange': add(no, tick, 1, bytes([0xb0 | ch, 23, v & 127]))
        elif nm == 'prg':
            cur_prg[no] = v
            add(no, tick, 0, bytes([0xc0 | (9 if v in drum_progs else ch), v & 127]))
    for n in res.notes:
        no = n['track']
        if no not in events: continue
        ch = 9 if n['prg'] in drum_progs else chan[no]
        vel = max(1, min(127, n['vel'])); key = max(0, min(127, n['key']))
        add(no, n['tick'], 2, bytes([0x90 | ch, key, vel]))
        add(no, n['tick'] + max(1, n['dur']), 0, bytes([0x80 | ch, key, 0]))
    trks = []
    meta = bytearray(); last = 0
    last_t = {}
    for tk, bpm in res.tempo: last_t[tk] = bpm                           # later command at the same tick wins
    ev = [(tk, 0, b'\xff\x51\x03' + int(60000000 / bpm).to_bytes(3, 'big')) for tk, bpm in sorted(last_t.items())]
    ev.append((0, 0, b'\xff\x03' + bytes([len(name)]) + name.encode()[:255]))
    if res.loop:
        ev.append((res.loop[1], 3, b'\xff\x06\x07loopEnd')); ev.append((res.loop[0], 1, b'\xff\x06\x09loopStart'))
    ev.append((res.end_tick, 9, b'\xff\x2f\x00'))
    for tk, order, b in sorted(ev, key=lambda e: (e[0], e[1])):
        meta += _vlq(tk - last) + b; last = tk
    trks.append(bytes(meta))
    for no in used:
        body = bytearray(); last = 0
        nm = ('track %d' % no).encode(); body += b'\x00\xff\x03' + bytes([len(nm)]) + nm
        for tk, order, b in sorted(events[no], key=lambda e: (e[0], e[1])):
            body += _vlq(tk - last) + b; last = tk
        body += _vlq(0) + b'\xff\x2f\x00'; trks.append(bytes(body))
    out = b'MThd' + struct.pack('>IHHH', 6, 1, len(trks), PPQ)
    for t in trks: out += b'MTrk' + struct.pack('>I', len(t)) + t
    open(path, 'wb').write(out)
    return len(res.notes)


# --------------------------------------------------------------------------------------------- synthesizer
ATTACK_FAST = [0x00, 0x01, 0x05, 0x0e, 0x1a, 0x26, 0x33, 0x3f, 0x49, 0x54, 0x5c, 0x64, 0x6d, 0x74, 0x7b, 0x7f, 0x84, 0x89, 0x8f]
SR = 32000


def _db_to_gain(db): return 10 ** (db / 20.0)


def _law_db(v):            # squared volume law (value 0..127): gain = (v/127)^2
    return -90.0 if v <= 0 else 40.0 * math.log10(v / 127.0)


def _decay_speed_db_per_ms(r):
    """decay/release value -> dB per ms (units of 0.00015 dB/ms; matches the manual's table)"""
    if r >= 127: u = 0xffff
    elif r == 126: u = 0x3c00
    elif r < 50: u = r * 2 + 1
    else: u = 0x1e00 // (126 - r)
    return u * 0.00015


def _attack_rate(a):
    return (255 - a) / 255.0 if a < 109 else ATTACK_FAST[127 - a] / 255.0 if a <= 127 else 0.0


class Synth:
    def __init__(self, sound_dir, bank_name):
        self.dir = sound_dir
        self.bank = json.load(open(os.path.join(sound_dir, 'banks', bank_name + '.json')))
        idx = json.load(open(os.path.join(sound_dir, 'index.json')))
        self.wavearcs = [next(w for w in idx['wavearcs'] if w['id'] == i)['name'] for i in self.bank['wavearcs']]
        self.cache = {}

    def wave(self, arc_slot, n):
        k = (arc_slot, n)
        if k not in self.cache:
            p = os.path.join(self.dir, 'waves', self.wavearcs[arc_slot], '%03d.wav' % n)
            self.cache[k] = read_wav(p) if os.path.exists(p) else None
        return self.cache[k]

    def region(self, prg, key):
        if prg >= len(self.bank['instruments']): return None
        ins = self.bank['instruments'][prg]
        if not ins: return None
        t = ins['type']
        if t == 'drumset':
            if not ins['min'] <= key <= ins['max']: return None
            return ins['regions'][key - ins['min']]
        if t == 'keysplit':
            for k, r in zip(ins['keys'], ins['regions']):
                if key <= k: return r
            return ins['regions'][-1]
        return ins


def read_wav(path):
    d = open(path, 'rb').read(); p = 12; loop = None; rate = 32000; samples = None
    while p + 8 <= len(d):
        cid, sz = d[p:p + 4], struct.unpack_from('<I', d, p + 4)[0]
        if cid == b'fmt ': rate = struct.unpack_from('<I', d, p + 12)[0]
        elif cid == b'data':
            import numpy as np
            samples = np.frombuffer(d[p + 8:p + 8 + sz], dtype='<i2').astype(np.float32) / 32768.0
        elif cid == b'smpl' and sz >= 60:
            loop = struct.unpack_from('<2I', d, p + 8 + 44)
        p += 8 + sz + (sz & 1)
    return samples, rate, loop


def render(res, synth, path, main_volume=127, seq_volume=127, tail=2.0):
    import numpy as np
    sec = tick_seconds(res); end = sec(res.end_tick) + tail; n = int(end * SR) + 1
    out = np.zeros((n, 2), np.float32)
    # controller timelines per track as step functions
    ctrl = {}
    for tick, no, nm, v in sorted(res.ctrl, key=lambda c: c[0]): ctrl.setdefault((no, nm), []).append((sec(tick), v))
    def cval(no, nm, t, default):
        lst = ctrl.get((no, nm))
        if not lst: return default
        v = default
        for ts, x in lst:
            if ts <= t: v = x
            else: break
        return v
    for note in res.notes:
        if note.get('mute'): continue
        no = note['track']; t0 = sec(note['tick']); t1 = sec(note['tick'] + note['dur']); gate = max(0.005, t1 - t0)
        reg = synth.region(note['prg'], note['key'] - note.get('transpose', 0))
        if reg is None: continue
        typ = reg['type']
        env = [e if e >= 0 else d for e, d in zip(note['env'], (reg['attack'], reg['decay'], reg['sustain'], reg['release']))]
        a_rate, dec, sus, rel = _attack_rate(env[0]), _decay_speed_db_per_ms(env[1]), env[2], _decay_speed_db_per_ms(env[3])
        sus_db = _law_db(sus)
        total = gate + min(6.0, 72.3 / max(rel, 1e-6) / 1000.0)
        ns = int(total * SR); ns = min(ns, n - int(t0 * SR))
        if ns <= 0: continue
        tt = np.arange(ns) / SR
        # envelope in dB
        env_db = np.empty(ns, np.float32); lvl = -72.3; ms = 1000.0 / SR
        frame = int(0.005 * SR); level = -72.3; phase = 0; i = 0
        gate_n = int(gate * SR)
        while i < ns:
            blk = min(frame, ns - i); b_ms = blk * ms
            if i >= gate_n and phase < 3: phase = 3
            if phase == 0:
                level = level * a_rate if a_rate > 0 else 0.0
                if level > -0.5: level = 0.0; phase = 1
            elif phase == 1:
                level = max(sus_db, level - dec * b_ms)
                if level <= sus_db: phase = 2
            elif phase == 3: level -= rel * b_ms
            env_db[i:i + blk] = level; i += blk
            if phase == 3 and level < -72.3: env_db[i:] = -90; break
        gain = np.power(10.0, env_db / 20.0)
        # volume
        vdb = _law_db(note['vel']) + _law_db(cval(no, 'vol', t0, 127)) + _law_db(cval(no, 'expr', t0, 127)) + _law_db(seq_volume) + _law_db(main_volume)
        gain *= _db_to_gain(vdb)
        # pitch
        if typ == 1:
            wv = synth.wave(reg['wavearc'], reg['wave'])
            if wv is None or wv[0] is None: continue
            smp, rate, loop = wv
            semis = note['key'] - reg['original_key'] + cval(no, 'bend', t0, 0) * cval(no, 'bendrange', t0, 2) / 128.0
            step = rate / SR * 2 ** (semis / 12.0)
            pos = np.cumsum(np.full(ns, step)) - step
            if loop:
                ls, le = loop[0], loop[1] + 1
                over = pos >= le
                pos = np.where(over, ls + (pos - ls) % max(1, le - ls), pos)
            else:
                alive = pos < len(smp) - 1; gain = gain * alive; pos = np.minimum(pos, len(smp) - 2)
            i0 = pos.astype(np.int64); fr = (pos - i0).astype(np.float32)
            sig = smp[i0] * (1 - fr) + smp[np.minimum(i0 + 1, len(smp) - 1)] * fr
        elif typ == 2:
            duty = [0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 0.0][reg['wave'] & 7]
            freq = 440.0 * 2 ** ((note['key'] - 69) / 12.0)
            ph = (tt * freq) % 1.0; sig = np.where(ph < (duty or 0.5), 0.5, -0.5).astype(np.float32)
        elif typ == 3:
            rng = np.random.RandomState(note['key']); sig = (rng.rand(ns).astype(np.float32) - 0.5) * 0.8
        else: continue
        sig = sig * gain.astype(np.float32)
        pan = max(0, min(127, cval(no, 'pan', t0, 64) + reg['pan'] - 64)) / 127.0
        l, r = math.cos(pan * math.pi / 2), math.sin(pan * math.pi / 2)
        a = int(t0 * SR); out[a:a + ns, 0] += sig * l; out[a:a + ns, 1] += sig * r
    peak = float(np.abs(out).max()) or 1.0
    if peak > 1.0: out /= peak
    pcm = (out * 32767).astype('<i2')
    import wave
    with wave.open(path, 'wb') as w: w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(pcm.tobytes())
    return end


def export_all(sound_dir, out_dir, loops=1):
    """every sequence of the extracted sound archive -> out_dir/<name>.mid (+ index.json with tempo, tracks, loop points and lengths).
    Loops are unrolled `loops` times after the intro; the loop markers loopStart/loopEnd are written into the MIDI."""
    os.makedirs(out_dir, exist_ok=True)
    idx = json.load(open(os.path.join(sound_dir, 'index.json'))); banks = {b['id']: b['name'] for b in idx['banks']}; out = []; stats = {'sequences': 0, 'errors': 0, 'notes': 0}
    for sq in idx['sequences']:
        p = os.path.join(sound_dir, 'sequences', sq['name'] + '.sseq')
        if not os.path.exists(p): continue
        try:
            res = run(parse(open(p, 'rb').read()), loops=loops, max_seconds=900.0)
            bp = os.path.join(sound_dir, 'banks', banks.get(sq['bank'], '') + '.json'); bank = json.load(open(bp)) if os.path.exists(bp) else None
            n = to_midi(res, os.path.join(out_dir, sq['name'] + '.mid'), bank, sq['name']); f = tick_seconds(res)
            last = {}
            for tk, bpm in res.tempo: last[tk] = bpm
            out.append({'id': sq['id'], 'name': sq['name'], 'bank': sq['bank'], 'notes': n, 'tracks': sorted(res.tracks_used), 'tempo': [[tk, bpm] for tk, bpm in sorted(last.items())],
                        'seconds': round(f(res.end_tick), 2), 'loop_ticks': list(res.loop) if res.loop else None,
                        'loop_seconds': [round(f(res.loop[0]), 2), round(f(res.loop[1]), 2)] if res.loop else None, 'warnings': res.warnings})
            stats['sequences'] += 1; stats['notes'] += n
        except Exception as e: stats['errors'] += 1; out.append({'id': sq['id'], 'name': sq['name'], 'error': repr(e)[:100]})
    json.dump({'ppq': PPQ, 'sequences': out}, open(os.path.join(out_dir, 'index.json'), 'w'), indent=0)
    return stats


def main(argv):
    import argparse
    ap = argparse.ArgumentParser(); ap.add_argument('seq'); ap.add_argument('out'); ap.add_argument('--bank'); ap.add_argument('--loops', type=int, default=1)
    ap.add_argument('--max-seconds', type=float, default=300.0); ap.add_argument('--sound-dir'); ap.add_argument('--bank-name')
    a = ap.parse_args(argv)
    res = run(parse(open(a.seq, 'rb').read()), a.loops, a.max_seconds)
    if a.out.endswith('.mid'):
        bank = json.load(open(a.bank)) if a.bank else None
        print(to_midi(res, a.out, bank, os.path.basename(a.seq)), 'notes', res.warnings)
    else:
        print(render(res, Synth(a.sound_dir, a.bank_name), a.out), 's', res.warnings)


if __name__ == '__main__': main(sys.argv[1:])
