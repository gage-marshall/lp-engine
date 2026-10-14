"""Explorer: replay a recorded session's inputs (fast, no tracing) up to a frame, then run a script of further actions on
the emulator - taps, waits, captures, memory-write watches - to reach and record screens the session never showed.
    .venv/bin/python sandbox/ui/explore.py SESSION.jsonl FRAME SCRIPT.json OUT_DIR
SCRIPT: list of steps
    {"tap": [x, y], "hold": 4, "release": 20}      touch the bottom screen (DS coordinates)
    {"wait": N}                                     run N frames
    {"capture": "name"}                             full display capture (capture.py layout) into OUT_DIR/name
    {"watch": [addr, size, "label"]}                log every write to addr..addr+size (pc, lr, value) to OUT_DIR/watch.jsonl
    {"save": "name"}                                save state OUT_DIR/name.dst
A session prefix is reused through a cached state (OUT_DIR/../_prefix-fFRAME.dst) once it has been replayed."""
import json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools')); sys.path.insert(0, HERE)
from emu_driver import Emu  # noqa: E402
import capture  # noqa: E402


def replay_to(session, frame, cache_dir):
    recs = [json.loads(l) for l in open(session)]
    start = next(r for r in recs if r['ev'] == 'start')['state']
    cached = os.path.join(cache_dir, '_prefix-%s-f%d.dst' % (os.path.basename(session).split('.')[0], frame))
    e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), os.path.join(ROOT, start))
    e.pin_rtc()
    if os.path.exists(cached + '.json'):
        e.load(cached)
        meta = json.load(open(cached + '.json'))
        import ctypes
        ctypes.c_int32.from_address(e._rtc + 0x63568f8).value = meta['rtc_counter']
        e.frames = frame
        return e
    inputs = {r['f']: (r['mask'], r['touch']) for r in recs if r['ev'] == 'in'}
    touching = False
    while e.frames < frame:
        f = e.frames
        if f in inputs:
            mask, touch = inputs[f]
            e.e.input.keypad_update(mask)
            if touch:
                e.e.input.touch_set_pos(touch[0], touch[1]); touching = True
            elif touching:
                e.e.input.touch_release(); touching = False
        e.step()
    if touching:
        e.e.input.touch_release()
    e.e.input.keypad_update(0)
    e.save(cached)
    json.dump({'rtc_counter': e.rtc_state()[1]}, open(cached + '.json', 'w'))
    return e


def run(session, frame, script, out):
    os.makedirs(out, exist_ok=True)
    e = replay_to(session, frame, os.path.dirname(os.path.abspath(out)))
    watch_log = open(os.path.join(out, 'watch.jsonl'), 'a')
    mem = e.e.memory
    reg = e.e.memory.register_arm9
    for st in script:
        if 'tap' in st:
            x, y = st['tap']
            e.touch(x, y, hold=st.get('hold', 4), release=st.get('release', 20))
        elif 'wait' in st:
            e.step(st['wait'])
        elif 'capture' in st:
            capture.capture(e, os.path.join(out, st['capture']))
        elif 'save' in st:
            e.save(os.path.join(out, st['save'] + '.dst'))
        elif 'watch' in st:
            a, n, label = st['watch']

            def cb(addr, size, label=label):
                watch_log.write(json.dumps(dict(f=e.frames, label=label, addr=addr, pc=reg.pc, lr=reg.lr,
                                                value=mem.unsigned.read_long(addr & ~3))) + '\n')
                return True
            mem.register_write(a, cb, n)
    watch_log.close()
    return e


if __name__ == '__main__':
    run(sys.argv[1], int(sys.argv[2]), json.load(open(sys.argv[3])), sys.argv[4])
