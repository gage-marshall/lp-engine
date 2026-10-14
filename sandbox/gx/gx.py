"""Reference model of the DS geometry engine, driven by a captured GX command stream (sandbox/rec/gxcap.py).

Deliberately plain Python: it is an oracle, not an engine.  Semantics follow GBATEK (DS 3D):
  * matrices are 4x4, 20.12 fixed point, row-vector convention (v' = v * M); MULT premultiplies (C = M * C)
  * matrix modes: 0 projection, 1 position, 2 position+vector, 3 texture; SCALE in mode 2 touches the position matrix only
  * clip = v * position * projection; viewport maps clip x/w, y/w to the screen with y up
Each polygon keeps the full state it was emitted with (matrices index, attributes, texture params) for later analysis.
"""
import numpy as np

NPARAM = {0x10: 1, 0x11: 0, 0x12: 1, 0x13: 1, 0x14: 1, 0x15: 0, 0x16: 16, 0x17: 12, 0x18: 16, 0x19: 12, 0x1a: 9, 0x1b: 3, 0x1c: 3,
          0x20: 1, 0x21: 1, 0x22: 1, 0x23: 2, 0x24: 1, 0x25: 1, 0x26: 1, 0x27: 1, 0x28: 1, 0x29: 1, 0x2a: 1, 0x2b: 1,
          0x30: 1, 0x31: 1, 0x32: 1, 0x33: 1, 0x34: 32, 0x40: 1, 0x41: 0, 0x50: 1, 0x60: 1, 0x70: 3, 0x71: 2, 0x72: 1}
NAME = {0x10: 'MTX_MODE', 0x11: 'MTX_PUSH', 0x12: 'MTX_POP', 0x13: 'MTX_STORE', 0x14: 'MTX_RESTORE', 0x15: 'MTX_IDENTITY',
        0x16: 'MTX_LOAD_4x4', 0x17: 'MTX_LOAD_4x3', 0x18: 'MTX_MULT_4x4', 0x19: 'MTX_MULT_4x3', 0x1a: 'MTX_MULT_3x3',
        0x1b: 'MTX_SCALE', 0x1c: 'MTX_TRANS', 0x20: 'COLOR', 0x21: 'NORMAL', 0x22: 'TEXCOORD', 0x23: 'VTX_16', 0x24: 'VTX_10',
        0x25: 'VTX_XY', 0x26: 'VTX_XZ', 0x27: 'VTX_YZ', 0x28: 'VTX_DIFF', 0x29: 'POLYGON_ATTR', 0x2a: 'TEXIMAGE_PARAM',
        0x2b: 'PLTT_BASE', 0x30: 'DIF_AMB', 0x31: 'SPE_EMI', 0x32: 'LIGHT_VECTOR', 0x33: 'LIGHT_COLOR', 0x34: 'SHININESS',
        0x40: 'BEGIN_VTXS', 0x41: 'END_VTXS', 0x50: 'SWAP_BUFFERS', 0x60: 'VIEWPORT', 0x70: 'BOX_TEST', 0x71: 'POS_TEST', 0x72: 'VEC_TEST'}


def s32(x):
    x &= 0xffffffff
    return x - (1 << 32) if x & 0x80000000 else x


def sx(x, bits):
    x &= (1 << bits) - 1
    return x - (1 << bits) if x & (1 << (bits - 1)) else x


def packets(cmd, param, frame=None):
    """group FIFO entries into (cmd, [params], frame, entry index); zero-parameter commands occupy one entry"""
    i, n = 0, len(cmd)
    out = []
    while i < n:
        c = int(cmd[i]); k = max(1, NPARAM.get(c, 1))
        if c not in NPARAM:
            raise ValueError('unknown GX command %#x at entry %d' % (c, i))
        ps = [int(p) for p in param[i:i + k]]
        if len(ps) < k or any(int(x) != c for x in cmd[i:i + k]):
            raise ValueError('parameter run broken for %s at entry %d' % (NAME[c], i))
        out.append((c, ps if NPARAM[c] else [], int(frame[i]) if frame is not None else None, i))
        i += k
    return out


