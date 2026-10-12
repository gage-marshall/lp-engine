#!/usr/bin/env python3
"""Tiered automatic labeling of every function (static module + overlays).

Tier A  semantic names already known (SDK/NNS/donor/manual) - preserved
Tier B  structural role from decompiled shape (Get/Set/Thunk/Wrap/Init/Clear/Switch/...) + subsystem
Tier C  subsystem + generic role + address

Subsystem inference: library address extents -> data-table ownership -> call graph -> address neighbors -> segment fallback.
Usage: autolabel.py   (reads fid/features/*.tsv, fid/manual_labels.csv, fid/v7.csv, fid/static_segments.txt)
Writes fid/auto/static.txt, fid/auto/ovNN.txt (ApplySymbols format) and fid/auto/tiers.csv
"""
import re, os, sys, csv, bisect, collections, glob

FID = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', 'fid')
DEFAULT = re.compile(r'^(FUN_|thunk_FUN_|thunk_EXT_FUN_|LAB_)')

LIB_PREFIX = {  # semantic-name prefixes that denote SDK / NNS / runtime code (not game subsystems)
    'OS', 'OSi', 'MI', 'MIi', 'FS', 'FSi', 'GX', 'GXS', 'GXi', 'GXx', 'G2', 'G2S', 'G3', 'G3i', 'G3C', 'CARD', 'CARDi', 'SND', 'SNDi', 'PXI', 'PXIi',
    'PM', 'PMi', 'RTC', 'RTCi', 'SPI', 'TP', 'TPi', 'MIC', 'MICi', 'WM', 'WMi', 'WBT', 'WBTi', 'MB', 'MBi', 'FX', 'MTX', 'VEC', 'MATH', 'MATHi',
    'STD', 'CTRDG', 'CTRDGi', 'CHT', 'CP', 'CPi', 'DC', 'IC', 'NNS', 'NNSi', 'FID', 'EXT', 'WVR', 'WXC', 'PRC', 'SVC', 'CRYPTO', 'ENV',
    'memset', 'memcpy', 'sprintf', 'vsnprintf', 'strlen', 'strcpy', 'MemCopyBytes', 'DLAddFront', 'DLExtract', 'DLInsert',
}
LIB_OF = {  # library archive -> canonical prefix used for unnamed functions inside that library's extent
    'libcard': 'CARD', 'libcht': 'CHT', 'libcp': 'CP', 'libctrdg': 'CTRDG', 'libenv': 'ENV', 'libext': 'EXT', 'libfs': 'FS', 'libfx': 'FX',
    'libgx': 'GX', 'libmath': 'MATH', 'libmb': 'MB', 'libmi': 'MI', 'libos': 'OS', 'libpxi': 'PXI', 'librtc': 'RTC', 'libsnd': 'SND',
    'libspi': 'SPI', 'libstd': 'STD', 'libwbt': 'WBT', 'libwm': 'WM', 'libwxc': 'WXC', 'libnnsfnd': 'NNSFnd', 'libnnsg2d': 'NNSG2d',
    'libnnsg3d': 'NNSG3d', 'libnnsgfd': 'NNSGfd', 'libnnssnd': 'NNSSnd', 'crt0': 'Crt0', 'libcrypto': 'CRYPTO', 'libssp': 'SSP',
}

def prefix(name):
    m = re.match(r'^([A-Za-z]+[0-9]?)[_A-Z]', name)
    if name.startswith('_'): return name
    p = name.split('_')[0] if '_' in name else name
    return p

def is_default(n): return bool(DEFAULT.match(n))

# ------------------------------------------------------------------ data loading
def parse_data(field):
    out = []
    for tok in field.split(';'):
        if re.match(r'^[0-9a-f]{5,8}(=|$)', tok):
            a, _, lab = tok.partition('=')
            out.append([int(a, 16), lab])
        elif out and tok: out[-1][1] += ';' + tok          # label that itself contained ';'
    return [(a, l) for a, l in out]

