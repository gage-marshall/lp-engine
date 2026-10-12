import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
import collections
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/'+sys.argv[1]+'.dst')
if len(sys.argv)>2: e.touch(128,96,hold=6,release=6)
m=e.e.memory.unsigned
rb=m.read_byte if hasattr(m,'read_byte') else None
base=0x213d2e4
seq=[]
for t in range(240):
    e.step(1); seq.append((rb(base+0xa0),rb(base+0xa1),rb(base+0xa3)))
print(collections.Counter(seq).most_common(8)); print(seq[:24])
