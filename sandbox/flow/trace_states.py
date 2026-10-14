"""Trace the game's state machines through a recorded session: every state change at every depth of the state stack
(Main_FreeStateBuffer 0x203dbf0 = set the state at the current depth and reset the deeper ones; depth counter
0x2139cdc, states 0x2139ce0), overlay loads / unloads, screen requests (0x2089d8c (kind, screen, arg)) and screen
results (the request block 0x2152ea4: +0x14 result, +0x18 alternative result), each with the calling function.
    .venv/bin/python sandbox/flow/trace_states.py SESSION.jsonl [LAST_FRAME] > out.jsonl"""
import bisect, csv, json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from emu_driver import Emu  # noqa: E402

DEPTH, STATES = 0x2139cdc, 0x2139ce0
SET_STATE, OV_LOAD, OV_UNLOAD, SCREEN_REQ = 0x203dbf0, 0x20ef978, 0x20ef9ec, 0x2089d8c
REQ = 0x2152ea4


def labels():
    """region -> sorted [(addr, name)]"""
    out = {}
    for row in csv.reader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
        if len(row) < 3 or not row[1][:1].isdigit():
            continue
        out.setdefault(row[0], []).append((int(row[1], 16), row[2]))
    for v in out.values():
        v.sort()
    return out


L = labels()
loaded = set()


def name_of(addr):
    addr &= ~1
    regions = ['static'] + ['ov%02d' % o for o in sorted(loaded)] if addr >= 0x2150000 and addr < 0x2200000 else ['static']
    best = None
    for r in regions:
        v = L.get(r, [])
        i = bisect.bisect_right(v, (addr, '￿')) - 1
        if i >= 0 and addr - v[i][0] < 0x4000 and (best is None or v[i][0] > best[0]):
            best = v[i]
    return '%s+%x' % (best[1], addr - best[0]) if best else '%x' % addr


def main(session, last=None):
    recs = [json.loads(l) for l in open(session)]
    start = next(r for r in recs if r['ev'] == 'start')['state']
    if not start.endswith('.dst'):
        start = 'tools/emu_scripts/states/%s.dst' % start
    e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), os.path.join(ROOT, start))
    e.pin_rtc()
    m = e.e.memory
    reg = m.register_arm9
    out = sys.stdout

    def log(**kw):
        out.write(json.dumps(dict(f=e.frames, **kw)) + '\n')

    def on_set(a, s):
        d = m.unsigned.read_long(DEPTH)
        old = m.unsigned.read_long(STATES + 4 * d)
        log(ev='state', depth=d, old=old, new=reg.r0 if reg.r0 < 0x80000000 else reg.r0 - (1 << 32),
            by=name_of(reg.lr), stack=[m.unsigned.read_long(STATES + 4 * i) for i in range(d + 1)])
        return True

    def on_load(a, s):
        loaded.add(reg.r0)
        log(ev='ov_load', ov=reg.r0, by=name_of(reg.lr))
        return True

    def on_unload(a, s):
        ov = reg.r0 if reg.r0 < 0x80000000 else -1
        log(ev='ov_unload', ov=ov, by=name_of(reg.lr))
        if ov == -1:
            loaded.clear()
        else:
            loaded.discard(ov)
        return True

    def on_req(a, s):
        log(ev='screen', kind=reg.r0, screen=reg.r1, arg=reg.r2 if reg.r2 < 0x80000000 else reg.r2 - (1 << 32),
            by=name_of(reg.lr))
        return True

    def on_result(a, s):
        v = m.unsigned.read_long(a & ~3)
        log(ev='result', field='+%x' % ((a & ~3) - REQ), value=v if v < 0x80000000 else v - (1 << 32), by=name_of(reg.pc))
        return True

    m.register_exec(SET_STATE, on_set)
    m.register_exec(OV_LOAD, on_load)
    m.register_exec(OV_UNLOAD, on_unload)
    m.register_exec(SCREEN_REQ, on_req)
    m.register_write(REQ + 0x14, on_result, 8)

    inputs = {r['f']: (r['mask'], r['touch']) for r in recs if r['ev'] == 'in'}
    end = last or max(r['f'] for r in recs)
    touching = False
    while e.frames < end:
        f = e.frames
        if f in inputs:
            mask, touch = inputs[f]
            e.e.input.keypad_update(mask)
            if touch:
                e.e.input.touch_set_pos(touch[0], touch[1]); touching = True
            elif touching:
                e.e.input.touch_release(); touching = False
        e.step()


if __name__ == '__main__':
    main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else None)
