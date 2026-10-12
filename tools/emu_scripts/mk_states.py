import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
e=D.Emu('rom/loveplus_plus_j.nds'); e.step(1500); e.save(S+'/title.dst')
e.touch(128,96,hold=10); e.step(300); e.save(S+'/menu.dst')
e.touch(183,90,hold=10); e.step(60); e.touch(183,90,hold=10); e.step(400); e.save(S+'/slots.dst')
e.touch(130,98,hold=12); e.step(200); e.touch(127,98,hold=40); e.step(30); e.save(S+'/intro_clock.dst')
for i in range(3):
    e.touch(128,96,hold=6,release=6); e.step(100)
e.save(S+'/intro_text.dst'); print('saved')
