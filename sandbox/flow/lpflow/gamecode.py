"""Reading switch tables out of the game's ARM code (static image or an overlay), so the ports take them from the game:
    jump_table(fn)        the cases of the first `addls pc, pc, rN, lsl #2` in fn: [target address or None]
    first_literal(addr)   the word the first `ldr rN, [pc, #imm]` at or after addr loads"""
from .names import G


def _ins(addr, n, ov=None):
    return G.disasm(addr, addr + 4 * n, ov=ov)


def jump_table(fn, ov=None, search=64):
    ins = _ins(fn, search, ov)
    limit = None
    for k, i in enumerate(ins):
        if i.mnemonic == 'cmp' and i.op_str.split(', ')[-1].startswith('#'):
            limit = int(i.op_str.split('#')[1], 0)
        if i.mnemonic == 'addls' and i.op_str.startswith('pc, pc,'):
            base = i.address + 8
            out = []
            for c in range(limit + 1):
                e = _ins(base + 4 * c, 1, ov)[0]
                out.append(int(e.op_str[1:], 0) if e.mnemonic == 'b' else None)
            return out
    raise ValueError('no jump table in %x' % fn)


def first_literal(addr, ov=None, search=8):
    for i in _ins(addr, search, ov):
        if i.mnemonic == 'ldr' and ', [pc, #' in i.op_str:
            off = int(i.op_str.split('#')[1].rstrip(']'), 0)
            return G.word(i.address + 8 + off, ov=ov) if ov is not None else G.word(i.address + 8 + off)
    return None


def case_literals(fn, ov=None):
    """{case: literal} for a switch whose cases each start by loading one literal (None: case does nothing)"""
    return {c: (first_literal(t, ov) if t is not None else None) for c, t in enumerate(jump_table(fn, ov))}
