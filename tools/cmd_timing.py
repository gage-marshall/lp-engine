#!/usr/bin/env python3
"""Measure task timing of script commands in the real game: logs (frame-exact) script commands, TaskList_AllocTask calls and every
TaskList_FreeAfterCountdown callback while tapping through a save state.  Finding (st5, Nene break room): cmd 0x44 wait(N) allocates a
kind-0x22 task with count N and frees it after N callbacks spaced ~3.8 frames apart (4-vblank scene update, ~15 Hz).
Usage: cmd_timing.py OUT.json   (repo root, venv; uses tools/emu_scripts/states/st5.dst)"""
import sys,os,json
os.environ['SDL_VIDEODRIVER']='dummy'
ROOT=os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0,os.path.join(ROOT,'tools'))
import play_monitor as pm
from emu_driver import Emu
emu=Emu(os.path.join(ROOT,'rom/loveplus_plus_j.nds'),os.path.join(ROOT,'tools/emu_scripts/states/st5.dst'))
e=emu.e
def step(n=1):
    for _ in range(n):
        e.cycle(False); emu.frames+=1
emu.step=step
log=[]
def alloc(a,s):
    r=e.memory.register_arm9; log.append(('alloc',emu.frames,r.r0,r.r1,r.r2,r.r3))
def free(a,s):
    r=e.memory.register_arm9; log.append(('free',emu.frames,r.r0))
def cmdh(a,s):
    rd=emu.read32
    ctx=rd(0x21415b8); sp,stk=rd(ctx+0x10cc),rd(ctx+0x10d4); argc=rd(stk+sp*4)
    if argc<8: log.append(('cmd',emu.frames,rd(ctx+0x10c0),[rd(stk+(sp+argc-i)*4) for i in range(argc)]))
import csv
for r in csv.DictReader(open(os.path.join(ROOT,'fid/script_commands.csv'))):
    pass
names={r['name']:int(r['addr'],16) for r in csv.DictReader(open(os.path.join(ROOT,'fid/labels_all.csv'))) if r['module']=='static'}
e.memory.register_exec(names['TaskList_AllocTask'],alloc)
e.memory.register_exec(names['TaskList_FreeAfterCountdown'],free)
seen=set()
for r in csv.DictReader(open(os.path.join(ROOT,'fid/script_commands.csv'))):
    h=int(r['handler'],16)&~1
    if h not in seen: seen.add(h); e.memory.register_exec(h,cmdh)
while emu.frames<2400:
    emu.touch(30,100,hold=3,release=27)
json.dump(log,open(sys.argv[1],'w'))