def load_features(path, lo=None, hi=None):
    out = {}
    for l in open(path):
        p = l.rstrip('\n').split('\t')
        if len(p) < 9: continue
        a = int(p[0], 16)
        out[a] = dict(addr=a, size=int(p[1]), name=p[2], ninstr=int(p[3]), callers=int(p[4]), thunk=p[5] == '1',
                      callees=[(c.rsplit('@', 1)[0], int(c.rsplit('@', 1)[1], 16)) for c in p[6].split(';') if '@' in c],
                      data=parse_data(p[7]),
                      c=p[8])
    return out

# ------------------------------------------------------------------ role classification from decompiled text
BODY = re.compile(r'\{(.*)\}\s*$')
def body_of(c):
    c = re.sub(r'/\*.*?\*/', '', c)
    m = re.search(r'\)\s*\{(.*)\}\s*$', c)
    return (m.group(1) if m else c).strip()

def strip_decls(b):
    # remove local variable declarations at the start
    return re.sub(r'^(?:\s*(?:undefined\d?|u?int|char|byte|short|ushort|bool|u8|u16|u32|s32|code|longlong|ulonglong|float|double|void|BOOL|[A-Za-z_]\w*\s*\*)\s*\**\s*\w+(?:\[\d+\])?;)+', '', b).strip()

def wrap_detail(n):
    m = re.match(r'^[A-Za-z0-9]+_(.+_[0-9a-f]{6,7})$', n)
    return m.group(1) if (m and re.search(r'_[0-9a-f]{6,7}$', n) and not is_default(n)) else n

def classify(f, names_by_addr):
    """-> (role, detail). Roles are generic but descriptive."""
    c = f['c']; b = strip_decls(body_of(c)); nc = len(f['callees'])
    if c.startswith('DECOMPILE'): return 'Unk', ''
    if f['thunk'] or re.fullmatch(r'\(\*\w+\)\([^;]*\);\s*(return;)?', b) or re.fullmatch(r'\(\*\*\w+\)\([^;]*\);\s*(return;)?', b):
        return 'Thunk', ''
    # constant return
    m = re.fullmatch(r'return\s+(-?(?:0x[0-9a-fA-F]+|\d+))\w*;', b)
    if m: return 'Const', m.group(1).replace('-', 'N')
    # getter: return of a deref chain, no calls
    if nc == 0 and re.fullmatch(r'return\s+[^;]*;', b) and ('DAT_' in b or '*' in b or 'param_1' in b):
        offs = re.findall(r'\+\s*(0x[0-9a-fA-F]+|\d+)\)', b)
        return 'Get', (offs[-1] if offs else '')
    # setter: single store from a parameter
    m = re.fullmatch(r'(?:\*\(\w+ \*\)\(.*?\)|\*\w+|\*\(\w+ \*\)\w+|\w+\[\w+\]) = (?:\(\w+\))?param_\d+;\s*(?:return;)?', b)
    if m and nc == 0:
        offs = re.findall(r'\+\s*(0x[0-9a-fA-F]+|\d+)\)', b)
        return 'Set', (offs[-1] if offs else '')
    if nc == 0 and len(re.findall(r';', b)) <= 4 and re.search(r'=\s*param_\d+;', b) and '*' in b: return 'Set', ''
    # clear/init: memset-like or loop storing zeros
    if re.search(r'memset|MIi_CpuClear|CpuClear', c) and nc <= 3: return 'Clear', ''
    if re.search(r'do \{.*\} while', b) and re.search(r'= 0;', b) and nc <= 2 and len(b) < 700: return 'Clear', ''
    # single-call wrapper (maybe with arg shuffle)
    if nc == 1 and len(b) < 220 and not re.search(r'\b(if|while|for|do|switch)\b', b): return 'Wrap', wrap_detail(names_by_addr.get(f['callees'][0][1], f['callees'][0][0]))
    if nc == 2 and len(b) < 160 and not re.search(r'\b(if|while|for|do|switch)\b', b): return 'Seq', ''
    if re.search(r'\bswitch\s*\(', b): return 'Switch', ''
    if nc == 0 and re.search(r'\b(while|for|do)\b', b): return 'Loop', ''
    if nc == 0: return 'Calc', ''
    return 'Proc', ''

