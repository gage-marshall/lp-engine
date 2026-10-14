"""Replay a session to a frame and log function calls / memory writes over a window (a scripted, attached debugger).

    .venv/bin/python sandbox/rec/trace.py SESSION.jsonl OUT.jsonl --from F --to F \
        --exec 0x2002c5c:SetBaseTrans:r0v3 --exec 0x200392c:Draw \
        --write 0x212bdfc:0x30:glb_base

--exec ADDR:NAME[:DEREF]   log r0-r3, lr (caller) on entry; DEREF 'r0v3' = also read 3 words at [r0] (likewise r1v12, ...)
--write ADDR:LEN:NAME[:STEP] log every write into [ADDR, ADDR+LEN): address, writer pc, lr, new word value (read after the frame
                           would lose intermediate values, so the value is read in the callback: it is the word *before* the
                           store completes in some emulator paths; the 'post' field of the next event or the frame dump confirms it)
Every record: {"f": frame, "ev": "exec"|"write", ...}.  Frames use the monitor numbering (state after f frames).
"""
import argparse, json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools')); sys.path.insert(0, HERE)
from emu_driver import Emu  # noqa: E402
from record import load_session, apply_in  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument('session'); ap.add_argument('out')
ap.add_argument('--from', dest='f_from', type=int, required=True)
ap.add_argument('--to', dest='f_to', type=int, required=True)
ap.add_argument('--exec', action='append', default=[])
ap.add_argument('--write', action='append', default=[])
ap.add_argument('--fifo', action='store_true', help='add the GX FIFO tail (libdesmume gxFIFO) to every exec record')
ap.add_argument('--ret', action='append', default=[], help='ADDR:NAME: log entry args and the r0 returned to the caller')
ap.add_argument('--from-start', action='store_true', help='replay from the session start state (pinned RTC from frame 0)')
a = ap.parse_args()
if a.from_start:
    recs = [json.loads(l) for l in open(a.session)]
    st = recs[0]['state']
    STATES = os.path.join(ROOT, 'tools', 'emu_scripts', 'states')
    st = st if os.path.exists(st) else os.path.join(STATES, st + '.dst')
    ins = [r for r in recs if r['ev'] == 'in']; f0 = 0; init = {'mask': 0, 'touch': None}
    emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), st); emu.pin_rtc()
else:
    anc, f0, init, ins, recs = load_session(a.session)
    emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), anc['path'])
    emu.pin_rtc(*anc['rtc'])
apply_in(emu, init)
R = emu.e.memory.register_arm9
import ctypes
_gx = ctypes.addressof(ctypes.c_char.in_dll(emu.e.lib, 'gxFIFO'))
fifo_tail = lambda: ctypes.c_uint32.from_address(_gx + 0xf4244).value
mem = emu.e.memory.unsigned
out = open(a.out, 'w')
state = {'f': f0, 'seq': 0}
active = [False]


def log(**kw):
    kw['f'] = state['f'] + 1      # event happens while running frame f+1 (i.e. before the state 'after f+1 frames')
    kw['seq'] = state['seq']; state['seq'] += 1
    out.write(json.dumps(kw) + '\n')


def rd32(x):
    return mem.read_long(x)


def mk_exec(name, deref):
    def cb(addr, size):
        if not active[0]:
            return
        regs = [R.r0, R.r1, R.r2, R.r3]
        rec = {'ev': 'exec', 'fn': name, 'r': ['%x' % v for v in regs], 'lr': '%x' % R.lr}
        if a.fifo:
            rec['tail'] = fifo_tail()
        for d in deref:
            reg, n = int(d[1]), int(d[3:])
            base = regs[reg]
            if 0x2000000 <= base < 0x2400000:
                rec['r%d_mem' % reg] = [rd32(base + 4 * i) for i in range(n)]
        log(**rec)
    return cb


for spec in a.exec:
    parts = spec.split(':')
    emu.e.memory.register_exec(int(parts[0], 16), mk_exec(parts[1], parts[2].split(',') if len(parts) > 2 else []))


def mk_write(name):
    def cb(addr, size):
        if not active[0]:
            return
        log(ev='write', what=name, addr='%x' % addr, size=size, pc='%x' % R.pc, lr='%x' % R.lr, val=rd32(addr & ~3))
    return cb


for spec in a.write:
    parts = spec.split(':')
    base, ln, name = int(parts[0], 16), int(parts[1], 16), parts[2]
    step = int(parts[3]) if len(parts) > 3 else 4          # 2 or 1 to also catch halfword/byte stores
    for x in range(base, base + ln, step):
        emu.e.memory.register_write(x, mk_write(name), step)

ret_pending = {}


def mk_ret(name):
    def entry(addr, size):
        if not active[0]:
            return
        lr = R.lr & ~1
        ret_pending.setdefault(lr, []).append((name, [R.r0, R.r1, R.r2, R.r3]))
        emu.e.memory.register_exec(lr, on_return)
    return entry


def on_return(addr, size):
    st = ret_pending.get(addr)
    if not st:
        return
    name, args = st.pop()
    log(ev='ret', fn=name, args=['%x' % v for v in args], ret='%x' % R.r0, at='%x' % addr)


for spec in a.ret:
    ad, nm = spec.split(':')
    emu.e.memory.register_exec(int(ad, 16), mk_ret(nm))

qi = 0
while state['f'] < a.f_to:
    while qi < len(ins) and ins[qi]['f'] <= state['f']:
        apply_in(emu, ins[qi]); qi += 1
    active[0] = state['f'] >= a.f_from - 1
    emu.step(1); state['f'] += 1
out.close()
print('trace written', a.out)
