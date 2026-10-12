#!/usr/bin/env python3
"""Extract heroine save-record field accesses from the small accessor functions that call the record-pointer getter.

Usage: save_fields.py ARM9_MAIN_BIN LABELS_CSV [GETTER_ADDR=0x2063618]   -> prints  offset size sign get/set  function

The getter (GameState_GetSlotPtr, 0x2063618) returns the heroine record base for a slot.  In each accessor the result
register is tracked through `add rX, rX, #imm` / `add rX, rX, rY, lsl #n` (array index) and the next ldr/str
immediate offset is added, giving the record offset (array stride in the `stride` column when indexed).
"""
import bisect
import csv
import struct
import sys

import capstone as c
from capstone.arm_const import ARM_OP_IMM, ARM_OP_MEM, ARM_OP_REG, ARM_REG_R0

BASE = 0x02000000


def main():
    arm9 = open(sys.argv[1], 'rb').read()
    rows = sorted((int(r['addr'], 16), r['name']) for r in csv.DictReader(open(sys.argv[2])) if r['module'] == 'static')
    getter = int(sys.argv[3], 16) if len(sys.argv) > 3 else 0x2063618
    addrs = [a for a, _ in rows]
    md = c.Cs(c.CS_ARCH_ARM, c.CS_MODE_ARM)
    md.detail = True
    out = []
    for idx, (a, name) in enumerate(rows):
        size = (addrs[idx + 1] if idx + 1 < len(addrs) else a + 0x100) - a
        if size > 0x90:
            continue
        ins = list(md.disasm(arm9[a - BASE:a - BASE + size], a))
        if not any(i.mnemonic == 'bl' and i.op_str == '#%#x' % getter for i in ins):
            continue
        base_reg = None
        add = 0
        stride = None
        acc = []
        for i in ins:
            if i.mnemonic == 'bl':
                base_reg = ARM_REG_R0 if i.op_str == '#%#x' % getter else None
                add = 0
                stride = None
                continue
            if base_reg is None:
                continue
            ops = i.operands
            if i.mnemonic == 'add' and len(ops) == 3 and ops[0].type == ARM_OP_REG and ops[0].reg == base_reg:
                if ops[2].type == ARM_OP_IMM:
                    add += ops[2].imm
                elif ops[2].type == ARM_OP_REG:
                    sh = ops[2].shift.value if ops[2].shift.type else 0
                    stride = 1 << sh
            if i.mnemonic.startswith(('ldr', 'str')) and ops and ops[-1].type == ARM_OP_MEM and ops[-1].mem.base == base_reg:
                m = i.mnemonic
                sz = 4
                sg = 'u'
                if m.startswith(('ldrb', 'strb')):
                    sz = 1
                elif m.startswith(('ldrsb',)):
                    sz, sg = 1, 's'
                elif m.startswith(('ldrh', 'strh')):
                    sz = 2
                elif m.startswith(('ldrsh',)):
                    sz, sg = 2, 's'
                acc.append((add + ops[-1].mem.disp, sz, sg, 'get' if m.startswith('ldr') else 'set', stride))
        for off, sz, sg, gs, st in acc:
            out.append((off, sz, sg, gs, st, name))
    for off, sz, sg, gs, st, name in sorted(out):
        print('+%#05x  %d%s  %s  stride=%s  %s' % (off, sz, sg, gs, st or '-', name))


if __name__ == '__main__':
    main()
