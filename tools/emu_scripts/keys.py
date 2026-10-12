import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
st=sys.argv[1]; seq=sys.argv[2].split(','); fr=int(sys.argv[3]); out=sys.argv[4]; sv=sys.argv[5] if len(sys.argv)>5 else None
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/'+st+'.dst'); ims=[e.shot('/tmp/x.png',rotate=True).resize((288,192))]
for k in seq:
    if k.startswith('t'): x,y=map(int,k[1:].split(':')); e.touch(x,y,hold=6,release=4)
    else: e.press(k,hold=6,release=4)
    e.step(fr); ims.append(e.shot('/tmp/x.png',rotate=True).resize((288,192)))
sheet(ims,4,S+'/'+out+'.png')
if sv: e.save(S+'/'+sv+'.dst')
