"""Rebuild every transform of a captured polygon list from files + scene state, substitute them into the captured GX
stream, and re-render.  Proves (or refutes) the scene -> transform pipeline documented in docs/RENDER_PRIMITIVES.md.

    .venv/bin/python sandbox/gx/rebuild.py CAPTURE.npz SWAP_FRAME REF.npy OUT.png --preset 3014 --heroine 2 \
        --bca RAMDUMP_BCA.bin --anim-frame 3

Sources of each substituted value (nothing is taken from the capture):
  projection  perspective from Camera_Reset constants (half-angle sin 706 / cos 4035, aspect 0x1555, near 1, far 400,
              W-scale 10)
  look-at     preset record eye, target (z forced 0), up (-1, 0, 0)
  base        RotY(preset word 7) (camera +0xb4) in NNS RotY convention, translation (0, heroine height table[heroine], 0)
              (Gfx3D_DrawActor table @0x2102e2c: 0, -21488, 8380); accessories: Head-joint world matrix (computed here)
              times the block3d.arc/0202 offset for (accessory id % 100, heroine)
  joints      BCA sample of the part's animation at the animation frame; inverse-bind (envelope) matrices from the model
The op -> (part, node) assignment is taken by matching the captured op to the candidate set; the substituted *values*
come only from the sources above.  A wrong source shows up as a mismatch either in assignment error or in the render.
"""
import argparse, glob, os, struct, sys
import numpy as np
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'tools'))
import nns3d  # noqa: E402
from gx import Geo, packets, fx, s32  # noqa: E402
from render import render, vram_maps, to_rgb8, to6, rgb555  # noqa: E402

ap = argparse.ArgumentParser()
ap.add_argument('capture'); ap.add_argument('swap', type=int); ap.add_argument('ref'); ap.add_argument('out')
ap.add_argument('--preset', type=int, default=3014); ap.add_argument('--heroine', type=int, default=2)
ap.add_argument('--bca', required=True, help='BCA0 bytes of the motion file (dumped from RAM)')
ap.add_argument('--anim-frame', type=int, default=3)
ap.add_argument('--accessory', type=int, default=405)
a = ap.parse_args()
A = os.path.join(ROOT, 'content', 'assets', 'arc')

# ---- scene inputs -> global transforms
rec = None
for f in glob.glob(os.path.join(A, 'camera_data_*.bin', '*')):
    b = open(f, 'rb').read()
    if len(b) == 164 and struct.unpack_from('<i', b)[0] == a.preset:
        rec = struct.unpack('<41i', b)
eye = np.array(rec[1:4]) / 4096; tgt = np.array([rec[4] / 4096, rec[5] / 4096, 0.0]); up = np.array([-1.0, 0, 0])
zc = eye - tgt; zc /= np.linalg.norm(zc); xc = np.cross(up, zc); xc /= np.linalg.norm(xc); yc = np.cross(zc, xc)
LOOK = np.zeros((4, 3)); LOOK[:3, 0], LOOK[:3, 1], LOOK[:3, 2] = xc, yc, zc; LOOK[3] = [-xc @ eye, -yc @ eye, -zc @ eye]
s, c = 706 / 4096, 4035 / 4096; aspect = 0x1555 / 4096; near, far, W = 1.0, 400.0, 10.0
PERSP = np.zeros((4, 4)); PERSP[0, 0] = c / s / aspect * W; PERSP[1, 1] = c / s * W
PERSP[2, 2] = -(far + near) / (far - near) * W; PERSP[2, 3] = -W; PERSP[3, 2] = -2 * far * near / (far - near) * W
ang = (rec[7] & 0xffff) / 65536 * 2 * np.pi
ROTY = np.array([[np.cos(ang), 0, -np.sin(ang)], [0, 1, 0], [np.sin(ang), 0, np.cos(ang)]])
HEIGHT = [0, -21488, 8380][a.heroine] / 4096
acc = open(os.path.join(A, 'block3d.arc', '0202.bin'), 'rb').read()
ACC_OFS = None
for k in range(struct.unpack_from('<i', acc)[0]):
    rid = struct.unpack_from('<i', acc, 4 + 40 * k)[0]
    if rid == a.accessory % 100:
        ACC_OFS = np.array(struct.unpack_from('<3f', acc, 4 + 40 * k + 4 + 12 * a.heroine))

