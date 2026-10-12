"""Extract literal args at every CMD call site across all SBIN scripts -> fid/script_callsites.jsonl.gz-ish (jsonl)
Tracks the pattern: (acc=imm | acc=var/other ; push)*k ; acc=k ; push ; CMD id ; drop k+1.  Non-literal args become None."""
import os,sys,struct,json,collections
sys.path.insert(0,os.path.dirname(__file__))
from sbin import load,header
from sbin_scan import VALID
src='work/assets/arc/tmx_event.arc'; out=open('fid/script_callsites.jsonl','w'); n=0
for f in sorted(os.listdir(src)):
    d=load(os.path.join(src,f)); h,ents=header(d)
    if not ents: continue
    ents=sorted(set(ents)); secs=sorted(x for x in h[1:] if x>ents[0])+[len(d)]; b=ents+[secs[0]]
    for ei,e in enumerate(ents):
        pc=e; lim=b[ei+1]; ins=[]
        while pc<lim:
            op,na=d[pc],d[pc+1]
            if op not in VALID or pc+2+2*na>lim: break
            a=struct.unpack('>%dH'%na,d[pc+2:pc+2+2*na]); ins.append((pc,op,a)); pc+=2+2*na
            if op==0x13: break
        accv=None
        pushes=[]  # literal values pushed (None if unknown)
        for k,(pc,op,a) in enumerate(ins):
            if op==0x32: accv=a[0]
            elif op==0x33: accv=(a[0]<<16)|a[1]
            elif op==0x30: accv=0
            elif op==0x31: accv=1
            elif op in (0x40,0x41,0x42,0x02,0x50,0x51,0x52,0x53,0x54,0x55,0x56,0x57,0x58,0x59,0x5a,0x5b,0x5c,0x35,0x34): accv=None
            if op==0x01: pushes.append(accv)
            elif op==0x03:
                if a[0]: del pushes[max(0,len(pushes)-a[0]):]
            elif op==0x87:
                cmd=(a[0]<<16)|a[1]
                kk=pushes[-1] if pushes else None
                args=pushes[-1-kk:-1] if isinstance(kk,int) and 0<=kk<=len(pushes)-1 else None
                if isinstance(kk,int) and kk==0: args=[]
                out.write(json.dumps({'f':f,'e':ei,'pc':pc,'cmd':cmd,'k':kk,'args':args})+'\n'); n+=1
print(n,'call sites')
