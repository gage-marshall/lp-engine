#!/usr/bin/env python3
"""Rank unnamed static functions by leverage: calls from static code + calls from overlays (weighted by distinct overlays).
Usage: rank_funcs.py [N]   -> fid/rank.txt  (addr size score static_calls ov_calls ov_callers name)"""
import struct, bisect, csv, collections, sys
d = open('work/arm9_main.bin', 'rb').read(); B = 0x2000000
fn = sorted((int(l.split()[0], 16), int(l.split()[1]), l.split()[2]) for l in open('fid/funcs_final.txt'))
fn = [f for f in fn if 0x2000800 <= f[0] < 0x20fc000]
starts = [f[0] for f in fn]; fset = set(starts)
scall = collections.Counter(); sfrom = collections.defaultdict(set)
def tgt(w, pc): return pc + 8 + (((w & 0xFFFFFF) ^ 0x800000) - 0x800000) * 4
for a, sz, nm in fn:
    for x in range(a, min(a + sz, B + len(d) - 3), 4):
        w = struct.unpack_from('<I', d, x - B)[0]
        if (w >> 24) in (0xEB, 0xEA):
            t = tgt(w, x)
            if t in fset and not (a <= t < a + sz): scall[t] += 1; sfrom[t].add(a)
ocall = collections.Counter(); ovs = collections.defaultdict(set)
for r in csv.DictReader(open('work/overlays/arm9_overlays.csv')):
    o = open('work/overlays/' + r['file'], 'rb').read(); base = int(r['ram'], 16)
    for i in range(0, len(o) - 3, 4):
        w = struct.unpack_from('<I', o, i)[0]
        if (w >> 24) in (0xEB, 0xEA):
            t = tgt(w, base + i)
            if t in fset: ocall[t] += 1; ovs[t].add(int(r['id']))
rows = []
for a, sz, nm in fn:
    if not (nm.startswith(('FUN_', 'thunk_FUN', 'func_')) or nm.endswith('_%07x' % a) or nm.endswith('_%x' % a)): continue      # unnamed or auto-named only
    sc, oc, nv = scall[a], ocall[a], len(ovs[a])
    score = sc + oc + 3 * nv
    rows.append((score, a, sz, sc, oc, nv, nm))
rows.sort(reverse=True)
with open('fid/rank2.txt', 'w') as f:
    for score, a, sz, sc, oc, nv, nm in rows: f.write(f'{a:08x} {sz} {score} {sc} {oc} {nv} {nm}\n')
N = int(sys.argv[1]) if len(sys.argv) > 1 else 40
print('rank  addr      size score static_calls ov_calls ov_callers')
for k, (score, a, sz, sc, oc, nv, nm) in enumerate(rows[:N]):
    print(f'{k+1:3d}  {a:#x} {sz:5d} {score:5d} {sc:5d} {oc:5d} {nv:3d}')
tot = sum(r[0] for r in rows); top = sum(r[0] for r in rows[:100]); top300 = sum(r[0] for r in rows[:300])
print(f'unnamed functions ranked: {len(rows)}; total score {tot}; top100 hold {top*100//tot}%, top300 hold {top300*100//tot}%')
