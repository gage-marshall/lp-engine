"""The game's sprite system -> OAM (Sprite_DrawAll 0x20490e0 / Sprite_SubmitToOamManager / Sprite_BuildCellOams).

Sprite objects (pool of 0x100 x 0x38 bytes at the pointer in 0x213f624+0; per-frame priority lists 0x3f..0 at 0x213f6a0,
chained by +0x28):
  +0x00 current cell (NNSG2dCellData*: u16 nOAM, u16 attr, OAM attr array of 3 x u16), set by the animation tick
  +0x04 u16 tile base added to each OAM's tile (unless flags bit 0)   +0x08 / +0x0c x / y (pixel << 16)   +0x10 affine matrix
  +0x14 s8 palette offset   +0x18 1 main OAM, else sub   +0x19 list   +0x1a priority (OAM attr2 bits 10-11)
  +0x1b kind (1 raw OAM, 2 cell, 3 transformed cell)   +0x24 flags   +0x2c / +0x2e cell / animation bank handles
  +0x32 sequence   +0x34 animation time (12.12)
NNS_G2dMakeCellToOams: each cell OAM at (oam.x + x>>12, oam.y + y>>12), wrapped to 9 / 8 bits."""
import struct


class SprObj:
    def __init__(self, ram, addr, base=0x2000000):
        b = ram[addr - base:addr - base + 0x38]
        self.addr = addr
        (self.cell, self.tile_base, self.x, self.y, self.mtx) = (struct.unpack_from('<I', b, 0)[0], struct.unpack_from('<H', b, 4)[0],
                                                                 struct.unpack_from('<i', b, 8)[0], struct.unpack_from('<i', b, 0xc)[0],
                                                                 struct.unpack_from('<I', b, 0x10)[0])
        self.pal_ofs = struct.unpack_from('<b', b, 0x14)[0]
        self.engine, self.lst, self.prio, self.kind = b[0x18], b[0x19], b[0x1a], b[0x1b]
        self.flags = b[0x24]
        self.next = struct.unpack_from('<I', b, 0x28)[0]
        self.cell_bank, self.anim_bank = struct.unpack_from('<hh', b, 0x2c)
        self.seq, self.time = b[0x32], struct.unpack_from('<i', b, 0x34)[0]


def cell_oams(ram, cell_ptr, base=0x2000000):
    """[(a0, a1, a2)] of an NNSG2dCellData in RAM"""
    n = struct.unpack_from('<H', ram, cell_ptr - base)[0]
    arr = struct.unpack_from('<I', ram, cell_ptr - base + 4)[0]
    if not (base <= arr < base + 0x400000) or n > 128:
        return []
    return [struct.unpack_from('<3H', ram, arr - base + 6 * k) for k in range(n)]


def build(obj, oams, affine_idx=None):
    """OAM entries (a0, a1, a2) of one sprite object from its cell's OAM attributes. An object with a matrix (+0x10) is drawn
    affine: NNS_G2dEntryOamManagerAffine gives it the next affine slot, its OAMs get the rotate/scale bit and that slot."""
    out = []
    px, py = obj.x >> 16, obj.y >> 16          # positions are pixel << 16 (0x800000 = 128: verified against the hardware OAM)
    for a0, a1, a2 in oams:
        y = ((a0 & 0xff) + py) & 0xff
        x = ((a1 & 0x1ff) + px) & 0x1ff
        a0 = (a0 & ~0xff) | y
        a1 = (a1 & ~0x1ff) | x
        pal = ((a2 >> 12) + obj.pal_ofs) & 15
        a2 = (a2 & 0x0fff) | (pal << 12)
        if not obj.flags & 1:
            a2 = (a2 & ~0x3ff) | (((a2 & 0x3ff) + obj.tile_base) & 0x3ff)
        a2 = (a2 & ~0xc00) | ((obj.prio & 3) << 10)
        if affine_idx is not None:
            a0 |= 0x100
            a1 = (a1 & ~0x3e00) | ((affine_idx & 31) << 9)
        out.append((a0, a1, a2))
    return out


