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


def build(groups, like):
    """a TXRC holding groups[g][s][l] (bytes), keyed like the original file `like` (raw bytes): same header layout
    (+8 ngroups, +0xc/+0x10/+0x14 tables, +0x18 string base, tables 16-aligned), strings packed without terminators
    (the reader takes the length from the line table), zero padding, the game's cipher applied from byte 8"""
    al = lambda n: (n + 15) & ~15
    subs = [s for g in groups for s in g]
    lines = [l for s in subs for l in s]
    a = 0x20; b = al(a + 4 * len(groups)); c = al(b + 4 * len(subs)); base = al(c + 4 * len(lines))
    out = bytearray(base)
    out[:8] = like[:8]
    struct.pack_into('<5I', out, 8, len(groups), a, b, c, base)
    si = li = 0
    for g, ss in enumerate(groups):
        struct.pack_into('<HH', out, a + 4 * g, len(ss), si)
        for s in ss:
            struct.pack_into('<HH', out, b + 4 * si, len(s), li)
            si += 1
            li += len(s)
    pool = bytearray(); seen = {}
    for i, l in enumerate(lines):
        assert len(l) < 1024, 'a TXRC line holds at most 1023 bytes'
        if l not in seen:
            seen[l] = len(pool); pool += l
        struct.pack_into('<I', out, c + 4 * i, len(l) << 22 | seen[l])
    out += pool
    out += bytes(al(len(out)) - len(out))
    return decrypt(bytes(out))  # the cipher is an xor stream: applying it again encrypts