# ---- candidate local matrices from files
bca = open(a.bca, 'rb').read()
anims = {x['name']: x for x in nns3d.parse_bca(bca)}
MODELS = {'hair': 'n_block3d_00.arc/0071', 'body': 'n_block3d_01.arc/0141', 'rarm': 'n_block3d_01.arc/0144', 'larm': 'n_block3d_01.arc/0143',
          'waist': 'n_block3d_01.arc/0146', 'leg': 'n_block3d_01.arc/0148', 'rhand': 'tm_model_block_base.arc/0082',
          'lhand': 'tm_model_block_base.arc/0080', 'cap': 'block3d.arc/0106'}
cands = []
body_pose = None
for part, path in MODELS.items():
    dd = open(os.path.join(A, path + '.bin'), 'rb').read(); m = nns3d.parse_file(dd)['models'][0]; nodes = m['nodes']
    for nm, src in (('rest', None),) + tuple((k, v) for k, v in anims.items()):
        S = nodes if src is None else nns3d.sample_anim(src, nodes, a.anim_frame)
        if part == 'body' and nm.endswith('_body'):
            body_pose = {n['name']: n for n in S}
        for n in S:
            M = np.eye(4); M[:3, :3] = np.array(n['rot'], float); M[3, :3] = n['trans']
            cands.append(('local', part, nm, n['name'], M))
    for i, (R, t) in enumerate(nns3d.read_evp(dd, m) or []):
        M = np.eye(4); M[:3, :3] = np.array(R).T; M[3, :3] = t
        cands.append(('invbind', part, 'evp', nodes[i]['name'], M))


def to_fx(M43):
    return [int(round(v * 4096)) & 0xffffffff for v in M43.reshape(-1)]


# Head world matrix from our own pipeline: base x Hips x Spine x Spine1 x Spine2 x Neck x Head (body animation)
BASE = np.eye(4); BASE[:3, :3] = ROTY; BASE[3, :3] = [0, HEIGHT, 0]
HEADW = BASE.copy()
for nm in ('Hips', 'Spine', 'Spine1', 'Spine2', 'Neck', 'Head'):
    n = body_pose[nm]; M = np.eye(4); M[:3, :3] = np.array(n['rot'], float); M[3, :3] = n['trans']
    HEADW = M @ HEADW

