"""The game's code and data image (content/engine/: static.bin, ovNN.bin, memory_map.json - the same image the Rust
engine's rom.rs reads), for screen logic that needs the game's tables: values are read here by the address the game
uses, never copied into source.

Also the audit primitives (provenance of a constant):
  table(addr, fmt, n, ov)      the game's data table at addr
  literal(addr, ov)            a literal-pool word (a constant the code loads with ldr rX, [pc, #..])
  imm(addr, ov)                the immediate operand(s) of the instruction at addr
  find_value(lo, hi, value)    where a value appears inside a function: as an instruction immediate or a pool word"""
import functools, json, os, struct

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(HERE)))
ENGINE = os.path.join(ROOT, 'content', 'engine')


@functools.lru_cache(None)
def _map():
    return json.load(open(os.path.join(ENGINE, 'memory_map.json')))


@functools.lru_cache(None)
def _image(ov):
    """(base, bytes) of the static module (ov None) or overlay ov"""
    if ov is None:
        return _map()['static']['base'], open(os.path.join(ENGINE, 'static.bin'), 'rb').read()
    key = 'ov%02d' % ov
    return _map()[key]['base'], open(os.path.join(ENGINE, key + '.bin'), 'rb').read()


@functools.lru_cache(None)
def _region(name):
    return _map()[name]['base'], open(os.path.join(ENGINE, name + '.bin'), 'rb').read()


def raw(addr, n, ov=None):
    base, d = _image(ov)
    o = addr - base
    if 0 <= o <= len(d) - n:
        return d[o:o + n]
    if ov is not None:                          # overlay code refers to static data too
        return raw(addr, n, None)
    for name in ('itcm', 'dtcm'):               # ITCM code (e.g. Text_StepMessageDrawProgress) and DTCM data
        base, d = _region(name)
        if 0 <= addr - base <= len(d) - n:
            return d[addr - base:addr - base + n]
    raise ValueError('address %#x outside the static image' % addr)


def read(addr, fmt, ov=None):
    return struct.unpack('<' + fmt, raw(addr, struct.calcsize('<' + fmt), ov))


def table(addr, fmt, n, ov=None):
    """n records of struct format fmt (single-field formats come back as plain values)"""
    size = struct.calcsize('<' + fmt)
    rows = [struct.unpack('<' + fmt, raw(addr + i * size, size, ov)) for i in range(n)]
    return [r[0] for r in rows] if len(fmt.lstrip('<')) == 1 else rows


def word(addr, ov=None):
    return read(addr, 'I', ov)[0]


def pointer_table(lit, fmt, n, ov=None):
    """a table reached through a literal-pool pointer (DAT_xxx -> table)"""
    return table(word(lit, ov), fmt, n, ov)


# ---- audit primitives --------------------------------------------------------------------------------------------
@functools.lru_cache(None)
def _cs():
    import capstone
    md = capstone.Cs(capstone.CS_ARCH_ARM, capstone.CS_MODE_ARM)
    md.detail = True
    return md


def disasm(lo, hi, ov=None):
    return list(_cs().disasm(raw(lo, hi - lo, ov), lo))


def imm(addr, ov=None):
    """immediate operands of the ARM instruction at addr"""
    import capstone
    ins = next(_cs().disasm(raw(addr, 4, ov), addr))
    return [o.imm for o in ins.operands if o.type == capstone.arm.ARM_OP_IMM]


def literal(addr, ov=None):
    return word(addr, ov)


def find_value(lo, hi, value, ov=None):
    """[(kind, address)]: 'imm' instruction immediates and 'pool' words (pc-relative loads) equal to value inside
    the code range lo..hi"""
    import capstone
    out = []
    for ins in disasm(lo, hi, ov):
        for o in ins.operands:
            if o.type == capstone.arm.ARM_OP_IMM and (o.imm == value or o.imm & 0xffffffff == value & 0xffffffff):
                out.append(('imm', ins.address))
            if o.type == capstone.arm.ARM_OP_MEM and o.mem.base == capstone.arm.ARM_REG_PC and ins.mnemonic.startswith('ldr'):
                pool = ins.address + 8 + o.mem.disp
                try:
                    if word(pool, ov) == value & 0xffffffff:
                        out.append(('pool', pool))
                except ValueError:
                    pass
    return out
