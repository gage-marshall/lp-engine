"""Unicorn-based partial emulator for LovePlus+ ARM9 (static module + chosen overlays).

Calls are classified by depth relative to the entry function (depth 1 = direct callee):
  depth <= native_depth : executed natively (recorded as 'enter')
  deeper                : recorded as 'call' and stubbed (returns ret_default)
Special: TaskList_TickTaskMain records its descriptor as 'task'; python hooks for ctx/strings/memset.
Needs unicorn (use a venv).
"""
import csv, struct
from unicorn import *
from unicorn.arm_const import *

BASE = 0x2000000
ARM9 = open('work/arm9_dec.bin', 'rb').read()
REGS = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)
CTX, STK, REF, STACKTOP, RET = 0x2300000, 0x2310000, 0x2320000, 0x23f0000, 0x23fff00
OVBASE = {}  # filled from arm9_overlays.csv
OVSIZE = {}
for r in csv.DictReader(open('work/overlays/arm9_overlays.csv')):
    OVBASE[int(r['id'])] = int(r['ram'], 16)
    OVSIZE[int(r['id'])] = int(r['declen'])

LAB = {}      # static labels
OVLAB = {}    # overlay id -> {addr: name}
for r in csv.DictReader(open('fid/labels_all.csv')):
    a = int(r['addr'], 16)
    if r['module'] == 'static':
        LAB[a] = r['name']
    else:
        OVLAB.setdefault(int(r['module'][2:]), {})[a] = r['name']
N = {v: k for k, v in LAB.items()}
GETCTX, TASK = N['Script_GetContext'], N['TaskList_TickTaskMain']
PYHOOK = {N['Script_ReadLengthPrefixedString']: 'str', N['Char_LookupIdByName']: 'char',
          N['Libc_Strlen']: 'strlen', N['memset']: 'memset', N['MI_CpuFill8']: 'fill8', GETCTX: 'ctx'}
NATIVE_DEEP = {N['ScriptOp_ReadOptionalArg'], N['Gfx3D_UpdateObjectPosition']}


class Emu:
    def __init__(self, overlays=()):
        self.overlays = list(overlays)
        uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.mem_map(0x1ff8000, 0x8000)
        uc.mem_map(BASE, 0x400000)
        uc.mem_map(0x27e0000, 0x4000)
        uc.mem_write(BASE, ARM9)
        self.ovrange = []
        self.names = dict(LAB)
        for ov in self.overlays:
            d = open('work/overlays/arm9_ov%04d.bin' % ov, 'rb').read()
            uc.mem_write(OVBASE[ov], d)
            self.ovrange.append((OVBASE[ov], OVBASE[ov] + len(d)))
            self.names.update(OVLAB.get(ov, {}))
        self.uc = uc
        self.scratch = 0x2340000
        def unmapped(uc, access, addr, size, value, ud):
            page = addr & ~0xfff
            try:
                uc.mem_map(page, 0x1000)
            except UcError:
                return False
            return True
        uc.hook_add(UC_HOOK_MEM_UNMAPPED, unmapped)
        self.text_end = 0x2110000

    def name(self, a):
        return self.names.get(a, 'sub_%x' % a)

    def is_code(self, a):
        return BASE <= a < self.text_end or any(lo <= a < hi for lo, hi in self.ovrange)

    def setup_script_ctx(self, cmd, argc, style):
        uc = self.uc
        uc.mem_write(CTX, b'\0' * 0x2000); uc.mem_write(STK, b'\0' * 0x400); uc.mem_write(REF, b'\0' * 0x100)
        tag = (lambda i: 0x80001001 + i) if style == 'str' else (lambda i: 0x1001 + i)
        words = [argc] + [tag(i) for i in reversed(range(argc))]
        for k, w in enumerate(words):
            uc.mem_write(STK + (16 + k) * 4, struct.pack('<I', w))
        uc.mem_write(CTX + 0x10cc, struct.pack('<I', 16))
        uc.mem_write(CTX + 0x10d4, struct.pack('<I', STK))
        uc.mem_write(CTX + 0x10c0, struct.pack('<I', cmd))
        uc.mem_write(REF + 0xc, struct.pack('<I', REF + 0x40))
        uc.mem_write(REF + 0x14, struct.pack('<I', REF + 0x80))

    def run(self, entry, regs=(), native_depth=1, ret_default=0, maxins=30000, gate_loaded=None):
        uc = self.uc; ev = []; st = {'n': 0, 'last': 0}; shadow = []
        rd = uc.reg_read

        def hook(uc, addr, size, ud):
            st['n'] += 1
            if st['n'] > maxins or addr == RET:
                uc.emu_stop(); return
            while shadow and addr == shadow[-1]:
                shadow.pop()
            lr = rd(UC_ARM_REG_LR)
            is_call = (lr == st['last'] + 4) and addr != st['last'] + 4 and addr != entry
            st['last'] = addr
            if not is_call:
                return
            depth = len(shadow) + 1
            args = [rd(x) for x in REGS]
            h = PYHOOK.get(addr)
            if h:
                if h == 'ctx': uc.reg_write(UC_ARM_REG_R0, CTX)
                elif h == 'str':
                    s = b'S%x' % (args[3] & 0x7fffffff); uc.mem_write(args[1], s + b'\0')
                    ev.append(('str', depth, 'ReadString', [args[3]])); uc.reg_write(UC_ARM_REG_R0, len(s))
                elif h == 'char':
                    ev.append(('char', depth, 'CharLookup', [])); uc.reg_write(UC_ARM_REG_R0, 0x7000)
                elif h == 'strlen': uc.reg_write(UC_ARM_REG_R0, 2)
                elif h in ('memset', 'fill8'): uc.mem_write(args[0], bytes([args[1] & 0xff]) * args[2])
                uc.reg_write(UC_ARM_REG_PC, lr); return
            stub = False
            nm = self.name(addr)
            if addr == TASK and depth >= 1 and entry != TASK:
                snap = [struct.unpack('<I', bytes(uc.mem_read(args[0] + 4 * k, 4)))[0] for k in range(14)]
                ev.append(('task', depth, 'TaskList_TickTaskMain', snap)); stub = True
            elif not self.is_code(addr):
                ev.append(('ext', depth, 'ext_%x' % addr, args)); stub = True
            elif nm == 'Overlay_IsLoaded' and gate_loaded is not None:
                val = 1 if (gate_loaded == 'all' or args[0] in gate_loaded) else 0
                ev.append(('gate', depth, nm, args + [val])); uc.reg_write(UC_ARM_REG_R0, val); uc.reg_write(UC_ARM_REG_PC, lr); return
            elif depth > native_depth and addr not in NATIVE_DEEP:
                ev.append(('call', depth, nm, args)); stub = True
            else:
                ev.append(('enter', depth, nm, args))
            if stub:
                rv = self.scratch if nm.startswith(('TaskList_AllocTask', 'GameHeap_Alloc')) else ret_default
                uc.reg_write(UC_ARM_REG_R0, rv); uc.reg_write(UC_ARM_REG_PC, lr)
            else:
                shadow.append(lr)
        h = uc.hook_add(UC_HOOK_CODE, hook)
        uc.reg_write(UC_ARM_REG_SP, STACKTOP); uc.reg_write(UC_ARM_REG_LR, RET)
        for r, v in zip(REGS, regs): uc.reg_write(r, v)
        try:
            uc.emu_start(entry, RET, count=maxins * 2)
        except UcError as e:
            ev.append(('fault', 0, str(e), [rd(UC_ARM_REG_PC)]))
        uc.hook_del(h)
        return ev, rd(UC_ARM_REG_R0)
