#!/usr/bin/env python3
"""UI graphics relabelling: the Japanese drawn into 2D graphics (buttons, tabs, labels) redrawn in English.

An item is one cell of a cell bank (NCER) over its character data (NCGR) in a 0x15618 archive. Everything works on
palette indices, so no palette is needed: the cell is composed as an index image from its OAM objects; the
background is its most common index; the text is the strokes (connected pixels of other indices) that do not touch
the edge of the cell's opaque shape. The strokes are filled with the background, the English is drawn in the
game's own font (LC12, or LC10 when LC12 does not fit) in the strokes' most common index, centred on where the
Japanese was and turned like it (`rot`: 0, or 90 = read top to bottom, letters facing right, as on the sideways
screen), and the pixels are written back into the character data through the same OAM mapping.

Tables: translation/<lang>/gfx/<archive path with / as _>.json
    {"archive": "arc/title.bin", "items": [{"ncgr": 10, "ncer": 11, "cell": 0, "en": "Game Start",
                                           "rot": 90}]}
`apply` writes each changed archive to the patch overlay (patch/fs/<archive>), entries stored plain.

    lp_gfx.py apply CONTENT TABLE_DIR           write patch/fs/... for every table
    lp_gfx.py show CONTENT ARCHIVE NCGR NCER CELL PNG   one cell 4x with an 8 px grid, to read a box from
    lp_gfx.py show-bg CONTENT ARCHIVE NCGR NSCR PNG   a BG screen 2x with an 8 px grid; BG items: {"ncgr", "nscr",
                                                 "box": [x, y, w, h] (screen px around the text), "en", "rot", "mode"}
    lp_gfx.py check CONTENT TABLE...             relabel every item in memory: what does not fit
    lp_gfx.py preview CONTENT TABLE PNG          before / after sheet of a table's items (look at it)
"""
import json
import os
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns2d as N  # noqa: E402
from nftr import Font  # noqa: E402

ARC_MAGIC = 0x15618


# ---- archives -------------------------------------------------------------------------------------------------

def lz10(d):
    """Nintendo LZ10 (type 0x10) decompression"""
    assert d[0] == 0x10
    n = d[1] | d[2] << 8 | d[3] << 16
    out = bytearray(); p = 4
    while len(out) < n:
        flags = d[p]; p += 1
        for bit in range(8):
            if len(out) >= n:
                break
            if flags & (0x80 >> bit):
                b0, b1 = d[p], d[p + 1]; p += 2
                ln = (b0 >> 4) + 3; disp = ((b0 & 15) << 8 | b1) + 1
                for _ in range(ln):
                    out.append(out[-disp])
            else:
                out.append(d[p]); p += 1
    return bytes(out)


class Arc:
    """a 0x15618 archive: header (+4 count, +6 record size 12 or 44 with a 32-byte name), records (offset, stored
    size, unpacked size or 0 = stored plain); entries kept decompressed"""

    def __init__(self, raw):
        assert struct.unpack_from('<I', raw)[0] == ARC_MAGIC
        self.raw = raw
        self.count = struct.unpack_from('<H', raw, 4)[0]
        first = struct.unpack_from('<I', raw, 12)[0]  # the first entry's data follows the records
        self.rec = 44 if (first - 12) // self.count >= 44 else 12
        self.entries = []
        for i in range(self.count):
            o, s, u = struct.unpack_from('<3I', raw, 12 + self.rec * i)
            b = raw[o:o + s]
            self.entries.append(lz10(b) if u and b[:1] == b'\x10' else b)
        self.changed = set()

    def build(self):
        out = bytearray(self.raw[:12 + self.rec * self.count])
        out += bytes((-len(out)) & 3)
        for i, b in enumerate(self.entries):
            if i in self.changed:
                struct.pack_into('<3I', out, 12 + self.rec * i, len(out), len(b), 0)
                out += b
            else:  # unchanged entries keep their stored form
                o, s, u = struct.unpack_from('<3I', self.raw, 12 + self.rec * i)
                struct.pack_into('<3I', out, 12 + self.rec * i, len(out), s, u)
                out += self.raw[o:o + s]
            out += bytes((-len(out)) & 3)
        return bytes(out)


def char_range(d):
    """(start, end) of the character data in an NCGR file"""
    sig, b = N.blocks(d)
    o, n = b['CHAR']
    sz, roff = struct.unpack_from("<2I", d, o + 16)
    return o + roff, o + roff + sz


# ---- a cell as an index image ---------------------------------------------------------------------------------