# ---- substitute
z = np.load(a.capture)
pk = packets(z['cmd'], z['param'], z['frame'])
sw = [i for i, p in enumerate(pk) if p[0] == 0x50]
end = next(sw[i] for i in range(1, len(sw)) if pk[sw[i]][2] == a.swap); start = sw[sw.index(end) - 1] + 1
new = list(pk); mode = None; report = {}; worst = 0.0
for j in range(start, end):
    c, ps, fr, idx = pk[j]
    if c == 0x10:
        mode = ps[0] & 3
    if mode == 0 and c == 0x16 and abs(s32(ps[0]) - 175584) < 400:          # perspective (42.87 * 4096)
        new[j] = (c, [int(round(v * 4096)) & 0xffffffff for v in PERSP.reshape(-1)], fr, idx); report['projection'] = report.get('projection', 0) + 1
        worst = max(worst, np.abs(fx(ps).reshape(4, 4) - PERSP).max())
    elif mode == 0 and c == 0x19:                                           # camera into projection
        new[j] = (c, to_fx(LOOK), fr, idx); report['look-at'] = report.get('look-at', 0) + 1
    elif mode in (1, 2) and c == 0x19:
        L = np.eye(4); L[:, :3] = fx(ps).reshape(4, 3)
        if s32(ps[0]) == 201 and s32(ps[4]) == 4096:                       # base
            B = np.eye(4); B[:3, :3] = ROTY; B[3, :3] = [0, HEIGHT, 0] if abs(fx(ps)[10] - HEIGHT) < 1 else [0, 0, 0]
            if abs(fx(ps)[10] - HEIGHT) >= 1:
                B[3, :3] = ACC_OFS                                          # accessory base (overwritten by the Head load)
            new[j] = (c, to_fx(B[:, :3]), fr, idx); report['base'] = report.get('base', 0) + 1
            continue
        if np.allclose(L[:3, :3], np.eye(3), atol=1e-3) and np.abs(L[3, :3] - ACC_OFS).max() < 0.01:
            M = np.eye(4); M[3, :3] = ACC_OFS
            new[j] = (c, to_fx(M[:, :3]), fr, idx); report['accessory offset'] = report.get('accessory offset', 0) + 1
            continue
        best = min(cands, key=lambda q: np.abs(q[4][:, :3] - L[:, :3]).max())
        err = np.abs(best[4][:, :3] - L[:, :3]).max(); worst = max(worst, err)
        new[j] = (c, to_fx(best[4][:, :3]), fr, idx); key = best[0] + (' ' + best[2] if best[0] == 'local' and best[2] != 'rest' else ' ' + best[1])
        report[key] = report.get(key, 0) + 1
    elif mode in (1, 2) and c == 0x17 and abs(fx(ps)[10] - HEADW[3, 1]) < 2:   # accessory: Head world matrix load
        new[j] = (c, to_fx(HEADW[:, :3]), fr, idx); report['accessory head load'] = report.get('accessory head load', 0) + 1
        worst = max(worst, np.abs(fx(ps).reshape(4, 3) - HEADW[:, :3]).max())
for k, v in sorted(report.items(), key=lambda x: -x[1]):
    print('%4d substituted: %s' % (v, k))
print('largest |captured - rebuilt| over substituted values: %.4f' % worst)

# ---- render both and compare with the emulator
F = list(z['reg_frames']); i = F.index(a.swap + 1)
io = z['rawio'][i]; vid = z['video'][i]
h = lambda o: int.from_bytes(bytes(io[o:o + 2]), 'little')
tex, pal = vram_maps(vid[0x800:0x800 + 0xa4000], bytes(io[0x240:0x24a]))
toon = [h(0x380 + 2 * k) for k in range(32)]; edge = [h(0x330 + 2 * k) for k in range(8)]
ref = np.load(a.ref); refrgb = (to6(rgb555(ref & 0x7fff)) * 255 // 63).astype(np.uint8); refm = (ref & 0x7fff) != 0
imgs = []
for label, stream in (('captured transforms', pk), ('rebuilt transforms', new)):
    g = Geo(); g.run(stream)
    polys = {fr: p for fr, _, p in g.lists}[a.swap]
    col, alpha, _, _ = render(polys, tex, pal, toon, edge, h(0x60))
    img = to_rgb8(col); img[alpha == 0] = 0; m = alpha > 0
    d = np.abs(img.astype(int) - refrgb.astype(int)).max(-1)
    print('%-20s IoU %.4f  median diff %d  exact %.1f%%' % (label, (m & refm).sum() / (m | refm).sum(), np.median(d[m & refm]), 100 * (d[m & refm] == 0).mean()))
    imgs.append(img)
dd = np.abs(imgs[0].astype(int) - imgs[1].astype(int)).max(-1)
print('captured-vs-rebuilt render: pixels differing %d, max diff %d' % ((dd > 0).sum(), dd.max()))
panels = [Image.fromarray(x).rotate(90, expand=True) for x in (refrgb, imgs[1], np.stack([np.clip(dd * 8, 0, 255).astype(np.uint8)] * 3, -1))]
out = Image.new('RGB', (sum(p.width for p in panels) + 20 * (len(panels) - 1), panels[0].height), (40, 40, 40))
for k, p in enumerate(panels):
    out.paste(p, (k * (p.width + 20), 0))
out.resize((out.width * 2, out.height * 2), Image.NEAREST).save(a.out)
print('wrote', a.out)
