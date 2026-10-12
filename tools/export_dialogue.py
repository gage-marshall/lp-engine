"""Static linear sweep of every SBIN entry -> dialogue lines with speaker, text, voice id. Output work/dialogue/NNNN.txt + fid/dialogue_stats.json"""
import os,sys,struct,json,collections
sys.path.insert(0,os.path.dirname(__file__))
from lpvm import Script
from sbin_scan import VALID
src='work/assets/arc/tmx_event.arc'; out='work/dialogue'; os.makedirs(out,exist_ok=True)
VOICE={0x5c,0xfb}
def seg(bs): return ''.join(t if k=='s' else '{v%d}'%t for k,t in bs)
stats=collections.Counter(); speakers=collections.Counter(); alllines=0
for f in sorted(os.listdir(src)):
    sc=Script(open(os.path.join(src,f),'rb').read()); d=sc.d
    lines=[]
    ents=sorted(set(sc.entries)); h=sc.hdr
    secs=sorted(x for x in h[1:] if x>(ents[0] if ents else 0))+[len(d)]; b=ents+[secs[0]]
    for ei,e in enumerate(ents):
        pc=e; lim=b[ei+1]; bufs=[[],[],[]]; stack=[]; acc=0; voice=None
        while pc<lim:
            op,n=d[pc],d[pc+1]
            if op not in VALID or pc+2+2*n>lim: break
            a=struct.unpack('>%dH'%n,d[pc+2:pc+2+2*n]); nxt=pc+2+2*n
            need={0x03:1,0x80:1,0x81:3,0x82:2,0x87:2,0x32:1,0x33:2}.get(op)
            if need is not None and len(a)<need: break
            if op==0x32: acc=a[0]
            elif op==0x33: acc=(a[0]<<16)|a[1]
            elif op==0x30: acc=0
            elif op==0x31: acc=1
            elif op in (0x40,0x41,0x42,0x02,0x35,0x34) or 0x50<=op<=0x5c: acc=None
            elif op==0x01: stack.append(acc)
            elif op==0x03 and a[0]: del stack[max(0,len(stack)-a[0]):]
            elif op in (0x80,0x81,0x82) and (not a or a[0]>2): break
            elif op==0x80: bufs[a[0]]=[]
            elif op==0x81: bufs[a[0]].append(('s',sc.string((a[1]<<16)|a[2])))
            elif op==0x82: bufs[a[0]].append(('v',a[1]))
            elif op==0x87:
                c=(a[0]<<16)|a[1]; k=stack[-1] if stack else None
                if c in VOICE and isinstance(k,int) and k>=1 and len(stack)>=k+1: voice=(c,stack[-1-k:-1])
            elif op in (0x83,0x84):
                lines.append((ei,pc,seg(bufs[2]),seg(bufs[1]),seg(bufs[0]),voice)); voice=None
            pc=nxt
    alllines+=len(lines)
    for ei,pc,name,sub,text,voice in lines:
        speakers[name]+=1; stats['voiced' if voice else 'unvoiced']+=1
    if lines:
        with open('%s/%s.txt'%(out,f.replace('.sbin','')),'w') as fo:
            for ei,pc,name,sub,text,voice in lines:
                v='' if not voice else ' [voice %s]'%(voice[1][:2])
                fo.write('e%d@%x %s%s: %s\n'%(ei,pc,name or '-',v,text.replace('\n','\\n')))
json.dump({'lines':alllines,'voiced':stats['voiced'],'unvoiced':stats['unvoiced'],'speakers':speakers.most_common(40)},open('fid/dialogue_stats.json','w'),ensure_ascii=False)
print(alllines,'dialogue lines;',dict(stats)); print(speakers.most_common(12))
