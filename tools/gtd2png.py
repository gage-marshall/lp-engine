"""LovePlus+ GTD image (8bpp indexed, RGB555 palette) -> PNG."""
import struct,sys
from PIL import Image
def gtd(d, stride=None):
    b0,b1,b2,b3,w,h,po,pl,px=struct.unpack('<BBBBHHHHI',d[:16])
    pal=[struct.unpack('<H',d[po+i*2:po+i*2+2])[0] for i in range(pl//2)]
    rgb=[((c&31)*255//31,((c>>5)&31)*255//31,((c>>10)&31)*255//31) for c in pal]
    pix=d[po+pl:po+pl+px]
    st=stride or next(1<<k for k in range(3,13) if (1<<k)>=w)
    rows=px//st
    img=Image.new('P',(st,rows)); flat=[]
    for r in rgb: flat+=list(r)
    img.putpalette(flat+[0]*(768-len(flat)))
    img.putdata(list(pix[:st*rows]))
    img=img.crop((0,0,w,min(h,rows)))
    if b3==2: img=img.transpose(Image.ROTATE_90)
    return img,(b0,b1,b2,b3,w,h,po,pl,px)
if __name__=='__main__':
    img,h=gtd(open(sys.argv[1],'rb').read()); print(h); img.save(sys.argv[2])
