#!/usr/bin/env python3
"""Estimate how many of r0..r3 a static ARM9 function reads as arguments (registers read before written along its first
straight-line block, following bl / tail calls to callees).  Used by the condition/stage oracle to key the leaf mocks."""
import struct

import capstone as c
from capstone.arm_const import (ARM_REG_R0, ARM_REG_R1, ARM_REG_R2, ARM_REG_R3, ARM_GRP_JUMP, ARM_REG_PC, ARM_REG_LR, ARM_REG_IP,
                                ARM_OP_IMM, ARM_OP_REG, ARM_CC_AL)

BASE = 0x02000000
ARGS = (ARM_REG_R0, ARM_REG_R1, ARM_REG_R2, ARM_REG_R3)


class Arity:
    def __init__(self, arm9):
        self.d = arm9
        self.md = c.Cs(c.CS_ARCH_ARM, c.CS_MODE_ARM)
        self.md.detail = True
        self.cache = {}

    def word(self, a):
        return struct.unpack_from('<I', self.d, a - BASE)[0]

    def of(self, addr, depth=0):
        addr &= ~1
        if addr in self.cache:
            return self.cache[addr]
        self.cache[addr] = 0
        res = self._scan(addr, depth)
        self.cache[addr] = res
        return res

    def _scan(self, addr, depth):
        if not (BASE <= addr < BASE + len(self.d)):
            return 0
        reads = set()
        written = set()
        pc = addr
        ipval = None
        for _ in range(60):
            if not (BASE <= pc < BASE + len(self.d) - 4):
                break
            ins = next(self.md.disasm(self.d[pc - BASE:pc - BASE + 4], pc), None)
            if ins is None:
                break
            cond_al = ins.cc in (ARM_CC_AL, 0)
            rr, rw = ins.regs_access()
            if ins.mnemonic.startswith(('push', 'stmdb', 'stm')) and ARM_REG_LR in rr:
                rr = []                                   # `push {r3, lr}` only keeps the stack aligned
            for r in rr:
                if r in ARGS and r not in written:
                    reads.add(r)
            m = ins.mnemonic
            if m in ('bl', 'blx') and ins.op_str.startswith('#'):
                tgt = int(ins.op_str[1:], 16)
                if depth < 5:
                    n = self.of(tgt, depth + 1)
                    for i in range(n):
                        if ARGS[i] not in written:
                            reads.add(ARGS[i])
                if cond_al:
                    written |= set(ARGS)
                    break
                pc += 4
                continue
            if m.startswith('b') and ins.op_str.startswith('#') and not m.startswith('bl'):
                tgt = int(ins.op_str[1:], 16)
                if cond_al:
                    if not (addr <= tgt < addr + 0x400):          # tail call
                        if depth < 5:
                            n = self.of(tgt, depth + 1)
                            for i in range(n):
                                if ARGS[i] not in written:
                                    reads.add(ARGS[i])
                    break
                pc += 4
                continue
            if m.startswith('bx'):
                # `ldr ip,[pc,#x]; bx ip` thunk
                if ipval is not None and depth < 5:
                    n = self.of(ipval, depth + 1)
                    for i in range(n):
                        if ARGS[i] not in written:
                            reads.add(ARGS[i])
                break
            if m == 'ldr' and ins.op_str.startswith('ip, [pc'):
                try:
                    off = int(ins.op_str.split('#')[1].rstrip(']'), 16)
                    ipval = self.word(pc + 8 + off)
                except Exception:
                    ipval = None
            if m.startswith('pop') and ARM_REG_PC in rw:
                break
            for r in rw:                       # conditional writes count as writes (the reading half is conditional too)
                if r in ARGS:
                    written.add(r)
            pc += 4
        n = 0
        for i, r in enumerate(ARGS):
            if r in reads:
                n = i + 1
        return n


if __name__ == '__main__':
    import csv
    import sys
    arm9 = open(sys.argv[1], 'rb').read()
    ar = Arity(arm9)
    names = {r['name']: int(r['addr'], 16) for r in csv.DictReader(open(sys.argv[2])) if r['module'] == 'static'}
    for n in sys.argv[3:]:
        print(n, ar.of(names[n]))
