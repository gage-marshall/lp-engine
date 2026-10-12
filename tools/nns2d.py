"""NitroSystem G2D format converters (NCLR, NCGR, NSCR, NCER, NANR) -> PNG / JSON.
Written from the on-disk struct layouts of the NNS G2D binary formats (read-only reference); no vendor code is included.

File = header{sig u32, byteOrder u16 (0xFEFF), version u16, fileSize u32, headerSize u16, nBlocks u16}, then blocks
{kind u32, size u32, data...}. Stored signatures are byte-reversed ('RLCN' = NCLR). Offsets inside a block struct are relative
to the start of that struct (the byte after the 8-byte block header).
"""
import struct
from PIL import Image

U16 = struct.Struct('<H'); U32 = struct.Struct('<I')


def blocks(d):
    sig, bo, ver, fsz, hsz, nb = struct.unpack_from('<IHHIHH', d, 0)
    out = {}; p = hsz
    for _ in range(nb):
        kind, size = struct.unpack_from('<II', d, p)
        out[d[p:p + 4][::-1].decode('latin1')] = (p + 8, size - 8)
        p += size
    return d[:4][::-1].decode('latin1'), out


def rgb555(c):
    return ((c & 31) * 255 // 31, ((c >> 5) & 31) * 255 // 31, ((c >> 10) & 31) * 255 // 31)


# ---------------------------------------------------------------- NCLR
def parse_nclr(d):
    sig, b = blocks(d); o, n = b['PLTT']
    fmt, ext, sz, roff = struct.unpack_from('<4I', d, o)
    raw = d[o + roff:o + roff + sz]
    cols = [rgb555(c) for c in struct.unpack('<%dH' % (len(raw) // 2), raw)]
    return {'fmt': fmt, 'colors': cols, 'bpp': 4 if fmt == 3 else 8}


# ---------------------------------------------------------------- NCGR
def parse_ncgr(d):
    sig, b = blocks(d); o, n = b['CHAR']
    H, W, pfmt, mapping, cfmt, sz, roff = struct.unpack_from('<2H5I', d, o)
    raw = d[o + roff:o + roff + sz]
    return {'tiles_h': H, 'tiles_w': W, 'bpp': 4 if pfmt == 3 else 8, 'mapping': mapping, 'bitmap': (cfmt & 0xff) == 1, 'raw': raw}


def tile_pixels(ch, idx, unit=None):
    """8x8 indices of tile idx (tile unit = 32 bytes for 4bpp, 64 for 8bpp unless unit given)"""
    bs = 32 if ch['bpp'] == 4 else 64
    off = idx * (unit or bs)
    t = ch['raw'][off:off + bs]
    if len(t) < bs: return None
    if ch['bpp'] == 8: return list(t)
    px = []
    for byte in t: px += [byte & 15, byte >> 4]
    return px


class Canvas:
    def __init__(self, w, h): self.w, self.h = w, h; self.buf = bytearray(w * h * 4)
    def put(self, x, y, rgb):
        if 0 <= x < self.w and 0 <= y < self.h:
            o = (y * self.w + x) * 4; self.buf[o:o + 4] = bytes(rgb) + b'\xff'
    def image(self): return Image.frombytes('RGBA', (self.w, self.h), bytes(self.buf))


def _pal_lookup(pal, bpp, pal_index):
    cols = pal['colors'] if pal else None
    base = (pal_index * 16) if bpp == 4 else 0
    def f(v):
        if cols is None: return (v * 17,) * 3 if bpp == 4 else (v,) * 3
        i = base + v
        return cols[i] if i < len(cols) else (255, 0, 255)
    return f


def ncgr_image(ch, pal=None, width_tiles=None, pal_index=0):
    bs = 32 if ch['bpp'] == 4 else 64; n = len(ch['raw']) // bs
    W = ch['tiles_w'] if ch['tiles_w'] not in (0, 0xffff) else (width_tiles or min(32, max(1, n)))
    H = ch['tiles_h'] if ch['tiles_h'] not in (0, 0xffff) else (n + W - 1) // W
    cv = Canvas(W * 8, H * 8); col = _pal_lookup(pal, ch['bpp'], pal_index)
    for t in range(min(n, W * H)):
        px = tile_pixels(ch, t); tx, ty = (t % W) * 8, (t // W) * 8
        for y in range(8):
            for x in range(8):
                v = px[y * 8 + x]
                if v or ch['bpp'] == 8 and pal is None: cv.put(tx + x, ty + y, col(v))
    return cv.image()


# ---------------------------------------------------------------- NSCR
def parse_nscr(d):
    sig, b = blocks(d); o, n = b['SCRN']
    w, h, cmode, sfmt, sz = struct.unpack_from('<4HI', d, o)
    raw = d[o + 12:o + 12 + sz]
    return {'w': w, 'h': h, 'color_mode': cmode, 'format': sfmt, 'raw': raw}


def nscr_image(sc, ch, pal):
    W, H = sc['w'] // 8, sc['h'] // 8; cv = Canvas(sc['w'], sc['h'])
    raw = sc['raw']
    if sc['format'] == 1: ents = list(raw)                           # affine: u8 tile index
    else: ents = list(struct.unpack('<%dH' % (len(raw) // 2), raw[:len(raw) // 2 * 2]))
    cache = {}
    for i, e in enumerate(ents[:W * H]):
        if sc['format'] == 0: tile, hf, vf, pl = e & 0x3ff, (e >> 10) & 1, (e >> 11) & 1, e >> 12
        elif sc['format'] == 1: tile, hf, vf, pl = e, 0, 0, 0
        else: tile, hf, vf, pl = e & 0x3ff, (e >> 10) & 1, (e >> 11) & 1, e >> 12
        px = tile_pixels(ch, tile)
        if px is None: continue
        key = pl if ch['bpp'] == 4 else 0
        col = cache.get(key) or cache.setdefault(key, _pal_lookup(pal, ch['bpp'], key))
        tx, ty = (i % W) * 8, (i // W) * 8
        for y in range(8):
            sy = 7 - y if vf else y
            for x in range(8):
                v = px[sy * 8 + (7 - x if hf else x)]
                if v == 0 and ch['bpp'] == 4: continue
                cv.put(tx + x, ty + y, col(v))
    return cv.image()


# ---------------------------------------------------------------- NCER
SHAPE = {(0, 0): (8, 8), (0, 1): (16, 16), (0, 2): (32, 32), (0, 3): (64, 64), (1, 0): (16, 8), (1, 1): (32, 8), (1, 2): (32, 16), (1, 3): (64, 32),
         (2, 0): (8, 16), (2, 1): (8, 32), (2, 2): (16, 32), (2, 3): (32, 64)}


def parse_ncer(d):
    sig, b = blocks(d); o, n = b['CEBK']
    ncell, attr, cells_off, mapping, vram_off, str_off, ext_off = struct.unpack_from('<2H5I', d, o)
    has_br = attr & 1; stride = 16 if has_br else 8
    cells = []
    for i in range(ncell):
        p = o + cells_off + i * stride
        noam, cattr, oam_off = struct.unpack_from('<HHI', d, p)
        br = struct.unpack_from('<4h', d, p + 8) if has_br else None
        # OAM attribute arrays are stored after the cell table; offsets are relative to the start of that area
        cells.append({'n': noam, 'attr': cattr, 'oam_off': oam_off, 'br': br})
    oam_base = o + cells_off + ncell * stride
    for c in cells:
        c['oams'] = [struct.unpack_from('<3H', d, oam_base + c['oam_off'] + 6 * k) for k in range(c['n'])]
    return {'mapping': mapping, 'has_br': bool(has_br), 'cells': cells}


def oam_decode(a0, a1, a2):
    y = a0 & 0xff; rot = (a0 >> 8) & 1; dbl = (a0 >> 9) & 1; color256 = (a0 >> 13) & 1; shape = a0 >> 14
    x = a1 & 0x1ff; x = x - 512 if x >= 256 else x; y = y - 256 if y >= 128 else y
    hf, vf = (a1 >> 12) & 1, (a1 >> 13) & 1; size = a1 >> 14
    return dict(x=x, y=y, rot=rot, hide=(rot == 0 and dbl == 1), c256=color256, w=SHAPE[(shape, size)][0], h=SHAPE[(shape, size)][1],
                hflip=hf, vflip=vf, tile=a2 & 0x3ff, prio=(a2 >> 10) & 3, pal=a2 >> 12)


def tile_at(ch, byte_off):
    bs = 32 if ch['bpp'] == 4 else 64
    t = ch['raw'][byte_off:byte_off + bs]
    if len(t) < bs: return None
    if ch['bpp'] == 8: return list(t)
    px = []
    for b in t: px += [b & 15, b >> 4]
    return px


def render_cell(cell, mapping, ch, pal):
    """Compose one cell (list of OAM objects) into an RGBA image; returns (image, (x0, y0)) or None."""
    items = [oam_decode(*o) for o in cell['oams']]
    if not items: return None
    x0 = min(i['x'] for i in items); y0 = min(i['y'] for i in items)
    x1 = max(i['x'] + i['w'] for i in items); y1 = max(i['y'] + i['h'] for i in items)
    cv = Canvas(max(1, x1 - x0), max(1, y1 - y0)); boundary = 32 << (mapping if mapping in (0, 1, 2, 3) else 0)
    bs = 32 if ch['bpp'] == 4 else 64
    for it in reversed(items):                      # lower OAM index is drawn on top
        if it['hide']: continue
        tw, th = it['w'] // 8, it['h'] // 8; col = _pal_lookup(pal, ch['bpp'], it['pal'])
        for ty in range(th):
            for tx in range(tw):
                px = tile_at(ch, it['tile'] * boundary + (ty * tw + tx) * bs)
                if px is None: continue
                for y in range(8):
                    for x in range(8):
                        v = px[y * 8 + x]
                        if v == 0: continue
                        xx = (7 - x if it['hflip'] else x) + ((tw - 1 - tx) if it['hflip'] else tx) * 8
                        yy = (7 - y if it['vflip'] else y) + ((th - 1 - ty) if it['vflip'] else ty) * 8
                        cv.put(it['x'] - x0 + xx, it['y'] - y0 + yy, col(v))
    return cv.image(), (x0, y0)


# ---------------------------------------------------------------- NANR
def parse_nanr(d):
    sig, b = blocks(d); o, n = b['ABNK']
    nseq, nframes, seq_off, frm_off, cont_off, str_off, ext_off = struct.unpack_from('<2H5I', d, o)
    seqs = []
    for i in range(nseq):
        nf, loop, atype, mode, fa = struct.unpack_from('<HHIII', d, o + seq_off + i * 16)
        elem = atype & 0xffff; kind = atype >> 16
        frames = []
        for k in range(nf):
            content, dur, _pad = struct.unpack_from('<IHH', d, o + frm_off + fa + k * 8)
            q = o + cont_off + content
            if elem == 0: fr = {'index': U16.unpack_from(d, q)[0]}
            elif elem == 1:
                idx, rot, sx, sy, px, py = struct.unpack_from('<HHiihh', d, q); fr = {'index': idx, 'rot': rot, 'sx': sx / 4096, 'sy': sy / 4096, 'x': px, 'y': py}
            else:
                idx, _p, px, py = struct.unpack_from('<HHhh', d, q); fr = {'index': idx, 'x': px, 'y': py}
            fr['frames'] = dur; frames.append(fr)
        seqs.append({'loop_start': loop, 'type': kind, 'element': elem, 'play_mode': mode, 'frames': frames})
    return {'sequences': seqs}
