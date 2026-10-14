"""LovePlus+ save file (2 Mbit backup memory) - reader and writer, derived from the game's code and proven on real saves.

Card (code: `Save_WriteBlock` 0x20f0fb8, slot offsets 0x20f087c, slot counts 0x20f0d10, generation index 0x20f0d64):
  six block kinds; kind k has `slots` blocks, each stored in two copies (ping-pong). A write goes to the other copy with
  generation + 1 (0 wraps to 1). Block at the card offset below = 0x20-byte header {u32 magic 0x10190573, u32 stored size,
  u16 generation, u16 version 3, 20-byte SHA-1 of the stored bytes} + the stored bytes = `MI_CompressLZ` (LZ10) of the
  packed block, or the packed block itself when compression does not shrink it.
Packed block (overlay 0 `Ov00Init_PackSave<K>`): sections of entries laid out back to back from offset 0; entry e of
  section s holds `count` items of `size` bytes ({u16 size, u16 count} at *(desc + 4 s) + 4 e); the number of entries of
  section s is the word at counts + 4 s. Kind B pads its total to 4 bytes. Section 0 entry 0 is the block's own size... see
  layout() for the exact numbers. The meaning of each entry (which game struct field it carries) is in savemap.json.
"""
import hashlib, os, struct

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OV0_BASE = 0x2158280
MAGIC = 0x10190573

# kind: (descriptor list, entry-count table, sections, pad to 4, card offset(slot, copy), slots)
KINDS = {
    'A': (0x2165e80, 0x2164f00, 10, False, lambda i, c: (i + c) * 0xa20, 1),
    'B': (0x2165ea8, 0x21657e0, 73, True, lambda i, c: 0x1440 + (i + 3 * c) * 0x4020, 3),
    'C': (0x2165fcc, 0x21659d0, 16, False, lambda i, c: 0x19500 + (i + c) * 0x120, 1),
    'D': (0x2166a2c, 0x2165cc0, 2, False, lambda i, c: 0x19740 + (i + c) * 0x2020, 1),
    'E': (0x2166a34, 0x2165d50, 2, False, lambda i, c: 0x1d780 + (i + 100 * c) * 0x120, 100),
    'F': (0x2166a3c, 0x2165e10, 2, False, lambda i, c: 0x2b880 + (i + 3 * c) * 0x1520, 3),
}
TAGS = {'A': b'LDSD', 'B': b'LDGD', 'C': b'LDOD'}

_OV0 = None


def ov0():
    global _OV0
    if _OV0 is None:
        _OV0 = open(os.path.join(ROOT, 'content/engine/ov00.bin'), 'rb').read()
    return _OV0


def _w(a):
    return struct.unpack_from('<I', ov0(), a - OV0_BASE)[0]


def _h(a):
    return struct.unpack_from('<H', ov0(), a - OV0_BASE)[0]


def layout(kind):
    """[[(offset, size, count) per entry] per section] and the total size"""
    desc, counts, nsec, pad, _, _ = KINDS[kind]
    off = 0
    out = []
    for s in range(nsec):
        d, n = _w(desc + 4 * s), _w(counts + 4 * s)
        ents = []
        for e in range(n):
            size, cnt = _h(d + 4 * e), _h(d + 4 * e + 2)
            ents.append((off, size, cnt))
            off += size * cnt
        out.append(ents)
    total = (off + 3) & ~3 if pad else off
    return out, total


def parse(kind, data):
    """packed block -> [[bytes per entry] per section]"""
    lay, total = layout(kind)
    assert len(data) == total, '%s: %d bytes, layout says %d' % (kind, len(data), total)
    return [[data[o:o + s * c] for o, s, c in sec] for sec in lay]


def serialize(kind, sections):
    lay, total = layout(kind)
    out = bytearray(total)
    for sec, vals in zip(lay, sections):
        for (o, s, c), v in zip(sec, vals):
            assert len(v) == s * c
            out[o:o + s * c] = v
    return bytes(out)


# ------------------------------------------------------------------ LZ10 (NitroSDK MI_UncompressLZ8 / MI_CompressLZ)
def lz10_decompress(d):
    n = d[1] | d[2] << 8 | d[3] << 16
    out = bytearray(); p = 4
    while len(out) < n:
        flags = d[p]; p += 1
        for b in range(8):
            if len(out) >= n:
                break
            if flags & (0x80 >> b):
                x = d[p] << 8 | d[p + 1]; p += 2
                for _ in range((x >> 12) + 3):
                    out.append(out[-((x & 0xfff) + 1)])
            else:
                out.append(d[p]); p += 1
    return bytes(out)


