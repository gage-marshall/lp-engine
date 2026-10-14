"""Software renderer for a decoded GX polygon list (gx.py) using captured VRAM and 3D registers (gxcap.py).

Oracle quality, not speed.  Implements what heroine scenes use (checked by census): textures of every DS format,
palettes, texture-coordinate modes 0/1, modulate / decal / toon shading, polygon alpha, front/back culling,
z-buffer, edge marking.  Not implemented: antialiasing, fog, shadow polygons, highlight shading, translucent sorting
(translucent polygons are drawn after opaque ones in submission order).
"""
import numpy as np
from gx import to_screen

BANK_OFS = dict(A=0, B=0x20000, C=0x40000, D=0x60000, E=0x80000, F=0x90000, G=0x94000)
BANK_SIZE = dict(A=0x20000, B=0x20000, C=0x20000, D=0x20000, E=0x10000, F=0x4000, G=0x4000)


def vram_maps(lcd, vramcnt):
    """texture image space (512 KB, 4 slots) and texture palette space (96 KB) as mapped by VRAMCNT_A..G"""
    tex = np.zeros(0x80000, np.uint8); pal = np.zeros(0x18000, np.uint8)
    for i, b in enumerate('ABCDEFG'):
        c = vramcnt[i] if i < 7 else 0
        if not c & 0x80 or (c & 7) != 3:
            continue
        ofs = (c >> 3) & 3
        data = lcd[BANK_OFS[b]:BANK_OFS[b] + BANK_SIZE[b]]
        if b in 'ABCD':
            tex[ofs * 0x20000:ofs * 0x20000 + 0x20000] = data
        elif b == 'E':
            pal[0:0x10000] = data
        else:
            a = (ofs & 1) * 0x4000 + (ofs >> 1) * 0x10000
            pal[a:a + 0x4000] = data
    return tex, pal


def rgb555(c):
    c = np.asarray(c, np.uint32)
    return np.stack([(c & 31), (c >> 5) & 31, (c >> 10) & 31], -1).astype(np.int32)


def to6(c5):
    c5 = np.asarray(c5, np.int32)
    return c5 * 2 + (c5 > 0)


