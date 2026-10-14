"""Headless: from a state, tap through until the game starts a script, substitute FORCE_ID (as play_monitor --force),
keep tapping text, save shots and states.
    .venv/bin/python sandbox/ui/force_scene.py STATE FORCE_ID OUT_DIR [frames] [tap_every]"""
import os, sys
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
from emu_driver import Emu  # noqa: E402
from PIL import Image  # noqa: E402
SCRIPT_START, SESSION = 0x205e7c4, 0x2156898
state, fid, out = sys.argv[1], int(sys.argv[2], 16), sys.argv[3]
frames = int(sys.argv[4]) if len(sys.argv) > 4 else 3000
every = int(sys.argv[5]) if len(sys.argv) > 5 else 100
os.makedirs(out, exist_ok=True)
e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), state)
m = e.e.memory
st = {'forced': None, 'starts': []}


def on_start(a, s):
    r = m.register_arm9
    st['starts'].append((e.frames, '%08x' % (r.r0 & 0xffffffff)))
    if st['forced'] is None:
        st['forced'] = e.frames
        r.r0 = fid; r.r2 = 0xf
        top = fid >> 24
        slot = 0 if 0xa0 <= top < 0xb0 else 1 if 0x58 <= top < 0x68 else 2
        rd = m.unsigned.read_long
        w4 = (rd(SESSION + 4) & ~0xff00) | (5 << 8)
        m.write_long(SESSION + 4, (w4 & ~0xff) | slot)
        m.write_long(SESSION + 8, fid)
        m.write_long(SESSION + 0xc, (rd(SESSION + 0xc) & ~0xffff) | 0xf)
m.register_exec(SCRIPT_START, on_start)
shots = []
for i in range(frames):
    if i % every == every // 2:
        e.touch(128, 96)
    e.step()
    if st['forced'] is not None and i % 30 == 29:
        p = os.path.join(out, 's%05d.png' % e.frames); e.shot(p); shots.append(p)
        e.save(os.path.join(out, 's%05d.dst' % e.frames))
print('starts', st['starts'][:6], 'forced at', st['forced'])
ims = [Image.open(p).convert('RGB').crop((0, 192, 256, 384)).rotate(90, expand=True).resize((96, 128)) for p in shots]
if ims:
    sheet = Image.new('RGB', (96 * 12, 128 * ((len(ims) + 11) // 12)))
    for k, im in enumerate(ims): sheet.paste(im, ((k % 12) * 96, (k // 12) * 128))
    sheet.save(os.path.join(out, 'sheet.png'))
print('shots', len(shots))
