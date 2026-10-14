"""Text canvases as screen content: each text box's canvas = the Text_DrawString calls made on it since it was last cleared
(each: string, pen x / y, map left / top, colour) plus its own typewriter string (lpui.textbox). This is the text part of
a screen spec: rendering it needs no RAM.

Extraction (from a replay trace): CanvasInitForBG / a clear of the whole tile buffer start a canvas afresh; DrawString
(canvas, str, x, y, left, top, colour) appends a draw; the box's own string state is read at the capture. Canvases the game
clears and redraws every frame can be caught part way: the screen shows the last completed draw list."""
import json, os
import numpy as np
from lpui import text, textbox


class CanvasSpec:
    def __init__(self, box, draws):
        self.box = box                   # lpui.textbox.TextBox (geometry + own string state)
        self.draws = list(draws)         # [(string, x, y, left, top, colour)]

    def canvas(self, colour_own=2):
        c = text.Canvas(self.box.w, self.box.h)
        for s, x, y, left, top, cl in self.draws:
            text.draw_string(c, s, x, y, cl)
        if self.box.string and (self.box.drawn or self.box.done):
            own = self.box.canvas(colour_own, None if self.box.done else self.box.drawn)
            c.px = np.where(own.px != 0, own.px, c.px)
        return c

    def places(self):
        p = [(left, top) for s, x, y, left, top, cl in self.draws]
        if self.box.mapped():
            p.append((self.box.left, self.box.top))
        return list(dict.fromkeys(p))

    def on_screen(self, mem):
        bx = self.box
        for left, top in self.places():
            o = bx.vram_off(bx.scr_ptr) + (top * 32 + left) * 2
            if o + 1 < len(mem) and int(mem[o]) | int(mem[o + 1]) << 8 == 0xf000 | (bx.char_ofs // 32 + 1):
                return True
        return False

    def tile_range(self):
        bx = self.box
        a = bx.vram_off(bx.char_ptr) + bx.char_ofs + 0x20
        return bx.mem, a, a + bx.w * bx.h * 32

    def apply(self, vram, colour_own=2):
        bx = self.box
        mem = vram.mem[bx.mem]
        tiles = np.frombuffer(self.canvas(colour_own).tiles(), np.uint8)
        a = bx.vram_off(bx.char_ptr) + bx.char_ofs + 0x20
        mem[a:a + len(tiles)] = tiles
        for left, top in self.places():
            s = bx.vram_off(bx.scr_ptr)
            n = bx.char_ofs // 32 + 1
            for yy in range(top, top + bx.h):
                for xx in range(left, left + bx.w):
                    o = s + (yy * 32 + xx) * 2
                    if o + 1 < len(mem):
                        v = 0xf000 | n
                        mem[o], mem[o + 1] = v & 0xff, v >> 8
                    n += 1


def walk(trace_dir):
    """yield (frame, capture path, {canvas address: [draws]}) at each capture"""
    draws, sizes, done = {}, {}, {}     # done: the draw list as it stood at the last clear (the last completed cycle)
    for line in open(os.path.join(trace_dir, 'trace.jsonl')):
        if '"ev": "text"' not in line and '"ev": "canvas' not in line and '"ev": "capture"' not in line:
            continue
        r = json.loads(line)
        ev = r['ev']
        if ev == 'canvas':
            draws[r['pcc']] = []
            sizes[r['pcc']] = r['w'] * r['h'] * 32
        elif ev == 'canvas_fill':
            if r['off'] == 0 and r['size'] >= sizes.get(r['pcc'], 1 << 30):
                done[r['pcc']] = draws.get(r['pcc'], [])
                draws[r['pcc']] = []
        elif ev == 'text' and r['fn'] == 'DrawString':
            a = r['args']
            s = bytes.fromhex(r['raw']).decode('utf8', 'replace') if r.get('raw') else ''
            sg = lambda v: v - (1 << 32) if v >= 1 << 31 else v
            draws.setdefault(a[0], []).append((s, sg(a[2]), sg(a[3]), a[4], a[5], a[6]))
        elif ev == 'capture':
            # a canvas cleared and redrawn every frame may be caught mid-redraw: VRAM still holds the completed cycle
            yield r['f'], r['path'], {k: list(done[k] if k in done and len(v) < len(done[k]) and done[k][:len(v)] == v else v)
                                      for k, v in draws.items()}


def specs(ram, draws):
    """[CanvasSpec] of the live text boxes in a RAM image"""
    return [CanvasSpec(bx, draws.get(bx.addr, [])) for bx in textbox.find(ram)]