def decode_texture(tex, pal, teximage, pltt):
    """-> (H, W, 4) int array: rgb 6-bit, alpha 0..31"""
    fmt = (teximage >> 26) & 7
    w = 8 << ((teximage >> 20) & 7); h = 8 << ((teximage >> 23) & 7)
    addr = (teximage & 0xffff) * 8
    col0t = (teximage >> 29) & 1
    n = w * h
    out = np.zeros((h, w, 4), np.int32)
    pbase = pltt * (8 if fmt == 2 else 16)
    palw = pal.view('<u2')

    def palette(idx):
        return palw[(pbase // 2 + idx) % len(palw)]
    if fmt == 7:
        px = tex[addr:addr + n * 2].view('<u2')
        out[..., :3] = to6(rgb555(px)).reshape(h, w, 3); out[..., 3] = np.where(px >> 15, 31, 0).reshape(h, w)
    elif fmt in (2, 3, 4):
        bits = {2: 2, 3: 4, 4: 8}[fmt]
        raw = tex[addr:addr + n * bits // 8]
        if bits == 8:
            idx = raw.astype(np.int32)
        elif bits == 4:
            idx = np.stack([raw & 15, raw >> 4], -1).reshape(-1).astype(np.int32)
        else:
            idx = np.stack([(raw >> s) & 3 for s in (0, 2, 4, 6)], -1).reshape(-1).astype(np.int32)
        c = palette(idx)
        out[..., :3] = to6(rgb555(c)).reshape(h, w, 3)
        out[..., 3] = np.where((idx == 0) & bool(col0t), 0, 31).reshape(h, w)
    elif fmt in (1, 6):
        raw = tex[addr:addr + n].astype(np.int32)
        if fmt == 1:
            idx, a3 = raw & 31, raw >> 5; alpha = (a3 * 4 + a3 // 2)
        else:
            idx, a5 = raw & 7, raw >> 3; alpha = a5
        out[..., :3] = to6(rgb555(palette(idx))).reshape(h, w, 3); out[..., 3] = alpha.reshape(h, w)
    elif fmt == 5:   # 4x4 compressed: index data in slot 1 (slot0 tex -> slot1+addr/2, slot2 tex -> slot1+0x10000+(addr-0x40000)/2)
        blocks = tex[addr:addr + n // 4].view('<u4')
        islot = 0x20000 + (addr // 2 if addr < 0x40000 else 0x10000 + (addr - 0x40000) // 2)
        pidx = tex[islot:islot + len(blocks) * 2].view('<u2')
        bw = w // 4
        for bi, blk in enumerate(blocks):
            pi = int(pidx[bi]); mode = pi >> 14; pa = (pi & 0x3fff) * 2
            cols = [int(palette(pa + k)) for k in range(4)]
            c = [to6(rgb555(x)) for x in cols]
            if mode == 0:
                lut = [(c[0], 31), (c[1], 31), (c[2], 31), (c[3] * 0, 0)]
            elif mode == 1:
                lut = [(c[0], 31), (c[1], 31), ((c[0] + c[1]) // 2, 31), (c[3] * 0, 0)]
            elif mode == 2:
                lut = [(c[0], 31), (c[1], 31), (c[2], 31), (c[3], 31)]
            else:
                lut = [(c[0], 31), (c[1], 31), ((c[0] * 5 + c[1] * 3) // 8, 31), ((c[0] * 3 + c[1] * 5) // 8, 31)]
            by, bx = divmod(bi, bw)
            for yy in range(4):
                for xx in range(4):
                    t = (int(blk) >> (2 * (yy * 4 + xx))) & 3
                    out[by * 4 + yy, bx * 4 + xx, :3] = lut[t][0]; out[by * 4 + yy, bx * 4 + xx, 3] = lut[t][1]
    return out


def wrap(u, size, repeat, flip):
    if not repeat:
        return np.clip(u, 0, size - 1)
    if flip:
        m = np.mod(u, 2 * size)
        return np.where(m >= size, 2 * size - 1 - m, m)
    return np.mod(u, size)


def render(polys, tex, pal, toon, edge, disp3dcnt, clear_rgba=(0, 0, 0, 0), front_ccw=True, W=256, H=192):
    color = np.zeros((H, W, 3), np.int32); color[:] = clear_rgba[:3]
    alpha = np.full((H, W), clear_rgba[3], np.int32)
    depth = np.full((H, W), np.inf); pid = np.full((H, W), 0x3f, np.int32)
    toon6 = to6(rgb555(toon))
    cache = {}
    order = sorted(range(len(polys)), key=lambda i: (((polys[i]['attr'] >> 16) & 31) < 31, i))
    for pi in order:
        P = polys[pi]
        attr, ti = P['attr'], P['tex']
        pts = [to_screen(v['clip'], P['viewport']) for v in P['v']]
        if any(p is None for p in pts):
            continue
        # culling: signed area in screen space (y down)
        area = 0.0
        for i in range(len(pts)):
            x0, y0 = pts[i][:2]; x1, y1 = pts[(i + 1) % len(pts)][:2]
            area += x0 * y1 - x1 * y0
        front = (area < 0) if front_ccw else (area > 0)
        if (front and not attr & 0x80) or (not front and not attr & 0x40):
            continue
        fmt = (ti >> 26) & 7
        if fmt:
            key = (ti & 0x3fffffff, P['pltt'])
            if key not in cache:
                cache[key] = decode_texture(tex, pal, ti, P['pltt'])
            T = cache[key]; th, tw = T.shape[:2]
        mode = (attr >> 4) & 3
        palpha = (attr >> 16) & 31
        for t in ([0, 1, 2], [0, 2, 3])[:len(pts) - 2]:
            a, b, c = (pts[i] for i in t); va, vb, vc = (P['v'][i] for i in t)
            xs = [a[0], b[0], c[0]]; ys = [a[1], b[1], c[1]]
            x0, x1 = max(0, int(np.floor(min(xs)))), min(W - 1, int(np.ceil(max(xs))))
            y0, y1 = max(0, int(np.floor(min(ys)))), min(H - 1, int(np.ceil(max(ys))))
            if x0 > x1 or y0 > y1:
                continue
            d = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
            if abs(d) < 1e-9:
                continue
            X, Y = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            l1 = ((X - a[0]) * (c[1] - a[1]) - (Y - a[1]) * (c[0] - a[0])) / d
            l2 = ((b[0] - a[0]) * (Y - a[1]) - (b[1] - a[1]) * (X - a[0])) / d
            l0 = 1 - l1 - l2
            inside = (l0 >= 0) & (l1 >= 0) & (l2 >= 0)
            if not inside.any():
                continue
            z = l0 * a[2] + l1 * b[2] + l2 * c[2]
            sub = depth[y0:y1 + 1, x0:x1 + 1]
            win = inside & (z < sub)
            if not win.any():
                continue
            iw = np.array([1 / a[3], 1 / b[3], 1 / c[3]])
            q0, q1, q2 = l0 * iw[0], l1 * iw[1], l2 * iw[2]; qs = q0 + q1 + q2
            def interp(fa, fb, fc):
                return (q0 * fa + q1 * fb + q2 * fc) / qs
            vcol = [to6(v['color']) for v in (va, vb, vc)]
            r = interp(vcol[0][0], vcol[1][0], vcol[2][0]); g = interp(vcol[0][1], vcol[1][1], vcol[2][1]); bl = interp(vcol[0][2], vcol[1][2], vcol[2][2])
            vrgb = np.stack([r, g, bl], -1)
            if mode == 2 and not disp3dcnt & 2:   # toon: vertex red indexes the toon table
                vrgb = toon6[np.clip((r.astype(np.int32)) >> 1, 0, 31)].astype(np.float64)
            out_a = np.full(r.shape, palpha, np.float64)
            if fmt:
                s = interp(va['tc'][0], vb['tc'][0], vc['tc'][0]); tt = interp(va['tc'][1], vb['tc'][1], vc['tc'][1])
                si = wrap(np.floor(s).astype(np.int64), tw, ti & 0x10000, ti & 0x40000)
                tj = wrap(np.floor(tt).astype(np.int64), th, ti & 0x20000, ti & 0x80000)
                tx = T[tj, si]
                trgb, ta = tx[..., :3].astype(np.float64), tx[..., 3].astype(np.float64)
                if mode == 1:   # decal
                    rgb = (trgb * ta[..., None] + vrgb * (31 - ta[..., None])) / 31
                else:           # modulate (and toon/highlight after the table lookup)
                    rgb = ((trgb + 1) * (vrgb + 1) - 1) / 64
                    out_a = ((ta + 1) * (palpha + 1) - 1) / 32
            else:
                rgb = vrgb
            win &= out_a > 0
            if not win.any():
                continue
            cs = color[y0:y1 + 1, x0:x1 + 1]; als = alpha[y0:y1 + 1, x0:x1 + 1]
            opaque = out_a >= 31
            wo = win & opaque
            cs[wo] = np.clip(rgb[wo], 0, 63).astype(np.int32); als[wo] = 31
            wt = win & ~opaque
            if wt.any() and disp3dcnt & 8:
                f = (out_a[wt] / 31)[..., None]
                cs[wt] = np.clip(rgb[wt] * f + cs[wt] * (1 - f), 0, 63).astype(np.int32); als[wt] = np.maximum(als[wt], out_a[wt].astype(np.int32))
            if (palpha >= 31) or (attr & 0x800):
                sub[wo | (wt if attr & 0x800 else False)] = z[wo | (wt if attr & 0x800 else False)]
            pid[y0:y1 + 1, x0:x1 + 1][wo] = (attr >> 24) & 63
    if disp3dcnt & 0x20:   # edge marking: pixel whose 4-neighbour has a different polygon id and is further away
        e6 = to6(rgb555(edge))
        out = color.copy()
        for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
            nid = np.full_like(pid, 0x3f); nd = np.full_like(depth, np.inf)
            ys = slice(max(0, dy), H + min(0, dy)); yd = slice(max(0, -dy), H + min(0, -dy))
            xs = slice(max(0, dx), W + min(0, dx)); xd = slice(max(0, -dx), W + min(0, -dx))
            nid[yd, xd] = pid[ys, xs]; nd[yd, xd] = depth[ys, xs]
            m = (alpha > 0) & (nid != pid) & (depth < nd)
            out[m] = e6[pid[m] >> 3]
        color = out
    return color, alpha, depth, pid


def to_rgb8(color6):
    return (np.asarray(color6) * 255 // 63).astype(np.uint8)
