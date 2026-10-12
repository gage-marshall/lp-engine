import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
import numpy as np
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/end22.dst')
e.touch(30,100,hold=4,release=1); e.step(60); e.touch(30,100,hold=4,release=4); e.step(120)
im=e.image().crop((0,192,256,384)).rotate(90,expand=True); a=np.asarray(im).astype(int)
m=(a[:,:,0]>=248)&(a[:,:,1]>=248)&(a[:,:,2]>=248)
sub=m[200:248,8:170]
ys=np.where(sub.sum(axis=1)>0)[0]; lines=[]; s=p=ys[0]
for y in ys[1:]:
    if y-p>1: lines.append((s,p)); s=y
    p=y
lines.append((s,p))
for y0,y1 in lines:
    cols=np.where(sub[y0:y1+1].sum(axis=0)>0)[0]; print('line rows',y0+198,y1+198,'cols',cols.min()+8,cols.max()+8)
