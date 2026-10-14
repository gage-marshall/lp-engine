"""Run the game's handwriting recogniser (overlay 6 Thumb block) outside the game, in unicorn, through the same calls
the profile screen's glue makes (Ov06ProfileDlt_ProcessProfileDltData / ShutdownProfileDlt / UpdateProfileSystem):

  ctx    = 16 bytes: dictionary (dltjdb.dat, whole file), work buffer (size from GetDltGlobalValue), 500-byte buffer, 500
  init   = FlushDltGlyphProcessor(ctx) -> 0
  params = 0x1c bytes: [0] 0x1c, +4 character-set mask 0xf302f, +8 9, +0xa 0x81, +0xc 0x74, +0xe 0xec, +0x10 0,
           +0x12 0xc, +0x14 1, +0x15 0
  strokes: descriptor {n strokes, table}; table entries {point count, pointer}; points u16 pairs in the writing box's
           own space (0..0x78: a = y - 9, b = 0x81 - (x - 0x74) from the screen touch, as RunProfileDlt stores them)
  recognise = RenderDltGlyphBuffer(ctx, params, descriptor, out[12] u16, work[48]) -> 0; candidates are UCS-2
           (ASCII 0x21..0x7e is shifted to full width: c + 0xfee0, MeasureProfileDltData)
Only the game image (content/engine) and the dictionary (content/assets/dltjdb.dat) are used; nothing is copied.
    .venv/bin/python sandbox/dlt/harness.py"""
import os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'sandbox/ui'))
from lpui import gamedata as G
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UcError
from unicorn.arm_const import (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_SP, UC_ARM_REG_LR,
                               UC_ARM_REG_PC)

GET_SIZE, INIT, RECOGNISE = 0x216aa38, 0x216c870, 0x216c8a4      # Thumb entry points (called with the Thumb bit)
STOP = 0x23ff000                                                 # return address: emulation stops here
HEAP0, STACK = 0x2280000, 0x23fe000


