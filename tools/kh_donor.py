#!/usr/bin/env python3
"""Build donor symbol files from the khdays-decomp config (main module only).
Usage: kh_donor.py KHREPO OUTDIR
Writes OUTDIR/kh_symbols.txt   (addr name size kind src F|D static mode)  all function symbols
       OUTDIR/kh_donor_names.txt  names of functions inside libs/nns, libs/nitro/nns, libs/msl (donor ranges)
"""
import re, sys, os
repo, out = sys.argv[1], sys.argv[2]
cfg = os.path.join(repo, 'config/arm9')
syms = {}
for l in open(os.path.join(cfg, 'symbols.txt')):
    m = re.match(r'(\S+) kind:function\((arm|thumb),size=0x([0-9a-f]+)\) addr:0x([0-9a-f]+)', l)
    if m: syms[int(m.group(4), 16)] = (m.group(1), int(m.group(3), 16), m.group(2))
ranges, cur = [], None
for l in open(os.path.join(cfg, 'delinks.txt')):
    m = re.match(r'^(\S+):\s*$', l)
    if m: cur = m.group(1); continue
    m = re.match(r'\s+\.text\s+start:0x([0-9a-f]+)\s+end:0x([0-9a-f]+)', l)
    if m and cur: ranges.append((int(m.group(1), 16), int(m.group(2), 16), cur))
donor_pref = ('libs/nns/', 'libs/nitro/nns/', 'libs/msl/')
donor = set()
for s, e, f in ranges:
    if f.startswith(donor_pref):
        for a, (n, sz, md) in syms.items():
            if s <= a < e: donor.add(n)
with open(os.path.join(out, 'kh_symbols.txt'), 'w') as f:
    for a in sorted(syms):
        n, sz, md = syms[a]; f.write(f'{a:08x} {n} {sz} kh khdays F static {md}\n')
open(os.path.join(out, 'kh_donor_names.txt'), 'w').write('\n'.join(sorted(donor)) + '\n')
print(len(syms), 'functions;', len(donor), 'donor functions;', sum(1 for _, _, f in ranges if f.startswith(donor_pref)), 'donor ranges')
