#!/usr/bin/env python3
"""overlay_dossier.py [ids...]  - summary evidence for naming overlays (scenes)."""
import sys, os, re, csv, collections, struct
F = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fid')
W = os.path.join(F, '..', 'work', 'overlays')
final = {int(l.split()[0], 16): l.split()[2] for l in open(os.path.join(F, 'funcs_final.txt'))}
ovs = list(csv.DictReader(open(os.path.join(W, 'arm9_overlays.csv'))))
ids = [int(x) for x in sys.argv[1:]] or [int(r['id']) for r in ovs]
for r in ovs:
    i = int(r['id'])
    if i not in ids: continue
    d = open(os.path.join(W, r['file']), 'rb').read(); base = int(r['ram'], 16)
    rows = [l.rstrip('\n').split('\t') for l in open(os.path.join(F, 'features/ov%02d.tsv' % i))]
    th = collections.Counter(); nf = len(rows)
    for p in rows:
        for c in p[6].split(';'):
            if '@' not in c: continue
            nm, ad = c.rsplit('@', 1); ad = int(ad, 16)
            if ad < 0x2158280 or 0x1ff8000 <= ad < 0x2000000:
                n = final.get(ad, nm); th[re.split(r'_', n)[0]] += 1
    strs = []
    for m in re.finditer(rb'[\x20-\x7e]{6,}', d):
        s = m.group().decode()
        if re.search(r'[A-Za-z]{3}', s) and len(set(s)) > 4: strs.append(s)
    seen = []; 
    for s in strs:
        if s not in seen: seen.append(s)
    pathish = [s for s in seen if '/' in s or '.' in s][:8]
    print(f'ov{i:<2d} {r["ram"]} size={len(d):6d} funcs={nf:4d}  calls: ' + ' '.join(f'{k}:{v}' for k, v in th.most_common(8)))
    if pathish: print('      strings:', ' | '.join(s[:34] for s in pathish))
