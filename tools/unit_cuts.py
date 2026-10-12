#!/usr/bin/env python3
"""Detect translation-unit boundaries in the static module from data-layout monotonicity.
Link order => each object's code and its .bss/.data/.rodata are contiguous. A cut between function i and i+1 is valid if,
for every data class, max(refs of funcs<=i) <= min(refs of funcs>i) (ignoring 'hub' data referenced from widely spread code).
Usage: unit_cuts.py [--eval]  -> fid/unit_cuts.txt"""
import struct, bisect, csv, collections, sys
d = open('work/arm9_main.bin', 'rb').read(); B = 0x2000000
RO = (0x20fc000, 0x2116680); DA = (0x2116680, 0x212bd40); BS = (0x212bd40, 0x2158280)
CLASSES = {'ro': RO, 'da': DA, 'bs': BS}
fn = sorted((int(l.split()[0], 16), int(l.split()[1]), l.split()[2]) for l in open('fid/funcs.txt'))
fn = [f for f in fn if 0x2000800 <= f[0] < 0x20fc000]
n = len(fn)
refs = []                      # per function: {cls: [addrs]}
fend = [fn[i + 1][0] if i + 1 < n else 0x20fc000 for i in range(n)]
for (a, sz, nm), e in zip(fn, fend):
    r = collections.defaultdict(list)
    for x in range(a, min(e, B + len(d) - 3), 4):     # literal pools follow the body, so scan to the next function
        w = struct.unpack_from('<I', d, x - B)[0]
        for c, (lo, hi) in CLASSES.items():
            if lo <= w < hi: r[c].append(w)
    refs.append(r)
# hub data: address referenced by functions spread widely
spread = collections.defaultdict(list)
for i, r in enumerate(refs):
    for c, l in r.items():
        for w in set(l): spread[w].append(fn[i][0])
hub = {w for w, l in spread.items() if max(l) - min(l) > 0x6000}
# tolerant test: inversions between the last W refs-bearing functions before the cut and the next W after
W = 14
def class_refs(c):
    return [[w for w in refs[i].get(c, []) if w not in hub] for i in range(n)]
CR = {c: class_refs(c) for c in CLASSES}
def inv_frac(c, i):
    L = CR[c]
    left = []; j = i - 1
    while j >= 0 and len(left) < W:
        if L[j]: left.append(min(L[j]) if len(L[j]) == 1 else sorted(L[j])[len(L[j]) // 2])
        j -= 1
    right = []; j = i
    while j < n and len(right) < W:
        if L[j]: right.append(min(L[j]) if len(L[j]) == 1 else sorted(L[j])[len(L[j]) // 2])
        j += 1
    if len(left) < 3 or len(right) < 3: return None
    inv = sum(1 for x in left for y in right if x > y)
    return inv / (len(left) * len(right)), len(left) + len(right)
THR = float(sys.argv[sys.argv.index('--thr') + 1]) if '--thr' in sys.argv else 0.03
cuts = []
for i in range(1, n):
    ok = True; info = 0
    for c in CLASSES:
        r = inv_frac(c, i)
        if r is None: continue
        info += 1
        if r[0] > THR: ok = False; break
    if ok and info: cuts.append(i)
# collapse runs of consecutive cut indices (no refs between) into the earliest
units = []; start = 0
for i in cuts:
    units.append((start, i)); start = i
units.append((start, n))
# merge tiny units (<1 function with no data) is left to caller
with open('fid/unit_cuts.txt', 'w') as f:
    for a, b in units:
        f.write(f'{fn[a][0]:#x} {(fn[b][0] if b < n else 0x20fc000):#x} {b - a}\n')
print('functions', n, 'hub data addrs', len(hub), 'cuts', len(cuts), 'units', len(units))
if '--eval' in sys.argv:
    # ground truth: object starts from the matcher
    objs = collections.defaultdict(list)
    for r in csv.DictReader(open('fid/v7.csv')):
        if r['kind'] in ('unique', 'resolved', 'gapfill', 'fuzzy') and r['section'] == '.text' and r['region'] == 'main' and r['addr']:
            a = int(r['addr'].split(';')[0], 16)
            if 0x2013000 <= a < 0x202c300 or 0x20020fc <= a < 0x20136a0: objs[(r['lib'], r['obj'])].append((a, int(r['size'])))
    truth = sorted((min(a for a, s in v), max(a + s for a, s in v)) for v in objs.values() if len(v) >= 1)
    tstarts = [t[0] for t in truth]
    fstarts = [f[0] for f in fn]
    cutaddr = {fn[i][0] for i in cuts}
    hit = 0; tot = 0
    for ts, te in truth[1:]:
        tot += 1
        # nearest function start at/after ts
        k = bisect.bisect_left(fstarts, ts)
        if any(fn[j][0] in cutaddr for j in range(max(0, k - 1), min(n, k + 2))): hit += 1
    inside = 0; incnt = 0
    for i in cuts:
        a = fn[i][0]
        if not (0x2013000 <= a < 0x202c300 or 0x20020fc <= a < 0x20136a0): continue
        incnt += 1
        k = bisect.bisect_right(tstarts, a) - 1
        # a cut is a false positive if it falls strictly inside an object's extent (not near its start)
        if k >= 0 and truth[k][0] + 4 < a < truth[k][1] - 4:
            inside += 1
    print(f'EVAL objects: {tot}, boundaries recovered: {hit} ({hit*100//tot}%); cuts in labeled area: {incnt}, strictly inside a known object: {inside}')
