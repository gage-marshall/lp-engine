#!/usr/bin/env python3
"""Whole-object SDK matcher for NITRO/TWL static modules.

Finds SDK .o sections (relocation-masked) inside a module image made of several regions
(main / ITCM / DTCM), then derives more symbols from the relocations of the placed code.

Usage: sdk_match.py LIBDIR OUT_PREFIX REGION [REGION...]
  REGION = name:file:base   e.g. main:work/arm9_main.bin:0x02000000
Writes OUT_PREFIX.csv (section placements) and OUT_PREFIX.txt (symbols: addr name size kind src type)
"""
import struct, sys, os, glob, collections, bisect

# ---------------------------------------------------------------- ar / ELF parsing
def parse_ar(path):
    d = open(path, 'rb').read()
    assert d[:8] == b'!<arch>\n'
    p, names, out = 8, None, []
    while p + 60 <= len(d):
        h = d[p:p+60]; p += 60
        name, size = h[:16].decode('latin1').rstrip(), int(h[48:58])
        body = d[p:p+size]
        if name.startswith('#1/'):
            n = int(name[3:]); name = body[:n].rstrip(b'\0').decode('latin1'); body = body[n:]
        elif name == '//':
            names = body; name = None
        elif name in ('/', '/SYM64/'):
            name = None
        elif name.startswith('/') and name[1:].isdigit() and names:
            o = int(name[1:]); name = names[o:names.index(b'\n', o)].decode('latin1').rstrip('/')
        elif name.endswith('/'):
            name = name[:-1]
        if name: out.append((name, body))
        p += size + (size & 1)
    return out

class Sec: pass

def parse_elf(b):
    if b[:4] != b'\x7fELF': return None
    shoff, = struct.unpack_from('<I', b, 0x20)
    shentsize, shnum, shstrndx = struct.unpack_from('<HHH', b, 0x2E)
    sh = []
    for i in range(shnum):
        v = struct.unpack_from('<10I', b, shoff + i * shentsize)
        s = Sec(); (s.name_i, s.type, s.flags, s.addr, s.off, s.size, s.link, s.info, s.align, s.entsize) = v
        sh.append(s)
    strtab = b[sh[shstrndx].off: sh[shstrndx].off + sh[shstrndx].size]
    for s in sh: s.name = strtab[s.name_i: strtab.index(b'\0', s.name_i)].decode('latin1')
    return sh

