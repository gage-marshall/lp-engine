#!/usr/bin/env python3
"""Differential oracle for the condition-op handlers: runs the *real* ARM handler (arm9_main.bin in Unicorn) with every call
that leaves the condition module (0x2067300-0x206c440) answered by a deterministic mock game state (tools/cond_mock.py),
and returns bit 0 of r0 (the caller `CondList_EvaluateGroups` ANDs `r0 & 1`).

    from cond_oracle import Oracle
    o = Oracle(); o.run(op, a, b, mock) -> (passed: bool, trace: [(leaf name, args...)])
"""
import csv
import json
import os
import struct

from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED, UcError
from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR,
                               UC_ARM_REG_PC)

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
BASE = 0x2000000
MOD_LO, MOD_HI = 0x2067300, 0x206c440
RET = 0x23fff00
STACKTOP = 0x23f0000
SCRATCH = 0x2340000
REGS = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)


class Oracle:
    def __init__(self):
        arm9 = open(os.path.join(ROOT, 'work/arm9_main.bin'), 'rb').read()
        self.names = {int(r['addr'], 16): r['name'] for r in csv.DictReader(open(os.path.join(ROOT, 'fid/labels_all.csv')))
                      if r['module'] == 'static'}
        ev = json.load(open(os.path.join(ROOT, 'content/engine/event_selection.json')))
        self.handlers = ev['cond_handlers']
        uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.mem_map(0x1ff8000, 0x8000)
        uc.mem_map(BASE, 0x400000)
        uc.mem_map(0x27e0000, 0x4000)
        uc.mem_write(BASE, arm9)

        def unmapped(uc, access, addr, size, value, ud):
            try:
                uc.mem_map(addr & ~0xfff, 0x1000)
            except UcError:
                return False
            return True
        uc.hook_add(UC_HOOK_MEM_UNMAPPED, unmapped)
        self.uc = uc

    def run(self, op, a, b, mock, maxins=200000):
        h = self.handlers[op]
        return self.call(h, (op, a & 0xffffffff, b & 0xffffffff, 0), mock, maxins)

    def call(self, h, regs, mock, maxins=200000):
        uc = self.uc
        trace = []
        st = {'n': 0, 'last': 0}
        rd = uc.reg_read

        def hook(uc, addr, size, ud):
            st['n'] += 1
            if st['n'] > maxins or addr == RET:
                uc.emu_stop()
                return
            lr = rd(UC_ARM_REG_LR)
            last = st['last']
            in_mod = MOD_LO <= addr < MOD_HI
            # a call (bl) or a tail call (bx/b to another function) leaving the module is a leaf call
            is_call = ((lr == last + 4) and addr != last + 4 and addr != h) or \
                      (not in_mod and MOD_LO <= last < MOD_HI and addr != last + 4)
            st['last'] = addr
            if not is_call:
                return
            if in_mod:
                return                      # helper inside the condition module: run natively
            args = [rd(r) for r in REGS]
            name = self.names.get(addr, 'sub_%x' % addr)
            r0 = mock.leaf(name, args, uc, rd(UC_ARM_REG_SP))
            trace.append((name,) + tuple(args))
            uc.reg_write(UC_ARM_REG_R0, r0 & 0xffffffff)
            uc.reg_write(UC_ARM_REG_PC, lr)
        hk = uc.hook_add(UC_HOOK_CODE, hook)
        uc.reg_write(UC_ARM_REG_SP, STACKTOP)
        uc.reg_write(UC_ARM_REG_LR, RET)
        for r, v in zip(REGS, regs):
            uc.reg_write(r, v)
        err = None
        try:
            uc.emu_start(h, RET, count=maxins * 2)
        except UcError as e:
            err = str(e)
        uc.hook_del(hk)
        r0 = rd(UC_ARM_REG_R0)
        return (r0 & 1) == 1, trace, err, r0

    def collect(self, term_bytes, thr, mock):
        """Native CondList_CollectPassingRecords (0x206743c) on one TERM file.  Returns ([(id, flag_marker)], final_threshold)."""
        uc = self.uc
        FILE, OUT, ARR = 0x2400000, 0x2410000, 0x2420000
        try:
            uc.mem_map(0x2400000, 0x100000)
        except Exception:
            pass
        uc.mem_write(FILE, term_bytes)
        uc.mem_write(OUT, struct.pack('<2I', 0, ARR))
        ok, tr, err, r0 = self.call(0x206743c, (FILE, OUT, thr & 0xffffffff, 0), mock)
        cnt = r0
        final = struct.unpack('<I', bytes(uc.mem_read(OUT, 4)))[0]
        arr = [struct.unpack('<2I', bytes(uc.mem_read(ARR + 8 * i, 8))) for i in range(min(cnt, 512))]
        return arr, final, err
