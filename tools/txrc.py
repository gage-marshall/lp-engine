"""Parse decrypted LovePlus+ TXRC text resource (layout from TextRes_GetCurrentEntry @ 0x209eccc).
hdr: +8 ngroups, +0xc tabA(group->(nsub,base) u16x2), +0x10 tabB(sub->(nline,base) u16x2), +0x14 tabC(line->u32 len<<22|off), +0x18 data base."""
import struct,sys,os
sys.path.insert(0,os.path.dirname(__file__))
from lp_decrypt import decrypt
def parse(raw,decrypted=False):
    d=raw if decrypted else decrypt(raw); assert d[:4]==b'TXRC'
    ng,a,b,c,base=struct.unpack('<5I',d[8:28])
    groups=[]
    for g in range(ng):
        ns,sb=struct.unpack('<HH',d[a+g*4:a+g*4+4]); subs=[]
        for s in range(ns):
            nl,lb=struct.unpack('<HH',d[b+(sb+s)*4:b+(sb+s)*4+4]); lines=[]
            for l in range(nl):
                v=struct.unpack('<I',d[c+(lb+l)*4:c+(lb+l)*4+4])[0]
                o=base+(v&0x3fffff); lines.append(d[o:o+(v>>22)])
            subs.append(lines)
        groups.append(subs)
    return groups
if __name__=='__main__':
    n=0
    for g,subs in enumerate(parse(open(sys.argv[1],'rb').read())):
        for s,lines in enumerate(subs):
            for l,t in enumerate(lines):
                print(g,s,l,repr(t.decode('utf-8',errors='replace'))); n+=1
    print(n,'lines',file=sys.stderr)
