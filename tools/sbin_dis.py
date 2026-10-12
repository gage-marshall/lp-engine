"""Disassemble LovePlus+ SBIN scripts. Usage: sbin_dis.py file.sbin [entry#]"""
import sys,os,struct,csv
sys.path.insert(0,os.path.dirname(__file__))
from sbin import load,header
from sbin_scan import VALID
CMD={}
p=os.path.join(os.path.dirname(__file__),'..','fid','script_command_spec2.csv')
if os.path.exists(p):
    import re
    for r in csv.DictReader(open(p)):
        e=r['effect']; m=re.findall(r'([A-Za-z0-9]+_[A-Za-z0-9_]+)\(',e)
        m=[x for x in m if not x.startswith(('TaskList_Alloc','Input_Set','StateLogic_UpdateDataChi_d34d8','GameFlags_SetBitRange'))]
        CMD[int(r['cmd'],16)]=(m[0] if m else (re.findall(r'TASK(0x[0-9a-f]+)',e) or ['?'])[0] and 'task'+(re.findall(r'TASK(0x[0-9a-f]+)',e) or ['?'])[0])
N={0:'nop',1:'push acc',2:'pop acc',3:'drop %d',0x10:'jmp %+d',0x11:'jt %+d',0x12:'jf %+d',0x13:'ret',0x14:'ret id %s',0x16:'call id %s',0x20:'flag=acc!=0',0x21:'flag=acc==top',
 0x30:'acc=0',0x31:'acc=1',0x32:'acc=%d',0x33:'acc=%d',0x34:'acc=!!acc',0x35:'acc=-acc',0x40:'acc=reg[%d]',0x41:'acc=var[%d]',0x42:'acc=local[%d]',0x43:'reg[%d]=acc',0x44:'var[%d]=acc',0x45:'local[%d]=acc',
 0x50:'acc=pop+pop',0x51:'acc=a-b',0x52:'acc=a*b',0x53:'acc=a/b',0x54:'acc=a&b',0x55:'acc=a|b',0x56:'acc=a^b',0x57:'acc=a==b',0x58:'acc=a!=b',0x59:'acc=a<b',0x5a:'acc=a<=b',0x5b:'acc=a>b',0x5c:'acc=a>=b',
 0x80:'str[%d].clear',0x81:'str[%d].append_tbl(%d@%d)',0x82:'str[%d].append_str(%d)',0x83:'text_commit',0x84:'op84 %s',0x86:'op86 %s',0x88:'wait_fade'}
def sg(v): return v-65536 if v&0x8000 else v
def strat(d,base,off):
    p=base+(off&0x7fffffff); n=d[p]; q=p+1
    if n&0x80: n=d[p+1]+0x80; q=p+2
    return d[q:q+n].decode('utf-8','replace')
def dis(d,e,lim,sbase=None):
    pc=e
    while pc<lim:
        op,n=d[pc],d[pc+1]
        if op not in VALID or pc+2+2*n>lim: break
        a=struct.unpack('>%dH'%n,d[pc+2:pc+2+2*n]); nxt=pc+2+2*n
        if op==0x87: c=(a[0]<<16)|a[1]; s='CMD %#x %s'%(c,CMD.get(c,'?'))
        elif op==0x81 and sbase is not None: s='buf[%d].append %r'%(a[0],strat(d,sbase,(a[1]<<16)|a[2]))
        elif op in (0x10,0x11,0x12): s=N[op]%sg(a[0])+'  ->%x'%(nxt+sg(a[0]))
        elif op==0x32: s='acc=%d'%a[0]
        elif op==0x33: s='acc=%d'%(struct.unpack('>i',struct.pack('>HH',*a))[0])
        elif op in N and '%' in N[op]:
            try: s=N[op]%a
            except TypeError: s=N[op]%(a[0] if a else '')
        else: s=N.get(op,'op%02x %s'%(op,a))
        print('  %04x: %s'%(pc,s)); pc=nxt
        if op==0x13: break
if __name__=='__main__':
    d=load(sys.argv[1]); h,ents=header(d); ents=sorted(set(ents)); secs=sorted(x for x in h[1:] if x>ents[0])+[len(d)]
    b=ents+[secs[0]]
    for i,e in enumerate(ents):
        if len(sys.argv)>2 and i!=int(sys.argv[2]): continue
        print('entry',i,'@%x'%e); dis(d,e,b[i+1],h[1])