CALLEE_THEME = [  # callee-name prefix -> role flavor, first strong match wins
    (r'^(Text_|TextBox_|TextCanvas_|NNS_G2dCharCanvas|NNS_G2dText|NNS_G2dFont)', 'Text'),
    (r'^(Sprite_|SprObj_|NNS_G2d(Cell|Anim|Oam|Entity)|NNS_G2dMulti|G2_|G2S_|Bg_|TextBg_)', 'Gfx'),
    (r'^(GX_|GXS_|G3_|G3C_|G3X|GXi|MTX_|VEC_|NNS_G3d|NNS_Gfd)', 'Gfx'),
    (r'^(ResTable_|FS_|Overlay_|CARD_|MI_Uncompress|NNS_Fnd)', 'Load'),
    (r'^(SND_|NNS_Snd|SNDi_)', 'Snd'),
    (r'^(GameClock_|Calendar_|TimePeriod_|Date_|RTC_)', 'Time'),
    (r'^(Touch_|Input_|HitList_|TouchCtl_|PAD_|TP_)', 'Input'),
    (r'^(ScreenFade_)', 'Fade'),
    (r'^(GameFlags_|GameState_)', 'State'),
    (r'^(OS_|OSi_)', 'Sys'),
    (r'^(WM_|WBT_|MB_|WMi_)', 'Net'),
]

def theme(f, names_by_addr):
    cnt = collections.Counter()
    for nm, ad in f['callees']:
        n = names_by_addr.get(ad, nm)
        for pat, th in CALLEE_THEME:
            if re.match(pat, n): cnt[th] += 1; break
    if not cnt: return ''
    th, k = cnt.most_common(1)[0]
    return th if k * 3 >= len(f['callees']) or k >= 3 else ''

