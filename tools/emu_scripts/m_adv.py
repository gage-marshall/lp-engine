import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
import numpy as np
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/end22.dst')
def reg():
    im=e.image().crop((0,192,256,384)).rotate(90,expand=True); a=np.asarray(im.convert('L')).astype(int); return int((a[200:252,4:178]>225).sum())
e.touch(30,100,hold=4,release=1)
vals=[]
for t in range(300):
    e.step(1); vals.append(reg())
ch=[t for t in range(1,300) if vals[t]>vals[t-1]]
print('growth frames',ch[:60]); print('intervals',np.diff(ch)[:60].tolist()); print(vals[::30])
