"""Check lpui.text against the game: replay every traced canvas init (NNS_G2dCharCanvasInitForBG), fill/clear inside a
canvas and glyph draw (NNS_G2dCharCanvasDrawChar: canvas, font, x, y, colour, code) onto simulated canvases, and compare
each live canvas with its tile buffer in the captured RAM.
    .venv/bin/python sandbox/ui/verify_text.py TRACE_DIR"""
import collections, json, struct, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import numpy as np
from lpui import text


def main(trace, fontname='LC12_UTF8_R90'):
    canv, meta = {}, {}
    f = text.font(fontname)
    res, by_font = [], collections.Counter()
    fonts = {}
    for line in open(os.path.join(trace, 'trace.jsonl')):
        if '"canvas' not in line and '"glyph"' not in line and '"capture"' not in line:
            continue
        r = json.loads(line)
        ev = r['ev']
        if ev == 'canvas':
            canv[r['pcc']] = text.Canvas(r['w'], r['h']); meta[r['pcc']] = (r['base'], r['w'], r['h'])
        elif ev == 'canvas_fill' and r['pcc'] in canv:
            c = canv[r['pcc']]
            buf = bytearray(c.tiles())
            v = r['value'] & 0xff if r['how'] != 'clear' else r['value'] & 0xff
            buf[r['off']:r['off'] + r['size']] = bytes([v]) * min(r['size'], len(buf) - r['off'])
            c.set_tiles(bytes(buf))
        elif ev == 'glyph' and r['pcc'] in canv:
            c = canv[r['pcc']]
            fonts.setdefault(r['font'], 0)
            fonts[r['font']] += 1
            gi = f.cmap.get(r['code'], f.alter)
            left, gw, cw = f.metrics(gi)
            c.glyph(f, r['x'] - f.cell_w, r['y'] + left, r['cl'], gi)
            c.last_font = r['font']
        elif ev == 'capture':
            ram = open(os.path.join(r['path'], 'ram.bin'), 'rb').read()
            for pcc, c in canv.items():
                base, w, h = meta[pcc]
                o = pcc - 0x2000000
                b2, w2, h2 = struct.unpack_from('<Iii', ram, o)
                if (b2, w2, h2) != (base, w, h):
                    continue                                  # freed / reused
                got = np.frombuffer(ram[base - 0x2000000:base - 0x2000000 + w * h * 32], np.uint8)
                if not got.any() and not c.px.any():
                    continue
                d = int((got != np.frombuffer(c.tiles(), np.uint8)).sum())
                res.append((r['f'], pcc, d, getattr(c, 'last_font', None)))
                by_font[(getattr(c, 'last_font', None), d == 0)] += 1
    exact = sum(1 for x in res if x[2] == 0)
    print('%s: %d live canvases checked at captures, %d exact; fonts used %s' % (fontname, len(res), exact, {hex(k): v for k, v in fonts.items()}))
    print('by font (font, exact):', {('%x' % k if k else k, e): v for (k, e), v in by_font.items()})
    print('mismatches:', [(fr, hex(o), n) for fr, o, n, _ in [x for x in res if x[2]][:8]])


if __name__ == '__main__':
    main(sys.argv[1], *(sys.argv[2:3]))