# ------------------------------------------------------------------ subsystem inference (static)
def infer_static(feats, names, lib_extents, segments, game_seed_sub):
    addrs = sorted(feats)
    idx = {a: i for i, a in enumerate(addrs)}
    sub = dict(game_seed_sub)                   # addr -> subsystem (seeds)
    src = {a: 'seed' for a in sub}
    # library extents
    for a in addrs:
        if a in sub or not is_default(names[a]): continue
        k = bisect.bisect_right(lib_extents[0], a) - 1
        if k >= 0 and a < lib_extents[1][k][0]:
            sub[a] = lib_extents[1][k][1]; src[a] = 'libextent'
    # evidence-based voting. Membership evidence: owned (non-hub) data, adjacency, exclusive callers, private helper callees.
    callers = collections.defaultdict(set)
    for a, f in feats.items():
        for nm, ad in f['callees']: callers[ad].add(a)
    dataref = collections.defaultdict(set)
    for a, f in feats.items():
        for d, lab in f['data']: dataref[d].add(a)
    hub = {d for d, s in dataref.items() if len(s) > 30}
    unlab = [a for a in addrs if a not in sub]
    labeled_global = {}
    for a, f in feats.items():
        for d, lab in f['data']:
            if lab in GLOBAL_OWNER: labeled_global[d] = GLOBAL_OWNER[lab]
    for a in unlab:
        if a in sub: continue
        gv = collections.Counter(labeled_global[d] for d, lab in feats[a]['data'] if d in labeled_global)
        if gv and len(gv) == 1: sub[a] = next(iter(gv)); src[a] = 'gown'
    unlab = [a for a in addrs if a not in sub]
    LOCAL = 0x1800                                   # a subsystem must have a real seed within this many bytes
    def near_seed(a, s):
        i = idx[a]
        for step in (1, -1):
            j = i
            while 0 <= j < len(addrs) and abs(addrs[j] - a) <= LOCAL:
                if sub.get(addrs[j]) == s and src.get(addrs[j]) in ('seed', 'gown'): return True
                j += step
        return False
    def member(g): return g in sub and src.get(g) in ('seed', 'gown', 'inferred', 'sandwich') and sub[g] in GAME_SUBS_ALL
    for rnd in range(5):
        newsub = {}
        owner = collections.defaultdict(collections.Counter)
        for d, fs in dataref.items():
            if d in hub: continue
            for g in fs:
                if member(g): owner[d][sub[g]] += 1
        for a in unlab:
            if a in sub: continue
            votes = collections.Counter(); f = feats[a]
            for d, lab in f['data']:
                if d in hub: continue
                for s_, k in owner.get(d, {}).items(): votes[s_] += 3.0
            cs = callers.get(a, set())
            if cs and len(cs) <= 6:
                cvotes = collections.Counter(sub[c] for c in cs if member(c))
                if cvotes and len(cvotes) == 1 and sum(cvotes.values()) == len(cs): votes[next(iter(cvotes))] += 3.0     # called only from one subsystem
            for nm, ad in f['callees']:
                if ad in sub and member(ad) and len(callers.get(ad, ())) <= 4: votes[sub[ad]] += 1.5               # private helper of S
            i = idx[a]
            for off, wt in ((-1, 2.0), (1, 2.0), (-2, 1.0), (2, 1.0)):
                j = i + off
                if 0 <= j < len(addrs) and member(addrs[j]) and abs(addrs[j] - a) <= 0x400: votes[sub[addrs[j]]] += wt
            votes = collections.Counter({s_: v for s_, v in votes.items() if near_seed(a, s_)})
            if votes:
                top, sc = votes.most_common(1)[0]; tot = sum(votes.values())
                if sc >= 3.0 and sc / tot >= 0.6: newsub[a] = (top, 'inferred')
        for a, (s_, how) in newsub.items(): sub[a] = s_; src[a] = how
        if not newsub: break
    # sandwich: unlabeled between two same-subsystem neighbors within 12 functions and 0x1000 bytes
    for a in unlab:
        if a in sub: continue
        i = idx[a]; L = R = None; la = ra = 0
        for j in range(i - 1, max(-1, i - 13), -1):
            if member(addrs[j]): L = sub[addrs[j]]; la = addrs[j]; break
        for j in range(i + 1, min(len(addrs), i + 13)):
            if member(addrs[j]): R = sub[addrs[j]]; ra = addrs[j]; break
        if L and L == R and ra - la <= 0x1000: sub[a] = L; src[a] = 'sandwich'
    # clustering of everything still unlabeled: shared private data tables + local private-call edges -> modules
    parent = {x: x for x in addrs}
    def find(x):
        while parent[x] != x: parent[x] = parent[parent[x]]; x = parent[x]
        return x
    def union(x, y):
        rx, ry = find(x), find(y)
        if rx != ry: parent[max(rx, ry)] = min(rx, ry)
    def clusterable(x): return x in sub and src.get(x) in ('libextent',) or (x not in sub) or src.get(x) in ('seed', 'inferred', 'sandwich')
    lib_named = {x for x in addrs if (not is_default(names[x])) and x not in game_seed_sub}      # SDK / NNS / runtime names stay out
    for d, fs in dataref.items():
        if d in hub or len(fs) > 12: continue
        fl = sorted(x for x in fs if x not in lib_named and not (src.get(x) == 'libextent'))
        for x, y in zip(fl, fl[1:]):
            if y - x <= 0x3000: union(x, y)
    for a2, f in feats.items():
        if a2 in lib_named or src.get(a2) == 'libextent': continue
        for nm, ad in f['callees']:
            if ad in idx and ad not in lib_named and src.get(ad) != 'libextent' and abs(idx[ad] - idx[a2]) <= 10 and len(callers.get(ad, ())) <= 8: union(a2, ad)
    comps = collections.defaultdict(list)
    for x in addrs:
        if x in lib_named or src.get(x) == 'libextent': continue
        comps[find(x)].append(x)
    for root, mem in comps.items():
        mem.sort()
        # split giant/sprawling components into address-contiguous chunks
        chunks = [[mem[0]]]
        for x in mem[1:]:
            if x - chunks[-1][-1] > 0x900 or len(chunks[-1]) >= 80: chunks.append([x])
            else: chunks[-1].append(x)
        for ch in chunks:
            seeds = collections.Counter(sub[x] for x in ch if x in sub and src.get(x) in ('seed', 'inferred', 'sandwich') and sub[x] in GAME_SUBS_ALL)
            name = None
            if seeds:
                top, k = seeds.most_common(1)[0]
                if k >= 2 and k / sum(seeds.values()) >= 0.6: name = top
            if name is None:
                if len(ch) < 3: continue                                  # leave tiny fragments for neighbor/segment fallback
                name = 'Mod%x' % ch[0]
            for x in ch:
                if x not in sub: sub[x] = name; src[x] = 'cluster'
    # tiny fragments: adopt the nearest assigned neighbor within 0x300 bytes
    for x in addrs:
        if x in sub: continue
        i = idx[x]
        for off in (-1, 1, -2, 2):
            j = i + off
            if 0 <= j < len(addrs) and addrs[j] in sub and abs(addrs[j] - x) <= 0x300 and src.get(addrs[j]) in ('cluster', 'neighbor') and sub[addrs[j]].startswith('Mod'):
                sub[x] = sub[addrs[j]]; src[x] = 'neighbor'; break
    # segment fallback
    segstarts = [s[0] for s in segments]
    for a in unlab:
        if a in sub: continue
        k = bisect.bisect_right(segstarts, a) - 1
        sub[a] = ('Seg%x' % segments[k][0]) if k >= 0 else 'Seg0'; src[a] = 'segment'
    return sub, src

