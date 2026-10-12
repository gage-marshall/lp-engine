#!/usr/bin/env python3
"""sem_merge.py: merge agent-written semantic names (fid/sem/*.csv: module,addr,name,conf,note) into fid/auto/sem.txt.
Overlay names get the OvNN<Scene>_ prefix added; collisions get _2.. suffix. Output lines use the ApplySymbols format
(src column 'sem' => always renames).  Then run fid/apply_sem.sh and tools/export_labels.py."""
import csv, glob, os, re, collections, sys
F = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fid')
ovn = {int(r['id']): r['name'] for r in csv.DictReader(open(f'{F}/overlay_names.csv'))}
lab = {(r['module'], r['addr'].zfill(8)): r for r in csv.DictReader(open(f'{F}/labels_all.csv'))}
size = {}
for fn in glob.glob(f'{F}/auto/*.txt'):
    b = os.path.basename(fn)
    if not (b == 'static.txt' or re.match(r'ov\d\d\.txt', b)): continue
    mod = b[:-4]
    for l in open(fn):
        p = l.split()
        if len(p) > 7: size[(mod, p[0].zfill(8))] = (p[2], p[7] if len(p) > 7 and p[7] == 'thumb' else '')
FILLER = set('Alpha Beta Gamma Delta Epsilon Zeta Eta Theta Iota Kappa Lambda Sigma Omega Cedar Linden Willow Yarrow Maple Birch Aspen Fern Moss Sage Clover Juniper Hazel Alder Rowan Thyme Poppy Olive Basil'.split())
def defiller(nm, a, conf, note):
    ws = re.findall(r'[A-Z][a-z0-9]*|[0-9]+|[a-z]+', nm)
    if not any(w in FILLER for w in ws): return nm, conf, note
    nm2 = re.sub('|'.join(sorted(FILLER, key=len, reverse=True)), '', nm).strip('_')
    nm2 = re.sub(r'_+', '_', nm2) or 'Helper'
    return f'{nm2}_{a[-5:]}', 'low', (note + ' [filler word removed; name is mechanical]')[:160]
rows = []; bad = 0
for fn in sorted(glob.glob(f'{F}/sem/*.csv')):
    for r in csv.DictReader(open(fn)):
        m, a = r['module'], r['addr'].zfill(8)
        nm = re.sub(r'[^A-Za-z0-9_]', '', r['name'].strip())
        if (m, a) not in lab or not nm: bad += 1; continue
        if lab[(m, a)]['tier'] == 'A' and lab[(m, a)]['source'] != 'sem': continue   # never override SDK/manual
        nm, cf, nt = defiller(nm, a, r['conf'], r['note'])
        rows.append((m, a, nm, cf, nt))
used = collections.Counter(); out = []; final = {}
seen = {}
for m, a, nm, conf, note in rows: seen[(m, a)] = (nm, conf, note)       # later rows win
for (m, a), (nm, conf, note) in sorted(seen.items()):
    if m.startswith('ov'):
        i = int(m[2:]); pre = f'Ov{i:02d}' + ovn.get(i, '').replace(' ', '')
        if not nm.startswith(pre): nm = f'{pre}_{nm}'
    used[nm] += 1
    if used[nm] > 1: nm = f'{nm}_{used[nm]}'
    final[(m, a)] = (nm, conf, note)
with open(f'{F}/auto/sem.txt', 'w') as f:
    for (m, a), (nm, conf, note) in final.items():
        sz, th = size.get((m, a), ('4', ''))
        f.write(f'{a} {nm} {sz} sem game F {m}' + (f' {th}' if th else '') + '\n')
with open(f'{F}/sem_final.csv', 'w', newline='') as f:
    w = csv.writer(f); w.writerow(['module', 'addr', 'name', 'conf', 'note'])
    for (m, a), v in final.items(): w.writerow([m, a, *v])
print('sem labels', len(final), 'rejected', bad)