def read_symtab(b, sh):
    symtab = next((s for s in sh if s.type == 2), None)
    syms = []
    if symtab:
        strs = sh[symtab.link]; sb = b[strs.off: strs.off + strs.size]
        for i in range(symtab.size // 16):
            n, val, size, info, other, shndx = struct.unpack_from('<IIIBBH', b, symtab.off + i * 16)
            nm = sb[n: sb.index(b'\0', n)].decode('latin1') if n else ''
            syms.append(dict(name=nm, val=val, size=size, type=info & 0xF, bind=info >> 4, shndx=shndx))
    return syms

def object_info(b):
    """-> (sections, symbols). Section = loadable PROGBITS with data, mask, relocs, defined symbols."""
    sh = parse_elf(b)
    if sh is None: return [], []
    syms = read_symtab(b, sh)
    res = []
    for idx, s in enumerate(sh):
        if s.type != 1 or not (s.flags & 2) or s.size == 0: continue
        data = bytes(b[s.off: s.off + s.size]); mask = bytearray(b'\xff' * s.size); relocs = []
        for r in sh:
            if r.type in (4, 9) and r.info == idx:
                esz = 12 if r.type == 4 else 8
                for k in range(r.size // esz):
                    if r.type == 4: off, info, add = struct.unpack_from('<IIi', b, r.off + k * esz)
                    else:
                        off, info = struct.unpack_from('<II', b, r.off + k * esz); add = struct.unpack_from('<i', data, off)[0] if off + 4 <= s.size else 0
                    t, si = info & 0xFF, info >> 8
                    for j in range(4):
                        if off + j < s.size: mask[off + j] = 0
                    relocs.append((off, t, si, add))
        names = [(y['name'], y['val'] & ~1, y['size'], y['type'], y['bind']) for y in syms
                 if y['shndx'] == idx and y['name'] and y['type'] in (0, 1, 2) and not y['name'].startswith(('$', '.', '@'))]
        res.append(dict(idx=idx, sec=s.name, exec=bool(s.flags & 4), align=s.align or 1, data=data,
                        mask=bytes(mask), relocs=relocs, syms=names))
    return res, syms

# ---------------------------------------------------------------- image / regions
class Image:
    def __init__(self, specs):
        self.regs, buf = [], bytearray()
        for sp in specs:
            name, f, base = sp.split(':')
            d = open(f, 'rb').read(); base = int(base, 0)
            voff = len(buf); buf += d
            self.regs.append((voff, len(d), base, name))
            buf += b'\xff' * (-len(buf) % 0x1000 + 0x1000)
        self.data = bytes(buf)
        self.starts = [r[0] for r in self.regs]
    def reg(self, c):
        i = bisect.bisect_right(self.starts, c) - 1
        if i < 0: return None
        v, n, b, nm = self.regs[i]
        return i if c < v + n else None
    def va(self, c):
        v, n, b, nm = self.regs[self.reg(c)]
        return b + c - v
    def off(self, va):
        for v, n, b, nm in self.regs:
            if b <= va < b + n: return v + va - b
        return None
    def u32(self, c): return struct.unpack_from('<I', self.data, c)[0]

# ---------------------------------------------------------------- matching primitives
def find_all(img, data, mask, align, limit=64):
    best, cur, bs = (0, 0), 0, 0
    for i, m in enumerate(mask):
        if m == 0xff:
            if cur == 0: bs = i
            cur += 1
            if cur > best[1]: best = (bs, cur)
        else: cur = 0
    a, ln = best
    if ln < 4: return None
    anchor = data[a:a + min(ln, 24)]
    out, pos, n, buf = [], 0, len(data), img.data
    while True:
        p = buf.find(anchor, pos)
        if p < 0: break
        pos = p + 1
        s = p - a
        if s < 0 or s + n > len(buf) or img.reg(s) is None or img.reg(s) != img.reg(s + n - 1): continue
        if img.va(s) % align: continue
        win = buf[s:s + n]
        if all(mask[i] != 0xff or win[i] == data[i] for i in range(n)):
            out.append(s)
            if len(out) >= limit: break
    return out

def runs(mask, minlen=8):
    out, cur, st = [], 0, 0
    for i, m in enumerate(mask):
        if m == 0xff:
            if cur == 0: st = i
            cur += 1
        else:
            if cur >= minlen: out.append((st, cur))
            cur = 0
    if cur >= minlen: out.append((st, cur))
    return sorted(out, key=lambda x: -x[1])

def ndiff(img, c, data, mask):
    buf, d, unm = img.data, 0, 0
    for i in range(len(data)):
        if mask[i] == 0xff:
            unm += 1
            if buf[c + i] != data[i]: d += 1
    return d, unm

def fuzzy(img, x, maxratio=0.15):
    data, mask = x['data'], x['mask']; n = len(data); unm = sum(1 for m in mask if m == 0xff)
    if n < 32 or unm < 24: return None
    votes = collections.Counter()
    for a, l in runs(mask)[:6]:
        anchor = data[a:a + min(l, 16)]; pos = 0; hits = 0
        while hits < 200:
            p = img.data.find(anchor, pos)
            if p < 0: break
            pos = p + 1; hits += 1; votes[p - a] += 1
    scored = []
    for st, v in votes.most_common(8):
        if st < 0 or st + n > len(img.data) or img.reg(st) is None or img.reg(st) != img.reg(st + n - 1): continue
        if img.va(st) % x.get('ma', max(x['align'], 4) if x['align'] >= 4 or not x['exec'] else 2): continue
        d, _ = ndiff(img, st, data, mask)
        scored.append((d, -v, st))
    scored.sort()
    if not scored: return None
    d, v, st = scored[0]
    if d / unm > maxratio: return None
    if len(scored) > 1 and scored[1][0] / unm <= maxratio and scored[1][0] - d < 4: return None
    return st, d

def up(a, al): return (a + al - 1) // al * al
PLACED = ('unique', 'resolved', 'gapfill', 'fuzzy')

def sext24(w):
    v = w & 0xFFFFFF
    return v - (1 << 24) if v & 0x800000 else v


# ---------------------------------------------------------------- donor (another game's binary + dsd config)
_NAMES = {}
def _load_names(repo, mod):
    if mod not in _NAMES:
        import re
        d = {}
        path = os.path.join(repo, 'config/arm9', '' if mod == 'main' else ('overlays/' + mod if mod.startswith('ov') else mod), 'symbols.txt')
        if os.path.exists(path):
            for l in open(path):
                m = re.match(r'(\S+) kind:(\w+)(?:\(([^)]*)\))? addr:0x([0-9a-f]+)', l)
                if m: d[int(m.group(4), 16)] = (m.group(1), m.group(2), m.group(3) or '')
        _NAMES[mod] = d
    return _NAMES[mod]

def _donor_lib(path, mod):
    parts = path.split('/')
    if parts[0] != 'libs' or len(parts) < 3: return None
    comp = parts[2] if parts[1] == 'nitro' and len(parts) > 3 else parts[1]
    comp = ('nitro-' + comp) if parts[1] == 'nitro' else comp
    if mod == 'main':
        if comp in ('nns', 'nitro-nns'): return 'kh_nns'
        if comp == 'msl': return 'kh_msl'
        return None
    return 'kh_%s_%s' % (comp.replace('-', '_'), mod)

def donor_objects(repo, mod, binpath, base):
    """Turn library functions of a donor module into pseudo-sections: bytes + relocation masks from dsd relocs.txt."""
    import re
    cfg = os.path.join(repo, 'config/arm9', '' if mod == 'main' else ('overlays/' + mod if mod.startswith('ov') else mod))
    img = open(binpath, 'rb').read(); names = _load_names(repo, mod)
    relocs = {}
    for l in open(os.path.join(cfg, 'relocs.txt')):
        m = re.match(r'from:0x([0-9a-f]+) kind:(\w+) to:0x([0-9a-f]+)(?: module:(\w+))?', l)
        if m: relocs[int(m.group(1), 16)] = (m.group(2), int(m.group(3), 16), m.group(4) or mod)
    ranges, cur = [], None
    for l in open(os.path.join(cfg, 'delinks.txt')):
        m = re.match(r'^(\S+):\s*$', l)
        if m: cur = m.group(1); continue
        m = re.match(r'\s+\.text\s+start:0x([0-9a-f]+)\s+end:0x([0-9a-f]+)', l)
        if m and cur: ranges.append((int(m.group(1), 16), int(m.group(2), 16), cur))
    out = []
    for s0, e0, f in sorted(ranges):
        lib = _donor_lib(f, mod)
        if not lib or e0 - s0 < 4: continue
        nm = names.get(s0)
        if not nm or nm[1] != 'function' or nm[0].startswith(('func_', 'data_')): continue
        thumb = 'thumb' in nm[2]
        data = img[s0 - base:e0 - base]; mask = bytearray(b'\xff' * len(data)); rl = []; syms = []
        if len(data) != e0 - s0: continue
        for a in range(s0, e0, 2 if thumb else 4):
            r = relocs.get(a)
            if not r: continue
            kind, to, tmod = r; off = a - s0
            if off + 4 > len(data): continue
            for j in range(4): mask[off + j] = 0
            tn = _load_names(repo, tmod).get(to)
            if not tn or tn[0].startswith(('func_', 'data_')) or tn[1] not in ('function', 'data'): continue
            t = 28 if kind == 'arm_call' else (2 if kind == 'load' else 99)
            syms.append(dict(name=tn[0], val=to, size=0, type=2 if tn[1] == 'function' else 1, bind=1, shndx=1))
            rl.append((off, t, len(syms) - 1, -8 if t == 28 else 0))
        sec = dict(idx=len(out), sec='.text', exec=True, align=2 if thumb else 4, data=bytes(data), mask=bytes(mask), relocs=rl,
                   syms=[(nm[0], 0, e0 - s0, 2, 1)], kind='none', cands=[], diffs=0, ma=2 if thumb else 4, donor=True)
        out.append((lib, f, [sec], syms))
    merged = {}
    for lib, f, secs, syms in out:
        m = merged.setdefault(lib, (lib, lib + '.a', [], []))
        off = len(m[3])
        for sc in secs:
            sc['relocs'] = [(o, t, si + off, a) for (o, t, si, a) in sc['relocs']]; sc['idx'] = len(m[2]); m[2].append(sc)
        m[3].extend(syms)
    return list(merged.values())

# ---------------------------------------------------------------- main
def main():
    argv = sys.argv[:]; donor = []
    while '--donor' in argv:
        i = argv.index('--donor'); donor.append(argv[i + 1:i + 5]); del argv[i:i + 5]
    libdir, outp = argv[1], argv[2]
    img = Image(argv[3:])
    objs, defs = [], {}            # objs: (lib, oname, secs, syms) ; defs: global name -> (type,size,exec,lib)
    inputs = []
    for lib in sorted(sum((glob.glob(os.path.join(d, 'lib*.a')) for d in libdir.split(':')), [])):
        b = os.path.basename(lib)[:-2]
        if any(x in b for x in ('TWL', 'thumb', 'CALLTRACE', 'FUNCTIONCOST')): continue
        for oname, ob in parse_ar(lib): inputs.append((b, oname, ob))
    for o in sorted(glob.glob(os.path.join(libdir.split(':')[0], 'crt0.o'))): inputs.append(('crt0', 'crt0.o', open(o, 'rb').read()))
    for b, oname, ob in inputs:
        secs, syms = object_info(ob)
        for x in secs: x.update(kind='none', cands=[], diffs=0)
        for y in syms:
            if y['shndx'] not in (0, 0xfff1, 0xfff2) and y['bind'] in (1, 2) and y['name'] and y['type'] in (0, 1, 2):
                defs.setdefault(y['name'], (y['type'], y['size'], b))
        objs.append((b, oname, secs, syms))
    if donor:
        for dr, dm, db, dbase in donor:
            dobjs = donor_objects(dr, dm, db, int(dbase, 0))
            objs.extend(dobjs); print('donor', dm, 'functions:', sum(len(o[2]) for o in dobjs))
    # pass 1 exact, pass 2 fuzzy
    for b, o, secs, syms in objs:
        for x in secs:
            m = find_all(img, x['data'], x['mask'], x.get('ma', max(x['align'], 4) if x['align'] >= 4 or not x['exec'] else 2))
            x['cands'] = m or []
            x['kind'] = 'none' if not m else ('unique' if len(m) == 1 else 'multi')
    for b, o, secs, syms in objs:
        for x in secs:
            if x['kind'] == 'none':
                r = fuzzy(img, x)
                if r: x['cands'], x['kind'], x['diffs'] = [r[0]], 'fuzzy', r[1]
    # pass 3 link-order resolution of multi candidates (same region neighbours only)
    for _ in range(6):
        changed = 0
        for b, o, secs, syms in objs:
            for i, x in enumerate(secs):
                if x['kind'] != 'multi': continue
                prev = next((secs[j] for j in range(i - 1, -1, -1) if secs[j]['kind'] in PLACED), None)
                nxt = next((secs[j] for j in range(i + 1, len(secs)) if secs[j]['kind'] in PLACED), None)
                if not prev and not nxt: continue
                lowinfo = sum(1 for q in x['mask'] if q == 0xff) < 12
                best = []
                for c in x['cands']:
                    r = img.reg(c)
                    if prev and img.reg(prev['cands'][0]) == r and c < prev['cands'][0] + len(prev['data']): continue
                    if nxt and img.reg(nxt['cands'][0]) == r and c + len(x['data']) > nxt['cands'][0]: continue
                    gap, ok = 0, False
                    if prev and img.reg(prev['cands'][0]) == r: gap += c - (prev['cands'][0] + len(prev['data'])); ok = True
                    if nxt and img.reg(nxt['cands'][0]) == r: gap += nxt['cands'][0] - (c + len(x['data'])); ok = True
                    if ok: best.append((gap, c))
                best.sort()
                if best and (len(best) == 1 or best[1][0] > best[0][0] + 0x100) and best[0][0] < (8 if lowinfo else 0x2000):
                    x['cands'], x['kind'] = [best[0][1]], 'resolved'; changed += 1
        if not changed: break
    # pass 3b gap filling
    def ok_at(x, c):
        n = len(x['data'])
        if c < 0 or c + n > len(img.data) or img.reg(c) is None or img.reg(c) != img.reg(c + n - 1): return False
        if img.va(c) % x.get('ma', max(x['align'], 4) if x['align'] >= 4 or not x['exec'] else 2): return False
        d, unm = ndiff(img, c, x['data'], x['mask'])
        return d == 0 if unm < 12 else d / unm <= 0.15
    bylib = collections.OrderedDict()
    for b, o, secs, syms in objs: bylib.setdefault(b, []).extend(secs)
    nfill = 0
    for b, flat in bylib.items():
        idx = [i for i, x in enumerate(flat) if x['kind'] in PLACED]
        for pi, qi in zip(idx, idx[1:]):
            run = flat[pi + 1:qi]
            if not run: continue
            P, Q = flat[pi], flat[qi]
            if img.reg(P['cands'][0]) != img.reg(Q['cands'][0]): continue
            start, end = P['cands'][0] + len(P['data']), Q['cands'][0]
            if end <= start or end - start > 0x4000 or len(run) > 40: continue
            memo = {}
            def go(k, addr):
                if k == len(run): return [] if up(addr, Q['align']) == end else None
                if (k, addr) in memo: return memo[(k, addr)]
                x = run[k]; res = None; a = up(addr, x['align'])
                if a + len(x['data']) <= end and ok_at(x, a):
                    r = go(k + 1, a + len(x['data']))
                    if r is not None: res = [(k, a)] + r
                if res is None: res = go(k + 1, addr)
                memo[(k, addr)] = res
                return res
            sol = go(0, start)
            if sol:
                for k, a in sol: run[k]['cands'], run[k]['kind'] = [a], 'gapfill'; nfill += 1
    print('gapfill placed', nfill)
    # pass 4 arbitration
    claims = collections.defaultdict(list)
    for b, o, secs, syms in objs:
        for x in secs:
            if x['kind'] in ('unique', 'resolved', 'gapfill'): claims[x['cands'][0]].append(x)
    for c, xs in claims.items():
        if any(not x.get('donor') for x in xs):
            for x in xs:
                if x.get('donor'): x['kind'], x['cands'] = 'none', []
            claims[c] = xs = [x for x in xs if not x.get('donor')]
        if len(xs) > 1:
            for x in xs: x['kind'] = 'ambiguous'
    taken = sorted((c, c + len(xs[0]['data'])) for c, xs in claims.items() if xs[0]['kind'] != 'ambiguous')
    for b, o, secs, syms in objs:
        for x in secs:
            if x['kind'] == 'fuzzy':
                c, e = x['cands'][0], x['cands'][0] + len(x['data'])
                k = bisect.bisect_left(taken, (c, 0))
                if any(t0 < e and c < t1 for t0, t1 in taken[max(0, k - 2):k + 2]): x['kind'], x['cands'] = 'none', []

    # pass 4b: outlier rejection. A library's code is contiguous: drop exec placements far from the lib's confident cluster.
    clus = collections.defaultdict(list)
    for b, o, secs, syms in objs:
        for x in secs:
            if x['exec'] and x['kind'] in PLACED and len(x['data']) >= 64 and x['sec'] != '.version':
                clus[(b, img.reg(x['cands'][0]))].append(img.va(x['cands'][0]))
    nout = 0
    for b, o, secs, syms in objs:
        for x in secs:
            if x['exec'] and x['kind'] in PLACED and b != 'crt0':
                c = clus.get((b, img.reg(x['cands'][0])))
                a = img.va(x['cands'][0])
                if not c or a < min(c) - 0x2000 or a > max(c) + 0x2000:
                    x['kind'], x['cands'] = 'outlier', []; nout += 1
    print('outlier placements rejected:', nout)
    # pass 5: relocation-derived symbols. A relocation in placed code/data names the final address of its target.
    STATIC = ('main', 'itcm', 'dtcm')
    def tag_of(a, srctag):
        for v, n, b, nm in img.regs:
            if nm in STATIC and b <= a < b + n: return 'static'
        return srctag
    def srcname(c): return img.regs[img.reg(c)][3]
    votes = collections.defaultdict(collections.Counter)      # (key, tag) -> addr -> n
    secvotes = collections.defaultdict(collections.Counter)   # (obj uid, shndx) -> section base va -> n
    kinds = {}
    for uid, (b, o, secs, syms) in enumerate(objs):
        for x in secs:
            if x['kind'] not in PLACED: continue
            c = x['cands'][0]; stag = 'static' if srcname(c) in STATIC else srcname(c)
            for off, t, si, add in x['relocs']:
                if c + off + 4 > len(img.data): continue
                w = img.u32(c + off); P = img.va(c) + off
                if t == 2: S = (w - add) & 0xFFFFFFFF
                elif t in (1, 28, 29):
                    disp = sext24(w) << 2
                    if (w >> 28) == 0xF: disp += ((w >> 24) & 1) * 2
                    S = (P + disp - add) & 0xFFFFFFFF
                else: continue
                y = syms[si] if si < len(syms) else None
                if y is None: continue
                if y['type'] == 3:                                       # section symbol: base of a section in this object
                    secvotes[(uid, y['shndx'])][S] += 1
                elif y['name'] and not y['name'].startswith(('$', '.', '@')):
                    key = y['name'] if (y['bind'] in (1, 2) or y['shndx'] == 0) else '%d:%s' % (uid, y['name'])
                    votes[(key, tag_of(S, stag))][S] += 1; kinds[key] = (t, y)
    # sections never placed by content but located via section-symbol relocations
    nsecplace = 0
    for uid, (b, o, secs, syms) in enumerate(objs):
        for x in secs:
            if x['kind'] in PLACED or x['kind'] == 'ambiguous': continue
            v = secvotes.get((uid, x['idx']))
            if v:
                a, n = v.most_common(1)[0]
                if n >= 1 and (len(v) == 1 or n >= 2 * v.most_common(2)[1][1]) and img.off(a) is not None:
                    x['cands'], x['kind'] = [img.off(a)], 'secreloc'; nsecplace += 1
    print('sections located via section relocs:', nsecplace)

    # ---- output
    rows = []
    for uid, (b, o, secs, syms) in enumerate(objs):
        for x in secs:
            for nm, v, sz, ty, bind in (x['syms'] or [(None, 0, 0, 0, 0)]):
                if x['kind'] in PLACED + ('secreloc',) and nm:
                    rg = srcname(x['cands'][0]); rows.append((img.va(x['cands'][0]) + v, nm, sz, x['kind'], f'{b}/{o}', 'F' if x['exec'] and ty != 1 else 'D', bind, 'static' if rg in STATIC else rg))
    out, conflicts = [], 0
    for (key, tg), cnt in votes.items():
        a, n = cnt.most_common(1)[0]
        if len(cnt) > 1:
            if n < 2 * cnt.most_common(2)[1][1]: conflicts += 1; continue
        nm = key.split(':', 1)[1] if ':' in key else key
        t, y = kinds[key]
        d = defs.get(nm)
        isf = (t in (1, 28, 29)) or (d is not None and d[0] == 2)
        if img.off(a) is None and not (0x1ff8000 <= a < 0x27e4000 or 0x2000000 <= a < 0x2200000): continue
        out.append((a, nm, d[1] if d else 0, 'reloc%d' % n, d[2] if d else 'extern', 'F' if isf else 'D', 1, tg))
    # merge: content wins, check agreement
    final, agree, disagree = {}, 0, []
    for a, nm, sz, k, src, t, bind, tg in rows: final[(tg, a, nm)] = (a, nm, sz, k, src, t, tg)
    cmap = {}
    for a, nm, sz, k, src, t, bind, tg in rows:
        if bind in (1, 2): cmap.setdefault((nm, tg), set()).add(a)
    for a, nm, sz, k, src, t, bind, tg in out:
        if (nm, tg) in cmap:
            if a in cmap[(nm, tg)]: agree += 1
            else: disagree.append((nm, tg, hex(a), [hex(z) for z in cmap[(nm, tg)]]))
        elif (tg, a, nm) not in final: final[(tg, a, nm)] = (a, nm, sz, k, src, t, tg)
    print('reloc-derived symbols: %d, agree with content placement: %d, disagree: %d, vote conflicts: %d' % (len(out), agree, len(disagree), conflicts))
    for z in disagree[:15]: print('  DISAGREE', z)
    with open(outp + '.txt', 'w') as f:
        for k in sorted(final): a, nm, sz, kd, src, t, tg = final[k]; f.write(f'{a:08x} {nm} {sz} {kd} {src} {t} {tg}\n')
    stats = collections.OrderedDict()
    with open(outp + '.csv', 'w') as f:
        f.write('lib,obj,section,size,kind,diffs,region,addr,symbol,symoff,symsize\n')
        for uid, (b, o, secs, syms) in enumerate(objs):
            st = stats.setdefault(b, collections.Counter())
            for x in secs:
                st[x['kind']] += 1
                addrs = ';'.join('%08x' % img.va(c) for c in x['cands'][:8])
                rgn = srcname(x['cands'][0]) if len(x['cands']) == 1 and x['kind'] in PLACED + ('secreloc',) else ''
                for nm, v, sz, ty, bind in (x['syms'] or [('', 0, 0, 0, 0)]):
                    f.write(f"{b},{o},{x['sec']},{len(x['data'])},{x['kind']},{x['diffs']},{rgn},{addrs},{nm},{v},{sz}\n")
    tot = collections.Counter()
    cols = ('unique', 'resolved', 'gapfill', 'fuzzy', 'secreloc', 'outlier', 'ambiguous', 'multi', 'none')
    for k, v in stats.items():
        print(f'{k:14s} ' + ' '.join(f'{kk}={v[kk]:4d}' for kk in cols)); tot.update(v)
    print('TOTAL ' + ' '.join(f'{kk}={tot[kk]}' for kk in cols))
    print('symbols written:', len(final))

if __name__ == '__main__': main()