def fx(m):
    return np.array([s32(v) for v in m], dtype=np.float64) / 4096.0


class Geo:
    def __init__(self):
        I = np.eye(4)
        self.mode = 0
        self.proj = I.copy(); self.pos = I.copy(); self.vec = I.copy(); self.tex = I.copy()
        self.proj_stack = [I.copy()]; self.pos_stack = [I.copy() for _ in range(32)]; self.vec_stack = [I.copy() for _ in range(32)]
        self.sp = 0
        self.viewport = (0, 0, 255, 191)
        self.color = (31, 31, 31); self.normal = None; self.texcoord = (0.0, 0.0)
        self.attr = 0; self.attr_pending = 0; self.teximage = 0; self.pltt = 0
        self.dif = (0, 0, 0); self.amb = (0, 0, 0); self.spe = (0, 0, 0); self.emi = (0, 0, 0); self.use_shine = False
        self.lights = [((0, 0, -1), (0, 0, 0))] * 4
        self.shine = [0] * 128
        self.prim = None; self.vbuf = []; self.last = (0.0, 0.0, 0.0)
        self.polys = []            # current list (since the last SWAP)
        self.lists = []            # finished lists: (frame of SWAP, swap param, polys)
        self.mtx_log = []          # (entry idx, frame, cmd, matrix values) for provenance

    # matrix helpers
    def _cur(self):
        return {0: ('proj',), 1: ('pos',), 2: ('pos', 'vec'), 3: ('tex',)}[self.mode]

    def _set(self, M, mult, names=None):
        for nm in names or self._cur():
            cur = getattr(self, nm)
            setattr(self, nm, M @ cur if mult else M.copy())

    def clip(self):
        return self.pos @ self.proj

    def run(self, pk):
        for c, ps, fr, idx in pk:
            self.cmd(c, ps, fr, idx)

    def cmd(self, c, ps, fr, idx):
        if c == 0x10:
            self.mode = ps[0] & 3
        elif c == 0x11:   # push
            if self.mode == 0:
                self.proj_stack[0] = self.proj.copy()
            elif self.mode != 3:
                self.pos_stack[self.sp & 31] = self.pos.copy(); self.vec_stack[self.sp & 31] = self.vec.copy(); self.sp += 1
        elif c == 0x12:   # pop n
            if self.mode == 0:
                self.proj = self.proj_stack[0].copy()
            elif self.mode != 3:
                self.sp -= sx(ps[0], 6)
                self.pos = self.pos_stack[self.sp & 31].copy(); self.vec = self.vec_stack[self.sp & 31].copy()
        elif c == 0x13:   # store
            if self.mode == 0:
                self.proj_stack[0] = self.proj.copy()
            elif self.mode != 3:
                k = ps[0] & 31; self.pos_stack[k] = self.pos.copy(); self.vec_stack[k] = self.vec.copy()
        elif c == 0x14:   # restore
            if self.mode == 0:
                self.proj = self.proj_stack[0].copy()
            elif self.mode != 3:
                k = ps[0] & 31; self.pos = self.pos_stack[k].copy(); self.vec = self.vec_stack[k].copy()
        elif c == 0x15:
            self._set(np.eye(4), False)
        elif c in (0x16, 0x18):
            M = fx(ps).reshape(4, 4); self._set(M, c == 0x18); self.mtx_log.append((idx, fr, c, ps))
        elif c in (0x17, 0x19):
            M = np.eye(4); M[:, :3] = fx(ps).reshape(4, 3); self._set(M, c == 0x19); self.mtx_log.append((idx, fr, c, ps))
        elif c == 0x1a:
            M = np.eye(4); M[:3, :3] = fx(ps).reshape(3, 3); self._set(M, True); self.mtx_log.append((idx, fr, c, ps))
        elif c == 0x1b:
            M = np.diag(list(fx(ps)) + [1.0])
            self._set(M, True, ('pos',) if self.mode == 2 else None); self.mtx_log.append((idx, fr, c, ps))
        elif c == 0x1c:
            M = np.eye(4); M[3, :3] = fx(ps); self._set(M, True); self.mtx_log.append((idx, fr, c, ps))
        elif c == 0x20:
            v = ps[0]; self.color = (v & 31, (v >> 5) & 31, (v >> 10) & 31)
        elif c == 0x21:
            v = ps[0]
            n = np.array([sx(v, 10), sx(v >> 10, 10), sx(v >> 20, 10), 0], dtype=np.float64) / 512.0
            self.normal = n
            self.color = self.light(n)
        elif c == 0x22:
            v = ps[0]; self.texcoord = (sx(v, 16) / 16.0, sx(v >> 16, 16) / 16.0)
        elif c == 0x23:
            self.vertex((sx(ps[0], 16) / 4096.0, sx(ps[0] >> 16, 16) / 4096.0, sx(ps[1], 16) / 4096.0))
        elif c == 0x24:
            v = ps[0]; self.vertex((sx(v, 10) / 64.0, sx(v >> 10, 10) / 64.0, sx(v >> 20, 10) / 64.0))
        elif c == 0x25:
            v = ps[0]; self.vertex((sx(v, 16) / 4096.0, sx(v >> 16, 16) / 4096.0, self.last[2]))
        elif c == 0x26:
            v = ps[0]; self.vertex((sx(v, 16) / 4096.0, self.last[1], sx(v >> 16, 16) / 4096.0))
        elif c == 0x27:
            v = ps[0]; self.vertex((self.last[0], sx(v, 16) / 4096.0, sx(v >> 16, 16) / 4096.0))
        elif c == 0x28:
            v = ps[0]; l = self.last
            self.vertex((l[0] + sx(v, 10) / 4096.0, l[1] + sx(v >> 10, 10) / 4096.0, l[2] + sx(v >> 20, 10) / 4096.0))
        elif c == 0x29:
            self.attr_pending = ps[0]
        elif c == 0x2a:
            self.teximage = ps[0]
        elif c == 0x2b:
            self.pltt = ps[0]
        elif c == 0x30:
            v = ps[0]; self.dif = (v & 31, (v >> 5) & 31, (v >> 10) & 31); self.amb = ((v >> 16) & 31, (v >> 21) & 31, (v >> 26) & 31)
            if v & 0x8000:
                self.color = self.dif
        elif c == 0x31:
            v = ps[0]; self.spe = (v & 31, (v >> 5) & 31, (v >> 10) & 31); self.emi = ((v >> 16) & 31, (v >> 21) & 31, (v >> 26) & 31)
            self.use_shine = bool(v & 0x8000)
        elif c == 0x32:
            v = ps[0]; i = (v >> 30) & 3
            L = np.array([sx(v, 10), sx(v >> 10, 10), sx(v >> 20, 10), 0], dtype=np.float64) / 512.0
            L = (L @ self.vec)[:3]
            self.lights[i] = (tuple(L), self.lights[i][1])
        elif c == 0x33:
            v = ps[0]; i = (v >> 30) & 3
            self.lights[i] = (self.lights[i][0], (v & 31, (v >> 5) & 31, (v >> 10) & 31))
        elif c == 0x34:
            t = []
            for w in ps:
                t += [w & 255, (w >> 8) & 255, (w >> 16) & 255, (w >> 24) & 255]
            self.shine = t
        elif c == 0x40:
            self.attr = self.attr_pending
            self.prim = ps[0] & 3; self.vbuf = []
        elif c == 0x41:
            self.prim = None
        elif c == 0x50:
            self.lists.append((fr, ps[0], self.polys)); self.polys = []
        elif c == 0x60:
            v = ps[0]; self.viewport = (v & 255, (v >> 8) & 255, (v >> 16) & 255, (v >> 24) & 255)

    def light(self, n):
        N = (n @ self.vec)[:3]
        col = np.array(self.emi, dtype=np.float64)
        for i in range(4):
            if not (self.attr_pending >> i) & 1:
                continue
            L, lc = self.lights[i]; L = np.array(L); lc = np.array(lc, dtype=np.float64)
            d = max(0.0, -float(L @ N))
            H = (L - np.array([0, 0, -1.0])) / 2.0   # GBATEK: (LightVector - LineOfSight) / 2, LineOfSight = (0, 0, -1)
            s = max(0.0, float(-H @ N)); s = s * s
            s = min(1.0, s)
            if self.use_shine:
                s = self.shine[min(127, int(s * 127))] / 255.0
            col += lc / 31.0 * (np.array(self.spe) * s + np.array(self.dif) * d + np.array(self.amb))
        return tuple(int(min(31, x)) for x in col)

    def vertex(self, v):
        self.last = v
        p = np.array([v[0], v[1], v[2], 1.0]) @ self.clip()
        self.vbuf.append({'obj': v, 'clip': p, 'color': self.color, 'tc': self.texcoord})
        b, k = self.vbuf, self.prim
        if k == 0 and len(b) == 3:
            self.emit(b); self.vbuf = []
        elif k == 1 and len(b) == 4:
            self.emit(b); self.vbuf = []
        elif k == 2 and len(b) >= 3:
            n = getattr(self, '_strip_n', 0)
            tri = [b[-3], b[-2], b[-1]] if n % 2 == 0 else [b[-2], b[-3], b[-1]]
            self.emit(tri); self._strip_n = n + 1
            if len(b) > 3:
                self.vbuf = b[-3:]
        elif k == 3 and len(b) >= 4 and len(b) % 2 == 0:
            self.emit([b[-4], b[-3], b[-1], b[-2]])
            self.vbuf = b[-2:]
        if len(b) == 1 and k == 2:
            self._strip_n = 0

    def emit(self, vs):
        self.polys.append({'v': [dict(x) for x in vs], 'attr': self.attr, 'tex': self.teximage, 'pltt': self.pltt,
                           'viewport': self.viewport})


