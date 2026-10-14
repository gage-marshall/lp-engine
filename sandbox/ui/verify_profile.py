"""Check lpui.logic.profile: on every capture showing the profile summary form, the form canvas's draw list (traced
Text_DrawString calls) equals Profile.summary_draws() computed from the profile state in RAM.
    .venv/bin/python sandbox/ui/verify_profile.py TRACE_DIR"""
import collections, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpui import canvases
from lpui.logic import profile
B = 0x2000000

if __name__ == '__main__':
    res = collections.Counter(); shown = []
    for f, path, draws in canvases.walk(sys.argv[1]):
        ram = open(os.path.join(path, 'ram.bin'), 'rb').read()
        rd = lambda a: struct.unpack_from('<i', ram, a - B)[0]
        st = rd(profile.STATE_HOLDER + 500)
        if not B <= st < B + 0x400000:
            continue
        dl = draws.get(st + 0x145c)
        if not dl or dl[0][1] != 0xdc:
            continue
        pairs = lambda a: [(rd(a + 8 * i), rd(a + 8 * i + 4)) for i in range(4)]
        cs = lambda a: ram[a - B:a - B + 0x16].split(b'\0')[0].decode('utf8', 'replace')
        p = profile.Profile(pairs(st + 0x26cc), pairs(st + 0x26ec), pairs(st + 0x2840), cs(st + 0x277c), cs(st + 0x2792),
                            rd(st + 0x27d4), rd(st + 0x27d8), rd(st + 0x27dc), rd(st + 0x27e0))
        want = p.summary_draws()
        got = [(s, x, y) for s, x, y, left, top, cl in dl]
        # the tracer reads DrawString's string only from main RAM: one-character copies on the stack (DrawProfileTextEntry)
        # come through as '' - compare those by position
        ok = len(want) == len(got) and all(a == b or (b[0] == '' and a[1:] == b[1:]) for a, b in zip(want, got))
        res[ok] += 1
        if not ok:
            print(f, 'differs', [(a, b) for a, b in zip(want, got) if a != b][:4], len(want), len(got))
        else:
            shown.append((f, p.chars(p.family), p.handle, p.chars(p.given), p.nickname, p.chars(p.called), p.month, p.day, p.blood, p.hometown))
    print('profile summary captures, form draw list = Profile.summary_draws():', dict(res))
    for s in shown[:3]:
        print('  ', s)
