import struct,sys,os
rom=open(sys.argv[1],'rb'); out=sys.argv[2]
d=rom.read()
fnt_off,fnt_sz,fat_off,fat_sz=struct.unpack('<4I',d[0x40:0x50])
fat=[struct.unpack('<2I',d[fat_off+i*8:fat_off+i*8+8]) for i in range(fat_sz//8)]
def dirs(did,path):
    ent=struct.unpack('<IHH',d[fnt_off+(did&0xfff)*8:fnt_off+(did&0xfff)*8+8])
    p=fnt_off+ent[0]; fid=ent[1]
    while True:
        t=d[p]; p+=1
        if t==0: break
        n=t&0x7f; name=d[p:p+n].decode('latin1'); p+=n
        if t&0x80:
            sub=struct.unpack('<H',d[p:p+2])[0]; p+=2
            yield from dirs(sub,path+name+'/')
        else:
            yield fid,path+name; fid+=1
n=0
for fid,path in dirs(0xf000,''):
    s,e=fat[fid]; fp=os.path.join(out,path); os.makedirs(os.path.dirname(fp),exist_ok=True)
    open(fp,'wb').write(d[s:e]); n+=1
print(n,'files; total fat',len(fat))
