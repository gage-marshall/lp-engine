import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
import numpy as np
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/st5.dst')
prev=None; ch=[]
for t in range(120):
    e.step(1); a=np.asarray(e.image().convert('RGB'))[:192].astype(int)   # top screen raw
    b=np.asarray(e.image().convert('RGB'))[192:].astype(int)
    if prev is not None: ch.append((int(np.abs(a-prev[0]).sum()>0),int(np.abs(b-prev[1]).sum()>0)))
    prev=(a,b)
print('TOPCH',''.join(str(c[0]) for c in ch)); print('BOTCH',''.join(str(c[1]) for c in ch))
