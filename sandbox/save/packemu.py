"""Run the game's own save packers (overlay 0, `Ov00Init_PackSave*`) under Unicorn on the RAM of a savestate, and log every
entry write. Used to derive and prove the save layout (savefmt.py).

    from packemu import PackEmu
    pe = PackEmu(dst_path)          # RAM of a py-desmume savestate taken together with a backup export
    buf, log = pe.pack('A')         # packed block bytes (before LZ / header) + [(section, entry, src, data)]
"""
import csv, os, struct, sys
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE, UC_HOOK_MEM_UNMAPPED, UcError
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR, UC_ARM_REG_PC

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OV0 = 0x2158280
STOP = 0x3000000          # return address that ends a call
STACK = 0x3100000         # our stack (top)
HEAP = 0x3200000          # bump allocator for GameHeap_AllocZeroed
REGS = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)

LABELS = {}
for r in csv.DictReader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
    if r['module'] in ('static', 'ov00'):
        LABELS[r['name']] = int(r['addr'], 16)
A = LABELS

# per block kind: (source getter or None, init, size, set buffer, pack, entry writers)
KINDS = {
    'A': dict(src=0x2093c94, init='Ov00Init_InitSectionOffsetTableA', size='Ov00Init_GetTotalSizeA',
              setbuf='Ov00Init_SetBufferA', pack='Ov00Init_PackSaveA', write='Ov00Init_WriteEntryA'),
    'B': dict(src='GameState_GetPtrForSlotOrDefault', init='Ov00Init_InitSectionOffsetTableB', size='Ov00Init_GetTotalSizeB',
              setbuf='Ov00Init_SetBufferB', pack='Ov00Init_PackSaveB', write='Ov00Init_WriteEntryB'),
    'C': dict(src='SaveData_GetHeaderBlock', init='Ov00Init_InitSectionOffsetTableC', size='Ov00Init_GetTotalSizeC',
              setbuf='Ov00Init_SetBufferC', pack='Ov00Init_PackSaveC', write=None),
    'D': dict(src='Schedule_GetEventStatePtr', init='Ov00Init_InitSectionOffsetTableD', size='Ov00Init_GetTotalSizeD',
              setbuf='Ov00Init_SetBufferD', pack='Ov00Init_PackSaveD', write=None),
    'E': dict(src=0x20f85a0, init='Ov00Init_InitSectionOffsetTableE', size='Ov00Init_GetTotalSizeE',
              setbuf='Ov00Init_SetBufferE', pack='Ov00Init_PackSaveE', write=None),
    'F': dict(src=None, init='Ov00Init_InitSectionOffsetTableF', size='Ov00Init_GetTotalSizeF',
              setbuf='Ov00Init_SetBufferF', pack='Ov00Init_PackSaveF', write=None),
}


def addr(x):
    return x if isinstance(x, int) else A[x]


def ram_of_state(dst):
    """main RAM + DTCM of a savestate (py-desmume)"""
    sys.path.insert(0, os.path.join(ROOT, 'tools'))
    from emu_driver import Emu
    e = Emu(os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'), dst)
    main = bytes(e.ram())
    dtcm = bytes(e.e.memory.unsigned[0x27e0000:0x27e4000])
    return main, dtcm


class PackEmu:
    def __init__(self, dst=None, main=None, dtcm=None, ov00=True):
        """ov00=False keeps the overlays loaded in the savestate's RAM (for calling game code outside the save module)"""
        if main is None:
            main, dtcm = ram_of_state(dst)
        self.main0, self.dtcm0, self.ov00 = main, dtcm, ov00
        self.reset()

    def reset(self):
        uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.mem_map(0x1ff8000, 0x8000)
        uc.mem_write(0x1ff8000, open(os.path.join(ROOT, 'content/engine/itcm.bin'), 'rb').read())
        uc.mem_map(0x2000000, 0x400000)
        uc.mem_write(0x2000000, self.main0)
        ov = open(os.path.join(ROOT, 'content/engine/ov00.bin'), 'rb').read()
        if self.ov00:
            uc.mem_write(OV0, ov + b'\0' * 0x400)                   # code + data, bss cleared
        uc.mem_map(0x27e0000, 0x4000)
        uc.mem_write(0x27e0000, self.dtcm0)
        uc.mem_map(STOP, 0x1000)
        uc.mem_write(STOP, b'\x1e\xff\x2f\xe1' * 4)                 # bx lr (never reached: we stop at STOP)
        uc.mem_map(STACK - 0x10000, 0x10000)
        uc.mem_map(HEAP, 0x100000)
        self.heap = HEAP
        self.hooks = {}

        def code(uc, a, size, _):
            f = self.hooks.get(a)
            if f:
                f(uc)
        uc.hook_add(UC_HOOK_CODE, code)
        self.uc = uc
        # heap: bump allocation, zeroed; free is a no-op
        self.stub(A['GameHeap_AllocZeroed'], lambda uc: self._ret(uc, self._alloc(uc.reg_read(UC_ARM_REG_R0))))
        self.stub(A['GameHeap_Free'], lambda uc: self._ret(uc, 0))

    def _alloc(self, n):
        p = self.heap
        self.heap += (n + 0x1f) & ~0x1f
        self.uc.mem_write(p, b'\0' * n)
        return p

    def _ret(self, uc, v):
        uc.reg_write(UC_ARM_REG_R0, v & 0xffffffff)
        uc.reg_write(UC_ARM_REG_PC, uc.reg_read(UC_ARM_REG_LR))

    def stub(self, a, f):
        self.hooks[a] = f

    def call(self, f, *args):
        uc = self.uc
        for r, v in zip(REGS, args):
            uc.reg_write(r, v & 0xffffffff)
        uc.reg_write(UC_ARM_REG_SP, STACK - 0x100)
        uc.reg_write(UC_ARM_REG_LR, STOP)
        uc.emu_start(addr(f), STOP, count=50_000_000)
        return uc.reg_read(UC_ARM_REG_R0)

    def rd(self, a, n):
        return bytes(self.uc.mem_read(a, n))

    def w32(self, a):
        return struct.unpack('<I', self.rd(a, 4))[0]

    def pack(self, kind, src=None, writers=('Ov00Init_WriteEntryA', 'Ov00Init_WriteEntryB')):
        """run the packer of `kind` on `src` (default: the game's own source getter); returns (bytes, log)"""
        k = KINDS[kind]
        if src is None:
            g = k['src']
            src = self.call(g, 0xffffffff) if g == 'GameState_GetPtrForSlotOrDefault' else self.call(g)
        log = []
        for wname in writers:
            def mk(wname):
                def f(uc):
                    s, e, p = (uc.reg_read(r) for r in REGS[:3])
                    log.append(dict(writer=wname, section=s, entry=e, src=p, lr=uc.reg_read(UC_ARM_REG_LR)))
                return f
            self.hooks[A[wname]] = mk(wname)
        self.call(k['init'])
        n = self.call(k['size'])
        buf = self._alloc(n)
        self.call(k['setbuf'], buf)
        self.call(k['pack'], src)
        for wname in writers:
            self.hooks.pop(A[wname], None)
        return self.rd(buf, n), log, src