class Recogniser:
    def __init__(self, trace_calls=False):
        mu = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        mu.mem_map(0x2000000, 0x400000)                          # main RAM: static code + overlay 6 + our heap
        base, img = G._image(None)
        mu.mem_write(base, img)
        ob, ov = G._image(6)
        mu.mem_write(ob, ov)
        mu.mem_write(STOP, struct.pack('<I', 0xe12fff1e))        # bx lr (never reached: we stop at STOP)
        self.mu, self.heap = mu, HEAP0
        self.outside = {}
        if trace_calls:
            from unicorn import UC_HOOK_CODE
            lo, hi = 0x216a920, 0x217bc20

            def hook(uc, addr, size, ud):
                if not (lo <= addr < hi) and addr != STOP:
                    self.outside[addr] = self.outside.get(addr, 0) + 1
            mu.hook_add(UC_HOOK_CODE, hook)
        dic = open(os.path.join(ROOT, 'content/assets/dltjdb.dat'), 'rb').read()
        self.dict_ptr = self.alloc(len(dic)); mu.mem_write(self.dict_ptr, dic)
        work_size = self.call(GET_SIZE)
        self.ctx = self.alloc(0x10)
        work = self.alloc(work_size)
        buf = self.alloc(500)
        mu.mem_write(self.ctx, struct.pack('<4I', self.dict_ptr, work, buf, 500))
        self.work_size = work_size
        r = self.call(INIT, self.ctx)
        if r != 0:
            raise RuntimeError('recogniser init returned %d' % r)
        self.params = self.alloc(0x1c)
        p = bytearray(0x1c)
        struct.pack_into('<I', p, 0, 0x1c); struct.pack_into('<I', p, 4, 0xf302f)
        struct.pack_into('<6H', p, 8, 9, 0x81, 0x74, 0xec, 0, 0xc)
        p[0x14], p[0x15] = 1, 0
        mu.mem_write(self.params, bytes(p))
        self.mark = self.heap

    def alloc(self, n):
        a = (self.heap + 31) & ~31
        self.heap = a + n
        self.mu.mem_write(a, bytes(n))
        return a

    def call(self, fn, *args):
        mu = self.mu
        regs = (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)
        sp = STACK
        extra = list(args[4:])
        if extra:
            sp -= 4 * len(extra)
            mu.mem_write(sp, struct.pack('<%dI' % len(extra), *extra))
        for r, v in zip(regs, args[:4]):
            mu.reg_write(r, v)
        mu.reg_write(UC_ARM_REG_SP, sp)
        mu.reg_write(UC_ARM_REG_LR, STOP)
        try:
            mu.emu_start(fn | 1, STOP, count=200_000_000)
        except UcError as e:
            raise RuntimeError('emulation error %s at %#x' % (e, mu.reg_read(UC_ARM_REG_PC)))
        return struct.unpack('<i', struct.pack('<I', mu.reg_read(UC_ARM_REG_R0)))[0]

    def recognise_raw(self, strokes):
        """strokes: lists of (a, b) points as RunProfileDlt stores them -> the 12 candidate codes as the recogniser
        writes them (0 = none)"""
        self.heap = self.mark                                    # per call: fresh stroke / output buffers
        npts = sum(len(s) for s in strokes)
        pts = self.alloc(4 * max(1, npts))
        table = self.alloc(8 * max(1, len(strokes)))
        desc = self.alloc(8)
        out = self.alloc(2 * 12)
        work = self.alloc(48)
        i = 0
        for k, s in enumerate(strokes):
            self.mu.mem_write(table + 8 * k, struct.pack('<II', len(s), pts + 4 * i))
            for a, b in s:
                self.mu.mem_write(pts + 4 * i, struct.pack('<HH', a, b)); i += 1
        self.mu.mem_write(desc, struct.pack('<II', len(strokes), table))
        r = self.call(RECOGNISE, self.ctx, self.params, desc, out, work)
        if r != 0:
            raise RuntimeError('recognise returned %d' % r)
        return struct.unpack('<12H', bytes(self.mu.mem_read(out, 24)))

    def recognise(self, strokes):
        """-> 12 candidate strings ('' = none)"""
        res = []
        for c in self.recognise_raw(strokes):
            if c == 0:
                res.append('')
                continue
            if 0x20 < c < 0x7f:
                c = (c - 0x120) & 0xffff                         # MeasureProfileDltData: ASCII -> full width
            res.append(chr(c))
        return res


def line(a0, b0, a1, b1, n=12):
    if n == 1:
        return [(a0, b0)]
    return [(round(a0 + (a1 - a0) * t / (n - 1)), round(b0 + (b1 - b0) * t / (n - 1))) for t in range(n)]


if __name__ == '__main__':
    import time
    t = time.time()
    R = Recogniser(trace_calls='--trace' in sys.argv)
    print('init ok: work buffer %d bytes, dictionary at %#x (%.2fs)' % (R.work_size, R.dict_ptr, time.time() - t))
    # the box space: first coordinate a = y - 9 (down the screen when held sideways), second b runs across
    tests = {
        'horizontal bar': [line(60, 15, 60, 105)],
        'vertical bar': [line(15, 60, 105, 60)],
        'two horizontal bars': [line(40, 20, 40, 100), line(80, 20, 80, 100)],
        'three horizontal bars': [line(25, 25, 25, 95), line(60, 25, 60, 95), line(95, 15, 95, 105)],
        'cross': [line(60, 15, 60, 105), line(15, 60, 105, 60)],
    }
    for name, strokes in tests.items():
        t = time.time()
        for orient in ('as given', 'swapped axes'):
            s = strokes if orient == 'as given' else [[(b, a) for a, b in st] for st in strokes]
            print('%-22s %-13s -> %s  (%.2fs)' % (name, orient, ' '.join(c or '·' for c in R.recognise(s)), time.time() - t))
    if R.outside:
        print('code run outside the Thumb block:', {hex(a): n for a, n in sorted(R.outside.items())[:20]})
