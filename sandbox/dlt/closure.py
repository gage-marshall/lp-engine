"""DLT (overlay 6 handwriting recogniser) closure and instruction census: which functions the profile screen reaches in
the recogniser, everything those call (overlay 6 + static), the globals they touch, I/O registers, SWIs, indirect calls,
and which ARM / Thumb instructions an interpreter needs.
    .venv/bin/python sandbox/dlt/closure.py"""
import bisect, collections, csv, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'sandbox/ui'))
from lpui import gamedata as G
import capstone
from capstone import arm

THUMB = [(0x216a920, 0x217bc20), (0x2183920, 0x2183f20)]          # fid/thumb_ranges.txt, overlay 6
DLT_LO, DLT_HI = 0x216a920, 0x217bc20
OV = 6


def labels():
    out = []
    for r in csv.DictReader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
        if r['module'] in ('ov06', 'static'):
            out.append((int(r['addr'], 16), r['name'], r['module']))
    return sorted(set(out))


LAB = labels()
STARTS = [a for a, _, _ in LAB]


def is_thumb(a):
    return any(lo <= a < hi for lo, hi in THUMB)


def func_of(a):
    i = bisect.bisect_right(STARTS, a) - 1
    return LAB[i] if i >= 0 else (a, '?', '?')


def func_range(a):
    i = STARTS.index(a)
    nxt = STARTS[i + 1] if i + 1 < len(STARTS) else a + 0x400
    return a, min(nxt, a + 0x4000)


def in_ov(a):
    base, d = G._image(OV)
    return base <= a < base + len(d)


_cs = {}


def cs(thumb):
    if thumb not in _cs:
        md = capstone.Cs(capstone.CS_ARCH_ARM, capstone.CS_MODE_THUMB if thumb else capstone.CS_MODE_ARM)
        md.detail = True
        _cs[thumb] = md
    return _cs[thumb]


def disasm(a):
    lo, hi = func_range(a)
    thumb = is_thumb(a) if in_ov(a) else False
    ov = OV if in_ov(a) else None
    try:
        code = G.raw(lo, hi - lo, ov)
    except ValueError:
        return thumb, []
    return thumb, list(cs(thumb).disasm(code, lo))


def main():
    # entry points: calls from overlay-6 ARM code (the profile UI) into the recogniser's Thumb block
    entries = collections.Counter()
    callers = collections.defaultdict(set)
    for a, name, mod in LAB:
        if mod != 'ov06' or DLT_LO <= a < DLT_HI or is_thumb(a):
            continue
        thumb, ins = disasm(a)
        for x in ins:
            if x.mnemonic in ('bl', 'blx') and x.operands and x.operands[0].type == arm.ARM_OP_IMM:
                t = x.operands[0].imm
                if DLT_LO <= t < DLT_HI or any(lo <= t < hi for lo, hi in THUMB):
                    entries[t] += 1; callers[t].add(name)
    print('entry points from the profile UI into the Thumb code: %d' % len(entries))
    for t, n in sorted(entries.items()):
        print('   %#x %-50s called by %s' % (t, func_of(t)[1], sorted(callers[t])[:3]))
    # closure
    seen, todo = set(), list(entries)
    mn = collections.Counter(); special = collections.Counter(); io = collections.Counter(); swi = collections.Counter()
    indirect = []; globals_ = collections.Counter(); static_funcs = set(); bytes_ = 0
    while todo:
        f = todo.pop()
        f0 = func_of(f)[0] if func_of(f)[0] in STARTS and func_of(f)[0] <= f else f
        if f0 in seen:
            continue
        seen.add(f0)
        thumb, ins = disasm(f0)
        if not in_ov(f0):
            static_funcs.add(f0)
        for x in ins:
            bytes_ += x.size
            key = ('T:' if thumb else 'A:') + x.mnemonic
            mn[key] += 1
            m = x.mnemonic.split('.')[0]
            if m in ('svc', 'swi'):
                swi[x.op_str] += 1
            if m in ('mrc', 'mcr', 'clz', 'qadd', 'qsub', 'qdadd', 'qdsub', 'ldrd', 'strd', 'pld', 'bkpt', 'mrs', 'msr', 'ldm', 'stm') or m.startswith(('smla', 'smul', 'smlal', 'umull', 'umlal', 'smull', 'ldrex', 'strex')):
                special[key] += 1
            if m in ('bl', 'blx', 'b') and x.operands and x.operands[0].type == arm.ARM_OP_IMM and m != 'b':
                todo.append(x.operands[0].imm)
            if m in ('blx', 'bx') and x.operands and x.operands[0].type == arm.ARM_OP_REG and x.operands[0].reg != arm.ARM_REG_LR:
                indirect.append((hex(x.address), func_of(x.address)[1], x.mnemonic, x.op_str))
            if m == 'mov' and 'pc' in x.op_str.split(',')[0]:
                indirect.append((hex(x.address), func_of(x.address)[1], x.mnemonic, x.op_str))
            for o in x.operands:
                if o.type == arm.ARM_OP_MEM and o.mem.base == arm.ARM_REG_PC and m.startswith('ldr'):
                    pool = ((x.address + (4 if thumb else 8)) & ~3) + o.mem.disp
                    try:
                        v = G.word(pool, OV if in_ov(pool) else None)
                    except ValueError:
                        continue
                    if 0x4000000 <= v < 0x4002000:
                        io[hex(v)] += 1
                    elif 0x2000000 <= v < 0x2400000 or 0x27e0000 <= v < 0x2800000 or 0x1ff8000 <= v < 0x2000000:
                        lo_, hi_ = func_range(f0)
                        if not (DLT_LO <= v < DLT_HI) and not (lo_ <= v < hi_ + 0x400):
                            globals_[hex(v)] += 1
    tf = [f for f in seen if is_thumb(f) and in_ov(f)]
    af = [f for f in seen if in_ov(f) and not is_thumb(f)]
    print('\nclosure: %d functions (%d Thumb in overlay 6, %d ARM in overlay 6, %d static), %d bytes of code' % (
        len(seen), len(tf), len(af), len(static_funcs), bytes_))
    print('static functions used:', sorted(func_of(f)[1] for f in static_funcs))
    print('ARM functions in overlay 6 used:', sorted(func_of(f)[1] for f in af)[:20])
    print('\nI/O registers referenced:', dict(io))
    print('SWIs:', dict(swi))
    print('indirect branches/calls (%d):' % len(indirect)); [print('   ', i) for i in indirect[:25]]
    print('\nglobals outside the code referenced (%d distinct):' % len(globals_), sorted(globals_)[:40])
    print('\nspecial instructions:', dict(special))
    print('\ninstruction census: %d distinct mnemonics' % len(mn))
    print(sorted(mn.items(), key=lambda x: -x[1]))