GAME_SUBS_ALL = set()
MODMAP = {}
OVNAMES = {}
GLOBAL_OWNER = {'g_SpriteSlots': 'Sprite', 'g_GameClock': 'GameClock', 'g_GameHeap': 'GameHeap', 'g_ScreenFade': 'ScreenFade', 'g_ResTables': 'ResTable',
                'g_OverlaySlots': 'Overlay'}

def main():
    global GAME_SUBS_ALL, MODMAP, OVNAMES
    MODMAP = load_module_names(); OVNAMES = load_overlay_names()
    manual = list(csv.DictReader(open(os.path.join(FID, 'manual_labels.csv'))))
    feats = load_features(os.path.join(FID, 'features/static.tsv'))
    if os.path.exists(os.path.join(FID, 'features/itcm.tsv')): feats.update(load_features(os.path.join(FID, 'features/itcm.tsv')))
    names = {a: f['name'] for a, f in feats.items()}
    for r in manual:                                   # manual labels (possibly newer than the feature dump) win
        a = int(r['addr'], 16)
        if r['type'] == 'F' and a in names: names[a] = r['name']
        if r['type'] == 'F' and a in feats: feats[a]['name'] = r['name']
    # library extents from the matcher output (main region .text placements)
    ext = collections.defaultdict(lambda: [1 << 40, 0])
    for r in csv.DictReader(open(os.path.join(FID, 'v7.csv'))):
        if r['kind'] in ('unique', 'resolved', 'gapfill', 'fuzzy') and r['section'] == '.text' and r['region'] == 'main' and r['addr'] and r['lib'] in LIB_OF:
            a = int(r['addr'].split(';')[0], 16)
            e = ext[r['lib']]; e[0] = min(e[0], a); e[1] = max(e[1], a + int(r['size']))
    ext_list = sorted((lo, hi, LIB_OF[l]) for l, (lo, hi) in ext.items() if lo < (1 << 39))
    ext_list.append((0x202c22c, 0x203d8b0, 'Rtl'))                     # C runtime / float / libc region
    ext_list.sort()
    lib_extents = ([e[0] for e in ext_list], [(e[1], e[2]) for e in ext_list])
    segments = [tuple(int(x, 16) for x in l.split()[:2]) for l in open(os.path.join(FID, 'static_segments.txt'))]
    # game subsystem seeds = manual labels with non-library prefixes
    game_seed_sub = {}
    for r in manual:
        if r['type'] != 'F': continue
        a = int(r['addr'], 16); p = prefix(r['name'])
        if p in LIB_PREFIX or p.startswith('_'): continue
        game_seed_sub[a] = SUB_ALIAS.get(p, p)
    GAME_SUBS_ALL = set(game_seed_sub.values())
    # also: functions with non-default, non-library names found in features (e.g., from earlier labeling) are seeds
    for a, n in names.items():
        if not is_default(n) and a not in game_seed_sub:
            p = prefix(n)
            if p not in LIB_PREFIX and not p.startswith(('_', 'FID')) and p in GAME_SUBS_ALL: game_seed_sub[a] = SUB_ALIAS.get(p, p)
    sub, src = infer_static(feats, names, lib_extents, segments, game_seed_sub)
    out = {}
    tiers = []
    names2 = dict(names)
    for rnd in range(2):
      out = {}; tiers = []
      for a in sorted(feats):
        f = feats[a]; n = names[a]
        if not is_default(n):
            tiers.append(('static', a, n, 'A', 'existing'));  continue
        role, detail = classify(f, names2)
        th = theme(f, names)
        s = MODMAP.get(sub.get(a, 'Seg0'), sub.get(a, 'Seg0'))
        gl = ''
        if role in ('Get', 'Set'):
            gl = next((lab for d, lab in f['data'] if lab), '')
            gl = re.sub(r'^g_', '', gl)
        rname = role
        if role == 'Wrap' and detail:
            rname = 'Wrap_' + re.sub(r'[^A-Za-z0-9_]', '', detail)[:28]
        elif role in ('Get', 'Set'):
            rname = role + (gl[:20] if gl else '') + (('_' + detail.replace('0x', '')) if detail else '')
        elif role == 'Const':
            rname = 'Const' + detail
        elif role == 'Proc' or role == 'Seq':
            rname = role + th if th else role
        name = f'{s}_{rname}_{a:07x}'
        out[a] = name
        tier = 'B' if role not in ('Proc', 'Seq', 'Unk', 'Calc', 'Loop') else 'C'
        tiers.append(('static', a, name, tier, src.get(a, '')))
      names2.update(out)
    os.makedirs(os.path.join(FID, 'auto'), exist_ok=True)
    with open(os.path.join(FID, 'auto/static.txt'), 'w') as fo:
        for a, nme in sorted(out.items()):
            fo.write(f'{a:08x} {nme} {feats[a]["size"]} auto game F static\n')
    with open(os.path.join(FID, 'auto/tiers_static.csv'), 'w', newline='') as fo:
        w = csv.writer(fo); w.writerow(['module', 'addr', 'name', 'tier', 'subsystem_source']); w.writerows([(m, f'{a:08x}', n, t, s) for m, a, n, t, s in tiers])
    final_static = {a: (names[a] if a not in out else out[a]) for a in feats}
    label_overlays(final_static)
    cnt = collections.Counter((t, s) for _, _, _, t, s in tiers)
    print('static: total', len(feats), 'auto-named', len(out))
    print(' tiers:', collections.Counter(t for _, _, _, t, _ in tiers))
    print(' subsystem source:', collections.Counter(s for _, _, _, t, s in tiers if t != 'A'))
    print(' top subsystems:', collections.Counter(n.split('_')[0] for n in out.values()).most_common(25))
    print(' roles:', collections.Counter(n.split('_')[1][:6] for n in out.values()).most_common(14))

