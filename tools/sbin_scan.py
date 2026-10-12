import os,sys,struct,collections
sys.path.insert(0,os.path.dirname(__file__))
from sbin import load,header
VALID=set(range(0,0x22))|{0x30,0x31,0x32,0x33,0x34,0x35}|set(range(0x40,0x46))|set(range(0x50,0x5d))|set(range(0x80,0x89))
def scan(d):
    h,ents=header(d)
    if not ents: return [],0
    secs=sorted(x for x in h[1:] if x>ents[0])+[len(d)]
    end=secs[0]
    ins=[]; pc=ents[0]; ok=True
    while pc<end:
        op,n=d[pc],d[pc+1]
        if op not in VALID or pc+2+2*n>len(d): ok=False; break
        ins.append((pc,op,struct.unpack('>%dH'%n,d[pc+2:pc+2+2*n]))); pc+=2+2*n
    return ins,(pc==end)
if __name__=='__main__':
    src=sys.argv[1]; cmds=collections.Counter(); ops=collections.Counter(); clean=0; n=0; tot=0
    for f in sorted(os.listdir(src)):
        d=load(os.path.join(src,f)); ins,ok=scan(d); n+=1; clean+=ok; tot+=len(ins)
        for pc,op,a in ins:
            ops[op]+=1
            if op==0x87 and len(a)==2: cmds[(a[0]<<16)|a[1]]+=1
    print(n,'files',clean,'parse exactly to section start',tot,'instructions')
    print('opcodes used:',' '.join('%02x:%d'%(o,c) for o,c in sorted(ops.items())))
    print(len(cmds),'distinct 0x87 command ids; top 40:')
    print(' '.join('%x:%d'%(c,k) for c,k in cmds.most_common(40)))
    ids=sorted(cmds); print('range',hex(ids[0]),hex(ids[-1]))