def to_screen(p, vp):
    x1, y1, x2, y2 = vp
    w = p[3]
    if w <= 0:
        return None
    sxp = (p[0] / w + 1) * (x2 - x1 + 1) / 2 + x1
    syp = (p[1] / w + 1) * (y2 - y1 + 1) / 2 + y1
    return sxp, 191 - syp, p[2] / w, w


def raster(polys, size=(256, 192), shade=True):
    """flat z-buffered fill (no clipping beyond w>0, no textures): coverage/depth oracle"""
    W, H = size
    img = np.zeros((H, W, 3), np.uint8); zb = np.full((H, W), np.inf); ids = np.full((H, W), -1, np.int32)
    for pi, P in enumerate(polys):
        pts = [to_screen(v['clip'], P['viewport']) for v in P['v']]
        if any(p is None for p in pts):
            continue
        cols = [v['color'] for v in P['v']]
        for t in ([0, 1, 2], [0, 2, 3])[:len(pts) - 2]:
            a, b, c = (pts[i] for i in t)
            xs = [a[0], b[0], c[0]]; ys = [a[1], b[1], c[1]]
            x0, x1 = max(0, int(np.floor(min(xs)))), min(W - 1, int(np.ceil(max(xs))))
            y0, y1 = max(0, int(np.floor(min(ys)))), min(H - 1, int(np.ceil(max(ys))))
            if x0 > x1 or y0 > y1:
                continue
            X, Y = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
            if abs(d) < 1e-9:
                continue
            l1 = ((X - a[0]) * (c[1] - a[1]) - (Y - a[1]) * (c[0] - a[0])) / d
            l2 = ((b[0] - a[0]) * (Y - a[1]) - (b[1] - a[1]) * (X - a[0])) / d
            l0 = 1 - l1 - l2
            inside = (l0 >= 0) & (l1 >= 0) & (l2 >= 0)
            z = l0 * a[2] + l1 * b[2] + l2 * c[2]
            sub = zb[y0:y1 + 1, x0:x1 + 1]
            win = inside & (z < sub)
            sub[win] = z[win]
            ids[y0:y1 + 1, x0:x1 + 1][win] = pi
            col = np.array(cols[t[0]]) * 8 if shade else np.array([255, 255, 255])
            img[y0:y1 + 1, x0:x1 + 1][win] = col.clip(0, 255).astype(np.uint8)
    return img, zb, ids
