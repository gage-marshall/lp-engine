import sys,os,struct,collections,csv
sys.path.insert(0,os.path.dirname(__file__))
from sbin import load,header
from sbin_scan import VALID
src='work/assets/arc/tmx_event.arc'; cmds=collections.Counter(); argc=collections.defaultdict(collections.Counter); nfiles=collections.defaultdict(set)
for f in sorted(os.listdir(src)):
    d=load(os.path.join(src,f)); h,ents=header(d)
    if not ents: continue
    ents=sorted(set(ents)); secs=sorted(x for x in h[1:] if x>ents[0])+[len(d)]; b=ents+[secs[0]]
    for i,e in enumerate(ents):
        pc=e; lim=b[i+1]; ins=[]
        while pc<lim:
            op,n=d[pc],d[pc+1]
            if op not in VALID or pc+2+2*n>lim: break
            a=struct.unpack('>%dH'%n,d[pc+2:pc+2+2*n]); ins.append((op,a)); pc+=2+2*n
            if op==0x13: break
        for k,(op,a) in enumerate(ins):
            if op==0x87:
                c=(a[0]<<16)|a[1]; cmds[c]+=1; nfiles[c].add(f)
                # argc = imm pushed just before: pattern 32 imm; 01 push; 87
                if k>=2 and ins[k-1][0]==1 and ins[k-2][0]==0x32: argc[c][ins[k-2][1][0]]+=1
w=csv.writer(open('fid/script_cmd_usage.csv','w')); w.writerow(['cmd','uses','files','argc_histogram'])
for c,n in cmds.most_common(): w.writerow([hex(c),n,len(nfiles[c]),' '.join('%d:%d'%x for x in sorted(argc[c].items()))])
print(len(cmds))
