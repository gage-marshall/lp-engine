"""Emulate script command handlers (GameFlow_RunMainLoop @0x205a8f4) with tagged args and trace what they call.

Setup mirrors the VM ABI: stack[sp]=argc, stack[sp+1]=last pushed arg ... stack[sp+argc]=first arg (a0).
Arg tags: a_i = 0x1001+i (int style) or 0x80001001+i (string style, bit31 set).
Depth 0 = RunMainLoop itself, depth 1 = handler callee (executed natively), depth >= 2 = recorded + stubbed.
Needs unicorn (use a venv). Output: fid/emu_cmds.json
"""
import csv, json, struct, sys, collections
from unicorn import *
from unicorn.arm_const import *

BASE = 0x2000000
RUNMAIN = 0x205a8f4
arm9 = open('work/arm9_dec.bin', 'rb').read()
LAB = {}
for r in csv.DictReader(open('fid/labels_all.csv')):
    if r['module'] == 'static':
        LAB[int(r['addr'], 16)] = r['name']
N = {v: k for k, v in LAB.items()}
GETCTX, TASK = N['Script_GetContext'], N['TaskList_TickTaskMain']
# python-implemented helpers (cheap, side-effect-free for our purposes)
PYHOOK = {N['Script_ReadLengthPrefixedString']: 'str', N['Char_LookupIdByName']: 'char',
          N['Libc_Strlen']: 'strlen', N['memset']: 'memset', N['MI_CpuFill8']: 'fill8', GETCTX: 'ctx'}
# small arg-reader helpers executed natively even at depth >= 2
NATIVE_DEEP = {N['ScriptOp_ReadOptionalArg'], N['Gfx3D_UpdateObjectPosition']}
CTX, STK, REF, STACKTOP, RET = 0x2300000, 0x2310000, 0x2320000, 0x23f0000, 0x23fff00
TEXT_END = 0x2110000
REGS = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)


def make():
    uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    uc.mem_map(0x1ff8000, 0x8000)
    uc.mem_map(BASE, 0x400000)
    uc.mem_map(0x27e0000, 0x4000)
    uc.mem_write(BASE, arm9)
    return uc


def run_one(uc, cmd, argc, style, ret_default=0, maxins=20000):
    ev = []
    st = {'n': 0, 'last': 0}
    shadow = []  # return addresses of active calls
    uc.mem_write(CTX, b'\0' * 0x2000)
    uc.mem_write(STK, b'\0' * 0x400)
    uc.mem_write(REF, b'\0' * 0x100)

    def tag(i):
        return (0x80001001 + i) if style == 'str' else 0x1001 + i
    words = [argc] + [tag(i) for i in reversed(range(argc))]
    sp = 16
    for k, w in enumerate(words):
        uc.mem_write(STK + (sp + k) * 4, struct.pack('<I', w))
    uc.mem_write(CTX + 0x10cc, struct.pack('<I', sp))
    uc.mem_write(CTX + 0x10d4, struct.pack('<I', STK))
    uc.mem_write(CTX + 0x10c0, struct.pack('<I', cmd))
    uc.mem_write(REF + 0xc, struct.pack('<I', REF + 0x40))
    uc.mem_write(REF + 0x14, struct.pack('<I', REF + 0x80))
    rd = uc.reg_read

    def hook(uc, addr, size, ud):
        st['n'] += 1
        if st['n'] > maxins or addr == RET:
            uc.emu_stop(); return
        while shadow and addr == shadow[-1]:
            shadow.pop()
        lr = rd(UC_ARM_REG_LR)
        is_call = (lr == st['last'] + 4) and addr != st['last'] + 4 and addr != RUNMAIN
        st['last'] = addr
        if not is_call:
            return
        depth = len(shadow) + 1  # callee depth relative to RunMainLoop
        args = [rd(x) for x in REGS]
        h = PYHOOK.get(addr)
        if h:
            if h == 'ctx': uc.reg_write(UC_ARM_REG_R0, CTX)
            elif h == 'str':
                s = b'S%x' % (args[3] & 0x7fffffff)
                uc.mem_write(args[1], s + b'\0')
                ev.append(('str', depth, 'ReadString', [args[3]]))
                uc.reg_write(UC_ARM_REG_R0, len(s))
            elif h == 'char':
                ev.append(('char', depth, 'CharLookup', []))
                uc.reg_write(UC_ARM_REG_R0, 0x7000)
            elif h == 'strlen': uc.reg_write(UC_ARM_REG_R0, 2)
            elif h in ('memset', 'fill8'): uc.mem_write(args[0], bytes([args[1] & 0xff]) * args[2])
            uc.reg_write(UC_ARM_REG_PC, lr)
            return
        stub = False
        if addr == TASK:
            snap = [struct.unpack('<I', bytes(uc.mem_read(args[0] + 4 * k, 4)))[0] for k in range(14)]
            ev.append(('task', depth, 'TaskList_TickTaskMain', snap)); stub = True
        elif addr >= TEXT_END or addr < BASE:
            ev.append(('ext', depth, 'ext_%x' % addr, args)); stub = True
        elif depth >= 2 and addr not in NATIVE_DEEP:
            ev.append(('call', depth, LAB.get(addr, 'sub_%x' % addr), args)); stub = True
        elif depth == 1:
            ev.append(('enter', depth, LAB.get(addr, 'sub_%x' % addr), args))
        if stub:
            uc.reg_write(UC_ARM_REG_R0, ret_default)
            uc.reg_write(UC_ARM_REG_PC, lr)
        else:
            shadow.append(lr)
    h = uc.hook_add(UC_HOOK_CODE, hook)
    uc.reg_write(UC_ARM_REG_SP, STACKTOP)
    uc.reg_write(UC_ARM_REG_LR, RET)
    uc.reg_write(UC_ARM_REG_R0, REF)
    try:
        uc.emu_start(RUNMAIN, RET, count=maxins * 2)
    except UcError as e:
        ev.append(('fault', 0, str(e), [rd(UC_ARM_REG_PC)]))
    uc.hook_del(h)
    return ev


if __name__ == '__main__':
    uc = make()
    cmds = [int(r['cmd'], 16) for r in csv.DictReader(open('fid/script_commands.csv'))]
    if len(sys.argv) > 1:
        cmds = [int(x, 16) for x in sys.argv[1:]]
    out = {}
    for c in cmds:
        out[hex(c)] = {}
        for argc in range(0, 9):
            for style in ('int', 'str'):
                out[hex(c)]['%d%s' % (argc, style)] = run_one(uc, c, argc, style)
    json.dump(out, open('fid/emu_cmds.json', 'w'))
    print(len(out), 'commands traced')
