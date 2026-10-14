"""Generate the GX command stream of a LovePlus+ heroine part draw from game data only (NSBMD, NSBCA, code tables),
by re-implementing NitroSystem G3D's SBC interpreter, joint animation sampling and the game's per-part callbacks in
exact fixed-point arithmetic.  Output packets have the same shape as gx.packets(): (cmd, [u32 params]).

Behaviour follows the NitroSystem G3D library (sbc.c, glbstate.c, util.c, anm/nsbca.c, cgtool/basic.c) and NitroSDK
fx (VEC_Normalize), read as reference only, plus the game's code (callbacks decoded in docs/RENDER_PRIMITIVES.md).
"""
import struct

M64 = (1 << 64) - 1


def s64(x):
    x &= M64
    return x - (1 << 64) if x >> 63 else x


def s32(x):
    x &= 0xffffffff
    return x - (1 << 32) if x >> 31 else x


def u32(x):
    return x & 0xffffffff


def isqrt(n):
    if n <= 0:
        return 0
    x = 1 << ((n.bit_length() + 1) // 2)
    while True:
        y = (x + n // x) // 2
        if y >= x:
            return x
        x = y


def vec_normalize(v):
    """NitroSDK VEC_Normalize (hardware divider + square root)"""
    t = v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
    if t == 0:
        return list(v)
    div = (1 << (12 + 12 + 32)) // t                  # CP_SetDiv64_64 (unsigned)
    sq = isqrt(t << 2) & 0xffffffff                   # CP_GetSqrtResult32
    tt = s64(div * sq)
    return [s32((s64(tt * c) + (1 << 44)) >> 45) for c in v]


def vec_cross(a, b):
    """NitroSDK VEC_CrossProduct (rounded: + 0x800 before >> 12)"""
    return [s32((a[1] * b[2] - a[2] * b[1] + 0x800) >> 12), s32((a[2] * b[0] - a[0] * b[2] + 0x800) >> 12),
            s32((a[0] * b[1] - a[1] * b[0] + 0x800) >> 12)]


def cross_row(a, b):
    return [(a[1] * b[2] - a[2] * b[1]) >> 12, (a[2] * b[0] - a[0] * b[2]) >> 12, (a[0] * b[1] - a[1] * b[0]) >> 12]


PIVOT = [(4, 5, 7, 8), (3, 5, 6, 8), (3, 4, 6, 7), (1, 2, 7, 8), (0, 2, 6, 8), (0, 1, 6, 7), (1, 2, 4, 5), (0, 2, 3, 5), (0, 1, 3, 4)]


def S16(d, o): return struct.unpack_from('<h', d, o)[0]
def U16(d, o): return struct.unpack_from('<H', d, o)[0]
def S32(d, o): return struct.unpack_from('<i', d, o)[0]
def U32(d, o): return struct.unpack_from('<I', d, o)[0]


def read_dict(d, o):
    """NNS resource dictionary -> [(name, data_offset)] (data entries of size unitSize)"""
    n = d[o + 1]; ofs_entry = U16(d, o + 6)
    e = o + ofs_entry; unit, ofs_name = U16(d, e), U16(d, e + 2)
    out = []
    for i in range(n):
        nm = d[e + ofs_name + 16 * i:e + ofs_name + 16 * i + 16].split(b'\0')[0].decode('latin1')
        out.append((nm, e + 4 + unit * i))
    return out


def file_blocks(d):
    nblk = U16(d, 14); out = {}
    for i in range(nblk):
        o = U32(d, 16 + 4 * i); out[d[o:o + 4].decode('latin1')] = o
    return out


# ------------------------------------------------------------------ model (NSBMD)
class Model:
    def __init__(self, d, index=0):
        self.d = d
        mdl0 = file_blocks(d)['MDL0']
        name, de = read_dict(d, mdl0 + 8)[index]
        mo = mdl0 + U32(d, de); self.name = name; self.mo = mo
        size, ofs_sbc, ofs_mat, ofs_shp, ofs_evp = struct.unpack_from('<5I', d, mo)
        ii = mo + 20
        self.sbc_type, self.scaling, self.texmtx_mode, self.nnode, self.nmat, self.nshp, self.first_unused, _ = struct.unpack_from('<8B', d, ii)
        self.pos_scale, self.inv_pos_scale = S32(d, ii + 8), S32(d, ii + 12)
        self.sbc = d[mo + ofs_sbc:mo + ofs_mat]
        self.evp = mo + ofs_evp if ofs_evp else None
        ni = ii + 44
        self.node_names = []; self.nodes = []
        for nm, e in read_dict(d, ni):
            self.node_names.append(nm); self.nodes.append(ni + U32(d, e))
        mb = mo + ofs_mat
        self.mats = [mb + U32(d, e) for _, e in read_dict(d, mb + 4)]
        self.mat_names = [nm for nm, _ in read_dict(d, mb + 4)]
        sb = mo + ofs_shp
        self.shps = []
        for nm, e in read_dict(d, sb):
            so = sb + U32(d, e); tag, sz, flag, ofs_dl, size_dl = struct.unpack_from('<HHIII', d, so)
            self.shps.append(d[so + ofs_dl:so + ofs_dl + size_dl])

    def node_result(self, i):
        """model SRT of node i as a joint-animation result (getMdl* / NODEDESC default path)"""
        d = self.d; o = self.nodes[i]; flag = U16(d, o); m00 = S16(d, o + 2); p = o + 4
        r = {'flag': 0}
        if flag & 1:
            r['trans'] = None
        else:
            r['trans'] = [S32(d, p), S32(d, p + 4), S32(d, p + 8)]; p += 12
        if flag & 2:
            r['rot'] = None
        elif flag & 8:
            A, B = S16(d, p), S16(d, p + 2); p += 4
            piv = (flag >> 4) & 15; rot = [0] * 9
            rot[piv] = -4096 if flag & 0x100 else 4096
            u = PIVOT[piv]; rot[u[0]] = A; rot[u[1]] = B; rot[u[2]] = -B if flag & 0x200 else B; rot[u[3]] = -A if flag & 0x400 else A
            r['rot'] = rot
        else:
            r['rot'] = [m00] + [S16(d, p + 2 * k) for k in range(8)]; p += 16
        r['scale'] = None if flag & 4 else [S32(d, p), S32(d, p + 4), S32(d, p + 8)]
        return r

    def node_srt_ptrs(self, i):
        return self.node_result(i)

    def evp_inv(self, j):
        """(invM 12 fx32, invN 9 fx32)"""
        o = self.evp + 84 * j
        return [S32(self.d, o + 4 * k) for k in range(12)], [S32(self.d, o + 48 + 4 * k) for k in range(9)]

    def mat(self, i):
        d = self.d; o = self.mats[i]
        tag, size, diffamb, specemi, polyattr, polymask, teximg, texmask, plttbase, flag, ow, oh, magw, magh = struct.unpack_from('<HHIIIIIIHHHHii', d, o)
        return {'diffamb': diffamb, 'specemi': specemi, 'polyattr': polyattr, 'polymask': polymask, 'teximg': teximg, 'pltt': plttbase,
                'flag': flag, 'texmtx_off': o + 44}


# ------------------------------------------------------------------ joint animation (NSBCA), exact
class JntAnim:
    def __init__(self, d, ja, name):
        self.d, self.ja, self.name = d, ja, name
        self.nframe, self.nnode, self.flag, self.ofs3, self.ofs5 = struct.unpack_from('<HHIII', d, ja + 4)

    def _rot_idx(self, idx):
        d = self.d
        if idx & 0x8000:
            o = self.ja + self.ofs3 + (idx & 0x7fff) * 6
            info, A, B = U16(d, o), S16(d, o + 2), S16(d, o + 4)
            rot = [0] * 9; piv = info & 15
            rot[piv] = -4096 if info & 0x10 else 4096
            u = PIVOT[piv]; rot[u[0]] = A; rot[u[1]] = B; rot[u[2]] = -B if info & 0x20 else B; rot[u[3]] = -A if info & 0x40 else A
            return rot, False
        o = self.ja + self.ofs5 + (idx & 0x7fff) * 10
        v = [S16(d, o + 2 * k) for k in range(5)]
        x = v[4] & 7; r11 = v[4] >> 3
        x = (x << 3) | (v[0] & 7); r00 = v[0] >> 3
        x = (x << 3) | (v[1] & 7); r01 = v[1] >> 3
        x = (x << 3) | (v[2] & 7); r02 = v[2] >> 3
        x = (x << 3) | (v[3] & 7); r10 = v[3] >> 3
        x16 = (x << 3) & 0xffff
        x16 = x16 - 0x10000 if x16 & 0x8000 else x16
        r12 = x16 >> 3
        return [r00, r01, r02, r10, r11, r12, 0, 0, 0], True

    def _rot(self, frame, info, off):
        d = self.d; arr = self.ja + off
        idx_at = lambda i: U16(d, arr + 2 * i)

        def blend(i0, i1, w0, w1, sh):
            a, ca = self._rot_idx(idx_at(i0)); b, cb = self._rot_idx(idx_at(i1))
            out = [(a[k] * w0 + b[k] * w1) >> sh for k in range(6)]
            r0 = vec_normalize(out[0:3]); r1 = vec_normalize(out[3:6])
            if not (ca or cb):
                r2 = vec_normalize([(a[k] * w0 + b[k] * w1) >> sh for k in range(6, 9)])
            else:
                r2 = cross_row(r0, r1)
            return r0 + r1 + r2

        def single(i):
            r, c = self._rot_idx(idx_at(i))
            if c:
                r[6:9] = cross_row(r[0:3], r[3:6])
            return r
        step = info & 0xc0000000
        if not step:
            return single(frame)
        last = (info & 0x1fff0000) >> 16
        if info & 0x40000000:
            if frame & 1:
                if frame > last:
                    return single((last >> 1) + 1)
                i = frame >> 1
                return blend(i, i + 1, 1, 1, 0)
            return single(frame >> 1)
        if frame & 3:
            if frame > last:
                return single((last >> 2) + (frame & 3))
            if frame & 1:
                if frame & 2:
                    isub = frame >> 2; i = isub + 1
                else:
                    i = frame >> 2; isub = i + 1
                return blend(i, isub, 3, 1, 0)
            i = frame >> 2
            return blend(i, i + 1, 1, 1, 0)
        return single(frame >> 2)

    def _val(self, frame, info, off, scale_pair=False):
        """translation component (getTransData_) or scale value (pair layout: value, inverse)"""
        d = self.d; arr = self.ja + off; fx16 = bool(info & 0x20000000); st = 2 if scale_pair else 1
        get = (lambda i: S16(d, arr + 2 * i * st)) if fx16 else (lambda i: S32(d, arr + 4 * i * st))
        step = info & 0xc0000000
        if not step:
            return get(frame)
        last = (info & 0x1fff0000) >> 16
        if info & 0x40000000:
            if frame & 1:
                if frame > last:
                    return get((last >> 1) + 1)
                i = frame >> 1
                return (get(i) + get(i + 1)) >> 1 if fx16 else (get(i) >> 1) + (get(i + 1) >> 1)
            return get(frame >> 1)
        if frame & 3:
            if frame > last:
                return get((last >> 2) + (frame & 3))
            if frame & 1:
                if frame & 2:
                    isub = frame >> 2; i = isub + 1
                else:
                    i = frame >> 2; isub = i + 1
                return (get(i) * 3 + get(isub)) >> 2
            i = frame >> 2
            return (get(i) + get(i + 1)) >> 1 if fx16 else (get(i) >> 1) + (get(i + 1) >> 1)
        return get(frame >> 2)

    def result(self, node, frame_fx, model):
        """NNSi_G3dAnmCalcNsBca for data index `node` at an integer frame (fx32), basic scaling rule"""
        frame = min(max(frame_fx, 0), (self.nframe << 12) - 1) >> 12
        d = self.d
        to = self.ja + U16(d, self.ja + 20 + 2 * node); tag = U32(d, to); p = to + 4
        r = {'trans': None, 'rot': None, 'scale': None}
        if tag & 1:
            return r
        base = model.node_result(node)
        if not tag & 0x6:
            t = []
            for cbit in (0x8, 0x10, 0x20):
                if tag & cbit:
                    t.append(S32(d, p)); p += 4
                else:
                    t.append(self._val(frame, U32(d, p), U32(d, p + 4))); p += 8
            r['trans'] = t
        elif tag & 0x4:
            r['trans'] = base['trans']
        if not tag & 0xc0:
            if tag & 0x100:
                rot, c = self._rot_idx(U32(d, p) & 0xffff); p += 4
                if c:
                    rot[6:9] = cross_row(rot[0:3], rot[3:6])
                r['rot'] = rot
            else:
                r['rot'] = self._rot(frame, U32(d, p), U32(d, p + 4)); p += 8
        elif tag & 0x80:
            r['rot'] = base['rot']
        if not tag & 0x600:
            s = []
            for cbit in (0x800, 0x1000, 0x2000):
                if tag & cbit:
                    s.append(S32(d, p)); p += 8
                else:
                    s.append(self._val(frame, U32(d, p), U32(d, p + 4), scale_pair=True)); p += 8
            r['scale'] = s
        elif tag & 0x400:
            r['scale'] = base['scale']
        return r


class AnimChain:
    """NNSi_G3dAnmBlendJnt (TwlSystem g3d/src/anm.c) over a chain of joint AnmObjs [(JntAnim, frame_fx, ratio), ...] in
    list order (head first). One object: plain NNSi_G3dAnmCalcNsBca. Results use nnsgen's form (None = TRANS_ZERO /
    ROT_ZERO / SCALE_ONE)."""
    def __init__(self, objs):
        self.objs = objs
        self.nnode = max(a.nnode for a, f, r in objs)

    def result(self, node, _frame_fx, model):
        live = [(a, f, r) for a, f, r in self.objs if node < a.nnode]          # mapData EXIST (identity map)
        if not live:
            return model.node_result(node)
        if len(self.objs) == 1:
            return live[0][0].result(node, live[0][1], model)
        total = sum(min(r, 4096) for a, f, r in live if r > 0)
        if total == 0:
            return model.node_result(node)
        if len(live) == 1:
            return live[0][0].result(node, live[0][1], model)
        scale = [0, 0, 0]; trans = [0, 0, 0]; rot = [0] * 9
        t_zero = r_zero = s_one = True; keep_x = keep_z = None
        for i1, (a, f, ratio) in enumerate(live):
            if ratio <= 0:
                continue
            r = a.result(node, f, model)
            if i1 == 0:
                rr = r['rot'] or [4096, 0, 0, 0, 4096, 0, 0, 0, 4096]
                keep_x, keep_z = rr[0:3], rr[6:9]
            w = fx_div(ratio, total) if total != 4096 else ratio
            sv = r['scale'] or [4096, 4096, 4096]
            scale = [scale[k] + ((w * sv[k]) >> 12) for k in range(3)]
            if r['trans'] is not None:
                trans = [trans[k] + ((w * r['trans'][k]) >> 12) for k in range(3)]
            if r['rot'] is not None:
                for k in range(6):
                    rot[k] += (w * r['rot'][k]) >> 12
            else:
                rot[0] += w; rot[4] += w
            t_zero &= r['trans'] is None; r_zero &= r['rot'] is None
            s_one &= r['scale'] is None or r['scale'] == [4096, 4096, 4096]
        rot[6:9] = vec_cross(rot[0:3], rot[3:6])
        rot[0:3] = keep_x if rot[0:3] == [0, 0, 0] else vec_normalize(rot[0:3])
        rot[6:9] = keep_z if rot[6:9] == [0, 0, 0] else vec_normalize(rot[6:9])
        rot[3:6] = vec_cross(rot[6:9], rot[0:3])
        return {'trans': None if t_zero else trans, 'rot': None if r_zero else rot, 'scale': None if s_one else scale}


def _f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]


def motion_blend_ratios(blend, ticks, third=False):
    """Game motion cross-fade (Gfx3D_InitializeModel 0x207d218 + Motion_UpdateBlendWeights 0x1ffa56c), after `ticks`
    updates since the motion command with blend length `blend`: AnmObj ratios (layer 0 = new, 1 = previous, 2 = third).
    Single-precision float steps as in the code: inc0 = dec1 = (int)(4096f / (blend * 0.7f)), inc2 = (int)(4096f /
    (blend * 0.4f)) or 0 without a third motion (chara+0x40 == -1); thresholds 0.3/0.4/0.6 as floats 1228.8/1638.4/2457.6.
    Returns None once the blend is over (remaining <= 0: layer 0 alone at 4096)."""
    import numpy as np
    f = np.float32
    inc0 = int(f(4096.0) / (f(blend) * f(0.7)))
    inc2 = int(f(4096.0) / (f(blend) * f(0.4))) if third else 0
    remaining, w = blend, [0, 4096, 0]
    n = None
    for _ in range(ticks):
        if remaining <= 0:
            return None
        t = fx_div(blend - remaining, blend)
        if f(t) >= f(1228.8):
            w[0] = min(w[0] + inc0, 4096)
        w[1] = max(w[1] - inc0, 0)
        if f(t) <= f(1638.4):
            w[2] = min(w[2] + inc2, 4096)
        elif f(t) >= f(2457.6):
            w[2] = max(w[2] - inc2, 0)
        remaining -= 1
        s = sum(w)
        n0, n1 = fx_div(w[0], s), fx_div(w[1], s)
        n = [n0, n1, 4096 - n0 - n1]
    return n


def load_bca(d):
    jnt = file_blocks(d)['JNT0']
    return {nm: JntAnim(d, jnt + U32(d, e), nm) for nm, e in read_dict(d, jnt + 8)}


def bundle_entries(d):
    """async3d / tmb bundle: u32 count, u32 entry size 0x28, entries {u32 offset, u32 size, char[32] name}"""
    n, es = struct.unpack_from('<II', d)
    out = {}
    for i in range(n):
        off, size = struct.unpack_from('<II', d, 8 + i * es)
        nm = d[16 + i * es:8 + (i + 1) * es].split(b'\0')[0].decode('latin1')
        out[nm] = d[off:off + size]
    return out


# ------------------------------------------------------------------ geometry engine state (fixed point), for read-backs
def mtx_mul(a, b):
    """4x4 fixed-point product a*b (row-vector convention: GX MULT computes M*Current)"""
    return [[(sum(a[i][k] * b[k][j] for k in range(4))) >> 12 for j in range(4)] for i in range(4)]


def m43(p):
    M = [[0, 0, 0, 0] for _ in range(4)]
    for r in range(4):
        for c in range(3):
            M[r][c] = s32(p[r * 3 + c])
    M[3][3] = 4096
    return M


def m33(p):
    M = [[4096 if i == j else 0 for j in range(4)] for i in range(4)]
    for r in range(3):
        for c in range(3):
            M[r][c] = s32(p[r * 3 + c])
    return M


IDENT = [[4096 if i == j else 0 for j in range(4)] for i in range(4)]


class GE:
    """tracks the matrices the commands produce (mode, position, vector, projection, stacks)"""
    def __init__(self):
        self.mode = 0; self.proj = [r[:] for r in IDENT]; self.pos = [r[:] for r in IDENT]; self.vec = [r[:] for r in IDENT]
        self.pstack = [r[:] for r in IDENT]; self.stack = [[[r[:] for r in IDENT], [r[:] for r in IDENT]] for _ in range(32)]; self.sp = 0
        self.pj_level = 0

    def run(self, c, ps):
        cur = {0: ['proj'], 1: ['pos'], 2: ['pos', 'vec'], 3: []}.get(self.mode, [])
        if c == 0x10:
            self.mode = ps[0] & 3
        elif c == 0x11:
            if self.mode == 0:
                self.pstack = [r[:] for r in self.proj]; self.pj_level = 1
            elif self.mode in (1, 2):
                self.stack[self.sp & 31] = [[r[:] for r in self.pos], [r[:] for r in self.vec]]; self.sp += 1
        elif c == 0x12:
            if self.mode == 0:
                self.proj = [r[:] for r in self.pstack]; self.pj_level = 0
            elif self.mode in (1, 2):
                n = ps[0] & 63; n = n - 64 if n & 32 else n
                self.sp -= n; e = self.stack[self.sp & 31]; self.pos = [r[:] for r in e[0]]; self.vec = [r[:] for r in e[1]]
        elif c == 0x13:
            if self.mode == 0:
                self.pstack = [r[:] for r in self.proj]
            elif self.mode in (1, 2):
                self.stack[ps[0] & 31] = [[r[:] for r in self.pos], [r[:] for r in self.vec]]
        elif c == 0x14:
            if self.mode == 0:
                self.proj = [r[:] for r in self.pstack]
            elif self.mode in (1, 2):
                e = self.stack[ps[0] & 31]; self.pos = [r[:] for r in e[0]]; self.vec = [r[:] for r in e[1]]
        elif c == 0x15:
            for nm in cur:
                setattr(self, nm, [r[:] for r in IDENT])
        elif c in (0x16, 0x18):
            M = [[s32(ps[r * 4 + k]) for k in range(4)] for r in range(4)]
            for nm in cur:
                setattr(self, nm, mtx_mul(M, getattr(self, nm)) if c == 0x18 else M)
        elif c in (0x17, 0x19):
            M = m43(ps)
            for nm in cur:
                setattr(self, nm, mtx_mul(M, getattr(self, nm)) if c == 0x19 else M)
        elif c == 0x1a:
            M = m33(ps)
            for nm in cur:
                setattr(self, nm, mtx_mul(M, getattr(self, nm)))
        elif c == 0x1b:
            M = [r[:] for r in IDENT]
            for k in range(3):
                M[k][k] = s32(ps[k])
            for nm in (['pos'] if self.mode == 2 else cur):
                setattr(self, nm, mtx_mul(M, getattr(self, nm)))
        elif c == 0x1c:
            M = [r[:] for r in IDENT]; M[3][:3] = [s32(x) for x in ps]
            for nm in cur:
                setattr(self, nm, mtx_mul(M, getattr(self, nm)))

    def clip(self):
        return mtx_mul(self.pos, self.proj)


# ------------------------------------------------------------------ SBC interpreter
DL_NPARAM = {0x10: 1, 0x11: 0, 0x12: 1, 0x13: 1, 0x14: 1, 0x15: 0, 0x16: 16, 0x17: 12, 0x18: 16, 0x19: 12, 0x1a: 9, 0x1b: 3, 0x1c: 3,
             0x20: 1, 0x21: 1, 0x22: 1, 0x23: 2, 0x24: 1, 0x25: 1, 0x26: 1, 0x27: 1, 0x28: 1, 0x29: 1, 0x2a: 1, 0x2b: 1,
             0x30: 1, 0x31: 1, 0x32: 1, 0x33: 1, 0x34: 32, 0x40: 1, 0x41: 0, 0x50: 1, 0x60: 1, 0x70: 3, 0x71: 2, 0x72: 1}


def unpack_dl(dl):
    out = []; p = 0; n = len(dl)
    while p + 4 <= n:
        ops = dl[p:p + 4]; p += 4
        for c in ops:
            if c == 0:
                continue
            k = DL_NPARAM[c]
            ps = list(struct.unpack_from('<%dI' % k, dl, p)) if k else []
            p += 4 * k
            out.append((c, ps))
    return out


MAT_COLOR_MASK = [0x00000000, 0x00007fff, 0x7fff0000, 0x7fff7fff, 0x00008000, 0x0000ffff, 0x7fff8000, 0x7fffffff]
GLB_DIFFAMB_DEFAULT = (16 | 16 << 5 | 16 << 10) | 0x8000 | ((16 | 16 << 5 | 16 << 10) << 16)
GLB_SPECEMI_DEFAULT = (16 | 16 << 5 | 16 << 10) | 0x8000 | ((16 | 16 << 5 | 16 << 10) << 16)
GLB_POLYATTR_DEFAULT = 0xf | (0 << 4) | (2 << 6) | (0 << 24) | (31 << 16)   # light 0-3 on, modulate, cull back (render front)


def glb_from_code(diffamb_rgb):
    """global state the game sets before part draws (Mod2045ee8_ProcessessGraphics_10 / _8):
    NNS_G3dGlbMaterialColorDiffAmb(c, c, FALSE), NNS_G3dGlbMaterialColorSpecEmi(0, 0, FALSE),
    NNS_G3dGlbPolygonAttr(lights 0, GX_POLYGONMODE_TOON, GX_CULL_NONE, id 0, alpha 31, misc 0)"""
    return {'diffamb': diffamb_rgb | (diffamb_rgb << 16), 'specemi': 0, 'polyattr': (2 << 4) | (3 << 6) | (31 << 16)}


POLYID_TABLE = [1, 48, 2, 48, 48, 48]                      # 0x21064d8: [facing k=0..2, not facing k=0..2]
FACE_NAMES = ['brow', 'cheek', 'eye', 'face', 'mouth']     # 0x20a0f28 table, '%s_%s' with heroine suffix m/r/n


def polygon_ids(part, mat_names, heroine, facing, base_res_forced=False):
    """Char_UpdateModelAmbientColor (really polygon ids): material index -> id for one part"""
    suf = 'mrn'[heroine]
    k = (lambda i: POLYID_TABLE[i + (0 if facing else 3)])
    col, col1, col2 = (48, 48, k(2)) if base_res_forced else (k(0), k(1), k(2))
    want = {}
    if part in ('body', 'face_m'):
        for nm in FACE_NAMES:
            want['%s_%s' % (nm, suf)] = col
    if part == 'face_l':
        want['facel_%s' % suf] = col
    if part == 'hair':
        want['hair_%s' % suf] = col1
    if part == 'body':
        want['neck_%s' % suf] = col2
    return {i: want[n] for i, n in enumerate(mat_names) if n in want}


def facing_test(head_rot_rows, head_pos, eye):
    """Char_IsFacingTarget: camera within 15 degrees of the head's forward direction (0, sin15, cos15)"""
    d = [0, 1060, 3956]                                      # 0x21064cc (fx32)
    fwd = [(d[0] * head_rot_rows[0][j] + d[1] * head_rot_rows[1][j] + d[2] * head_rot_rows[2][j]) >> 12 for j in range(3)]
    v = vec_normalize([eye[i] - head_pos[i] for i in range(3)])
    dot = (v[0] * fwd[0] + v[1] * fwd[1] + v[2] * fwd[2]) >> 12
    return dot >= 3956, dot


def patch_material(mt, lights=True):
    """load/draw-time material changes made by the game: lights 0+1 on every material (Gfx3D_SetModelMaterialColors ->
    NNS light-enable-all(mdl, 3)) and the polygon-mode bits removed from the mask before each draw
    (Mod2045ee8_ProcessessGraphics_8 -> NNSi_G3dModifyPolygonAttrMask(mdl, FALSE, 0x30))"""
    mt = dict(mt)
    if lights:                                                # parts only (Model_CreatePartInstances); accessories keep theirs
        mt['polyattr'] = (mt['polyattr'] & ~0xf) | 3
    mt['polymask'] = mt['polymask'] & ~0x30
    return mt


def _frame_bytes(main, fn):
    """stack bytes an ARM prologue takes: push {reglist} (stmdb sp!) + optional sub sp, sp, #n.  A leading
    'push {r0, lr}; ...; pop {r0, lr}' profiler hook (main, Main_UpdateFrame) is skipped."""
    w = lambda a: struct.unpack_from('<I', main, a - 0x2000000)[0]
    a = fn
    if w(a) == 0xe92d4001:                                    # push {r0, lr} ... pop {r0, lr}
        while w(a) != 0xe8bd4001:
            a += 4
        a += 4
    ins = w(a); assert ins & 0xffff0000 == 0xe92d0000, hex(ins)
    n = 4 * bin(ins & 0xffff).count('1')
    nx = w(a + 4)
    if nx & 0xfffff000 == 0xe24dd000:                         # sub sp, sp, #imm (rotated immediate)
        imm, rot = nx & 0xff, (nx >> 8) & 0xf
        n += ((imm >> (2 * rot)) | (imm << (32 - 2 * rot))) & 0xffffffff
    return n


def nodemix_stack_tail(main, via_pg9):
    """The 3 words NODEMIX's G3_LoadMtx43(&sum.N) (0x20054a0, sp+0xe0) reads past its 0x104-byte frame: its saved r4, r5, r6.
    r4 = G3dDrawInternal_Loop_'s r4 = rs (NNS_G3dDraw's local NNSG3dRS at its sp); r5 = Loop's literal (0x2003724);
    r6 = PG10's r6 (mov r6, r4 with r4 = 0 at 0x2046944; NNS_G3dDraw's 0x20039c8 path and PG9 leave it).
    rs = system-mode SP from crt0 (0x2000820..0x2000864) minus the prologue frames on the call chain
    main 0x2000c38 -> Main_UpdateFrame 0x203d9d4 -> PG10 0x20468dc [-> PG9 0x20467e0] -> NNS_G3dDraw 0x200392c."""
    w = lambda a: struct.unpack_from('<I', main, a - 0x2000000)[0]
    dtcm = w(0x2000820 + 8 + 0xf0)
    r1 = dtcm + 0x3fc0 - 0x40 - w(0x200084c + 8 + 0xc8)
    sp = r1 - 4
    if sp & 4:
        sp -= 4
    for fn in (0x2000c38, 0x203d9d4, 0x20468dc) + ((0x20467e0,) if via_pg9 else ()) + (0x200392c,):
        sp -= _frame_bytes(main, fn)
    return [sp, w(0x20036ec + 8 + 0x38), 0]


class Draw:
    """one NNS_G3dDraw of a render object: emits packets and updates `ge`; rec = joint result record buffer (list per node)"""
    stack_tail = [0, 0, 0]                   # set per call path from nodemix_stack_tail()

    def __init__(self, model, ge, out, rec, anim=None, frame_fx=0, flags=0, glb=None, node_cb=None):
        self.m, self.ge, self.out, self.rec = model, ge, out, rec
        self.anim, self.frame_fx = anim, frame_fx
        self.record = bool(flags & 1); self.nogecmd = bool(flags & 2)
        self.glb = glb or {}
        self.node_cb = node_cb               # game NODEDESC callback (timing C): fn(draw, node_index)
        self.tex_bind = None                 # fn(model, mat_idx, mat) -> (teximg, pltt) after texture binding
        self.poly_id = {}                    # material index -> polygon id set by the game
        self.light_patch = True
        self.evp_cache = {}

    def emit(self, c, ps):
        if self.nogecmd:
            return
        self.out.append((c, [u32(x) for x in ps]))
        self.ge.run(c, [u32(x) for x in ps])

    def emit_direct(self, c, ps):            # G3_* immediate commands (also suppressed only by NOGECMD at the call sites)
        self.out.append((c, [u32(x) for x in ps]))
        self.ge.run(c, [u32(x) for x in ps])

    def joint_result(self, node):
        if self.rec is not None and not self.record:
            return self.rec[node]
        if self.anim is not None and node < self.anim.nnode:
            r = self.anim.result(node, self.frame_fx, self.m)
        else:
            r = self.m.node_result(node)
        if self.rec is not None:
            self.rec[node] = r
        return r

    def send_joint(self, r):                 # NNSi_G3dSendJointSRTBasic
        if r['trans'] is not None:
            if r['rot'] is not None:
                self.emit(0x19, r['rot'] + r['trans'])
            else:
                self.emit(0x1c, r['trans'])
        elif r['rot'] is not None:
            self.emit(0x1a, r['rot'])
        if r['scale'] is not None and r['scale'] != [4096, 4096, 4096]:
            self.emit(0x1b, r['scale'])

    def run(self):
        c = self.m.sbc; p = 0; visible = True; cur_mat = None
        while p < len(c):
            b = c[p]; cmd = b & 31; opt = b >> 5
            if cmd == 0x00:
                p += 1
            elif cmd == 0x01:
                break
            elif cmd == 0x02:
                visible = bool(c[p + 2] & 1); p += 3
            elif cmd == 0x03:
                if visible:
                    self.emit(0x14, [c[p + 1]])
                p += 2
            elif cmd == 0x04:
                idx = c[p + 1]
                if visible or cur_mat != idx:
                    self.material(idx); cur_mat = idx
                p += 2
            elif cmd == 0x05:
                if visible and not getattr(self, 'mat_transparent', False):
                    for cc, ps in unpack_dl(self.m.shps[c[p + 1]]):
                        self.emit(cc, ps)
                p += 2
            elif cmd == 0x06:
                node = c[p + 1]; ln = 4
                dest = src = None
                if opt in (1, 3):
                    dest = c[p + 4]; ln += 1
                if opt in (2, 3):
                    src = c[p + 4] if opt == 2 else c[p + 5]; ln += 1
                if src is not None:
                    self.emit(0x14, [src])
                skip = bool(self.node_cb and self.node_cb(self, node, 'A'))   # timing A callback may set RSFLAG_SKIP
                r = self.joint_result(node)
                if not skip:
                    self.send_joint(r)
                if self.node_cb and not skip:
                    self.node_cb(self, node, 'C')     # NNSi_G3dCallBackCheck_C runs after the joint matrix, before the store
                if dest is not None and not skip:    # RSFLAG_SKIP set at timing A also suppresses B/C work (incl. the store)
                    self.emit(0x13, [dest])
                p += ln
            elif cmd == 0x09:
                self.nodemix(c, p)
                p += 3 + c[p + 2] * 3
            elif cmd == 0x0b:
                s = self.m.pos_scale if opt == 0 else self.m.inv_pos_scale
                self.emit(0x1b, [s, s, s]); p += 1
            elif cmd in (0x07, 0x08):
                p += 2 + (1 if opt in (1, 3) else 0) + (1 if opt in (2, 3) else 0)
                raise NotImplementedError('billboard')
            else:
                raise NotImplementedError('SBC %#x' % cmd)

    def material(self, idx):
        mt = patch_material(self.m.mat(idx), lights=self.light_patch)
        if idx in self.poly_id:                       # NNS_G3dMdlSetMdlPolygonID calls of Char_UpdateModelAmbientColor
            mt['polyattr'] = (mt['polyattr'] & ~0x3f000000) | (self.poly_id[idx] << 24)
        if self.tex_bind:
            mt['teximg'], mt['pltt'] = self.tex_bind(self.m, idx, mt)
        m0 = MAT_COLOR_MASK[(mt['flag'] >> 6) & 7]; m1 = MAT_COLOR_MASK[(mt['flag'] >> 9) & 7]
        dif = (self.glb.get('diffamb', GLB_DIFFAMB_DEFAULT) & ~m0) | (mt['diffamb'] & m0)
        spe = (self.glb.get('specemi', GLB_SPECEMI_DEFAULT) & ~m1) | (mt['specemi'] & m1)
        pa = (self.glb.get('polyattr', GLB_POLYATTR_DEFAULT) & ~mt['polymask']) | (mt['polyattr'] & mt['polymask'])
        if not pa & 0x1f0000:
            self.mat_transparent = True
            return
        self.mat_transparent = False
        self.emit(0x30, [dif]); self.emit(0x31, [spe]); self.emit(0x29, [pa])
        self.emit(0x2a, [mt['teximg']]); self.emit(0x2b, [mt['pltt']])
        if mt['flag'] & 1:
            raise NotImplementedError('texture matrix')

    def nodemix(self, c, p):
        """NNSi_G3dFuncSbc_NODEMIX with the evp cache (NNS_G3D_USE_EVPCACHE 1)"""
        dest, num = c[p + 1], c[p + 2]
        sumM = [[0] * 3 for _ in range(4)]; sumN = [[0] * 3 for _ in range(3)]
        self.emit_direct(0x10, [0]); self.emit_direct(0x13, [1]); self.emit_direct(0x15, []); self.emit_direct(0x10, [2])
        w = 0; prevN = None
        q = p + 3
        for i in range(num):
            src, jnt, ratio = c[q], c[q + 1], c[q + 2]
            cached = jnt in self.evp_cache
            if not cached:
                invM, invN = self.m.evp_inv(jnt)
                self.emit_direct(0x14, [src]); self.emit_direct(0x10, [1]); self.emit_direct(0x19, invM)
            if i != 0:
                for r in range(3):
                    for k in range(3):
                        sumN[r][k] += (w * prevN[r][k]) >> 12
            if not cached:
                X = self.ge.clip()
                self.emit_direct(0x10, [2]); self.emit_direct(0x1a, invN)
                Y = [row[:3] for row in self.ge.vec[:3]]
                self.evp_cache[jnt] = (X, Y)
            X, Y = self.evp_cache[jnt]
            w = ratio << 4
            for r in range(4):
                for k in range(3):
                    sumM[r][k] += (w * X[r][k]) >> 12
            prevN = Y
            q += 3
        for r in range(3):
            for k in range(3):
                sumN[r][k] += (w * prevN[r][k]) >> 12
        # LoadMtx43(&sum.N): the 9 words of N + 3 words past NODEMIX's frame (saved r4-r6, see nodemix_stack_tail)
        self.emit_direct(0x17, [x for row in sumN for x in row] + list(self.stack_tail))
        self.emit_direct(0x10, [1]); self.emit_direct(0x17, [x for row in sumM for x in row])
        self.emit_direct(0x10, [0]); self.emit_direct(0x14, [1]); self.emit_direct(0x10, [2])
        self.emit_direct(0x13, [dest])


# ------------------------------------------------------------------ NitroSDK fx (exact) and the game's global state (FlushVP)
def fx_div64(numer, denom):
    """signed 64/32 hardware divide of (numer << 32) by denom, truncating"""
    n = numer << 32
    q = abs(n) // abs(denom)
    return q if (n >= 0) == (denom > 0) else -q


def fx_div(numer, denom):                                    # FX_Div: (q + 2^19) >> 20
    return s32((fx_div64(numer, denom) + (1 << 19)) >> 20)


def fx_mul32x64c(v32, v64c):
    return s32((v64c * v32 + 0x80000000) >> 32)


def fx_mul(a, b):
    return s32((a * b + 0x800) >> 12)


def mtx_perspective_w(fsin, fcos, aspect, n, f, scale_w):
    """MTX_PerspectiveW -> 4x4 (rows)"""
    one_tan = fx_div(fcos, fsin)
    t = fx_div64(4096, n - f)                                # FX_InvAsyncImm(n - f); GetInvResultFx64c
    if scale_w != 4096:
        one_tan = int((one_tan * scale_w) / 4096) if False else _cdiv(one_tan * scale_w, 4096)
        t = _cdiv(t * scale_w, 4096)
    m = [[0] * 4 for _ in range(4)]
    m[1][1] = one_tan; m[2][3] = -scale_w
    m[2][2] = fx_mul32x64c(f + n, t)
    m[3][2] = fx_mul32x64c(fx_mul(n << 1, f), t)
    m[0][0] = fx_div(one_tan, aspect)
    return m


def _cdiv(a, b):                                             # C integer division (truncate toward zero)
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


def vec_cross(a, b):
    return [s32((a[1] * b[2] - a[2] * b[1] + 2048) >> 12), s32((a[2] * b[0] - a[0] * b[2] + 2048) >> 12), s32((a[0] * b[1] - a[1] * b[0] + 2048) >> 12)]


def vec_dot(a, b):
    return s32((a[0] * b[0] + a[1] * b[1] + a[2] * b[2] + 2048) >> 12)


def mtx_look_at(pos, up, tgt):
    """MTX_LookAt -> 4x3 rows"""
    look = vec_normalize([pos[i] - tgt[i] for i in range(3)])
    right = vec_normalize(vec_cross(up, look))
    vup = vec_cross(look, right)
    return [[right[0], vup[0], look[0]], [right[1], vup[1], look[1]], [right[2], vup[2], look[2]],
            [-vec_dot(pos, right), -vec_dot(pos, vup), -vec_dot(pos, look)]]


class SinCos:
    def __init__(self, arm9_main):
        self.t = struct.unpack_from('<8192h', arm9_main, 0x20fc090 - 0x2000000)   # FX_SinCosTable_

    def sin(self, idx):
        return self.t[((idx & 0xffff) >> 4) << 1]

    def cos(self, idx):
        return self.t[(((idx & 0xffff) >> 4) << 1) + 1]


def rot_y33(s, c):
    return [[c, 0, -s], [0, 4096, 0], [s, 0, c]]


def rot_x33(s, c):
    return [[4096, 0, 0], [0, c, s], [0, -s, c]]


def concat33(a, b):
    return [[s32((a[i][0] * b[0][j] + a[i][1] * b[1][j] + a[i][2] * b[2][j]) >> 12) for j in range(3)] for i in range(3)]


def multvec33(v, m):
    return [s32((v[0] * m[0][j] + v[1] * m[1][j] + v[2] * m[2][j]) >> 12) for j in range(3)]


def deg_to_idx(deg):                                         # Model_ApplyGlobalLights: (deg << 16) / 360, truncating
    return _cdiv(deg << 16, 360) & 0xffff


def fx16(v):
    v &= 0xffff
    return v - 0x10000 if v & 0x8000 else v


def pack_light_vector(i, x, y, z):                           # GX_PACK_LIGHTVECTOR_PARAM
    return ((x >> 3) & 0x3ff) | (((y >> 3) & 0x3ff) << 10) | (((z >> 3) & 0x3ff) << 20) | (i << 30)


def game_lights(sc, light_angles, colors=(0x7fff, 0x4210, 0x2108, 0x7fff), D=-4095):
    """Model_ApplyGlobalLights: colours from 0x2102bac, vectors (0,0,D) * RotX(ax) RotY(ay) for lights 0-2,
    light 3 keeps NNS_G3dGlbInit's (0, -1, 0). light_angles = [(ay, ax)] * 3 from preset words (11+i, 8+i)."""
    vecs = []
    for i, (ay, ax) in enumerate(light_angles):
        ry = deg_to_idx(ay); rx = deg_to_idx(ax)
        m = concat33(rot_x33(sc.sin(rx), sc.cos(rx)), rot_y33(sc.sin(ry), sc.cos(ry)))
        v = multvec33([0, 0, D], m)
        vecs.append(pack_light_vector(i, fx16(v[0]), fx16(v[1]), fx16(v[2])))
    vecs.append(pack_light_vector(3, 0, -4096, 0))
    cols = [c | (i << 30) for i, c in enumerate(colors)]
    return vecs, cols


def flush_vp(proj, cam, lights, glb, base_rot, base_trans, base_scale, viewport=(0, 0, 255, 191), teximage=0):
    """NNS_G3dGlbFlushVP packet sequence"""
    vecs, cols = lights
    out = [(0x10, [0]), (0x16, [u32(x) for row in proj for x in row]), (0x19, [u32(x) for row in cam for x in row]), (0x10, [2]), (0x15, [])]
    out += [(0x32, [v]) for v in vecs]
    vp = viewport[0] | (viewport[1] << 8) | (viewport[2] << 16) | (viewport[3] << 24)
    out += [(0x30, [glb['diffamb']]), (0x31, [glb['specemi']]), (0x29, [glb['polyattr']]), (0x60, [vp])]
    out += [(0x33, [c]) for c in cols]
    out += [(0x19, [u32(x) for row in base_rot for x in row] + [u32(x) for x in base_trans]), (0x1b, [u32(x) for x in base_scale]), (0x2a, [teximage])]
    return out