SIZE = {(0, 0): (8, 8), (0, 1): (16, 16), (0, 2): (32, 32), (0, 3): (64, 64), (1, 0): (16, 8), (1, 1): (32, 8), (1, 2): (32, 16),
        (1, 3): (64, 32), (2, 0): (8, 16), (2, 1): (8, 32), (2, 2): (16, 32), (2, 3): (32, 64)}


def build_transformed(obj, b, oams, affine_idx=None):
    """kind 3 (Sprite_BuildCellOamsTransformed 0x20488e0 + Sprite_CopyCellOamsWithFlip 0x204948c), non-affine part:
    flags bit 0 / 1 flip the cell horizontally / vertically (x' = -(x + w), y' = -(y + h), OAM flip bits toggled); tile base
    +0x04 added unless flags bit 0; priority +0x1a; OBJ mode +0x20: 0 keeps the cell's mode and adds the s8 palette offset
    +0x17, 1 / 2 force that mode (+ offset), 3 = bitmap with alpha +0x1c in the palette field"""
    hflip, vflip = obj.flags & 1, obj.flags >> 1 & 1
    pal_ofs = struct.unpack_from('<b', b, 0x17)[0]
    mode, alpha = struct.unpack_from('<I', b, 0x20)[0], b[0x1c]
    px, py = obj.x >> 16, obj.y >> 16
    out = []
    for a0, a1, a2 in oams:
        w, h = SIZE.get((a0 >> 14, a1 >> 14), (8, 8))
        x = a1 & 0x1ff
        x = x - 512 if x >= 256 else x
        y = a0 & 0xff
        y = y - 256 if y >= 128 else y
        if hflip:
            x = -(x + w); a1 ^= 0x1000
        if vflip:
            y = -(y + h); a1 ^= 0x2000
        a0 = (a0 & ~0xff) | ((y + py) & 0xff)
        a1 = (a1 & ~0x1ff) | ((x + px) & 0x1ff)
        if not obj.flags & 1:
            a2 = (a2 & ~0x3ff) | (((a2 & 0x3ff) + obj.tile_base) & 0x3ff)
        a2 = (a2 & ~0xc00) | ((obj.prio & 3) << 10)
        if mode == 3:
            a0 = (a0 & ~0xc00) | (3 << 10)
            a2 = (a2 & 0x0fff) | ((alpha & 15) << 12)
        else:
            if mode in (1, 2):
                a0 = (a0 & ~0xc00) | (mode << 10)
            a2 = (a2 & 0x0fff) | ((((a2 >> 12) + pal_ofs) & 15) << 12)
        if affine_idx is not None:
            a0 |= 0x100
            a1 = (a1 & ~0x3e00) | ((affine_idx & 31) << 9)
        out.append((a0, a1, a2))
    return out


def oam_from_ram(ram, lists_addr=0x213f6a0, base=0x2000000, cells=None):
    """{'main': [...], 'sub': [...]} in submission order: lists 0x3f..0, each chain from its head. `cells(obj)` gives an
    object's cell OAMs (default: the cell its RAM pointer names; lpui.cells gives them from the NCER / NANR files)"""
    cells = cells or (lambda o: cell_oams(ram, o.cell, base))
    out = {'main': [], 'sub': []}
    aff = {'main': 0, 'sub': 0}
    for li in range(0x3f, -1, -1):
        p = struct.unpack_from('<I', ram, lists_addr - base + 4 * li)[0]
        guard = 0
        while base <= p < base + 0x400000 and guard < 256:
            guard += 1
            o = SprObj(ram, p, base)
            if o.kind in (2, 3) and o.cell:
                eng = 'main' if o.engine == 1 else 'sub'
                idx = None
                b = ram[p - base:p - base + 0x38]
                if o.kind == 2:
                    if 0x2000000 <= o.mtx < 0x2400000:         # an affine matrix pointer
                        idx = aff[eng]; aff[eng] += 1
                    out[eng] += build(o, cells(o), idx)
                else:
                    sx, sy, rot = struct.unpack_from('<hhH', b, 0x10)
                    if rot or (sx, sy) not in ((0x1000, 0x1000), (0, 0)) or b[0x16]:
                        idx = aff[eng]; aff[eng] += 1
                    out[eng] += build_transformed(o, b, cells(o), idx)
            p = o.next
    return out
