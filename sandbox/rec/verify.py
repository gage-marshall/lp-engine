"""Check that a headless replay reproduces the live play: compare replay screenshots with the monitor's live shots at the same frames.

    .venv/bin/python sandbox/rec/verify.py SESSION.jsonl [--since EPOCH]

Live shots (states/logs/shots/*-f<frame>.png, frame = emulated frames since the monitor started) after the anchor frame are
compared pixel by pixel with the replay at the same frame.  Prints max/mean abs difference per shot.
"""
import argparse, glob, os, re, sys
import numpy as np
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools')); sys.path.insert(0, HERE)
from emu_driver import Emu  # noqa: E402
from record import load_session, apply_in  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument('session')
ap.add_argument('--since', type=float, default=None, help='only shots modified after this time (default: session file ctime)')
a = ap.parse_args()
anc, f0, init, ins, recs = load_session(a.session)
end = recs[-1]['f']
since = a.since or os.path.getctime(a.session) - 1
shots = {}
for p in glob.glob(os.path.join(ROOT, 'tools', 'emu_scripts', 'states', 'logs', 'shots', '*-f*.png')):
    m = re.search(r'-f(\d+)\.png$', p)
    if m and os.path.getmtime(p) >= since and f0 < int(m.group(1)) <= end:
        shots[int(m.group(1))] = p
emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), anc['path'])
emu.pin_rtc(*anc['rtc'])
apply_in(emu, init)
qi, f = 0, f0
worst = 0
for target in sorted(shots):
    while f < target:
        while qi < len(ins) and ins[qi]['f'] <= f:
            apply_in(emu, ins[qi]); qi += 1
        emu.step(1); f += 1
    im = emu.image()
    top = im.crop((0, 0, 256, 192)).rotate(90, expand=True); bot = im.crop((0, 192, 256, 384)).rotate(90, expand=True)
    page = Image.new('RGB', (384, 256)); page.paste(top, (0, 0)); page.paste(bot, (192, 0))
    live = np.asarray(Image.open(shots[target]).convert('RGB'), dtype=np.int16)
    rep = np.asarray(page, dtype=np.int16)
    d = np.abs(live - rep)
    worst = max(worst, int(d.max()))
    print('f%d %-40s max %3d mean %.3f' % (target, os.path.basename(shots[target]), d.max(), d.mean()), flush=True)
print('shots', len(shots), 'worst max diff', worst)
