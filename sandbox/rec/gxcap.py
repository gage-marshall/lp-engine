"""Capture the complete 3D geometry command stream (every command and parameter the game sends to the geometry engine)
over a frame range of a recorded session, by reading DeSmuME's internal GX FIFO ring after every frame.

    .venv/bin/python sandbox/rec/gxcap.py SESSION.jsonl OUT.npz --from F --to F

libdesmume (bundled arm64 0.9.12) gxFIFO layout, from GFX_FIFOsend(cmd, param):
    u8 cmd[200000] @0, u32 param[200000] @0x30d40, u32 head @0xf4240, tail @0xf4244, size @0xf4248.
Every write path (packed GXFIFO, command ports, DMA) goes through GFX_FIFOsend, one (cmd, param) entry per parameter word
(commands without parameters get one entry).  Output arrays: frame (absolute monitor frame), cmd, param.
"""
import argparse, ctypes, os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools')); sys.path.insert(0, HERE)
from emu_driver import Emu  # noqa: E402
from record import load_session, apply_in  # noqa: E402

N = 200000

ap = argparse.ArgumentParser()
ap.add_argument('session')
ap.add_argument('out')
ap.add_argument('--from', dest='f_from', type=int, required=True)
ap.add_argument('--to', dest='f_to', type=int, required=True)
a = ap.parse_args()
anc, f0, init, ins, recs = load_session(a.session)
emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), anc['path'])
emu.pin_rtc(*anc['rtc'])
apply_in(emu, init)
g = ctypes.addressof(ctypes.c_char.in_dll(emu.e.lib, 'gxFIFO'))
cmds = np.ctypeslib.as_array((ctypes.c_uint8 * N).from_address(g))
params = np.ctypeslib.as_array((ctypes.c_uint32 * N).from_address(g + 0x30d40))
tail = lambda: ctypes.c_uint32.from_address(g + 0xf4244).value

f, qi = f0, 0
def step():
    global f, qi
    while qi < len(ins) and ins[qi]['f'] <= f:
        apply_in(emu, ins[qi]); qi += 1
    emu.step(1); f += 1

while f < a.f_from - 1:
    step()
out_f, out_c, out_p = [], [], []
regs = {}
vids = {}
rawio = {}
mmu = ctypes.addressof(ctypes.c_char.in_dll(emu.e.lib, 'MMU'))
shotdir = os.path.splitext(a.out)[0] + '_shots'
os.makedirs(shotdir, exist_ok=True)
m = emu.e.memory.unsigned
t0 = tail()
while f < a.f_to:
    step()
    # display registers after the frame: engine A 0x4000000-0x400006f (DISPCNT, BG, DISPCAPCNT @0x64), DISP3DCNT @0x4000060, VRAMCNT 0x4000240-0x4000249
    regs[f] = b''.join(m.read_short(0x4000000 + o).to_bytes(2, 'little') for o in range(0, 0x70, 2)) + \
        bytes(m.read_byte(0x4000240 + o) for o in range(10))
    emu.image().save(os.path.join(shotdir, 'f%d.png' % f))
    # raw video memory after the frame, independent of the current bank mapping (libdesmume MMU struct offsets, found by search):
    # palettes (A+B, 2 KB) @+0x2014000, VRAM banks A..I (0xA4000, LCDC order A 0, B 0x20000, C 0x40000, D 0x60000, E 0x80000,
    # F 0x90000, G 0x94000, H 0x98000, I 0xA0000) @+0x2014800, OAM (A+B, 2 KB) @+0x20d8808
    oam = ctypes.string_at(mmu + 0x20d8808, 0x800)
    assert oam[:64] == bytes(m.read_byte(0x7000000 + i) for i in range(64)), 'OAM offset mismatch'
    rawio[f] = ctypes.string_at(mmu + 0x100c000, 0x1000)   # raw ARM9 I/O as written (write-only regs too: scroll, toon table 0x380, edge colours 0x330, clear colour 0x350)
    vids[f] = ctypes.string_at(mmu + 0x2014000, 0x800) + ctypes.string_at(mmu + 0x2014800, 0xA4000) + oam + \
        b''.join(m.read_short(0x4001000 + o).to_bytes(2, 'little') for o in range(0, 0x70, 2)) + \
        m.read_short(0x4000304).to_bytes(2, 'little')
    t1 = tail()
    if t1 >= t0:
        sl = [slice(t0, t1)]
    else:
        sl = [slice(t0, N), slice(0, t1)]   # ring wrapped
    for s in sl:
        out_c.append(cmds[s].copy()); out_p.append(params[s].copy()); out_f.append(np.full(s.stop - s.start, f, '<u4'))
    t0 = t1
rf = sorted(regs)
np.savez_compressed(a.out, frame=np.concatenate(out_f), cmd=np.concatenate(out_c), param=np.concatenate(out_p),
                    reg_frames=np.array(rf, '<u4'), regs=np.frombuffer(b''.join(regs[k] for k in rf), 'u1').reshape(len(rf), -1),
                    # per frame: pal 0x800 | VRAM A..I 0xA4000 | OAM 0x800 | engine B regs 0x70 | POWCNT1 2
                    rawio=np.frombuffer(b''.join(rawio[k] for k in rf), 'u1').reshape(len(rf), -1),
                    video=np.frombuffer(b''.join(vids[k] for k in rf), 'u1').reshape(len(rf), -1))
n = sum(len(x) for x in out_c)
print('captured', n, 'entries over frames', a.f_from, '..', a.f_to)
