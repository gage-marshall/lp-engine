#!/usr/bin/env python3
"""make_chunks.py: split fid/bulk/<mod>.cond.txt into agent work chunks fid/bulk/chunks/<mod>_NN.txt (<=~70KB, <=120 funcs)."""
import glob, os, re
B = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fid', 'bulk')
n = 0
for fn in sorted(glob.glob(f'{B}/*.cond.txt')):
    mod = os.path.basename(fn).split('.')[0]
    blocks = [b for b in open(fn).read().split('\n//--\n') if b.strip()]
    cur, size, k = [], 0, 0
    def flush():
        global cur, size, k, n
        if cur:
            open(f'{B}/chunks/{mod}_{k:02d}.txt', 'w').write('\n//--\n'.join(cur)); k += 1; n += 1; cur, size = [], 0
    for b in blocks:
        if cur and (size + len(b) > 70000 or len(cur) >= 120): flush()
        cur.append(b); size += len(b)
    flush()
print(n, 'chunks')
