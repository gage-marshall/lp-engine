"""Unpack LovePlus+ 0x15618 archives (recursively), LZ10/LZ11-decompressing entries."""
import struct,sys,os,collections
MAGIC=0x15618
def lz(d):
    t=d[0]; n=d[1]|d[2]<<8|d[3]<<16; p=4
    if t==0x10:
        out=bytearray()
        while len(out)<n:
            fl=d[p]; p+=1
            for b in range(8):
                if len(out)>=n: break
                if fl&(0x80>>b):
                    a,c=d[p],d[p+1]; p+=2
                    l=(a>>4)+3; disp=((a&15)<<8|c)+1
                    for _ in range(l): out.append(out[-disp])
                else: out.append(d[p]); p+=1
        return bytes(out[:n])
    if t==0x11:
        out=bytearray()
        while len(out)<n:
            fl=d[p]; p+=1
            for b in range(8):
                if len(out)>=n: break
                if fl&(0x80>>b):
                    a=d[p]; k=a>>4
                    if k==0: l=((a&15)<<4|d[p+1]>>4)+0x11; disp=((d[p+1]&15)<<8|d[p+2])+1; p+=3
                    elif k==1: l=((a&15)<<12|d[p+1]<<4|d[p+2]>>4)+0x111; disp=((d[p+2]&15)<<8|d[p+3])+1; p+=4
                    else: l=k+1; disp=((a&15)<<8|d[p+1])+1; p+=2
                    for _ in range(l): out.append(out[-disp])
                else: out.append(d[p]); p+=1
        return bytes(out[:n])
    raise ValueError
def maybe_lz(b,u):
    if len(b)>=4 and b[0] in (0x10,0x11) and (b[1]|b[2]<<8|b[3]<<16)==u and u>0:
        try: return lz(b)
        except Exception: return b
    return b
def is_arc(d): return len(d)>12 and struct.unpack('<I',d[:4])[0]==MAGIC
def entries(d):
    cnt=struct.unpack('<H',d[4:6])[0]; first=struct.unpack('<I',d[12:16])[0]
    stride=(first-12)//cnt if cnt else 12
    for i in range(cnt):
        o,s,u=struct.unpack('<3I',d[12+i*stride:24+i*stride])
        name=d[24+i*stride:12+(i+1)*stride].split(b'\0')[0].decode('latin1') if stride>12 else ''
        yield i,o,s,u,name
EXT={b'RGCN':'ncgr',b'RLCN':'nclr',b'RCSN':'nscr',b'RECN':'ncer',b'RNAN':'nanr',b'RCMN':'nmcr',b'RNFN':'nftr',b'0DMB':'nsbmd',b'0XTB':'nsbtx',b'0ACB':'nsbca',b'0TAB':'nsbta',b'0APB':'nsbtp',b'0AMB':'nsbma',b'0CVB':'nsbva',b'SBIN':'sbin',b'TERM':'term'}
stats=collections.Counter()
def unpack(d,outdir,depth=0):
    os.makedirs(outdir,exist_ok=True)
    for i,o,s,u,name in entries(d):
        b=maybe_lz(d[o:o+s],u)
        if len(b)<=u and False: pass
        mg=b[:4]
        base=os.path.basename(name.replace('\\','/')) if name else ''
        nm='%04d_%s'%(i,base) if base else '%04d'%i
        if is_arc(b):
            unpack(b,os.path.join(outdir,nm),depth+1); continue
        ext=EXT.get(mg) or ('bin')
        stats[ext]+=1
        open(os.path.join(outdir,nm+('' if nm.lower().endswith('.'+ext) else '.'+ext)),'wb').write(b)
if __name__=='__main__':
    src,dst=sys.argv[1:3]
    for root,_,fs in os.walk(src):
        for f in fs:
            p=os.path.join(root,f)
            with open(p,'rb') as fh: h=fh.read(4)
            if h==b'\x18\x56\x01\x00':
                d=open(p,'rb').read(); rel=os.path.relpath(p,src)
                unpack(d,os.path.join(dst,rel))
    print(stats)
