"""Check lpui.logic.choices on every capture whose option text boxes hold strings (from RAM): sub BG2 tiles + map.
    .venv/bin/python sandbox/ui/verify_choices.py CAPTURE_DIR..."""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import ds2d
from lpui.vram import Vram
from lpui.logic import choices

OBJS = (0x2364890, 0x2364b38, 0x2364de0)

if __name__ == '__main__':
    n = ok = 0
    for d in sys.argv[1:]:
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        rd = lambda a: struct.unpack_from('<I', ram, a - 0x2000000)[0]
        if any(rd(o + 0x40) != 0x6202000 for o in OBJS):
            continue
        opts = [ram[o + 0x60 - 0x2000000:o + 0x260 - 0x2000000].split(b'\0')[0].decode('utf8', 'replace') for o in OBJS]
        if not any(opts) or not all(rd(o + 0x288) or not s for o, s in zip(OBJS, opts)):
            continue
        cap = ds2d.Capture(d)
        sub = np.frombuffer(cap.vram['bg_sub'], np.uint8)
        e = int(sub[0x2000 + (3 * 32 + 23) * 2]) | int(sub[0x2000 + (3 * 32 + 23) * 2 + 1]) << 8
        if e != 0xf000 | 141:
            continue                                         # no choice on screen
        v = Vram(regs=cap.regs); v.mem['bg_sub'][:] = sub
        c3d = struct.unpack_from('<I', ram, 0x213d3b0 - 0x2000000)[0] or struct.unpack_from('<I', ram, 0x213d3a8 - 0x2000000)[0]
        before = v.mem['bg_sub'].copy()
        choices.apply(v, opts, capture_3d=bool(c3d))
        diff = int((v.mem['bg_sub'] != before).sum())
        n += 1; ok += diff == 0
        print(os.path.basename(d), opts, 'differs by %d bytes' % diff if diff else 'exact')
    print('%d choice captures, %d reproduced byte for byte (sub BG2 option text tiles + map)' % (n, ok))
