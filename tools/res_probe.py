#!/usr/bin/env python3
"""Headless: advance a savestate with A presses and log archive opens / entry loads around script starts.
usage: res_probe.py STATE [frames]"""
import os, sys
HERE = os.path.dirname(os.path.abspath(__file__)); sys.path.insert(0, HERE)
from emu_driver import Emu
ROOT = os.path.dirname(HERE)
state = sys.argv[1]; maxf = int(sys.argv[2]) if len(sys.argv) > 2 else 6000
emu = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), state if os.path.exists(state) else os.path.join(HERE, 'emu_scripts', 'states', state + '.dst'))
m = emu.e.memory
def s(addr):
    try:
        b = bytearray()
        for i in range(48):
            c = m.unsigned.read_byte(addr + i)
            if c == 0: break
            b.append(c)
        return b.decode('latin1') if b and all(32 <= x < 127 for x in b) else None
    except Exception: return None
def hook(addr, name):
    def cb(a, sz):
        r = m.register_arm9
        args = [r.r0, r.r1, r.r2, r.r3]
        st = [s(x) for x in args if 0x2000000 <= x < 0x2400000]
        print('%6d %-22s %s %s' % (emu.frames, name, ' '.join('%x' % x for x in args), [x for x in st if x]))
    m.register_exec(addr, cb)
hook(0x204e644, 'ResTable_Open'); hook(0x204e838, 'LoadEntryAlloc'); hook(0x204e890, 'ReadEntry'); hook(0x204ea14, 'LoadEntryByName')
hook(0x205e7c4, 'SCRIPT_START')
from desmume.controls import keymask
from emu_driver import KEYS
for f in range(maxf):
    if f % 40 == 0: emu.e.input.keypad_update(keymask(KEYS['A']))
    elif f % 40 == 6: emu.e.input.keypad_update(0)
    emu.step(1)
