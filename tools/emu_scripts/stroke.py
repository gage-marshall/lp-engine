import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/end6.dst')
pts=[D.page_to_touch(rx,79) for rx in range(25,115,6)]
e.drag(pts); e.step(120); e.shot(S+'/o2.png',rotate=True,scale=2); e.save(S+'/end7.dst')
