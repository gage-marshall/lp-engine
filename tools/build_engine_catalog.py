#!/usr/bin/env python3
"""Build data/engine_tables.json: an index of the data tables the game code references (metadata only, no ROM bytes).

For every module (static, itcm, dtcm-free, ov00..ov43) it
  1. takes function ranges from fid/features/*.tsv (analysis output),
  2. scans aligned words in the code area for pointers into data (literal pools),
  3. clusters the referenced data addresses into table regions and records which functions reference each region.
Output is committed; the extractor ships it next to the dumped images as engine/tables.json.
"""
import csv, json, os, struct, collections, bisect, sys
ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..')
os.chdir(ROOT)

LABELS = {}
for _r in csv.DictReader(open('fid/labels_all.csv')):
    LABELS[(_r['module'], int(_r['addr'], 16))] = _r['name']

def load_funcs(tsv):
    out = []
    for l in open(tsv):
        c = l.rstrip('\n').split('\t')
        if len(c) > 2 and c[1].isdigit():
            try: out.append((int(c[0], 16), int(c[1]), LABELS.get((os.path.basename(tsv)[:-4].replace('itcm','static'), int(c[0], 16)), c[2])))
            except ValueError: pass
    out.sort(); return out

modules = {}   # name -> dict(base, data(bytes), funcs)
stat = open('work/arm9_dec.bin', 'rb').read()
modules['static'] = dict(base=0x2000000, data=stat, funcs=load_funcs('fid/features/static.tsv'))
modules['itcm'] = dict(base=0x1ff8000, data=open('work/arm9_itcm.bin', 'rb').read(), funcs=load_funcs('fid/features/itcm.tsv'))
for r in csv.DictReader(open('work/overlays/arm9_overlays.csv')):
    i = int(r['id'])
    modules['ov%02d' % i] = dict(base=int(r['ram'], 16), data=open('work/overlays/' + r['file'], 'rb').read(),
                                 funcs=load_funcs('fid/features/ov%02d.tsv' % i) if os.path.exists('fid/features/ov%02d.tsv' % i) else [])
dtcm = open('work/arm9_dtcm.bin', 'rb').read()

def contains(mod, a):
    m = modules[mod]; return m['base'] <= a < m['base'] + len(m['data'])
def in_dtcm(a): return 0x27e0000 <= a < 0x27e0000 + len(dtcm)

def classify(mod_of_ref, a):
    """which module image holds address a, as seen from code in module mod_of_ref"""
    if mod_of_ref.startswith('ov') and contains(mod_of_ref, a): return mod_of_ref
    if contains('static', a): return 'static'
    if contains('itcm', a): return 'itcm'
    if in_dtcm(a): return 'dtcm'
    return None

def is_data_addr(tm, a):
    """address a in module tm is data (not inside a real function)"""
    if tm == 'dtcm': return True
    fs = modules[tm]['funcs']
    if not fs: return True
    i = bisect.bisect_right([f[0] for f in fs], a) - 1
    if i >= 0 and fs[i][0] <= a < fs[i][0] + fs[i][1] and 'DataAsCode' not in fs[i][2] and 'Garbage' not in fs[i][2]:
        return False
    return True

refs = collections.defaultdict(lambda: collections.defaultdict(int))   # (target_module, addr) -> {(src_module, func): n}
for mname, m in modules.items():
    d, base, fs = m['data'], m['base'], m['funcs']
    fstarts = [f[0] for f in fs]
    n = len(d) // 4
    words = struct.unpack('<%dI' % n, d[:n * 4])
    for k, w in enumerate(words):
        if w < 0x1ff8000 or w > 0x2800000: continue
        tm = classify(mname, w)
        if tm is None or not is_data_addr(tm, w): continue
        addr = base + 4 * k
        # only words inside/after code (literal pools): require the word's own location to be in or right after a function
        j = bisect.bisect_right(fstarts, addr) - 1
        if j < 0: continue
        f = fs[j]
        if addr - f[0] > f[1] + 0x200: continue           # too far from any function: probably a data table itself
        refs[(tm, w)][(mname, f[2])] += 1

