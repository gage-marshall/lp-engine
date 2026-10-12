#!/usr/bin/env python3
"""export_labels.py -> fid/labels_all.csv (module, addr, name, tier, source, confidence, note) + fid/semantic_map.csv"""
import csv, re, collections, os
F = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fid')
man = {r['addr'].zfill(8): r for r in csv.DictReader(open(os.path.join(F, 'manual_labels.csv'))) if r['type'] == 'F'}
modn = {r['key']: r for r in csv.DictReader(open(os.path.join(F, 'module_names.csv')))}
ovn = {int(r['id']): r for r in csv.DictReader(open(os.path.join(F, 'overlay_names.csv')))}
sem = {(r['module'], r['addr'].zfill(8)): r for r in csv.DictReader(open(os.path.join(F, 'sem_final.csv')))} if os.path.exists(os.path.join(F, 'sem_final.csv')) else {}
rows = []
for fn in ('auto/tiers_static.csv', 'auto/tiers_overlays.csv'):
    for r in csv.DictReader(open(os.path.join(F, fn))):
        m, a, name, tier, src = r['module'], r['addr'].zfill(8), r['name'], r['tier'], r['subsystem_source']
        conf = note = ''
        if m == 'static' and a in man:
            name, tier, src, conf, note = man[a]['name'], 'A', 'manual', man[a]['confidence'] if 'confidence' in man[a] else man[a]['conf'], man[a]['note']
        elif tier == 'A':
            n = name
            src = 'nns' if n.startswith(('NNS', 'NNSi')) else 'sdk' if re.match(r'^(OS|MI|FS|GX|G2|G3|CARD|SND|PXI|PM|RTC|SPI|TP|WM|WBT|MB|FX|MTX|VEC|MATH|STD|CTRDG|CHT|CP|DC|IC|FID)', n) else 'runtime/donor/other'
        if (m, a) in sem and tier != 'A' or (m, a) in sem and src == 'sem':
            s_ = sem[(m, a)]; name, tier, src, conf, note = s_['name'], 'A', 'sem', s_['conf'], s_['note']
        rows.append((m, a, name, tier, src, conf, note))
with open(os.path.join(F, 'labels_all.csv'), 'w', newline='') as f:
    w = csv.writer(f); w.writerow(['module', 'addr', 'name', 'tier', 'source', 'confidence', 'note']); w.writerows(rows)
# semantic map: module/overlay names with evidence
with open(os.path.join(F, 'semantic_map.csv'), 'w', newline='') as f:
    w = csv.writer(f); w.writerow(['kind', 'key', 'name', 'confidence', 'evidence'])
    for k, r in modn.items(): w.writerow(['module', k, r['name'], r['conf'], r['note']])
    for k, r in ovn.items(): w.writerow(['overlay', 'ov%02d' % k, r['name'], r['conf'], r['note']])
c = collections.Counter(r[3] for r in rows)
print('labels', len(rows), dict(c), '| modules named', len(modn), '| overlays named', len(ovn))
