"""Python side of the Rust heroine-assembly differential test (engine/src/bin/heroine_dump.rs prints the same text).
usage: heroine_dump.py ARC_ROOT FACE_TABLES.json"""
import hashlib, json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import heroine

def f7(x):
    s = '%.7f' % x
    return '0.0000000' if s == '-0.0000000' else s

def H(lines):
    return hashlib.sha1('\n'.join(lines).encode()).hexdigest()[:16]

def configs(motions):
    n = len(motions)
    return [
        dict(),
        dict(outfit=17, mood=7, viseme=2, face=True),
        dict(outfit=3, hair=1, mood=12, blink=True, face=True),
        dict(motion=motions[0], frame=5, mood=3, face=True),
        dict(motion=motions[n // 3], frame=40, hair_color=2, outfit_variant=1, state=1, mood=20, viseme=4, eye_column=2, face=True),
        dict(motion=motions[n // 2], frame=12, ribbon=3, outfit_color=1, state=2, outfit=5, hair=2, face=True),
        dict(motion=motions[(2 * n) // 3], frame=90, outfit=8, mood=30, viseme=0, face=True),
    ]

def main(arc, ftp):
    heroine.A = arc
    tables = json.load(open(ftp))
    out = []
    for prefix in 'mrn':
        w = heroine.Wardrobe(prefix)
        motions = sorted(w.motions)
        out.append('W %s parts=%d motions=%d sd=%d pl=%d vt=%d' % (prefix, len(w.parts), len(motions), len(w.sd) if hasattr(w, 'sd') else -1, len(w.pl) if hasattr(w, 'pl') else -1, len(w.vt) if hasattr(w, 'vt') else -1))
        w._scan()
        out[-1] = 'W %s parts=%d motions=%d sd=%d pl=%d vt=%d tex=%d' % (prefix, len(w.parts), len(motions), len(w.sd), len(w.pl), len(w.vt), len(w.tex))
        for i, c in enumerate(configs(motions)):
            c = dict(c); face = c.pop('face', False)
            try:
                tris, mats, toon, edge = w.compose(tables=tables if face else None, **c)
            except Exception as e:
                out.append(' C%d ERR %s' % (i, type(e).__name__)); continue
            tl = []
            for t in tris:
                tl.append(str(t['material']))
                for pos, nrm, uv, col, joints in t['verts']:
                    tl.append(','.join(f7(x) for x in list(pos) + list(nrm) + list(uv) + list(col)))
            ml = ['%s|%s|%s' % (m.get('tex'), m.get('pal'), hashlib.sha1(m['image'].tobytes()).hexdigest()[:12] if m.get('image') is not None else 'None') for m in mats]
            out.append(' C%d tris=%d %s mats=%d %s toon=%s edge=%s' % (i, len(tris), H(tl), len(mats), H(ml), H([str(x) for x in toon]), H([str(x) for x in edge])))
    sys.stdout.write('\n'.join(out) + '\n')

main(sys.argv[1], sys.argv[2])
