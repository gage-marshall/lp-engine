import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/st1.dst'); ims=[]
def snap(): ims.append(e.shot('/tmp/x.png',rotate=True).resize((288,192)))
e.touch(180,16,hold=20,release=10); e.step(60); snap()
e.step(200); snap()
e.touch(165,10,hold=40,release=10); e.step(120); snap()
e.press('A',hold=10,release=10); e.step(120); snap()
sheet(ims,4,S+'/k7.png')
