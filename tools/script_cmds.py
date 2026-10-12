"""Extract the script command table: jump table of GameFlow_RunMainLoop @0x205a8f4 (cmd id = ctx+0x10c0, set by opcode 0x87). Needs capstone."""
import capstone,struct,csv,bisect,collections,sys
d=open('work/arm9_dec.bin','rb').read(); base=0x2000000
md=capstone.Cs(capstone.CS_ARCH_ARM,capstone.CS_MODE_ARM)
lab={int(r['addr'],16):r['name'] for r in csv.DictReader(open('fid/labels_all.csv')) if r['module']=='static'}
TB=0x205a928; N=0x229; DEF=0x205e17c
tab=[]
for i in range(N):
    w=struct.unpack('<I',d[TB+i*4-base:TB+i*4-base+4])[0]
    off=w&0xffffff; off-=(1<<24) if off&0x800000 else 0
    tab.append(TB+i*4+8+off*4)
tg=sorted(set(tab)-{DEF}); bounds=tg+[DEF]
rows=[]
for cid,t in enumerate(tab):
    if t==DEF: continue
    i=bisect.bisect_right(tg,t)-1; hi=bounds[i+1]
    calls=[]; 
    for ins in md.disasm(d[t-base:hi-base],t):
        if ins.mnemonic=='bl':
            a=int(ins.op_str.lstrip('#'),16); calls.append(lab.get(a,hex(a)))
    rows.append((cid,t,hi-t,list(dict.fromkeys(calls))))
if __name__=='__main__':
    sz=[r[2] for r in rows]
    import statistics
    print('impl',len(rows),'of',N,'distinct handlers',len(tg),'median',statistics.median(sz),'mean',round(statistics.mean(sz)),'max',max(sz))
    print('<=64B',sum(s<=64 for s in sz),'<=160B',sum(s<=160 for s in sz),'>160B',sum(s>160 for s in sz))
    with open('fid/script_commands.csv','w') as f:
        w=csv.writer(f); w.writerow(['cmd','handler','size','callees'])
        for cid,t,s,c in rows: w.writerow([hex(cid),hex(t),s,' '.join(c)])