def load_module_names():
    p = os.path.join(FID, 'module_names.csv')
    return {r['key']: r['name'] for r in csv.DictReader(open(p))} if os.path.exists(p) else {}

def load_overlay_names():
    p = os.path.join(FID, 'overlay_names.csv')
    return {int(r['id']): r['name'] for r in csv.DictReader(open(p))} if os.path.exists(p) else {}

def thumb_ranges():
    r = collections.defaultdict(list)
    p = os.path.join(FID, 'thumb_ranges.txt')
    if os.path.exists(p):
        for l in open(p):
            t = l.split()
            if len(t) == 3 and int(t[2], 16) - int(t[1], 16) >= 1536: r[int(t[0])].append((int(t[1], 16), int(t[2], 16)))
    return r

def label_overlays(final_static):
    tr = thumb_ranges()
    tot = collections.Counter(); alltiers = []
    for path in sorted(glob.glob(os.path.join(FID, 'features/ov*.tsv'))):
        ov = int(re.search(r'ov(\d+)\.tsv', path).group(1))
        feats = load_features(path)
        view = dict(final_static); view.update({a: f['name'] for a, f in feats.items()})
        out = {}; tiers = []
        for rnd in range(2):                       # second pass lets Wrap_ names use the callee's new name
            out = {}; tiers = []
            for a in sorted(feats):
                f = feats[a]; n = f['name']
                if not is_default(n):
                    tiers.append((f'ov{ov:02d}', a, n, 'A', 'existing')); continue
                role, detail = classify(f, view)
                th = theme(f, view)
                sub = 'Ov%02d%s' % (ov, OVNAMES.get(ov, ''))
                if ov == 6 and any(lo <= a < hi for lo, hi in tr[ov]): sub = 'Ov06Dlt'
                rname = role
                if role == 'Wrap' and detail: rname = 'Wrap_' + re.sub(r'[^A-Za-z0-9_]', '', detail)[:28]
                elif role in ('Get', 'Set'): rname = role + (('_' + detail.replace('0x', '')) if detail else '')
                elif role == 'Const': rname = 'Const' + detail
                elif role in ('Proc', 'Seq'): rname = role + th if th else role
                name = f'{sub}_{rname}_{a:07x}'
                out[a] = name
                tier = 'B' if role not in ('Proc', 'Seq', 'Unk', 'Calc', 'Loop') else 'C'
                tiers.append((f'ov{ov:02d}', a, name, tier, 'dlt-thumb' if sub == 'Ov06Dlt' else 'overlay'))
            view.update(out)
        for m, a, nm, t, s_ in tiers: tot[t] += 1
        alltiers.extend(tiers)
        with open(os.path.join(FID, f'auto/ov{ov:02d}.txt'), 'w') as fo:
            for a, nme in sorted(out.items()): fo.write(f'{a:08x} {nme} {feats[a]["size"]} auto game F ov{ov:02d}\n')
    with open(os.path.join(FID, 'auto/tiers_overlays.csv'), 'w', newline='') as fo:
        w = csv.writer(fo); w.writerow(['module', 'addr', 'name', 'tier', 'subsystem_source']); w.writerows([(m, f'{a:08x}', n, t, s_) for m, a, n, t, s_ in alltiers])
    print('overlays: functions', sum(tot.values()), dict(tot))

SUB_ALIAS = {'SprObj': 'Sprite', 'SpriteSystem': 'Sprite', 'TextBox': 'Text', 'TextCanvas': 'Text', 'TextBg': 'Text', 'TimePeriod': 'GameClock',
             'Date': 'GameClock', 'Calendar': 'GameClock', 'TouchCtl': 'Input', 'HitList': 'Input', 'Touch': 'Input', 'Rand': 'Input'}

if __name__ == '__main__':
    main()
