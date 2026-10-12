"""Decode LovePlus+ voice clips by running the game's own G.722.1/1C decoder (G7221_* @0x2000c64..) in Unicorn.
Usage as module: SirenEmu().decode(clip_bytes, swap=False) -> list of int16 samples (32 kHz mono)."""
import struct,sys
sys.path.insert(0,'tools')
import emu_core as E
from unicorn.arm_const import *
INIT=0x2001924; DECODE=0x2001a58
STATE,CBTAB,FRAME,OUT,CB=0x2350000,0x2351000,0x2352000,0x2353000,0x2330100
class SirenEmu:
    def __init__(self,bitrate=24000,bandwidth=14000):
        self.e=E.Emu([]); uc=self.e.uc
        for a in (STATE,CBTAB,FRAME,OUT): 
            try: uc.mem_map(a,0x2000)
            except Exception: pass
        # callback stub: ldr r0,[pc,#0]; bx lr; .word FRAME  (returns pointer to frame data)
        uc.mem_write(CB,struct.pack('<III',0xe59f0000,0xe12fff1e,FRAME))
        self.e.ovrange.append((CB,CB+0x10))
        uc.mem_write(CBTAB,struct.pack('<IIIII',0,0,0,0,CB))
        self.bitrate=bitrate; self.bw=bandwidth
        self.bpf=(bitrate//50)>>3
        ev,ret=self.e.run(INIT,regs=(STATE,CBTAB,bitrate,bandwidth),native_depth=99,maxins=3000000)
        self.init_ret=ret
        self.events=ev
    def decode(self,clip,swap=False):
        uc=self.e.uc; out=[]
        n=len(clip)//self.bpf
        for i in range(n):
            fr=clip[i*self.bpf:(i+1)*self.bpf]
            if swap: fr=b''.join(fr[j+1:j+2]+fr[j:j+1] for j in range(0,len(fr)-1,2))
            uc.mem_write(FRAME,fr+b'\0'*8)
            ev,ret=self.e.run(DECODE,regs=(STATE,OUT,0,0),native_depth=99,maxins=3000000)
            cnt=ret
            if cnt not in (320,640): out.extend([0]*640); continue
            out.extend(struct.unpack('<%dh'%cnt,bytes(uc.mem_read(OUT,cnt*2))))
        return out
if __name__=='__main__':
    import wave
    vtb=open('work/fs/sys/Siren14Voice.vtb','rb').read(); f=open('work/fs/sys/Siren14Voice.spp','rb')
    idx=int(sys.argv[1]) if len(sys.argv)>1 else 0
    o,s=struct.unpack('<II',vtb[idx*8:idx*8+8]); f.seek(o//16); clip=f.read(s//16)
    se=SirenEmu(); print('init ret',hex(se.init_ret),'state',hex(STATE))
    for swap in (False,True):
        pcm=se.decode(clip,swap)
        path='%s_clip%d_%s.wav'%(sys.argv[2] if len(sys.argv)>2 else 'out',idx,'swap' if swap else 'raw')
        w=wave.open(path,'wb'); w.setnchannels(1); w.setsampwidth(2); w.setframerate(32000); w.writeframes(struct.pack('<%dh'%len(pcm),*pcm)); w.close()
        import statistics
        print(path,len(pcm),'samples rms %.0f peak %d'%((sum(x*x for x in pcm)/len(pcm))**.5,max(abs(x) for x in pcm)))