# cluster referenced addresses per target module into table regions
by_mod = collections.defaultdict(list)
for (tm, a), r in refs.items(): by_mod[tm].append((a, r))
tables = []
for tm, lst in by_mod.items():
    lst.sort()
    img = {'static': modules['static'], 'itcm': modules['itcm'], 'dtcm': dict(base=0x27e0000, data=dtcm)}.get(tm) or modules[tm]
    end_img = img['base'] + len(img['data'])
    for idx, (a, r) in enumerate(lst):
        nxt = lst[idx + 1][0] if idx + 1 < len(lst) else end_img
        size = min(nxt - a, 0x10000, end_img - a)
        blob = img['data'][a - img['base']:a - img['base'] + size]
        printable = sum(1 for b in blob if 32 <= b < 127 or b in (0, 9, 10, 13))
        ptrs = sum(1 for k in range(0, len(blob) - 3, 4) if 0x1ff8000 <= struct.unpack('<I', blob[k:k + 4])[0] < 0x2800000)
        kind = 'strings' if size >= 4 and printable / max(1, len(blob)) > 0.9 and any(65 <= b < 123 for b in blob) else \
               'pointer_table' if size >= 8 and ptrs * 4 > 0.6 * len(blob) else 'data'
        agg = collections.Counter()
        for (sm, fn), c in r.items(): agg[(sm, fn)] += c
        tables.append({'module': tm, 'addr': '0x%07x' % a, 'size_est': size, 'kind': kind,
                       'referenced_by': [{'module': sm, 'function': fn, 'count': c} for (sm, fn), c in agg.most_common(6)],
                       'ref_functions': len(agg)})
tables.sort(key=lambda t: (t['module'], int(t['addr'], 16)))
# hand-identified tables from the analysis (names win over auto entries with the same start)
known = [
    ('static', 0x2113c10, 16, 's16[8]', 'g7221_expected_bits'), ('static', 0x2115110, 0x5c * 28, 's16 trees', 'g7221_region_power_diff_trees'),
    ('static', 0x2115b60, 0x200, 's16', 'g7221_region_stddev'), ('static', 0x2113c20, 28, 'ptr[7]', 'g7221_vq_huffman_trees'),
    ('static', 0x2113d74, 7 * 0x20, 's16[7][16]', 'g7221_vq_centroids'), ('static', 0x2113008, 200, 's16[10][10]', 'g7221_dct_matrix10'),
    ('static', 0x2113210, 640, 's16[320]', 'g7221_add_table320'), ('static', 0x2115be0, 640, 's16[320]', 'g7221_window320'),
    ('static', 0x2115e60, 1280, 's16[640]', 'g7221_window640'), ('static', 0x2112e60, 24, 'ptr[6]', 'g7221_dct_twiddle_ptrs'),
    ('static', 0x21067f0, 12, 'u32[3]', 'event_id_table_kind3'), ('static', 0x21067fc, 12, 'u32[3]', 'event_id_table_kind5'),
    ('static', 0x2106808, 12, 'u32[3]', 'event_id_table_kind6'), ('static', 0x2124628, 4, 'ptr', 'tmx_event_arc_path_ptr'),
]
bykey = {(t['module'], int(t['addr'], 16)): t for t in tables}
for mod, a, sz, typ, name in known:
    t = bykey.get((mod, a))
    if t is None:
        t = {'module': mod, 'addr': '0x%07x' % a, 'size_est': sz, 'kind': 'data', 'referenced_by': [], 'ref_functions': 0}; tables.append(t)
    t['name'] = name; t['size'] = sz; t['type'] = typ
doc = {'note': 'metadata only (addresses/names); the bytes live in the user-extracted engine/*.bin images',
       'modules': {k: {'base': '0x%07x' % v['base'], 'size': len(v['data'])} for k, v in modules.items()} | {'dtcm': {'base': '0x27e0000', 'size': len(dtcm)}},
       'tables': tables}
os.makedirs('data', exist_ok=True)
json.dump(doc, open('data/engine_tables.json', 'w'), indent=0)
c = collections.Counter(t['kind'] for t in tables)
print(len(tables), 'table regions;', dict(c), '; named', sum(1 for t in tables if 'name' in t))
for m in ('static', 'itcm', 'dtcm'):
    print(m, sum(1 for t in tables if t['module'] == m), 'regions')
print('overlay regions', sum(1 for t in tables if t['module'].startswith('ov')))