if __name__ == '__main__':
    main()


def whole_block():
    """census over every function in the Thumb block (a superset of what runs), the static functions they call, and the
    function-pointer tables (words pointing into the block with the Thumb bit) that the indirect calls go through"""
    funcs = [a for a, _, m in LAB if m == 'ov06' and is_thumb(a)]
    mn = collections.Counter(); calls_out = collections.Counter(); io = collections.Counter(); swi = collections.Counter()
    nbytes = 0; unknown = 0
    for f in funcs:
        thumb, ins = disasm(f)
        for x in ins:
            nbytes += x.size
            m = x.mnemonic.split('.')[0]
            mn[m] += 1
            if m in ('svc', 'swi'):
                swi[x.op_str] += 1
            if m in ('bl', 'blx') and x.operands and x.operands[0].type == arm.ARM_OP_IMM:
                t = x.operands[0].imm
                if not (DLT_LO <= t < DLT_HI) and not is_thumb(t):
                    calls_out[func_of(t)[1] + ('@%#x' % t)] += 1
            for o in x.operands:
                if o.type == arm.ARM_OP_MEM and o.mem.base == arm.ARM_REG_PC and m.startswith('ldr'):
                    pool = ((x.address + 4) & ~3) + o.mem.disp
                    try:
                        v = G.word(pool, OV)
                    except ValueError:
                        continue
                    if 0x4000000 <= v < 0x4002000:
                        io[hex(v)] += 1
    base, d = G._image(OV)
    ptrs = [base + i for i in range(0, len(d) - 3, 4) if (lambda v: v & 1 and DLT_LO <= (v & ~1) < DLT_HI)(int.from_bytes(d[i:i + 4], 'little'))]
    print('Thumb block: %d functions, %d bytes decoded' % (len(funcs), nbytes))
    print('calls out of the block:', dict(calls_out))
    print('I/O registers:', dict(io), ' SWIs:', dict(swi))
    print('function-pointer words into the block (Thumb bit set): %d, in data at %s..%s' % (
        len(ptrs), hex(min(ptrs)) if ptrs else '-', hex(max(ptrs)) if ptrs else '-'))
    print('census (%d distinct):' % len(mn), sorted(mn.items(), key=lambda x: -x[1]))


if __name__ == '__main__' and '--block' in sys.argv:
    whole_block()
