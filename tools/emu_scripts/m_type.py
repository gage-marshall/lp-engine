import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
import numpy as np
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/'+sys.argv[1]+'.dst')
def reg():
    im=e.image().crop((0,0,256,192)).rotate(90,expand=True); a=np.asarray(im.convert('L')).astype(int); return int((a[205:250,6:185]<90).sum())
if len(sys.argv)>2: e.touch(128,96,hold=6,release=6)
vals=[]
for t in range(500):
    e.step(1); vals.append(reg())
ch=[(t,vals[t]) for t in range(1,500) if vals[t]!=vals[t-1]]
print(len(ch),ch[:80])
