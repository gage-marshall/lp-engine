"""Check lpui.logic.dialogue on every scene capture: build BG2 from the message boxes' state in RAM (strings, characters
typed) and compare the boxes' tiles and map entries with the captured VRAM.
    .venv/bin/python sandbox/ui/verify_dialogue.py CAPTURE_DIR..."""
import os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import ds2d
from lpui.vram import Vram
from lpui.logic import dialogue

OBJS = {'name': 0x2364340, 'line1': 0x2365880, 'line2': 0x2365b28, 'line3': 0x2365dd0}


def box_state(ram, obj):
    rd = lambda a: struct.unpack_from('<I', ram, a - 0x2000000)[0]
    if rd(obj + 0x40) != 0x6002800 or rd(obj + 0x28) != 2:
        return None
    s = ram[obj + 0x60 - 0x2000000: obj + 0x260 - 0x2000000].split(b'\0')[0].decode('utf8', 'replace')
    return s, rd(obj + 0x280), rd(obj + 0x288)


if __name__ == '__main__':
    ok = n = 0
    bad = []
    for d in sys.argv[1:]:
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        st = {k: box_state(ram, o) for k, o in OBJS.items()}
        if any(v is None for v in st.values()):
            continue
        cap = ds2d.Capture(d)
        if cap.reg('main', 8 + 4) & 0xffff != 0x050c:           # BG2 = the dialogue text layer
            continue
        v = Vram(regs=cap.regs)
        c3d = struct.unpack_from('<I', ram, 0x213d3b0 - 0x2000000)[0] or struct.unpack_from('<I', ram, 0x213d3a8 - 0x2000000)[0]
        v.mem['bg_main'][:] = np.frombuffer(cap.vram['bg_main'], np.uint8)
        lines = [st['line%d' % i][0] for i in (1, 2, 3)]
        typed = [None if st['line%d' % i][2] else st['line%d' % i][1] for i in (1, 2, 3)]
        before = v.mem['bg_main'].copy()
        dialogue.apply(v, st['name'][0], lines, typed, capture_3d=bool(c3d))
        diff = int((v.mem['bg_main'] != before).sum())
        n += 1; ok += diff == 0
        if diff:
            bad.append((os.path.basename(d), diff, st['name'][0], lines, typed))
    print('%d dialogue captures, %d reproduced byte for byte (BG2 tiles + map of the four boxes)' % (n, ok))
    for b in bad[:6]:
        print('  differs:', b)
