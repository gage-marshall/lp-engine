"""The game's text box (TextBox_Init 0x2083e58, 0x2a8 bytes): an NNSG2dCharCanvas on a text BG plus the string and the
typewriter state. One widget for every boxed text in the game: the scene dialogue (name + 3 lines), the choice options,
the top-screen guide (onboarding / navigator lines) and the menus' text canvases.

Fields (decompiled TextBox_* / TextCanvas_InitForBG / Text_BuildLayout / Text_DrawNextChar; checked against RAM):
  +0x00 NNSG2dCharCanvas (charBase = tile buffer, areaWidth, areaHeight, dstBpp, param, vtable)
  +0x28 / +0x2c area width / height in tiles        +0x30 byte offset of the tiles in the BG's char block
  +0x34 tile bytes  +0x38 tile buffer  +0x3c char block address  +0x40 screen (map) address  +0x44 font  +0x48 engine (1 sub)
  +0x60 string (0x200, UTF-8)
  +0x274 / +0x278 pen x / y   +0x27c byte position in the string   +0x280 characters drawn   +0x284 line
  +0x26c / +0x270 line start pen x / y (TextBox_Layout: x0, y0 or the centred y)
  +0x288 done (whole string drawn)   +0x28c / +0x290 layout x0 / y0   +0x294 / +0x298 map left / top
  +0x29c character count   +0x2a0 layout mode (2 = centred along the area: y0 = (h*8 - 12 n) / 2)

Drawing (Text_BuildLayout = all at once, Text_DrawNextChar = one character per step): the map is written first
(NNS_G2dMapScrToCharText: charNo = offset/32 + 1, palette 15, 256-wide screen, row-major over the area at (left, top)),
then characters at the pen: x stays, y += advance (12, 6 for ASCII); '\\n' -> x -= 12, y = y0 - 6; '\\\\' toggles the
colour between the caller's and 3. The tiles go to char block + offset + 0x20 (charNo starts at 1)."""
import struct
import numpy as np
from lpui import text

B = 0x2000000
SIZE = 0x2a8


class TextBox:
    def __init__(self, ram, addr):
        o = addr - B
        rd = lambda k: struct.unpack_from('<I', ram, o + k)[0]
        rs = lambda k: struct.unpack_from('<i', ram, o + k)[0]
        self.addr = addr
        self.w, self.h, self.char_ofs = rd(0x28), rd(0x2c), rd(0x30)
        self.char_ptr, self.scr_ptr, self.engine = rd(0x3c), rd(0x40), rd(0x48)
        self.string = ram[o + 0x60:o + 0x260].split(b'\0')[0].decode('utf8', 'replace')
        self.pen = (rs(0x274), rs(0x278))
        self.drawn, self.done = rd(0x280), rd(0x288)
        self.start = (rs(0x26c), rs(0x270))           # line start as TextBox_Layout set it (centred y in mode 2)
        self.params = (rs(0x28c), rs(0x290))
        self.left, self.top = rs(0x294), rs(0x298)
        self.count, self.mode = rd(0x29c), rd(0x2a0)

    @property
    def mem(self):
        return 'bg_sub' if self.scr_ptr >= 0x6200000 else 'bg_main'

    @staticmethod
    def vram_off(p):
        return p - (0x6200000 if p >= 0x6200000 else 0x6000000)

    def mapped(self):
        """a box writes its map entries when it draws (the first DrawNextChar step or BuildLayout)"""
        return bool(self.done or self.drawn)

    def canvas(self, colour, upto=None):
        """the box's CharCanvas with the string drawn (first `upto` characters; None = whole string)"""
        c = text.Canvas(self.w, self.h)
        f = text.font()
        x, y = self.start
        x0, y0 = self.start
        cl = colour
        for i, ch in enumerate(self.string):
            if upto is not None and i >= upto:
                break
            if ch == '\n':
                x, y = x - 12, y0 - 6
                continue
            a = text.advance(ch)
            if a == 0:
                cl = colour if cl == 3 else 3
                continue
            c.draw_char(f, x, y, cl, ch)
            y += a
        return c

    def apply(self, vram, colour):
        """write this box's tiles and map entries into the VRAM model"""
        mem = vram.mem[self.mem]
        upto = None if self.done else self.drawn
        tiles = np.frombuffer(self.canvas(colour, upto).tiles(), np.uint8)
        a = self.vram_off(self.char_ptr) + self.char_ofs + 0x20
        mem[a:a + len(tiles)] = tiles
        if self.mapped():
            s = self.vram_off(self.scr_ptr)
            n = self.char_ofs // 32 + 1
            for y in range(self.top, self.top + self.h):
                for x in range(self.left, self.left + self.w):
                    o = s + (y * 32 + x) * 2
                    v = 0xf000 | n
                    mem[o], mem[o + 1] = v & 0xff, v >> 8
                    n += 1

    def on_screen(self, mem):
        """the screen's map points at this box (its first map entry = palette 15, charNo offset/32 + 1)"""
        o = self.vram_off(self.scr_ptr) + (self.top * 32 + self.left) * 2
        return int(mem[o]) | int(mem[o + 1]) << 8 == 0xf000 | (self.char_ofs // 32 + 1)

    def regions(self):
        """[(mem, start, end)] of the bytes this box owns (tiles; map entries when mapped)"""
        a = self.vram_off(self.char_ptr) + self.char_ofs + 0x20
        out = [(self.mem, a, a + self.w * self.h * 32)]
        if self.mapped():
            s = self.vram_off(self.scr_ptr)
            for y in range(self.top, self.top + self.h):
                o = s + (y * 32 + self.left) * 2
                out.append((self.mem, o, o + self.w * 2))
        return out


def find(ram, lo=0x2200000, hi=0x2400000):
    """every live text box in RAM (vtable +0x14 = the canvas vtable, screen / char pointers in BG VRAM)"""
    out = []
    for a in range(lo - B, min(hi - B, len(ram) - SIZE), 4):
        if struct.unpack_from('<I', ram, a + 0x14)[0] != 0x20fbedc:
            continue
        scr, chp = struct.unpack_from('<II', ram, a + 0x40)[0], struct.unpack_from('<I', ram, a + 0x3c)[0]
        if not any(lo2 <= scr < lo2 + 0x80000 for lo2 in (0x6000000, 0x6200000)):
            continue
        w, h = struct.unpack_from('<II', ram, a + 0x28)
        if 1 <= w <= 32 and 1 <= h <= 32:
            out.append(TextBox(ram, a + B))
    return out
