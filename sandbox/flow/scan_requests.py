"""Find every call to the screen manager's request setters in the game code (static + all overlays, ARM) and the
immediate argument it passes (the last `mov r0, #imm` / `mvn r0, #imm` before the call in the same block), with the
calling function's name: which screen asks for which next screen or which result.
    .venv/bin/python sandbox/flow/scan_requests.py [--json OUT]"""
import bisect, csv, json, os, sys
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'sandbox/ui'))
from lpui import gamedata as G
from capstone import Cs, CS_ARCH_ARM, CS_MODE_ARM

TARGETS = {0x20882c8: 'next_screen', 0x2088624: 'result', 0x208a200: 'alt_result', 0x2088644: 'finish',
           0x2089d8c: 'request_screen', 0x203dbf0: 'set_state'}


def labels():
    out = {}
    for row in csv.reader(open(os.path.join(ROOT, 'fid/labels_all.csv'))):
        if len(row) >= 3 and row[1][:1].isdigit():
            out.setdefault(row[0], []).append((int(row[1], 16), row[2]))
    for v in out.values():
        v.sort()
    return out


def scan():
    L = labels()
    md = Cs(CS_ARCH_ARM, CS_MODE_ARM)
    md.skipdata = True
    out = []
    regions = [('static', None)] + [('ov%02d' % i, i) for i in range(44)]
    for region, ov in regions:
        try:
            base, img = G._image(ov)
        except Exception:
            continue
        funcs = L.get(region, [])
        recent = {}
        for ins in md.disasm(bytes(img), base):
            m, op = ins.mnemonic, ins.op_str
            if m in ('mov', 'mvn') and op.startswith('r0, #'):
                v = int(op[5:], 0)
                recent[0] = (v if m == 'mov' else ~v, ins.address)
            elif m.startswith('b') and not m.startswith('bic') and m not in ('bl', 'blx'):
                recent = {}
            elif op.startswith('r0,') and m not in ('cmp', 'tst', 'str', 'strb', 'strh', 'cmn', 'teq'):
                recent.pop(0, None)
            if m in ('bl', 'blx') and op.startswith('#'):
                t = int(op[1:], 0)
                if t in TARGETS:
                    i = bisect.bisect_right(funcs, (ins.address, '￿')) - 1
                    fn = funcs[i][1] if i >= 0 else '?'
                    arg = recent.get(0)
                    arg = arg[0] if arg and ins.address - arg[1] <= 0x20 else None
                    out.append(dict(region=region, addr=ins.address, fn=fn, call=TARGETS[t], arg=arg))
                recent = {}
    return out


if __name__ == '__main__':
    res = scan()
    if '--json' in sys.argv:
        json.dump(res, open(sys.argv[sys.argv.index('--json') + 1], 'w'), indent=0)
    for r in res:
        if r['call'] != 'set_state':
            a = r['arg']
            print('%-6s %08x %-14s %-6s %s' % (r['region'], r['addr'], r['call'], '?' if a is None else hex(a & 0xffffffff) if a < 0 else a, r['fn']))
