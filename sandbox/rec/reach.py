"""Drive the game headlessly from a save state to the start of a target script, tapping to advance text.
Saves a state at the script's start so recordings can begin from a fixed point.

    .venv/bin/python sandbox/rec/reach.py --state quick-f42391 --target 08143402 --out sandbox/rec/states/s08143402.dst < /dev/null
"""
import argparse, os, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from emu_driver import Emu  # noqa: E402

SCRIPT_START = 0x205e7c4   # ScriptMgr_Start(id, ...)
STATES = os.path.join(ROOT, 'tools', 'emu_scripts', 'states')

ap = argparse.ArgumentParser()
ap.add_argument('--state', required=True)
ap.add_argument('--target', required=True)
ap.add_argument('--out', required=True)
ap.add_argument('--max-frames', type=int, default=20000)
ap.add_argument('--tap-every', type=int, default=24)
ap.add_argument('--shots', default=None, help='directory for a screenshot every 600 frames')
a = ap.parse_args()

st = a.state if os.path.exists(a.state) else os.path.join(STATES, a.state + '.dst')
emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), st)
target = int(a.target, 16)
hit = []

def on_start(addr, size):
    sid = emu.e.memory.register_arm9.r0
    print('f%d script_start %08x' % (emu.frames, sid), flush=True)
    if sid == target:
        hit.append(emu.frames)

emu.e.memory.register_exec(SCRIPT_START, on_start)
if a.shots: os.makedirs(a.shots, exist_ok=True)
while emu.frames < a.max_frames and not hit:
    if a.shots and emu.frames % 600 < a.tap_every and emu.frames % 600 == 0:
        emu.shot(os.path.join(a.shots, 'f%06d.png' % emu.frames), rotate=True)
    emu.touch(128, 96, hold=4, release=a.tap_every - 4)
if hit:
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    emu.save(a.out)
    print('reached %08x at frame %d -> %s' % (target, hit[0], a.out))
else:
    print('target not reached in', emu.frames, 'frames')
    sys.exit(1)
