"""Render one captured polygon list with render.py and compare it with an emulator 3D-only reference.

    .venv/bin/python sandbox/gx/frame.py CAPTURE.npz SWAP_FRAME REF.npy OUT.png [--cull-cw]

REF.npy: 192x256 u16 BG0-only capture (see docs/RENDER_PRIMITIVES.md).  The render uses the VRAM mapping and 3D
registers of the frame after the swap (the frame it is drawn in).
"""
import argparse, os, sys
import numpy as np
from PIL import Image
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gx import Geo, packets  # noqa: E402
from render import render, vram_maps, to_rgb8, to6, rgb555  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument('capture'); ap.add_argument('swap', type=int); ap.add_argument('ref'); ap.add_argument('out')
ap.add_argument('--cull-cw', action='store_true')
a = ap.parse_args()
z = np.load(a.capture)
g = Geo(); g.run(packets(z['cmd'], z['param'], z['frame']))
polys = {fr: p for fr, _, p in g.lists}[a.swap]
F = list(z['reg_frames']); i = F.index(a.swap + 1)
io = z['rawio'][i]; vid = z['video'][i]
h = lambda o: int.from_bytes(bytes(io[o:o + 2]), 'little')
vramcnt = bytes(io[0x240:0x24a])
tex, pal = vram_maps(vid[0x800:0x800 + 0xa4000], vramcnt)
toon = [h(0x380 + 2 * k) for k in range(32)]; edge = [h(0x330 + 2 * k) for k in range(8)]
disp3d = h(0x60)
color, alpha, depth, pid = render(polys, tex, pal, toon, edge, disp3d, front_ccw=not a.cull_cw)
mine = to_rgb8(color); mine[alpha == 0] = 0

ref = np.load(a.ref)
refrgb = (to6(rgb555(ref & 0x7fff)) * 255 // 63).astype(np.uint8)
refm = (ref & 0x7fff) != 0
mm = alpha > 0
both = mm & refm
diff = np.abs(mine.astype(int) - refrgb.astype(int))
print('coverage IoU %.4f' % ((mm & refm).sum() / (mm | refm).sum()))
print('on shared pixels: mean abs diff %.2f (0-255 scale), median %.0f, 95th pct %.0f, exact %.1f%%' % (
    diff[both].mean(), np.median(diff[both].max(-1)), np.percentile(diff[both].max(-1), 95), 100 * (diff[both].max(-1) == 0).mean()))
heat = np.clip(diff.max(-1) * 4, 0, 255).astype(np.uint8)
row = np.concatenate([refrgb, mine, np.stack([heat] * 3, -1)], 1)
Image.fromarray(row).resize((row.shape[1] * 2, row.shape[0] * 2), Image.NEAREST).save(a.out)
print('wrote', a.out)
