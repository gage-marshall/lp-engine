"""Derive the meaning of every save entry by running the game's packers (packemu) and attributing each entry's source.

    .venv/bin/python sandbox/save/savemap.py STATE.dst [out.json]

For every (kind, section, entry): the layout (offset, size, count) and either
  {"src": "<struct>+<offset>"}                    a direct copy of size * count bytes of a game struct
  {"narrow": "<struct>+<offset>", "ints": n}       n 32-bit ints stored as their low bytes (WriteEntryB_PackBytes3/4)
  {"computed": "<packer routine>"}                 built on the stack / heap by the routine (see docs/SAVE_FORMAT.md)
  {"unwritten": true}                              never written by the packer (stays 0)
"""
import bisect, csv, json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import packemu, savefmt as sf  # noqa: E402
from unicorn.arm_const import UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3, UC_ARM_REG_LR  # noqa: E402

LAB = {}
for r in csv.DictReader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
    if r['module'] in ('static', 'ov00'):
        LAB[int(r['addr'], 16)] = r['name']
KS = sorted(LAB)


def fn(a):
    return LAB[KS[bisect.bisect_right(KS, a) - 1]]


WRITERS = {'A': 'Ov00Init_WriteEntryA', 'B': 'Ov00Init_WriteEntryB', 'C': 'Ov00Init_WriteEntryC', 'D': 'Ov00Init_WriteEntryD',
           'E': 'Ov00Init_WriteEntryE', 'F': 'Ov00Init_WriteEntryF'}


def structs(pe):
    """known game structs: name -> (base, size)"""
    w = pe.w32
    out = {}
    for i in range(3):                                  # heroine records first: they lie inside GameState (+0x15c4)
        out['heroine%d' % i] = (0x2144250 + 0x688 * i, 0x688)
    out.update({'settings': (w(0x21544e8 + 4), 0x1f8), 'header': (w(0x21544e8 + 8), 0xf4),
                'gamestate': (0x2142c8c, 0x4000), 'flags': (w(0x2141904 + 4), 0x800), 'flowstate': (w(0x2156798), 0x100)})
    return out


def where(addr, known):
    for name, (b, n) in known.items():
        if b <= addr < b + n:
            return '%s+%#x' % (name, addr - b)
    return 'ram:%#x' % addr


def derive(pe, kinds='ABCDEF'):
    known = structs(pe)
    out = {}
    for kind in kinds:
        lay, total = sf.layout(kind)
        log = []
        reads = []
        from unicorn import UC_HOOK_MEM_READ

        def on_read(uc, access, a, size, value, ud):
            if 0x2110000 <= a < 0x2158280 or 0x2166ce0 <= a < 0x2400000:   # data (not code / overlay tables)
                if not (packemu.STACK - 0x10000 <= a < packemu.STACK):
                    reads.append(a)
        hr = pe.uc.hook_add(UC_HOOK_MEM_READ, on_read)

        def entry(name):
            def f(uc):
                a = [uc.reg_read(r) for r in (UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3)]
                lr = uc.reg_read(UC_ARM_REG_LR)
                rs = sorted(set(reads)); reads.clear()
                if name == 'ElemD':
                    log.append(dict(s=a[0], e=a[1], elem=a[2], src=a[3], lr=lr, how='elem'))
                elif name.startswith('Pack'):
                    log.append(dict(s=a[0], e=a[1], src=a[2], lr=lr, how=name))
                else:
                    log.append(dict(s=a[0], e=a[1], src=a[2], lr=lr, how='write', reads=rs))
            return f
        for nm, a in ((WRITERS[kind], WRITERS[kind]), ('ElemD', 'Ov00Init_WriteEntryElemD'),
                      ('PackBytes4', 'Ov00Init_WriteEntryB_PackBytes4'), ('PackBytes3', 'Ov00Init_WriteEntryB_PackBytes3')):
            pe.hooks[packemu.A[a]] = entry(nm)
        try:
            buf, _, src = pe.pack(kind, writers=())
        except Exception as ex:
            print(kind, 'packer did not run:', ex)
            continue
        finally:
            pe.uc.hook_del(hr)
            for a in ('Ov00Init_WriteEntryElemD', 'Ov00Init_WriteEntryB_PackBytes4', 'Ov00Init_WriteEntryB_PackBytes3', WRITERS[kind]):
                pe.hooks.pop(packemu.A[a], None)
        known['src_' + kind] = (src, 0)
        ents = {}
        for x in log:
            key = (x['s'], x['e'])
            if x['how'] in ('PackBytes4', 'PackBytes3'):
                ents[key] = dict(narrow=where(x['src'], known), ints=4 if x['how'] == 'PackBytes4' else 3)
                continue
            if key in ents and 'narrow' in ents[key]:
                continue                                       # the wrapper's own WriteEntryB call
            if 0x2000000 <= x['src'] < 0x2400000 and not (packemu.HEAP <= x['src']):
                d = dict(src=where(x['src'], known))
            else:
                d = dict(computed=fn(x['lr']))
                fields = sorted(set(where(r, known) for r in x.get('reads', []) if not where(r, known).startswith('ram:')))
                if fields:
                    d['from'] = fields
            if x['how'] == 'elem':
                ents.setdefault(key, dict(elems={}))['elems'][x['elem']] = d
            else:
                ents[key] = d
        sections = []
        for s, sec in enumerate(lay):
            row = []
            for e, (o, size, cnt) in enumerate(sec):
                d = dict(offset=o, size=size, count=cnt)
                d.update(ents.get((s, e), dict(unwritten=True)))
                row.append(d)
            sections.append(row)
        out[kind] = dict(total=total, packed_len=len(buf), sections=sections)
        print(kind, 'entries', sum(len(r) for r in sections), 'writes', len(log), 'packed', len(buf), 'layout', total)
    return out


if __name__ == '__main__':
    pe = packemu.PackEmu(sys.argv[1])
    m = derive(pe)
    path = sys.argv[2] if len(sys.argv) > 2 else os.path.join(HERE, 'savemap.json')
    json.dump(m, open(path, 'w'), indent=1)
    print('->', path)
