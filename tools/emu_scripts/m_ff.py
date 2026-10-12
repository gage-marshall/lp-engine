import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
import numpy as np
mode=sys.argv[1]
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/end22.dst')
def reg():
    im=e.image().crop((0,192,256,384)).rotate(90,expand=True); a=np.asarray(im.convert('L')).astype(int); return int((a[200:252,4:178]>225).sum())
e.touch(30,100,hold=4,release=1)
vals=[]
for t in range(300):
    if t==80:
        if mode=='tap': e.e.input.touch_set_pos(30,100)
        if mode=='hold': e.e.input.touch_set_pos(30,100)
    if t==84 and mode=='tap': e.e.input.touch_release()
    e.step(1); vals.append(reg())
e.e.input.touch_release()
ch=[t for t in range(1,300) if vals[t]>vals[t-1]]
print(mode,'growth frames',ch[:70]); print('final',vals[-1],'at200',vals[200])
