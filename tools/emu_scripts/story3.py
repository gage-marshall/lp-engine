import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
st,n,per,sv=sys.argv[1],int(sys.argv[2]),int(sys.argv[3]),sys.argv[4]
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/'+st+'.dst'); ims=[]
for i in range(n):
    e.touch(30,100,hold=4,release=4); e.step(per)
    e.touch(180,16,hold=10,release=6); e.step(20); e.touch(165,10,hold=10,release=6); e.step(per)
    ims.append(e.shot('/tmp/x.png',rotate=True).resize((288,192)))
sheet(ims,4,S+'/'+sv+'.png'); e.save(S+'/'+sv+'.dst')
