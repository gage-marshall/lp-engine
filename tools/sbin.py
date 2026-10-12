"""LovePlus+ SBIN script container + instruction framing (from Script_ParseFileHeader/ExecuteOpcode).
Decrypted file is big-endian: +8 nentries, +0xc,+0x10,+0x14,+0x18,+0x1c,+0x20,+0x24,+0x28,+0x2c,+0x30 section fields, entry offsets at 0x40.
Instruction: [op u8][nargs u8][nargs x u16 BE]."""
import struct,sys,os
sys.path.insert(0,os.path.dirname(__file__))
from lp_decrypt import decrypt
def load(path): return decrypt(open(path,'rb').read())
def header(d):
    h=struct.unpack('>11I',d[8:0x34]); n=h[0]
    ents=list(struct.unpack('>%dI'%n,d[0x40:0x40+4*n]))
    return h,ents
def decode(d,start,end):
    pc=start; out=[]
    while pc<end:
        op,n=d[pc],d[pc+1]
        args=struct.unpack('>%dH'%n,d[pc+2:pc+2+2*n]) if pc+2+2*n<=len(d) else None
        if args is None: break
        out.append((pc,op,args)); pc+=2+2*n
    return out,pc
if __name__=='__main__':
    import collections
    ops=collections.Counter(); files=0; bad=0; total=0; argn=collections.defaultdict(collections.Counter)
    for f in sorted(os.listdir(sys.argv[1])):
        d=load(os.path.join(sys.argv[1],f)); h,ents=header(d); files+=1
        if not ents: empty=globals().get('empty',0)+1; globals()['empty']=empty; continue
        end=min(x for x in list(h[1:]) +[len(d)] if x>ents[0]) if True else len(d)
        ins,pc=decode(d,ents[0],end)
        if pc!=end: bad+=1
        for _,op,a in ins: ops[op]+=1; argn[op][len(a)]+=1; total+=1
    print(files,'files',globals().get('empty',0),'with no entries',bad,'misaligned',total,'instr',len(ops),'distinct opcodes')
    for op,c in sorted(ops.items()): print('%02x %6d nargs=%s'%(op,c,dict(argn[op])))
