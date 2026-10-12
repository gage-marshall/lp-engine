#!/bin/bash
# Decompile all functions in address ranges and write a condensed reading file.
# Usage: decomp_ranges.sh NAME lo-hi [lo-hi ...]   -> fid/decomp/NAME.c and fid/decomp/NAME.cond.txt
set -u
cd "$(dirname "$0")/../fid"
name=$1; shift
export JAVA_HOME=${JAVA_HOME:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
H=/opt/homebrew/Cellar/ghidra/12.1.4/libexec/support/analyzeHeadless
python3 -I - "$name" "$@" <<'PY'
import sys
name=sys.argv[1]; R=[tuple(int(x,16) for x in a.split('-')) for a in sys.argv[2:]]
fn=sorted((int(l.split()[0],16),int(l.split()[1]),l.split()[2]) for l in open('funcs.txt'))
rank={int(l.split()[0],16):l.split() for l in open('rank.txt')}
out=[]
for lo,hi in R:
    for f in fn:
        if lo<=f[0]<hi:
            r=rank.get(f[0]); out.append(' '.join([f'{f[0]:08x}',str(f[1])]+(r[2:6] if r else ['0','0','0','0'])))
open(f'{name}.list','w').write('\n'.join(out)+'\n'); print(len(out),'functions')
PY
"$H" rom_proj arm9 -process arm9_main.bin -noanalysis -readOnly -scriptPath scripts -postScript DecompileFuncs.java "$PWD/$name.list" "$PWD/decomp/$name.c" 2>&1 | grep -E "DECOMPILED|ERROR|Exception" | cut -c1-160
python3 -I - "$name" <<'PY'
import re,sys
name=sys.argv[1]
t=open(f'decomp/{name}.c').read().split('// ================================================================\n')[1:]
out=[]
for b in t:
    lines=b.split('\n')
    body=[l for l in lines if l.strip() and not re.match(r'^\s+(undefined\d?|int|uint|char|byte|short|ushort|bool|u32|s32|code|longlong|ulonglong|void|float|double)\s*\*?\s*\w+(\[\d+\])?;\s*$',l) and 'WARNING' not in l]
    if len(body)>34: body=body[:30]+['  ... (%d more lines)'%(len(body)-30)]
    out.append('\n'.join(body))
open(f'decomp/{name}.cond.txt','w').write('\n//--\n'.join(out)); print(len(out),'condensed ->',f'decomp/{name}.cond.txt')
PY
wc -l decomp/$name.cond.txt
