#!/usr/bin/env python3
"""module_dossier.py MOD [MOD...]  - evidence summary for naming a module (subsystem prefix of auto names)."""
import sys, csv, re, json, os, struct, collections
F = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fid')
CACHE = os.path.join(F, 'ov_callers.json')
def build_cache():
    fn = {int(l.split()[0], 16) for l in open(os.path.join(F, 'funcs_final.txt'))}
    m = collections.defaultdict(set)
    base = os.path.join(F, '..', 'work', 'overlays')
    for r in csv.DictReader(open(os.path.join(base, 'arm9_overlays.csv'))):
        d = open(os.path.join(base, r['file']), 'rb').read(); b = int(r['ram'], 16)
        for i in range(0, len(d) - 3, 4):
            w = struct.unpack_from('<I', d, i)[0]
            if (w >> 24) in (0xEB, 0xEA):
                t = b + i + 8 + (((w & 0xFFFFFF) ^ 0x800000) - 0x800000) * 4
                if t in fn: m[t].add(int(r['id']))
    json.dump({hex(k): sorted(v) for k, v in m.items()}, open(CACHE, 'w'))
if not os.path.exists(CACHE): build_cache()
ovc = {int(k, 16): v for k, v in json.load(open(CACHE)).items()}
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import autolabel as A
feats = A.load_features(os.path.join(F, 'features/static.tsv'))
tiers = {int(r['addr'], 16): r for r in csv.DictReader(open(os.path.join(F, 'auto/tiers_static.csv')))}
score = {int(l.split()[0], 16): int(l.split()[2]) for l in open(os.path.join(F, 'rank2.txt'))}
final = {int(l.split()[0], 16): l.split()[2] for l in open(os.path.join(F, 'funcs_final.txt'))}
for mod in sys.argv[1:]:
    mem = sorted([a for a, r in tiers.items() if r['name'].split('_')[0] == mod and r['tier'] != 'A'])
    if not mem: print(mod, 'no members'); continue
    print('=' * 100); print(f'{mod}: {len(mem)} funcs, {sum(feats[a]["size"] for a in mem)} bytes, {mem[0]:#x}-{mem[-1]:#x}')
    top = sorted(mem, key=lambda a: -score.get(a, 0))[:8]
    print(' heaviest:', ', '.join(f'{final[a].split("_",1)[1] if "_" in final[a] else final[a]}({feats[a]["size"]}B,w{score.get(a,0)})' for a in top))
    ovs = collections.Counter(o for a in mem for o in ovc.get(a, []))
    print(' called by overlays:', ' '.join(f'ov{o}:{n}' for o, n in ovs.most_common(8)))
    ce = collections.Counter()
    for a in mem:
        for nm, ad in feats[a]['callees']:
            n = final.get(ad, nm)
            if not re.search(r'_[0-9a-f]{6,7}$', n) and not n.startswith(('FUN_', 'thunk')): ce[n] += 1
    print(' semantic callees:', ', '.join(f'{n}x{c}' for n, c in ce.most_common(14)))
    dl = collections.Counter(lab for a in mem for d, lab in feats[a]['data'] if lab)
    print(' data labels:', ', '.join(f'{n[:40]}x{c}' for n, c in dl.most_common(10)))
    ex = [a for a in top[:1]]
    for a in ex:
        print(f'  -- {final[a]} {feats[a]["c"][:170]}')
