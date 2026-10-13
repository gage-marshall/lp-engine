"""Compare two export trees: JSON by value, PNG by RGBA pixels.  usage: tree_diff.py REFERENCE_DIR CANDIDATE_DIR"""
import json, os, sys
from PIL import Image

def files(root):
    out = set()
    for r, _, fs in os.walk(root):
        for f in fs: out.add(os.path.relpath(os.path.join(r, f), root))
    return out

def main(a, b):
    fa, fb = files(a), files(b)
    only_a, only_b = sorted(fa - fb), sorted(fb - fa)
    same = diff = 0; bad = []
    for f in sorted(fa & fb):
        pa, pb = os.path.join(a, f), os.path.join(b, f)
        if f.endswith('.json'): eq = json.load(open(pa)) == json.load(open(pb))
        elif f.endswith('.png'):
            ia, ib = Image.open(pa).convert('RGBA'), Image.open(pb).convert('RGBA'); eq = ia.size == ib.size and ia.tobytes() == ib.tobytes()
        else: eq = open(pa, 'rb').read() == open(pb, 'rb').read()
        if eq: same += 1
        else: diff += 1; bad.append(f)
    print('identical %d, different %d, only in reference %d, only in candidate %d' % (same, diff, len(only_a), len(only_b)))
    for l in (bad[:8], only_a[:5], only_b[:5]): print(' ', l)
    return 0 if not (diff or only_a or only_b) else 1

if __name__ == '__main__': sys.exit(main(*sys.argv[1:3]))
