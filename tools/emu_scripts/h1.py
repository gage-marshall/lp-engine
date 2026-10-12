import os, sys; sys.path.insert(0,os.path.dirname(os.path.abspath(__file__))); from nav import *
e=D.Emu('rom/loveplus_plus_j.nds',state=S+'/st5.dst')
print('div bytes',[e.e.memory.unsigned.read_byte(0x213d2e4+o) for o in (0xa0,0xa1,0xa3)])
e.shot(S+'/h_full.png',rotate=True,scale=2)
