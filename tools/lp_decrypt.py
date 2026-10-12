"""LovePlus+ TXRC/SBIN stream cipher (from TextRes_GetEntryField4 @ 0x209e93c).
file = magic[4] key[3] mult[1] payload...;  plain[i] = c[i] ^ ((mult*(i//3) + key[i%3]) & 0xff)  (i = absolute offset)"""
def decrypt(d, start=8):
    m=d[7]; k=d[4:7]; out=bytearray(d)
    for i in range(start,len(d)): out[i]=d[i]^((m*(i//3)+k[i%3])&0xff)
    return bytes(out)
if __name__=='__main__':
    import sys; d=decrypt(open(sys.argv[1],'rb').read()); sys.stdout.buffer.write(d)
