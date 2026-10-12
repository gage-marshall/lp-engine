import os,sys,struct
sys.path.insert(0,os.path.dirname(__file__))
from gtd2png import gtd
src,dst=sys.argv[1:3]; n=bad=0
for r,_,fs in os.walk(src):
    for f in fs:
        if not f.endswith('.bin'): continue
        p=os.path.join(r,f); d=open(p,'rb').read()
        if len(d)<32 or d[0]!=4 or d[1] not in (4,5): continue
        w,h,po,pl,px=struct.unpack('<HHHHI',d[4:16])
        if po!=16 or 16+pl+px!=len(d) or not(0<w<=512 and 0<h<=512): continue
        try:
            img,_=gtd(d); o=os.path.join(dst,os.path.relpath(p,src)[:-4]+'.png')
            os.makedirs(os.path.dirname(o),exist_ok=True); img.convert('RGBA').save(o); n+=1
        except Exception as e: bad+=1
print(n,'converted',bad,'failed')
