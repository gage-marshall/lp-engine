"""Compare the Rust wgpu viewer against the reference numpy rasteriser (tools/toonrender.py) on the same assembled heroine.
usage: viewer_parity.py CONTENT_DIR VIEWER_BIN [--heroine m] [--outfit 17] [--hair 0] [--motion-index N --frame F] [--size 512]
Writes export/viewer/parity_<heroine>.png (reference | viewer | difference) and prints metrics; exit code 1 if too different."""
import json, os, subprocess, sys, math
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from PIL import Image
import heroine, toonrender

def main():
    a = sys.argv[1:]
    content, binary = a[0], a[1]
    opt = {k: a[i + 1] for i, k in enumerate(a) if k.startswith('--')}
    prefix = opt.get('--heroine', 'm'); outfit = int(opt.get('--outfit', 17)); hair = int(opt.get('--hair', 0)); size = int(opt.get('--size', 512))
    frame = int(opt.get('--frame', 0)); mi = opt.get('--motion-index')
    heroine.A = os.path.join(content, 'assets/arc')
    w = heroine.Wardrobe(prefix); tables = json.load(open(os.path.join(content, 'engine/face_tables.json')))
    motion = sorted(w.motions)[int(mi)] if mi is not None else None
    tris, mats, toon, edge = w.compose(outfit=outfit, hair=hair, motion=motion, frame=frame, mood=0, tables=tables)
    presets = json.load(open('export/camera/presets.json'))['presets']['1000']['lights']
    lights = [(l['yaw'], l['pitch']) for l in presets]
    bind = w.compose(outfit=outfit, hair=hair, motion=None, frame=0, tables=None)[0]
    center = np.array([v[0] for t in bind for v in t['verts']]).mean(axis=0)
    ref = toonrender.render(tris, mats, w.tex, toon, edge, lights, size=size, yaw=math.pi, pitch=0.0, center=center, ss=2)
    out = 'export/viewer/_parity_rust.png'; os.makedirs('export/viewer', exist_ok=True)
    cmd = [binary, content, '--heroine', prefix, '--outfit', str(outfit), '--hair', str(hair), '--size', str(size), '--ss', '2', '--png', out, '--frame', str(frame)]
    if motion: cmd += ['--motion', str(sorted(w.motions).index(motion))]
    subprocess.run(cmd, check=True, capture_output=True)
    rs = np.asarray(Image.open(out).convert('RGB')).astype(int); rf = np.asarray(ref.convert('RGB')).astype(int)
    # the viewer fits ext = max half extent of the bind pose; the reference does the same from the posed mesh -> compare after cropping to the figure bbox
    def bbox(im):
        bg = np.array([46, 46, 58]); m = (np.abs(im - bg).sum(axis=2) > 12); ys, xs = np.where(m); return xs.min(), xs.max(), ys.min(), ys.max()
    b1, b2 = bbox(rs), bbox(rf)
    diff = np.abs(rs - rf); mean = diff.mean(); bad = (diff.max(axis=2) > 40).mean()
    fg = (np.abs(rf - np.array([46, 46, 58])).sum(axis=2) > 12) | (np.abs(rs - np.array([46, 46, 58])).sum(axis=2) > 12)
    iou = ((np.abs(rs - np.array([46, 46, 58])).sum(axis=2) > 12) & (np.abs(rf - np.array([46, 46, 58])).sum(axis=2) > 12)).sum() / max(1, fg.sum())
    side = np.concatenate([rf, rs, np.clip(diff * 3, 0, 255)], axis=1).astype(np.uint8)
    Image.fromarray(side).save('export/viewer/parity_%s.png' % prefix)
    print('heroine %s: mean abs diff %.2f/255, pixels differing >40: %.2f%%, silhouette IoU %.3f, bbox ref %s viewer %s' % (prefix, mean, bad * 100, iou, b2, b1))
    sys.exit(0 if (mean < 6 and iou > 0.9) else 1)
main()