def lz10_compress(src):
    """`MI_CompressLZImpl` 0x201f20c with `SearchLZ` 0x201f3e0 (byte-identical on every block of the recorded saves);
    returns None when the result would not be smaller than the input (the game then stores the raw block). The game pads
    its buffer to 4 bytes but returns (and stores) the unpadded length."""
    n = len(src)
    out = bytearray([0x10, n & 0xff, (n >> 8) & 0xff, (n >> 16) & 0xff])
    i = 0
    while i < n:
        fpos = len(out); out.append(0); flags = 0
        for b in range(8):
            if i >= n:
                break
            best_len, best_off = 0, 0
            lo = max(0, i - 0x1000)
            maxlen = min(18, n - i)
            for j in range(lo, i - 1):                   # `SearchLZ` 0x201f3e0: farthest first, distance >= 2, only a
                                                         # strictly longer match replaces the best
                if src[j] != src[i]:
                    continue
                l = 1
                while l < maxlen and src[j + l] == src[i + l]:
                    l += 1
                if l > best_len:
                    best_len, best_off = l, i - j
                    if l == maxlen:
                        break
            if best_len >= 3:
                flags |= 0x80 >> b
                x = ((best_len - 3) << 12) | (best_off - 1)
                out += bytes([x >> 8, x & 0xff]); i += best_len
            else:
                out.append(src[i]); i += 1
        out[fpos] = flags
        if len(out) >= n:
            return None
    return bytes(out)


# ------------------------------------------------------------------ card
class Card:
    def __init__(self, image):
        self.image = bytearray(image)

    @classmethod
    def open(cls, path):
        return cls(open(path, 'rb').read())

    def header(self, off):
        magic, size, gen, ver = struct.unpack_from('<IIHH', self.image, off)
        return (size, gen, ver, bytes(self.image[off + 12:off + 32])) if magic == MAGIC else None

    def copies(self, kind, slot=0):
        """[(copy, card offset, generation)] of the valid copies of a block"""
        out = []
        for c in (0, 1):
            off = KINDS[kind][4](slot, c)
            h = self.header(off)
            if h and 0 < h[0] <= 0x40000 and hashlib.sha1(self.image[off + 0x20:off + 0x20 + h[0]]).digest() == h[3]:
                out.append((c, off, h[1]))
        return out

    def current(self, kind, slot=0):
        """(copy, offset, generation) of the newest valid copy (generations compare modulo 2^16)"""
        cs = self.copies(kind, slot)
        if not cs:
            return None
        best = cs[0]
        for c in cs[1:]:
            if ((c[2] - best[2]) & 0xffff) < 0x8000 and c[2] != best[2]:
                best = c
        return best

    def read(self, kind, slot=0):
        """the packed block (decompressed), or None"""
        cur = self.current(kind, slot)
        if cur is None:
            return None
        size = self.header(cur[1])[0]
        body = bytes(self.image[cur[1] + 0x20:cur[1] + 0x20 + size])
        return lz10_decompress(body) if body[:1] == b'\x10' and lz10_decompress(body) is not None else body

    def encode_block(self, packed, gen):
        stored = lz10_compress(packed)
        if stored is None:
            stored = packed
        return struct.pack('<IIHH', MAGIC, len(stored), gen, 3) + hashlib.sha1(stored).digest() + stored

    def write(self, kind, packed, slot=0):
        """store a packed block as `Save_WriteBlock` 0x20f0fb8 does: the game keeps {copy, generation} of every block
        (bytes at 0x2157fb0, words at 0x2158020, indexed per kind / slot) from the load; a write goes to copy (copy + 1) % 2
        with generation + 1 (0 -> 1). A block with no valid copy starts at {0, 0}, so its first write lands in copy 1
        (as on every recorded card)."""
        cur = self.current(kind, slot)
        copy, gen = (0, 0) if cur is None else (cur[0], cur[2])
        copy = (copy + 1) % 2
        gen = (gen + 1) & 0xffff or 1
        blk = self.encode_block(packed, gen)
        off = KINDS[kind][4](slot, copy)
        self.image[off:off + len(blk)] = blk
        return off, gen


# ------------------------------------------------------------------ struct images (bitmaps.json, derive_maps.py)
# kind -> the game struct a block is packed from / unpacked into:
#   A settings record (0x1f8, *(0x21544e8 + 4))       B GameState (0x3c58 at 0x2142c8c; heroine records at +0x15c4)
#   C header block (0xf4, *(0x21544e8 + 8))           D event state (0x1002, Schedule_GetEventStatePtr)
#   E schedule aux state (0xc4, 100 card slots)      F slot record (0x14c8, SaveData slot table entry, 3 card slots)
_MAPS = None


def _maps():
    global _MAPS
    if _MAPS is None:
        import json
        _MAPS = json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'bitmaps.json')))
    return _MAPS


def _apply(runs, src, const, n, bools=()):
    s = int.from_bytes(src, 'little')
    o = int.from_bytes(const, 'little')
    for t, i, k in runs:
        mask = (1 << k) - 1
        o = (o & ~(mask << t)) | (((s >> i) & mask) << t)
    out = bytearray(o.to_bytes(n, 'little'))
    for so, po, k in bools:
        for j in range(k):
            out[so + j] = 1 if src[po + j] else 0
    return bytes(out)


def unpack(kind, packed):
    """packed block -> the game struct image (`Ov00Init_UnpackSave<K>`); bit-exact with the game's unpacker"""
    m = _maps()[kind]
    assert len(packed) == m['packed_size']
    return _apply(m['unpack'], packed, bytes.fromhex(m['unpack_const']), m['struct_size'], m['unpack_bools'])


def pack(kind, image):
    """game struct image -> packed block (`Ov00Init_PackSave<K>`); bit-exact with the game's packer on every image the
    game can hold (booleans 0 / 1, bitfields within their width)"""
    m = _maps()[kind]
    assert len(image) == m['struct_size']
    return _apply(m['pack'], image, bytes.fromhex(m['pack_const']), m['packed_size'])
