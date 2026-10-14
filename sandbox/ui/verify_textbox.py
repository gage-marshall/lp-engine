"""Check lpui.textbox on every capture: each live text box with a string, drawn from its RAM state (string, characters
drawn, pen start, area, map position), equals the captured VRAM (its tiles and map entries). The draw colour is the
caller's argument: reported per box (the one that reproduces it).
    .venv/bin/python sandbox/ui/verify_textbox.py TRACE_DIR"""
import collections, glob, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
import ds2d
from lpui.vram import Vram
from lpui import textbox

if __name__ == '__main__':
    res = collections.Counter(); colours = collections.defaultdict(collections.Counter); bad = []
    for d in sorted(glob.glob(os.path.join(sys.argv[1], 'cap-f*'))):
        ram = open(os.path.join(d, 'ram.bin'), 'rb').read()
        cap = ds2d.Capture(d)
        for bx in textbox.find(ram):
            if not bx.string:
                continue
            act = np.frombuffer(cap.vram[bx.mem], np.uint8)
            if not bx.on_screen(act):
                res['not on screen'] += 1
                continue
            hit = None
            for colour in (1, 2, 3, 4):
                v = Vram(); v.mem[bx.mem][:] = act
                bx.apply(v, colour)
                if all((v.mem[m][a:b] == act[a:b]).all() for m, a, b in bx.regions()):
                    hit = colour; break
            res[hit is not None] += 1
            colours[hex(bx.addr)][hit] += 1
            if hit is None:
                bad.append((os.path.basename(d), hex(bx.addr), bx.string[:16], bx.drawn, bx.done, bx.start, (bx.left, bx.top)))
    print('text boxes reproduced byte for byte from their RAM state:', dict(res))
    for k, c in sorted(colours.items()):
        print('  box', k, 'colour', dict(c))
    for b in bad[:12]:
        print('  differs:', b)
