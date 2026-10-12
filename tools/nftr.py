"""NNS G2D bitmap font (NFTR) reader/renderer. Written from the on-disk struct layouts in the NitroSystem font format (read-only reference).

File: 'RTFN' header (byte order mark, version, size, header size, block count) then blocks {sig (byte-reversed), size, body}:
FNIF (font info), PLGC (glyph bitmaps), HDWC (per-glyph widths, linked list), PAMC (character code maps, linked list).
Offsets stored in the blocks are file offsets of the *block body* of the target block (pGlyph -> body of PLGC, pWidth -> body of HDWC, pMap -> body of PAMC).
"""
import struct
from PIL import Image


class Font:
    def __init__(self, data):
        d = data; self.d = d
        sig, bo, ver, fsz, hsz, nb = struct.unpack_from('<4sHHIHH', d, 0)
        if sig != b'RTFN': raise ValueError('not an NFTR')
        self.version = ver; p = hsz; blocks = {}
        for _ in range(nb):
            k, sz = struct.unpack_from('<4sI', d, p); blocks.setdefault(k[::-1].decode(), []).append((p + 8, sz - 8)); p += sz
        o = blocks['FINF'][0][0]
        self.type, self.linefeed, self.alter, dl, dg, dc, self.encoding = struct.unpack_from('<BbHbBbB', d, o)
        self.default_width = (dl, dg, dc)
        pglyph, pwidth, pmap = struct.unpack_from('<III', d, o + 8)
        self.height, self.width, self.ascent = struct.unpack_from('<BBB', d, o + 20) if ver >= 0x102 else (0, 0, 0)
        g = pglyph; self.cell_w, self.cell_h, self.cell_size, self.baseline, self.max_char_w, self.bpp, self.flags = struct.unpack_from('<BBHbBBB', d, g)
        self.glyph_off = g + 8; n = (blocks['CGLP'][0][1] - 8) // self.cell_size; self.nglyphs = n
        self.rot = (self.flags >> 1) & 3; self.vertical = bool(self.flags & 1)
        self.widths = {}                                    # glyph index -> (left, glyph width, char width)
        w = pwidth
        while w:
            b, e, nxt = struct.unpack_from('<HHI', d, w)
            for i in range(e - b + 1): self.widths[b + i] = struct.unpack_from('<bBb', d, w + 8 + 3 * i)
            w = nxt
        self.cmap = {}                                      # character code -> glyph index
        m = pmap
        while m:
            cb, ce, meth, _r, nxt = struct.unpack_from('<HHHHI', d, m)
            if meth == 0:
                idx0 = struct.unpack_from('<H', d, m + 12)[0]
                for c in range(cb, ce + 1): self.cmap[c] = idx0 + c - cb
            elif meth == 1:
                for c in range(cb, ce + 1):
                    i = struct.unpack_from('<H', d, m + 12 + 2 * (c - cb))[0]
                    if i != 0xffff: self.cmap[c] = i
            else:
                cnt = struct.unpack_from('<H', d, m + 12)[0]
                for k in range(cnt):
                    c, i = struct.unpack_from('<HH', d, m + 14 + 4 * k); self.cmap[c] = i
            m = nxt

    def glyph_index(self, ch):
        c = ord(ch) if isinstance(ch, str) else ch
        return self.cmap.get(c, self.alter)

    def metrics(self, index):
        return self.widths.get(index, self.default_width)

    def glyph_bitmap(self, index):
        """-> (w, h, list of rows of pixel values 0..2^bpp-1) in display orientation"""
        o = self.glyph_off + index * self.cell_size; raw = self.d[o:o + self.cell_size]
        W, H, bpp = self.cell_w, self.cell_h, self.bpp; bits = int.from_bytes(raw, 'big'); total = len(raw) * 8; px = []
        for i in range(W * H):
            sh = total - (i + 1) * bpp; px.append((bits >> sh) & ((1 << bpp) - 1))
        rows = [px[y * W:(y + 1) * W] for y in range(H)]
        for _ in range(self.rot):                             # stored rotated clockwise by rot*90 degrees -> rotate back counter-clockwise
            rows = [list(r) for r in zip(*rows)][::-1]
        return len(rows[0]), len(rows), rows

    def render(self, text, scale=1, fg=(255, 255, 255), bg=(0, 0, 0, 0), width=None):
        """simple one-line/multi-line layout (newline = linefeed) -> RGBA image"""
        lines = text.split('\n'); lh = self.height + self.linefeed if self.linefeed else self.cell_h + 1
        adv = []
        for ln in lines:
            x = 0; seg = []
            for ch in ln:
                gi = self.glyph_index(ch); l, gw, cw = self.metrics(gi); seg.append((gi, x + l)); x += cw
            adv.append((seg, x))
        W = max([x for _, x in adv] + [1]); H = max(1, len(lines)) * lh
        img = Image.new('RGBA', (W, H), bg); px = img.load(); mx = (1 << self.bpp) - 1
        for li, (seg, _) in enumerate(adv):
            for gi, x0 in seg:
                w, h, rows = self.glyph_bitmap(gi)
                for y in range(h):
                    for x in range(w):
                        v = rows[y][x]
                        if v and 0 <= x0 + x < W and li * lh + y < H: px[x0 + x, li * lh + y] = fg + (int(255 * v / mx),)
        return img.resize((W * scale, H * scale), Image.NEAREST) if scale != 1 else img

    def atlas(self, cols=64, scale=1):
        n = self.nglyphs; rows = (n + cols - 1) // cols; cw, ch = self.cell_w + 1, self.cell_h + 1
        img = Image.new('L', (cols * cw, rows * ch), 0); px = img.load(); mx = (1 << self.bpp) - 1
        for i in range(n):
            w, h, r = self.glyph_bitmap(i)
            for y in range(h):
                for x in range(w):
                    if r[y][x]: px[(i % cols) * cw + x, (i // cols) * ch + y] = int(255 * r[y][x] / mx)
        return img.resize((img.width * scale, img.height * scale), Image.NEAREST) if scale != 1 else img


def export_fonts(assets_dir, out_dir):
    """every NFTR below assets_dir -> out_dir/<name>.json (metrics, character map ranges, per-glyph widths) + <name>.png (glyph atlas, 64 columns,
    display orientation, white on transparent). Glyph index i sits at column i % 64, row i // 64, cell (cell_w + 1) x (cell_h + 1)."""
    import os, json
    os.makedirs(out_dir, exist_ok=True); n = 0
    for root, _, files in os.walk(assets_dir):
        for fn in sorted(files):
            p = os.path.join(root, fn)
            try:
                with open(p, 'rb') as fh:
                    if fh.read(4) != b'RTFN': continue
                f = Font(open(p, 'rb').read())
            except Exception: continue
            name = os.path.splitext(fn)[0]
            codes = sorted(f.cmap); ranges = []; s = prev = None
            for c in codes:                                   # runs where code and glyph index both increase by 1
                gi = f.cmap[c]
                if s is not None and c == prev + 1 and gi == f.cmap[prev] + 1: prev = c; continue
                if s is not None: ranges.append([s, prev, f.cmap[s]])
                s = prev = c
            if s is not None: ranges.append([s, prev, f.cmap[s]])
            meta = {'file': fn, 'version': f.version, 'encoding': ['UTF-8', 'UTF-16', 'SJIS', 'CP1252'][f.encoding], 'height': f.height, 'width': f.width, 'ascent': f.ascent,
                    'linefeed': f.linefeed, 'alter_glyph': f.alter, 'default_width': list(f.default_width), 'cell': [f.cell_w, f.cell_h], 'baseline': f.baseline,
                    'max_char_width': f.max_char_w, 'bpp': f.bpp, 'stored_rotation_deg': f.rot * 90, 'glyphs': f.nglyphs, 'atlas_columns': 64,
                    'cmap_ranges': ranges, 'widths': [list(f.metrics(i)) for i in range(f.nglyphs)]}
            json.dump(meta, open(os.path.join(out_dir, name + '.json'), 'w'), separators=(',', ':'))
            a = f.atlas(64); rgba = Image.new('RGBA', a.size, (255, 255, 255, 0)); rgba.putalpha(a); rgba.save(os.path.join(out_dir, name + '.png')); n += 1
    return n