class CellImage:
    """the cell's pixels as palette indices (0 transparent) and, per pixel, where it lives in the character data"""

    def __init__(self, ncgr, ncer, cell):
        self.ch = N.parse_ncgr(ncgr)
        self.ce = N.parse_ncer(ncer)
        items = [dict(N.oam_decode(*o), k=k) for k, o in enumerate(self.ce['cells'][cell]['oams'])]
        items = [i for i in items if not i['hide']]
        self.x0 = min(i['x'] for i in items); self.y0 = min(i['y'] for i in items)
        self.w = max(i['x'] + i['w'] for i in items) - self.x0
        self.h = max(i['y'] + i['h'] for i in items) - self.y0
        self.px = [[0] * self.w for _ in range(self.h)]
        self.at = [[None] * self.w for _ in range(self.h)]
        self.owner = [[None] * self.w for _ in range(self.h)]  # index of the object a pixel belongs to
        self.objmap = {}  # object index -> {(x, y): (byte offset, shift)} over all its pixels
        self.objs = []  # (index, x, y, w, h, palette) in cell coordinates
        bpp = self.ch['bpp']; bs = 32 if bpp == 4 else 64
        boundary = 32 << self.ce['mapping'] if self.ce['mapping'] in (0, 1, 2, 3) else 32
        raw = self.ch['raw']
        for it in items:  # lower OAM index on top: a pixel is the first opaque one; transparent ones show what is below
            tw, th = it['w'] // 8, it['h'] // 8
            self.objs.append((it['k'], it['x'] - self.x0, it['y'] - self.y0, it['w'], it['h'], it['pal']))
            om = self.objmap.setdefault(it['k'], {})
            for ty in range(th):
                for tx in range(tw):
                    base = it['tile'] * boundary + (ty * tw + tx) * bs
                    if base + bs > len(raw):
                        continue
                    for y in range(8):
                        for x in range(8):
                            k = y * 8 + x
                            off, shift = (base + k // 2, 4 * (k & 1)) if bpp == 4 else (base + k, 0)
                            v = (raw[off] >> shift) & 15 if bpp == 4 else raw[off]
                            xx = (7 - x if it['hflip'] else x) + ((tw - 1 - tx) if it['hflip'] else tx) * 8
                            yy = (7 - y if it['vflip'] else y) + ((th - 1 - ty) if it['vflip'] else ty) * 8
                            X, Y = it['x'] - self.x0 + xx, it['y'] - self.y0 + yy
                            om[(X, Y)] = (off, shift)
                            if self.px[Y][X]:
                                continue  # an upper object's opaque pixel
                            if v or self.at[Y][X] is None:
                                self.px[Y][X] = v
                                self.at[Y][X] = (off, shift)
                                self.owner[Y][X] = it['k']

    def write(self, raw):
        """the index image back into character data `raw` (bytearray)"""
        bpp = self.ch['bpp']
        for y in range(self.h):
            for x in range(self.w):
                a = self.at[y][x]
                if a is None:
                    continue
                off, shift = a
                if bpp == 4:
                    raw[off] = (raw[off] & ~(15 << shift) & 0xff) | (self.px[y][x] << shift)
                else:
                    raw[off] = self.px[y][x]


# ---- a box of a BG screen as an index image -------------------------------------------------------------------

class BgImage:
    """a box (x, y, w, h in pixels) of a text BG screen (NSCR over NCGR, 4 bpp) as an index image; a pixel's value is
    palette row << 4 | index (0 transparent). Writing back gives every tile of the box that the screen also uses
    elsewhere (or twice) a copy of its own, appended to the character data, so nothing outside the box changes."""

    def __init__(self, ncgr, nscr, box):
        self.ch = N.parse_ncgr(ncgr)
        self.sc = N.parse_nscr(nscr)
        assert self.ch['bpp'] == 4 and self.sc['format'] != 1, 'only 4 bpp text BGs'
        self.raw = bytearray(self.ch['raw'])
        self.ents = list(struct.unpack('<%dH' % (len(self.sc['raw']) // 2), self.sc['raw']))
        self.W = self.sc['w'] // 8
        x0, y0, w, h = box
        self.tx0, self.ty0 = x0 // 8, y0 // 8
        self.tx1, self.ty1 = (x0 + w + 7) // 8, (y0 + h + 7) // 8
        self.w, self.h = (self.tx1 - self.tx0) * 8, (self.ty1 - self.ty0) * 8
        self.read()

    def tiles(self):
        for ty in range(self.ty0, self.ty1):
            for tx in range(self.tx0, self.tx1):
                yield ty * self.W + tx, (tx - self.tx0) * 8, (ty - self.ty0) * 8

    def read(self):
        self.px = [[0] * self.w for _ in range(self.h)]
        for i, bx, by in self.tiles():
            e = self.ents[i]
            tile, hf, vf, pl = e & 0x3ff, e >> 10 & 1, e >> 11 & 1, e >> 12
            for y in range(8):
                for x in range(8):
                    sx, sy = 7 - x if hf else x, 7 - y if vf else y
                    k = sy * 8 + sx
                    b = tile * 32 + k // 2
                    v = (self.raw[b] >> 4 * (k & 1)) & 15 if b < len(self.raw) else 0
                    self.px[by + y][bx + x] = pl << 4 | v if v else 0

    def write(self):
        """-> (new character data, new screen data)"""
        from collections import Counter
        uses = Counter(e & 0x3ff for e in self.ents)
        inbox = Counter(self.ents[i] & 0x3ff for i, _, _ in self.tiles())
        for i, bx, by in self.tiles():
            e = self.ents[i]
            tile, hf, vf = e & 0x3ff, e >> 10 & 1, e >> 11 & 1
            block = [self.px[by + y][bx:bx + 8] for y in range(8)]
            orig = [[0] * 8 for _ in range(8)]
            for y in range(8):
                for x in range(8):
                    sx, sy = 7 - x if hf else x, 7 - y if vf else y
                    k = sy * 8 + sx
                    orig[y][x] = (self.raw[tile * 32 + k // 2] >> 4 * (k & 1)) & 15
            if all((block[y][x] & 15) == orig[y][x] for y in range(8) for x in range(8)):
                continue  # unchanged
            if uses[tile] > 1:  # shared: a copy of its own
                uses[tile] -= 1
                tile = len(self.raw) // 32
                assert tile < 1024, 'no tile numbers left'
                self.raw += bytes(32)
                self.ents[i] = (e & ~0x3ff) | tile
            for y in range(8):
                for x in range(8):
                    sx, sy = 7 - x if hf else x, 7 - y if vf else y
                    k = sy * 8 + sx
                    b = tile * 32 + k // 2
                    self.raw[b] = (self.raw[b] & ~(15 << 4 * (k & 1)) & 0xff) | ((block[y][x] & 15) << 4 * (k & 1))
        return bytes(self.raw), struct.pack('<%dH' % len(self.ents), *self.ents)


def replace_char(ncgr, raw):
    """an NCGR with new character data (sizes in the block, the header and the tile count updated)"""
    d = bytearray(ncgr)
    sig, b = N.blocks(ncgr)
    o, n = b['CHAR']
    s, e = char_range(ncgr)
    d[s:e] = raw
    grow = len(raw) - (e - s)
    struct.pack_into('<I', d, o + 16, len(raw))
    struct.pack_into('<I', d, o - 4, struct.unpack_from('<I', ncgr, o - 4)[0] + grow)
    struct.pack_into('<I', d, 8, len(d))
    th, tw = struct.unpack_from('<2H', ncgr, o)
    if tw not in (0, 0xffff) and grow:
        struct.pack_into('<H', d, o, (len(raw) // 32 + tw - 1) // tw)
    return bytes(d)


def replace_screen(nscr, raw):
    d = bytearray(nscr)
    sig, b = N.blocks(nscr)
    o, n = b['SCRN']
    d[o + 12:o + 12 + len(raw)] = raw
    return bytes(d)


def oam_slots(ncer):
    """per cell, per object: the byte offset of its third attribute (tile number) in the NCER file"""
    sig, b = N.blocks(ncer)
    o, n = b['CEBK']
    ncell, attr, cells_off = struct.unpack_from('<2HI', ncer, o)
    stride = 16 if attr & 1 else 8
    base = o + cells_off + ncell * stride
    out = []
    for i in range(ncell):
        noam, _, oam_off = struct.unpack_from('<HHI', ncer, o + cells_off + i * stride)
        out.append([base + oam_off + 6 * k + 4 for k in range(noam)])
    return out


def move_objs(ncer, cell, move):
    """the NCER bytes with objects of `cell` shifted: `move` {"obj index": [dx, dy]} (an item's "move": text objects
    set apart by gaps put side by side, so the English fits in them)"""
    if not move:
        return ncer
    d = bytearray(ncer)
    for k, (dx, dy) in move.items():
        at = oam_slots(ncer)[cell][int(k)] - 4
        a0, a1 = struct.unpack_from('<2H', d, at)
        it = N.oam_decode(a0, a1, 0)
        struct.pack_into('<2H', d, at, (a0 & ~0xff) | ((it['y'] + dy) & 0xff), (a1 & ~0x1ff) | ((it['x'] + dx) & 0x1ff))
    return bytes(d)


def unshare(raw, ncer, cell, bpp, listed=(), only=None, memo=None, key=None, own=False):
    """gives objects of `cell` (those in `only`, default all) whose tiles are used twice in the cell, or by a cell not
    `listed` (cells relabelled with the same English share them on purpose: animation frames), a copy of their own
    tiles - one copy per block and `key` (`memo`; `own`: per object too, for a frame repeating one block that the
    English runs across), in tiles no object of the bank uses when there is room, else appended to the character
    data `raw` (bytearray) - and points each at it; -> the new NCER bytes"""
    from collections import Counter
    ce = N.parse_ncer(ncer)
    unit = 32 << ce['mapping'] if ce['mapping'] in (0, 1, 2, 3) else 32
    bs = 32 if bpp == 4 else 64
    def span(o):
        it = N.oam_decode(*o)
        return it['tile'] * unit, (it['w'] // 8) * (it['h'] // 8) * bs
    users, any_use = Counter(), set()
    for i, c in enumerate(ce['cells']):
        for o in c['oams']:
            a, n = span(o)
            any_use.update(range(a, a + n, 32))
            if i == cell or i not in listed:
                for t in range(a, a + n, 32):
                    users[t] += 1
    d = bytearray(ncer)
    slots = oam_slots(ncer)

    def alloc(n):
        for start in range(0, len(raw) - n + 1, unit):
            if all(t not in any_use for t in range(start, start + n, 32)):
                return start
        while len(raw) % unit:
            raw.append(0)
        start = len(raw)
        raw.extend(bytes(n))
        return start
    for k, o in enumerate(ce['cells'][cell]['oams']):
        if only is not None and k not in only:
            continue
        a, n = span(o)
        if all(users[t] <= 1 for t in range(a, a + n, 32)):
            continue
        mk = (a, n, key, k) if own else (a, n, key)
        if memo is not None and mk in memo:
            new = memo[mk]
        else:
            new = alloc(n)
            assert new // unit < 1024, 'no tile numbers left'
            raw[new:new + n] = raw[a:a + n]
            if memo is not None:
                memo[mk] = new
        any_use.update(range(new, new + n, 32))
        for t in range(a, a + n, 32):
            users[t] -= 1
        at = slots[cell][k]
        a2 = struct.unpack_from('<H', d, at)[0]
        struct.pack_into('<H', d, at, (a2 & ~0x3ff) | new // unit)
    return bytes(d)

# ---- text detection and drawing --------------------------------------------------------------------------------

def components(img, keep):
    """8-connected components of pixels where keep(v)"""
    h, w = len(img), len(img[0])
    seen = [[False] * w for _ in range(h)]
    out = []
    for y in range(h):
        for x in range(w):
            if seen[y][x] or not keep(img[y][x]):
                continue
            stack = [(x, y)]; seen[y][x] = True; comp = []
            while stack:
                cx, cy = stack.pop(); comp.append((cx, cy))
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        nx, ny = cx + dx, cy + dy
                        if 0 <= nx < w and 0 <= ny < h and not seen[ny][nx] and keep(img[ny][nx]):
                            seen[ny][nx] = True; stack.append((nx, ny))
            out.append(comp)
    return out


def find_text(img, colours=None):
    """(background index, text pixels, ink index): strokes = components of non-background pixels that do not touch
    a transparent pixel or the image edge"""
    from collections import Counter
    h, w = len(img), len(img[0])
    cnt = Counter(v for row in img for v in row if v)
    bg = cnt.most_common(1)[0][0]

    def edge(x, y):
        if x == 0 or y == 0 or x == w - 1 or y == h - 1:
            return True
        return any(img[y + dy][x + dx] == 0 for dy in (-1, 0, 1) for dx in (-1, 0, 1))
    text = []
    for comp in components(img, lambda v: v not in (0, bg)):
        if not any(edge(x, y) for x, y in comp):
            text += comp
    # the ink is the strokes' core colour: the index least often next to the background (anti-aliasing sits there)
    near, total = Counter(), Counter()
    for x, y in text:
        v = img[y][x]
        total[v] += 1
        if any(img[y + dy][x + dx] == bg for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0))):
            near[v] += 1
    ink = min(total, key=lambda v: (near[v] / total[v], -total[v])) if text else None
    if text and colours:  # with the palette: the stroke colour furthest from the background (among common ones)
        lum = lambda v: luma(colours[v]) if v < len(colours) else 0
        common = [v for v in total if total[v] * 8 >= len(text)]
        ink = max(common, key=lambda v: abs(lum(v) - lum(bg)))
    return bg, text, ink


def luma(c):
    """perceived brightness of an (r, g, b) colour"""
    return 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]


_FONTS = {}


def font(content, size):
    if size not in _FONTS:
        _FONTS[size] = Font(open(os.path.join(content, 'assets/test_font/LC%d_UTF8_R90.NFTR' % size), 'rb').read())
    return _FONTS[size]


def embolden(bm):
    """each stroke one pixel wider to the right (bold, like the game's drawn labels)"""
    out = [r + [0] for r in bm]
    for y, r in enumerate(bm):
        for x, v in enumerate(r):
            if v:
                out[y][x + 1] = 1
    return out


def render(text, f):
    """the English as a 0/1 bitmap (rows), letters one pixel apart (proportional, as the engine's text)"""
    glyphs = []
    for c in text:
        gi = f.glyph_index(c)
        left, gw, cw = f.metrics(gi)
        w, h, rows = f.glyph_bitmap(gi)
        cols = [x for x in range(w) if any(r[x] for r in rows)]
        if c == ' ' or not cols:
            glyphs.append((None, 3))
            continue
        glyphs.append(([[1 if r[x] else 0 for x in range(cols[0], cols[-1] + 1)] for r in rows], cols[-1] - cols[0] + 1))
    width = sum(g[1] for g in glyphs) + max(0, len(glyphs) - 1)
    H = f.cell_h
    out = [[0] * max(width, 1) for _ in range(H)]
    x = 0
    for g, gw in glyphs:
        if g:
            for y in range(H):
                for i in range(gw):
                    if g[y][i]:
                        out[y][x + i] = 1
        x += gw + 1
    rows = [r for r in out if any(r)]
    top = next(i for i, r in enumerate(out) if any(r)) if rows else 0
    bot = max(i for i, r in enumerate(out) if any(r)) if rows else 0
    return out[top:bot + 1]


def rotate(bm, rot):
    """turn a bitmap: 90 = clockwise (read top to bottom, letters facing right)"""
    if rot == 90:
        return [list(r) for r in zip(*bm[::-1])]
    if rot == 270:
        return [list(r) for r in zip(*bm)][::-1]
    return bm


def rects(box):
    """a box: [x, y, w, h] or a list of them"""
    return box if box and isinstance(box[0], list) else ([box] if box else [])


def in_box(box, x, y, slack=0):
    return any(bx - slack <= x < bx + bw + slack and by - slack <= y < by + bh + slack for bx, by, bw, bh in rects(box))


def relabel_glyph(cell, en, rot, content, colours=None, box=None):
    """a free-standing word (ink with an outline on transparency): cleared and the English drawn in the ink with
    the same outline (the index of the pixels next to transparency) around it, centred, LC12 or LC10"""
    from collections import Counter
    img = cell.px
    h, w = cell.h, cell.w
    edge = Counter(); inner = Counter()
    for y in range(h):
        for x in range(w):
            v = img[y][x]
            if not v:
                continue
            nb = [img[y + dy][x + dx] if 0 <= x + dx < w and 0 <= y + dy < h else 0 for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0))]
            (edge if 0 in nb else inner)[v] += 1
    if not edge:
        return ['empty cell']
    keep_out = None
    if box:  # only the word inside the box: components whose extent lies in it (2 px slack); the rest stays
        inside = set()
        for comp in components(img, lambda v: v != 0):
            if all(in_box(box, x, y, 2) for x, y in comp):
                inside.update(comp)
        if not inside:  # the word is joined to other art: the whole box is cleared
            inside = {(x, y) for y in range(h) for x in range(w) if img[y][x] and in_box(box, x, y)}
        if not inside:
            return ['nothing in the box']
        edge = Counter(); inner = Counter()
        for x, y in inside:
            v = img[y][x]
            nb = [img[y + dy][x + dx] if 0 <= x + dx < w and 0 <= y + dy < h else 0 for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0))]
            (edge if 0 in nb else inner)[v] += 1
        keep_out = inside
    if not edge:  # no transparency around the word: its two commonest colours
        edge, inner = Counter(dict(inner.most_common()[:1])), Counter(dict(inner.most_common()[1:]))
    outline = edge.most_common(1)[0][0]
    inks = [v for v, _ in inner.most_common() if v != outline] or [v for v, _ in edge.most_common() if v != outline]
    ink = inks[0] if inks else outline
    if colours and inks:  # the ink: the inner colour furthest from the outline
        lum = lambda v: luma(colours[v]) if v < len(colours) else 0
        ink = max(inks[:3], key=lambda v: abs(lum(v) - lum(outline)))
    opaque = list(keep_out) if keep_out else [(x, y) for y in range(h) for x in range(w) if img[y][x]]
    if box:
        opaque = [(r[0], r[1]) for r in rects(box)] + [(r[0] + r[2] - 1, r[1] + r[3] - 1) for r in rects(box)]
    xs = [x for x, _ in opaque]; ys = [y for _, y in opaque]
    length, thick = ((max(ys) - min(ys) + 1), (max(xs) - min(xs) + 1)) if rot in (90, 270) else ((max(xs) - min(xs) + 1), (max(ys) - min(ys) + 1))
    for size in (12, 10):
        bm = render(en, font(content, size))
        if len(bm[0]) + 2 <= max(length, w if rot == 0 else h) and len(bm) + 2 <= thick + 4:
            break
    else:
        return ['"%s" is %d px, the word has %d' % (en, len(bm[0]) + 2, length)]
    bm = rotate(bm, rot)
    bh, bw = len(bm), len(bm[0])
    cx, cy = (min(xs) + max(xs) + 1) // 2, (min(ys) + max(ys) + 1) // 2
    ox, oy = cx - bw // 2, cy - bh // 2
    if keep_out:
        for x, y in keep_out:
            img[y][x] = 0
    else:
        for y in range(h):
            for x in range(w):
                img[y][x] = 0
    for y in range(bh):
        for x in range(bw):
            if bm[y][x]:
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        X, Y = ox + x + dx, oy + y + dy
                        if 0 <= X < w and 0 <= Y < h and not img[Y][X]:
                            img[Y][X] = outline
    for y in range(bh):
        for x in range(bw):
            if bm[y][x] and 0 <= ox + x < w and 0 <= oy + y < h:
                img[oy + y][ox + x] = ink
    return []


def fill_tones(img, box, rot, tones, full=False):
    """the text in a box over a ground of a few `tones` (two-tone buttons with a slanted split) repainted: along each
    line across the reading direction, a run of other colours takes the tone on either side; between two different
    tones the split follows the line fitted through the ground's own tone changes in the box. A tone can be a list
    of indices (a dithered tone: any of them is that tone; the text is repainted in the first). `full`: every pixel
    of the box redrawn from the fitted split (the tone left of it / right of it; a dithered tone as a checkerboard of
    its indices), for grounds with shading along the split. Returns the original colours of the repainted pixels"""
    h, w = len(img), len(img[0])
    group = {}
    for k, t in enumerate(tones):
        for v in (t if isinstance(t, list) else [t]):
            group[v] = k
    paint = [t[0] if isinstance(t, list) else t for t in tones]
    tones_l = [t if isinstance(t, list) else [t] for t in tones]
    tones = set(group)
    out = []
    for bx, by, bw, bh in rects(box):
        if rot in (90, 270):  # lines are rows, positions x
            lines = [[(x, y) for x in range(bx, bx + bw) if 0 <= x < w] for y in range(by, by + bh) if 0 <= y < h]
        else:
            lines = [[(x, y) for y in range(by, by + bh) if 0 <= y < h] for x in range(bx, bx + bw) if 0 <= x < w]
        get = lambda p: img[p[1]][p[0]]
        # the ground's own changes of tone (adjacent tone pixels), as (line, position) for the fit
        pts = [(li, i + 0.5) for li, ln in enumerate(lines) for i in range(len(ln) - 1)
               if get(ln[i]) in tones and get(ln[i + 1]) in tones and group[get(ln[i])] != group[get(ln[i + 1])]]
        if len(pts) >= 2 and len({p[0] for p in pts}) >= 2:
            n = len(pts); sx = sum(p[0] for p in pts); sy = sum(p[1] for p in pts)
            sxx = sum(p[0] ** 2 for p in pts); sxy = sum(p[0] * p[1] for p in pts)
            k = (n * sxy - sx * sy) / (n * sxx - sx * sx)
            fit = lambda li: (sy - k * sx) / n + k * li
        elif pts:
            fit = lambda li, c=pts[0][1]: c
        else:
            fit = None
        if full:
            from collections import Counter
            pairs = Counter((group[get(ln[i])], group[get(ln[i + 1])]) for li, ln in enumerate(lines) for i in range(len(ln) - 1)
                            if get(ln[i]) in tones and get(ln[i + 1]) in tones and group[get(ln[i])] != group[get(ln[i + 1])])
            only = Counter(group[get(p)] for ln in lines for p in ln if get(p) in tones).most_common(1)[0][0]
            left, right = pairs.most_common(1)[0][0] if pairs else (only, only)
            colour = lambda g, x, y: (tones_l[g][(x + y) % len(tones_l[g])])
            for li, ln in enumerate(lines):
                cut = fit(li) if fit else len(ln)
                for t, (x, y) in enumerate(ln):
                    out.append(img[y][x])
                    img[y][x] = colour(left if t + 0.5 < cut else right, x, y)
            continue
        for li, ln in enumerate(lines):
            vals = [get(p) for p in ln]
            i = 0
            while i < len(ln):
                if vals[i] in tones:
                    i += 1
                    continue
                j = i
                while j < len(ln) and vals[j] not in tones:
                    j += 1
                a = paint[group[vals[i - 1]]] if i > 0 else None
                b = paint[group[vals[j]]] if j < len(ln) else None
                if a is None and b is None:  # no tone on the whole line: the line before, as repainted
                    if li:
                        for t in range(i, j):
                            out.append(vals[t])
                            x, y = ln[t]; px, py = lines[li - 1][t]
                            img[y][x] = img[py][px]
                    i = j
                    continue
                if a is None or b is None or a == b or fit is None:
                    cut = j if b is None else i if a is None else (i + j) // 2 if a != b else j
                else:
                    cut = min(j, max(i, round(fit(li))))
                for t in range(i, j):
                    out.append(vals[t])
                    x, y = ln[t]
                    img[y][x] = (a if t < cut else b) if a is not None and b is not None else (a if a is not None else b)
                i = j
    return out


def relabel_fill(cell, en, rot, content, colours=None, box=None, ink=None, ground=None, outline=None):
    """explicit: every pixel of the box replaced, line by line across the reading direction, by the colour just
    outside the box on that line (flat fills, gradients along the label, cut-out windows); then the English drawn in
    `ink` (an index, "window" = transparent, default: the original's strongest colour in the box that is not a fill
    colour; transparent when the box had cut-out text)"""
    from collections import Counter
    img = cell.px
    h, w = cell.h, cell.w
    if not box:
        return ['fill mode needs a box']
    orig = Counter()
    fills = set()
    if ground == 'common':  # the box's own commonest colour (a label's ground under the text)
        cnt = Counter(img[y][x] for bx, by, bw, bh in rects(box) for y in range(max(0, by), min(h, by + bh)) for x in range(max(0, bx), min(w, bx + bw)) if img[y][x])
        ground = cnt.most_common(1)[0][0] if cnt else 0
    if isinstance(ground, list):  # the ground's own tones: only the text is repainted (fill_tones)
        for v in fill_tones(img, box, rot, ground):
            orig[v] += 1
        fills.update(v for t in ground for v in (t if isinstance(t, list) else [t]))
        ground, box_done = None, True
    elif isinstance(ground, dict) and 'tones' in ground:  # {"tones": [...]}: the whole box redrawn from the split
        for v in fill_tones(img, box, rot, ground['tones'], True):
            orig[v] += 1
        fills.update(v for t in ground['tones'] for v in (t if isinstance(t, list) else [t]))
        ground, box_done = None, True
    elif isinstance(ground, dict):  # {"rows": [y0, y1]}: the box tiled with the clean rows y0..y1 (dithered grounds)
        y0, y1 = ground['rows']
        pat = [list(img[y]) for y in range(y0, y1 + 1)]
        for bx, by, bw, bh in rects(box):
            for y in range(max(0, by), min(h, by + bh)):
                for x in range(max(0, bx), min(w, bx + bw)):
                    orig[img[y][x]] += 1
                    img[y][x] = pat[(y - y0) % len(pat)][x]
        fills.update(v for r in pat for v in r)
        ground, box_done = None, True
    else:
        box_done = False
    for bx, by, bw, bh in ([] if box_done else rects(box)):
        for y in range(by, by + bh):
            for x in range(bx, bx + bw):
                if 0 <= x < w and 0 <= y < h:
                    orig[img[y][x]] += 1
        if ground is not None:  # a given ground (0 = keep a cut-out window transparent)
            for y in range(max(0, by), min(h, by + bh)):
                for x in range(max(0, bx), min(w, bx + bw)):
                    img[y][x] = ground
            fills.add(ground)
        elif rot in (90, 270):  # a vertical label: each row filled from the pixels left / right of the box
            for y in range(max(0, by), min(h, by + bh)):
                a = img[y][bx - 1] if bx - 1 >= 0 else 0
                b = img[y][bx + bw] if bx + bw < w else 0
                for x in range(max(0, bx), min(w, bx + bw)):
                    img[y][x] = a if (x - bx) * 2 < bw or not b else b
                fills.update((a, b))
        else:  # a horizontal label: each column from the pixels above / below
            for x in range(max(0, bx), min(w, bx + bw)):
                a = img[by - 1][x] if by - 1 >= 0 else 0
                b = img[by + bh][x] if by + bh < h else 0
                for y in range(max(0, by), min(h, by + bh)):
                    img[y][x] = a if (y - by) * 2 < bh or not b else b
                fills.update((a, b))
    if ink == 'window':
        ink = 0
    elif ink is None:
        cand = [v for v, n in orig.most_common() if v not in fills and v]
        if 0 in orig and 0 not in fills and ground is None:
            ink = 0  # the text was cut out of the label
        elif cand and ground and colours:  # on a given ground: the original colour furthest from it
            lum = lambda v: luma(colours[v]) if v < len(colours) else 0
            ink = max(cand, key=lambda v: abs(lum(v) - lum(ground)))
        elif cand and ground == 0:  # a kept window: the original text's own colour
            ink = cand[0]
        elif cand and colours:
            ref = Counter(img[y][x] for bx, by, bw, bh in rects(box) for y in range(max(0, by), min(h, by + bh)) for x in range(max(0, bx), min(w, bx + bw))).most_common(1)[0][0]
            lum = lambda v: luma(colours[v]) if v < len(colours) else 0
            ink = max(cand[:4], key=lambda v: abs(lum(v) - lum(ref)))
        elif cand:
            ink = cand[0]
        else:
            return ['no ink colour: give "ink"']
    rs = rects(box)
    x0 = min(r[0] for r in rs); y0 = min(r[1] for r in rs)
    x1 = max(r[0] + r[2] for r in rs); y1 = max(r[1] + r[3] for r in rs)
    length, thick = (y1 - y0, x1 - x0) if rot in (90, 270) else (x1 - x0, y1 - y0)
    for size in (12, 10):
        bm = render(en, font(content, size))
        if len(bm[0]) <= length and len(bm) <= thick:
            break
    else:
        return ['"%s" is %d px long, %d high; the box has %d x %d' % (en, len(bm[0]), len(bm), length, thick)]
    bm = rotate(bm, rot)
    bh_, bw_ = len(bm), len(bm[0])
    ox, oy = (x0 + x1) // 2 - bw_ // 2, (y0 + y1) // 2 - bh_ // 2
    if outline is not None:  # a one-pixel ring around the letters
        for y in range(bh_):
            for x in range(bw_):
                if bm[y][x]:
                    for dy in (-1, 0, 1):
                        for dx in (-1, 0, 1):
                            X, Y = ox + x + dx, oy + y + dy
                            if 0 <= X < w and 0 <= Y < h:
                                img[Y][X] = outline
    for y in range(bh_):
        for x in range(bw_):
            if bm[y][x] and 0 <= ox + x < w and 0 <= oy + y < h:
                img[oy + y][ox + x] = ink
    return []


def relabel_objs(cell, en, rot, content, objs, raw, outline=None, ink=None):
    """the text is objects of its own (`objs`, OAM indices of the cell): they are cleared and the English drawn in
    their area, ink and outline from their own colours (outline: the colour next to transparency, ink: the inner one),
    LC12 or LC10; written straight into their tiles in `raw` (bytearray)"""
    from collections import Counter
    bpp = cell.ch['bpp']
    def get(a):
        off, sh = a
        return (raw[off] >> sh) & 15 if bpp == 4 else raw[off]
    pts, alias = {}, {}  # a point -> its pixel in the topmost listed object; where listed objects overlap, every one
    for k in sorted(objs, reverse=True):  # holds the pixel (the lower OAM index draws on top)
        for p, a in cell.objmap.get(k, {}).items():
            if p in pts:
                alias.setdefault(p, []).append(pts[p])
            pts[p] = a
    if not pts:
        return ['no such objects']
    val = {p: get(a) for p, a in pts.items()}
    edge, inner = Counter(), Counter()
    for (x, y), v in val.items():
        if v:
            nb = [val.get((x + dx, y + dy), 0) for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1))]
            (edge if 0 in nb else inner)[v] += 1
    if not edge:
        return ['the objects are empty']
    out_c = outline if outline is not None else edge.most_common(1)[0][0]
    ink_c = ink if ink is not None else (next((v for v, _ in inner.most_common() if v != out_c), None) or out_c)
    use_outline = outline is not None or bool(inner)
    xs = [x for x, _ in pts]; ys = [y for _, y in pts]
    x0, x1, y0, y1 = min(xs), max(xs) + 1, min(ys), max(ys) + 1
    length, thick = (y1 - y0, x1 - x0) if rot in (90, 270) else (x1 - x0, y1 - y0)
    for size in (12, 10):
        bm = render(en, font(content, size))
        if len(bm[0]) + 2 <= length and len(bm) + 2 <= thick:
            break
    else:
        return ['"%s" is %d x %d px, the objects hold %d x %d' % (en, len(bm[0]) + 2, len(bm) + 2, length, thick)]
    bm = rotate(bm, rot)
    bh, bw = len(bm), len(bm[0])
    ox, oy = (x0 + x1) // 2 - bw // 2, (y0 + y1) // 2 - bh // 2
    new = {p: 0 for p in pts}
    for y in range(bh):
        for x in range(bw):
            if bm[y][x] and use_outline:
                for dy in (-1, 0, 1):
                    for dx in (-1, 0, 1):
                        if (ox + x + dx, oy + y + dy) in new:
                            new[(ox + x + dx, oy + y + dy)] = out_c
    for y in range(bh):
        for x in range(bw):
            if bm[y][x] and (ox + x, oy + y) in new:
                new[(ox + x, oy + y)] = ink_c
    for p, a in pts.items():
        v = new[p]
        for off, sh in [a] + alias.get(p, []):
            if bpp == 4:
                raw[off] = (raw[off] & ~(15 << sh) & 0xff) | (v << sh)
            else:
                raw[off] = v
    return []


def mode_of(cell):
    """'label' (text on an opaque ground) or 'glyph' (a free-standing word on transparency)"""
    opaque = sum(1 for r in cell.px for v in r if v)
    return 'label' if opaque >= 0.7 * cell.w * cell.h else 'glyph'


def relabel(cell, en, rot, content, bold=False, colours=None, mode=None, box=None, ink=None, ground=None, outline=None):
    """the cell's Japanese replaced by `en`; returns problems (empty when fine). `colours`: the cell's 16 (or 256)
    palette colours, when known, to pick the stroke colour by contrast; `mode` label / glyph (default: from the
    cell's coverage)"""
    if mode == 'fill':
        return relabel_fill(cell, en, rot, content, colours, box, ink, ground, outline)
    if mode != 'label' and (box or (mode or mode_of(cell)) == 'glyph'):
        return relabel_glyph(cell, en, rot, content, colours, box)
    img = cell.px
    bg, text, ink = find_text(img, colours)
    if box:  # a label inside a box: everything in it that is not the box's ground is text
        from collections import Counter
        cnt = Counter(img[y][x] for y in range(cell.h) for x in range(cell.w) if img[y][x] and in_box(box, x, y))
        bg = cnt.most_common(1)[0][0]
        text = [(x, y) for y in range(cell.h) for x in range(cell.w) if img[y][x] and img[y][x] != bg and in_box(box, x, y)]
        tc = Counter(img[y][x] for x, y in text)
        ink = tc.most_common(1)[0][0] if tc else None
        if text and colours:
            lum = lambda v: luma(colours[v]) if v < len(colours) else 0
            ink = max([v for v in tc if tc[v] * 8 >= len(text)], key=lambda v: abs(lum(v) - lum(bg)))
    if not text:
        return ['no text found']
    xs = [x for x, y in text]; ys = [y for x, y in text]
    bx0, bx1, by0, by1 = min(xs), max(xs), min(ys), max(ys)
    for x, y in text:
        img[y][x] = bg
    # room: the background's extent around the text (along the reading direction), less a 2 px margin
    # room across: the ground's extent across the reading direction, at the text's middle
    if rot in (90, 270):
        col = [y for y in range(cell.h) if any(img[y][x] == bg for x in range(bx0, bx1 + 1))]
        length = (max(col) - min(col) + 1 - 4) if col else (by1 - by0 + 1)
        mid = (by0 + by1) // 2
        across = [x for x in range(cell.w) if img[mid][x] == bg]
        thick = max(bx1 - bx0 + 1, (max(across) - min(across) + 1 - 2) if across else 0)
    else:
        row = [x for x in range(cell.w) if any(img[y][x] == bg for y in range(by0, by1 + 1))]
        length = (max(row) - min(row) + 1 - 4) if row else (bx1 - bx0 + 1)
        mid = (bx0 + bx1) // 2
        across = [y for y in range(cell.h) if img[y][mid] == bg]
        thick = max(by1 - by0 + 1, (max(across) - min(across) + 1 - 2) if across else 0)
    for size in (12, 10):
        bm = render(en, font(content, size))
        if bold:
            bm = embolden(bm)
        if len(bm[0]) <= length and len(bm) <= thick + 2:
            break
    else:
        return ['"%s" is %d x %d px, the label has %d x %d' % (en, len(bm[0]), len(bm), length, thick)]
    bm = rotate(bm, rot)
    bh, bw = len(bm), len(bm[0])
    cx, cy = (bx0 + bx1 + 1) // 2, (by0 + by1 + 1) // 2
    ox, oy = cx - bw // 2, cy - bh // 2
    for y in range(bh):
        for x in range(bw):
            if bm[y][x]:
                X, Y = ox + x, oy + y
                if 0 <= X < cell.w and 0 <= Y < cell.h and img[Y][X]:
                    img[Y][X] = ink
    return []


# ---- tables ---------------------------------------------------------------------------------------------------

def guess_palette(arc, g):
    """the NCLR an NCGR most likely goes with: one in its own group (after it, before the next NCGR), else the
    latest one before it, else the next one"""
    kind = lambda b: b[:4][::-1].decode('latin1') if len(b) >= 4 else ''
    for i in range(g + 1, len(arc.entries)):
        k = kind(arc.entries[i])
        if k == 'NCGR':
            break
        if k == 'NCLR':
            return i
    before = [i for i in range(g) if kind(arc.entries[i]) == 'NCLR']
    after = [i for i in range(g + 1, len(arc.entries)) if kind(arc.entries[i]) == 'NCLR']
    return before[-1] if before else (after[0] if after else None)


def cell_colours(arc, t, it):
    """the colours of the item's palette row (the item's / table's "palette" NCLR entry, else guessed, and the
    cell's OAM palette)"""
    pe = it.get('palette', t.get('palette', guess_palette(arc, it['ncgr'])))
    if pe is None:
        return None
    pal = N.parse_nclr(arc.entries[pe])
    oam = N.oam_decode(*N.parse_ncer(arc.entries[it['ncer']])['cells'][it['cell']]['oams'][0])
    four = N.parse_ncgr(arc.entries[it['ncgr']])['bpp'] == 4  # the character data decides, not the palette's own flag
    return pal['colors'][oam['pal'] * 16:oam['pal'] * 16 + 16] if four else pal['colors']


def check(content, tables):
    """every item of the tables relabelled in memory: what does not fit or has no text"""
    problems = []
    for table in tables:
        t = json.load(open(table))
        arc = Arc(open(os.path.join(content, 'fs', t['archive']), 'rb').read())
        for it in t['items']:
            if not it.get('en'):
                continue
            try:
                if 'nscr' in it:
                    bg = BgImage(arc.entries[it['ncgr']], arc.entries[it['nscr']], it['box'])
                    p = relabel(bg, it['en'], it.get('rot', 0), content, False, bg_colours(arc, t, it), it.get('mode'), it.get('inner'), it.get('ink'), it.get('ground'), it.get('outline'))
                    problems += ['%s %d/%d %s: %s' % (t['archive'], it['ncgr'], it['nscr'], it['box'], x) for x in p]
                    continue
                cell = CellImage(arc.entries[it['ncgr']], move_objs(arc.entries[it['ncer']], it['cell'], it.get('move')), it['cell'])
                if it.get('objs') is not None:
                    p = relabel_objs(cell, it['en'], it.get('rot', 0), content, it['objs'], bytearray(cell.ch['raw']), it.get('outline'), it.get('ink'))
                else:
                  p = relabel(cell, it['en'], it.get('rot', 0), content, it.get('bold', t.get('bold', False)), cell_colours(arc, t, it), it.get('mode'), it.get('box'), it.get('ink'), it.get('ground'), it.get('outline'))
            except Exception as e:
                p = ['error: %s' % e]
            if 'nscr' not in it:
                problems += ['%s %d/%d/%d %s: %s' % (t['archive'], it['ncgr'], it['ncer'], it['cell'], it.get('ja', ''), x) for x in p]
    for p in problems:
        print(p)
    print('%d problems in %d tables' % (len(problems), len(tables)))
    return problems


def bg_colours(arc, t, it):
    """a BG box's colours, indexed by palette row << 4 | index (the BgImage values)"""
    pe = it.get('palette', t.get('palette', guess_palette(arc, it['ncgr'])))
    return N.parse_nclr(arc.entries[pe])['colors'] if pe is not None else None


def apply_table(content, table, patch_root):
    t = json.load(open(table))
    arc = Arc(open(os.path.join(content, 'fs', t['archive']), 'rb').read())
    problems = []
    raws, cers = {}, {}
    scrs = {}
    memos, written = {}, {}  # per NCGR: (block, size, English) -> copy; (NCGR, English) -> bytes written
    for it in t['items']:
        if not it.get('en') or 'nscr' not in it:
            continue
        g, sc = it['ncgr'], it['nscr']
        ncgr = replace_char(arc.entries[g], bytes(raws[g])) if g in raws else arc.entries[g]
        nscr = scrs.get(sc, arc.entries[sc])
        bg = BgImage(ncgr, nscr, it['box'])
        p = relabel(bg, it['en'], it.get('rot', 0), content, False, bg_colours(arc, t, it), it.get('mode'), it.get('inner'), it.get('ink'), it.get('ground'), it.get('outline'))
        if p:
            problems += ['%s %d/%d %s: %s' % (t['archive'], g, sc, it['box'], x) for x in p]
            continue
        raw, scr = bg.write()
        raws[g] = bytearray(raw)
        scrs[sc] = replace_screen(nscr, scr)
    for it in t['items']:
        if not it.get('en') or 'nscr' in it:
            continue
        g, c = it['ncgr'], it['ncer']
        if g not in raws:
            raws[g] = bytearray(N.parse_ncgr(arc.entries[g])['raw'])
        ncgr = arc.entries[g]
        bpp = N.parse_ncgr(ncgr)['bpp']
        if it.get('objs') is not None:
            trial_raw = bytearray(raws[g])
            cer0 = move_objs(cers.get(c, arc.entries[c]), it['cell'], it.get('move'))
            listed = set() if it.get('private') else {i['cell'] for i in t['items'] if i.get('ncer') == c and i.get('en') == it['en'] and not i.get('private')}
            trial_cer = unshare(trial_raw, cer0, it['cell'], bpp, listed, set(it['objs']), memos.setdefault(g, {}), ('private', c, it['cell']) if it.get('private') else it['en'], it.get('own_tiles', False))
            cell = CellImage(replace_char(ncgr, bytes(trial_raw)), trial_cer, it['cell'])
            p = relabel_objs(cell, it['en'], it.get('rot', 0), content, it['objs'], trial_raw, it.get('outline'), it.get('ink'))
            if p:
                problems += ['%s %d/%d/%d: %s' % (t['archive'], g, c, it['cell'], x) for x in p]
            else:
                raws[g], cers[c] = trial_raw, trial_cer
            continue
        args = (content, it.get('bold', t.get('bold', False)), cell_colours(arc, t, it), it.get('mode'), it.get('box'), it.get('ink'), it.get('ground'), it.get('outline'))
        cer0 = cers.get(c, arc.entries[c])
        probe = CellImage(replace_char(ncgr, bytes(raws[g])), cer0, it['cell'])
        before = [r[:] for r in probe.px]
        if relabel(probe, it['en'], it.get('rot', 0), args[0], *args[1:]):
            changed = set()
        else:  # the objects holding pixels the English changes
            changed = {probe.owner[y][x] for y in range(probe.h) for x in range(probe.w) if probe.px[y][x] != before[y][x]}
        trial_raw = bytearray(raws[g])
        listed = set() if it.get('private') else {i['cell'] for i in t['items'] if i.get('ncer') == c and i.get('en') == it['en'] and not i.get('private')}
        trial_cer = unshare(trial_raw, cer0, it['cell'], bpp, listed, changed, memos.setdefault(g, {}), ('private', c, it['cell']) if it.get('private') else it['en'], it.get('own_tiles', False))
        cell = CellImage(replace_char(ncgr, bytes(trial_raw)), trial_cer, it['cell'])
        p = relabel(cell, it['en'], it.get('rot', 0), content, it.get('bold', t.get('bold', False)), cell_colours(arc, t, it), it.get('mode'), it.get('box'), it.get('ink'), it.get('ground'), it.get('outline'))
        if p:
            problems += ['%s %d/%d/%d: %s' % (t['archive'], g, c, it['cell'], x) for x in p]
            continue
        cell.write(trial_raw)
        raws[g], cers[c] = trial_raw, trial_cer
    for g, raw in raws.items():
        arc.entries[g] = replace_char(arc.entries[g], bytes(raw))
        arc.changed.add(g)
    for c, b in cers.items():
        arc.entries[c] = b
        arc.changed.add(c)
    for sc, b in scrs.items():
        arc.entries[sc] = b
        arc.changed.add(sc)
    out = os.path.join(patch_root, t['archive'])
    os.makedirs(os.path.dirname(out), exist_ok=True)
    open(out, 'wb').write(arc.build())
    return problems


def apply(content, table_dir):
    patch_root = os.path.join(os.path.dirname(os.path.abspath(content)), 'patch/fs')
    problems = []
    n = 0
    for f in sorted(os.listdir(table_dir)):
        if f.endswith('.json'):
            problems += apply_table(content, os.path.join(table_dir, f), patch_root)
            n += 1
    for p in problems:
        print(p)
    print('%d tables, %d problems -> %s' % (n, len(problems), patch_root))
    # the LovePlus mode buttons are textures, not cells: their own tool (tools/lp_lpm_buttons.py)
    import lp_lpm_buttons
    lp_lpm_buttons.main(content)
    return problems


def preview(content, table, png, pal_entry=None):
    """the table's items before and after, side by side (palette: the table's "palette" entry or a grey ramp)"""
    from PIL import Image
    t = json.load(open(table))
    arc = Arc(open(os.path.join(content, 'fs', t['archive']), 'rb').read())
    pairs = []
    from PIL import Image as _I

    def bgimg(b, cols):
        im = _I.new('RGBA', (b.w, b.h))
        for y in range(b.h):
            for x in range(b.w):
                v = b.px[y][x]
                if v and cols and v < len(cols):
                    im.putpixel((x, y), tuple(cols[v]) + (255,))
        return im
    for it in [i for i in t['items'] if 'nscr' in i]:
        cols = bg_colours(arc, t, it)
        a = BgImage(arc.entries[it['ncgr']], arc.entries[it['nscr']], it['box'])
        b = BgImage(arc.entries[it['ncgr']], arc.entries[it['nscr']], it['box'])
        p = relabel(b, it['en'], it.get('rot', 0), content, False, cols, it.get('mode'), it.get('inner'), it.get('ink'), it.get('ground'), it.get('outline')) if it.get('en') else ['-']
        pairs.append((bgimg(a, cols), bgimg(b, cols), p))
    for it in [i for i in t['items'] if 'nscr' not in i]:
        pe = it.get('palette', t.get('palette', guess_palette(arc, it['ncgr'])))
        pal = N.parse_nclr(arc.entries[pe]) if pe is not None else None
        ncgr, ncer = arc.entries[it['ncgr']], arc.entries[it['ncer']]
        a = CellImage(ncgr, ncer, it['cell'])
        b = CellImage(ncgr, move_objs(ncer, it['cell'], it.get('move')), it['cell'])
        if it.get('objs') is not None and it.get('en'):
            r = bytearray(b.ch['raw'])
            p = relabel_objs(b, it['en'], it.get('rot', 0), content, it['objs'], r, it.get('outline'), it.get('ink'))
            b = CellImage(replace_char(ncgr, bytes(r)), move_objs(ncer, it['cell'], it.get('move')), it['cell'])
        else:
          p = relabel(b, it.get('en', ''), it.get('rot', 0), content, it.get('bold', t.get('bold', False)), cell_colours(arc, t, it), it.get('mode'), it.get('box'), it.get('ink'), it.get('ground'), it.get('outline')) if it.get('en') else ['-']
        pl = N.oam_decode(*N.parse_ncer(ncer)['cells'][it['cell']]['oams'][0])['pal']

        def img(c):
            im = Image.new('RGBA', (c.w, c.h))
            for y in range(c.h):
                for x in range(c.w):
                    v = c.px[y][x]
                    if v:
                        col = pal['colors'][pl * 16 + v] if pal else (v * 17,) * 3
                        im.putpixel((x, y), tuple(col) + (255,))
            return im
        pairs.append((img(a), img(b), p))
    W = sum(max(a.width, b.width) + 6 for a, b, _ in pairs)
    H = max(a.height + b.height for a, b, _ in pairs) + 6
    out = Image.new('RGBA', (W, H), (60, 60, 80, 255))
    x = 0
    for a, b, p in pairs:
        out.alpha_composite(a, (x, 0)); out.alpha_composite(b, (x, a.height + 6))
        x += max(a.width, b.width) + 6
    out.save(png)
    for (_, _, p), it in zip(pairs, [i for i in t['items'] if 'nscr' in i] + [i for i in t['items'] if 'nscr' not in i]):
        if p and p != ['-']:
            print(it.get('ja'), p)


def show(content, archive, ncgr, ncer, cell, png, scale=4):
    """one cell at `scale`x on a grid: a line every 8 px, labelled with its coordinate, to read boxes from"""
    from PIL import Image, ImageDraw
    arc = Arc(open(os.path.join(content, 'fs', archive), 'rb').read())
    c = CellImage(arc.entries[ncgr], arc.entries[ncer], cell)
    pe = guess_palette(arc, ncgr)
    cols = cell_colours(arc, {'palette': pe} if pe is not None else {}, {'ncgr': ncgr, 'ncer': ncer, 'cell': cell})
    m = 24
    im = Image.new('RGB', (c.w * scale + m, c.h * scale + m), (255, 0, 255))
    d = ImageDraw.Draw(im)
    for y in range(c.h):
        for x in range(c.w):
            v = c.px[y][x]
            col = tuple(cols[v]) if cols and v and v < len(cols) else ((v * 17,) * 3 if v else (255, 0, 255))
            d.rectangle([m + x * scale, m + y * scale, m + x * scale + scale - 1, m + y * scale + scale - 1], fill=col)
    for k, ox, oy, ow, oh, opl in c.objs:
        d.rectangle([m + ox * scale, m + oy * scale, m + (ox + ow) * scale - 1, m + (oy + oh) * scale - 1], outline=(255, 128, 0))
        d.text((m + ox * scale + 2, m + oy * scale + 2), 'obj%d' % k, fill=(255, 128, 0))
    for x in range(0, c.w + 1, 8):
        d.line([m + x * scale, m, m + x * scale, im.height], fill=(0, 200, 255))
        d.text((m + x * scale + 1, 2), str(x), fill=(0, 0, 0))
    for y in range(0, c.h + 1, 8):
        d.line([m, m + y * scale, im.width, m + y * scale], fill=(0, 200, 255))
        d.text((1, m + y * scale + 1), str(y), fill=(0, 0, 0))
    im.save(png)
    from collections import Counter
    cnt = Counter(v for r in c.px for v in r)
    print('%dx%d px (magenta = transparent); indices used: %s' % (c.w, c.h, ', '.join('%d%s x%d' % (v, ' %s' % (tuple(cols[v]),) if cols and v < len(cols) else '', n) for v, n in sorted(cnt.items()))))


def show_bg(content, archive, ncgr, nscr, png, palette=None, scale=2):
    """a whole BG screen at `scale`x on an 8 px grid with coordinates, to read boxes (screen pixels) from"""
    from PIL import Image, ImageDraw
    arc = Arc(open(os.path.join(content, 'fs', archive), 'rb').read())
    sc = N.parse_nscr(arc.entries[nscr])
    pe = palette if palette is not None else guess_palette(arc, ncgr)
    pal = N.parse_nclr(arc.entries[pe]) if pe is not None else None
    img = N.nscr_image(sc, N.parse_ncgr(arc.entries[ncgr]), pal).convert('RGB')
    m = 24
    im = Image.new('RGB', (img.width * scale + m, img.height * scale + m), (255, 255, 255))
    im.paste(img.resize((img.width * scale, img.height * scale), Image.NEAREST), (m, m))
    d = ImageDraw.Draw(im)
    for x in range(0, img.width + 1, 8):
        d.line([m + x * scale, m, m + x * scale, im.height], fill=(0, 200, 255) if x % 32 else (255, 0, 0))
        if x % 16 == 0:
            d.text((m + x * scale + 1, 2), str(x), fill=(0, 0, 0))
    for y in range(0, img.height + 1, 8):
        d.line([m, m + y * scale, im.width, m + y * scale], fill=(0, 200, 255) if y % 32 else (255, 0, 0))
        if y % 16 == 0:
            d.text((1, m + y * scale + 1), str(y), fill=(0, 0, 0))
    im.save(png)
    print('%dx%d px screen, palette entry %s' % (img.width, img.height, pe))


if __name__ == '__main__':
    cmd = sys.argv[1] if len(sys.argv) > 1 else ''
    if cmd == 'apply':
        sys.exit(1 if apply(sys.argv[2], sys.argv[3]) else 0)
    elif cmd == 'show-bg':
        show_bg(sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]), sys.argv[6])
    elif cmd == 'show':
        show(sys.argv[2], sys.argv[3], int(sys.argv[4]), int(sys.argv[5]), int(sys.argv[6]), sys.argv[7])
    elif cmd == 'check':
        sys.exit(1 if check(sys.argv[2], sys.argv[3:]) else 0)
    elif cmd == 'preview':
        preview(sys.argv[2], sys.argv[3], sys.argv[4])
    else:
        sys.exit(__doc__)
