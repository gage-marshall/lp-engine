#!/usr/bin/env python3
"""condense.py IN.c OUT.txt [maxlines]  - compact a DecompileFuncs dump for reading"""
import re, sys
t = open(sys.argv[1]).read().split('// ================================================================\n')[1:]
mx = int(sys.argv[3]) if len(sys.argv) > 3 else 30
out = []
for b in t:
    body = [l for l in b.split('\n') if l.strip() and 'WARNING' not in l
            and not re.match(r'^\s+(undefined\d?|int|uint|char|byte|short|ushort|bool|u32|s32|code|longlong|ulonglong|void|float|double)\s*\*?\s*\w+(\[\d+\])?;\s*$', l)]
    if len(body) > mx + 4: body = body[:mx] + ['  ... (%d more lines)' % (len(body) - mx)]
    out.append('\n'.join(body))
open(sys.argv[2], 'w').write('\n//--\n'.join(out)); print(len(out), 'functions ->', sys.argv[2])
