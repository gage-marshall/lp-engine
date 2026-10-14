"""Check lpui.logic.msgwin against the message window in every capture: the window's lines in RAM (TextLayout object
0x2364094: +0x27fa line count, +0x27fb + 0x40 i lines) equal msgwin.split of the script message they came from (matched by
content among all script strings), and the three boxes hold lines offset.. (control +0x27dc: +0x1c offset).
    .venv/bin/python sandbox/ui/verify_msgwin.py TRACE_DIR"""
import collections, glob, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools'))
import sbin, sbin_dis  # noqa: E402
from sbin_scan import VALID  # noqa: E402
from lpui.logic import msgwin  # noqa: E402
B = 0x2000000
WIN = 0x2364094


def script_messages():
    out = set()
    for p in glob.glob(os.path.join(ROOT, 'content/assets/**/*.sbin'), recursive=True):
        d = open(p, 'rb').read()
        try:
            h, ents = sbin.header(d)
        except Exception:
            continue
        pc, end = min(ents) if ents else len(d), h[1]
        while pc + 2 <= end:
            op, n = d[pc], d[pc + 1]
            if op not in VALID or pc + 2 + 2 * n > end:
                pc += 2; continue
            a = struct.unpack('>%dH' % n, d[pc + 2:pc + 2 + 2 * n])
            if op == 0x81 and n >= 3:
                try:
                    out.add(sbin_dis.strat(d, h[1], (a[1] << 16) | a[2]))
                except Exception:
                    pass
            pc += 2 + 2 * n
    return out


def cstr(ram, a, n=0x40):
    return ram[a - B:a - B + n].split(b'\0')[0].decode('utf8', 'replace')


if __name__ == '__main__':
    msgs = script_messages()
    by_flat = collections.defaultdict(set)
    for m in msgs:
        by_flat[m.replace('\n', '')].add(m)
    print(len(msgs), 'script strings')
    res = collections.Counter(); bad = []
    for d in sorted(glob.glob(os.path.join(sys.argv[1], 'cap-f*'))):
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        nl = ram[WIN + 0x27fa - B]
        if not 0 < nl <= 6:
            continue
        lines = [cstr(ram, WIN + 0x27fb + 0x40 * i) for i in range(nl)]
        cands = by_flat.get(''.join(lines))
        if not cands:
            res['message not found among script strings'] += 1
            if res['message not found among script strings'] <= 6:
                print('  not found:', os.path.basename(d), lines)
            continue
        ok = any(msgwin.split(m) == lines for m in cands)
        res['split equal' if ok else 'split differs'] += 1
        if not ok:
            bad.append((os.path.basename(d), lines, [msgwin.split(m) for m in cands][:1]))
        mode = struct.unpack_from('<I', ram, WIN - B)[0]
        boxes = WIN + (0x17ec if mode == 0 else 0x1fe4)
        off = struct.unpack_from('<b', ram, WIN + 0x27dc + 0x1c - B)[0]
        got = [cstr(ram, boxes + 0x2a8 * i + 0x60, 0x200) for i in range(3)]
        want = [lines[off + i] if off + i < nl else '' for i in range(3)]
        okb = got[:min(3, nl)] == want[:min(3, nl)]
        res['boxes = lines[offset..]' if okb else 'boxes differ'] += 1
        if not okb:
            print('  boxes:', os.path.basename(d), 'mode', mode, 'offset', off, got, want)
    print(dict(res))
    for b in bad[:8]:
        print('  ', b)
