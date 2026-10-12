"""Reference toon + outline renderer for assembled heroine meshes (numpy z-buffer; orthographic).

Reproduces the DS fixed-function look the game uses (rules verified from the game's loaders and the model data):
  * polygon mode "toon" (polyattr bits 4-5 == 2): per-vertex light intensity (R channel, 0..31) indexes the 32-entry toon table; the
    table colour replaces the lit colour and modulates the texture. The toon table is a per-time-of-day tint ramp (day/evening/night).
  * polyattr bits 0-3 enable lights 0-3; light vectors/colours come from the camera preset (3 lights: yaw/pitch degrees) and the fixed
    colour constants 0x7fff, 0x4210, 0x2108.
  * outline = DS edge marking: a pixel whose 4-neighbour belongs to a different polygon ID (or background) and that is not behind it gets
    edge_colour[polygon_id >> 3].
Inputs are the structures produced by nns3d.build_mesh / heroine.Wardrobe.build.
"""
import math
import numpy as np
from PIL import Image
import nns3d as N

LIGHT_COLORS = (0x7fff, 0x4210, 0x2108, 0x7fff)


def rgb555_to_f(c): return ((c & 31) / 31.0, ((c >> 5) & 31) / 31.0, ((c >> 10) & 31) / 31.0)


def light_vector(yaw_deg, pitch_deg):
    """Direction the light travels, in view space (DS convention: camera looks down -z). Rotation order follows the game:
    start (0,0,-1), row-vector rotate by Y then by X (SDK RotY/RotX33 matrices)."""
    y, p = math.radians(yaw_deg), math.radians(pitch_deg)
    v = (0.0, 0.0, -1.0)
    x, z = v[2] * math.sin(y), v[2] * math.cos(y)                     # RotY33: x = z*m20, z = z*m22 with m20 = sin, m22 = cos
    v = (x, 0.0, z)
    yy, zz = v[2] * -math.sin(p), v[2] * math.cos(p)                  # RotX33: y = z*m21 (-sin), z = z*m22 (cos)
    return (v[0], yy, zz)


def _toon_index(nrm, mat, lights, enable):
    """5-bit toon index for one vertex (DS lighting, R channel only: only R is used as the toon index)."""
    diff_r = (mat.get('diffuse', 0x7fff) & 31) / 31.0; amb_r = ((mat.get('ambient', 0) or 0) & 31) / 31.0
    acc = 0.0
    for i, (lv, lc) in enumerate(lights):
        if not (enable >> i) & 1: continue
        d = max(0.0, -(nrm[0] * lv[0] + nrm[1] * lv[1] + nrm[2] * lv[2]))
        acc += (diff_r * d + amb_r) * (lc & 31) / 31.0
    return min(31, int(acc * 31 + 0.5))


def render(tris, materials, tex_index, toon, edge, lights, size=512, yaw=math.pi, pitch=0.0, center=None, scale=None, bg=(46, 46, 58),
           ss=2, edge_px=1, outline=True, shade=True):
    """-> PIL RGB image. toon: 32 RGB555 ints; edge: 8 RGB555 ints; lights: [(yaw_deg, pitch_deg)] * 3 (view space)."""
    cyw, syw, cp, sp = math.cos(yaw), math.sin(yaw), math.cos(pitch), math.sin(pitch)
    lv = [(light_vector(a, b), LIGHT_COLORS[i]) for i, (a, b) in enumerate(lights)]
    pts = np.array([v[0] for t in tris for v in t['verts']], dtype=np.float64)
    if center is None: center = pts.mean(axis=0) if len(pts) else np.zeros(3)

    def proj(p):
        x, y, z = p[..., 0] - center[0], p[..., 1] - center[1], p[..., 2] - center[2]
        x, z = x * cyw + z * syw, -x * syw + z * cyw
        y, z = y * cp - z * sp, y * sp + z * cp
        return np.stack([x, -y, z], axis=-1)

    def view_dir(n):                       # model-space direction -> DS view space (camera on +z looking -z)
        x, y, z = n; x, z = x * cyw + z * syw, -x * syw + z * cyw; y, z = y * cp - z * sp, y * sp + z * cp
        return (-x, y, -z)
    pp = proj(pts) if len(pts) else pts
    if scale is None:
        ext = max(np.abs(pp[:, 0]).max(), np.abs(pp[:, 1]).max()) if len(pp) else 1.0; scale = size * 0.45 / (ext or 1.0)
    S = size * ss; sc = scale * ss
    col = np.zeros((S, S, 3), np.float32); col[:] = bg
    zb = np.full((S, S), np.inf, np.float32); idb = np.zeros((S, S), np.int16)
    toon_rgb = np.array([rgb555_to_f(c) for c in toon], np.float32); cache = {}; texnp = {}

    def tex_arr(mat):
        k = (mat.get('tex'), mat.get('pal'))
        if k not in texnp:
            im = N.material_image(mat, tex_index, cache); texnp[k] = None if im is None else np.asarray(im.convert('RGBA'))
        return texnp[k]
    k0 = 0
    for t in tris:
        mat = materials[t['material']]; attr = mat.get('polyattr', 0) or 0
        enable, mode, pid, alpha = attr & 15, (attr >> 4) & 3, (attr >> 24) & 63, (attr >> 16) & 31
        if alpha == 0: continue
        P = proj(np.array([v[0] for v in t['verts']], dtype=np.float64))
        sx = S / 2 + P[:, 0] * sc; sy = S / 2 + P[:, 1] * sc; sz = P[:, 2]
        x0, x1 = max(0, int(sx.min())), min(S - 1, int(sx.max()) + 1); y0, y1 = max(0, int(sy.min())), min(S - 1, int(sy.max()) + 1)
        if x1 < x0 or y1 < y0: continue
        den = (sy[1] - sy[2]) * (sx[0] - sx[2]) + (sx[2] - sx[1]) * (sy[0] - sy[2])
        if abs(den) < 1e-9: continue
        cull = mat.get('cull', 0)                                           # 0 none, 1 front, 2 back culled (DS: 2 = render front only)
        wind = den > 0                                                      # sign in screen space (y down)
        if cull == 2 and not wind: continue
        if cull == 1 and wind: continue
        xs, ys = np.meshgrid(np.arange(x0, x1 + 1), np.arange(y0, y1 + 1)); xs = xs + 0.5; ys = ys + 0.5
        a = ((sy[1] - sy[2]) * (xs - sx[2]) + (sx[2] - sx[1]) * (ys - sy[2])) / den
        b = ((sy[2] - sy[0]) * (xs - sx[2]) + (sx[0] - sx[2]) * (ys - sy[2])) / den; c = 1 - a - b
        m = (a >= -1e-4) & (b >= -1e-4) & (c >= -1e-4)
        if not m.any(): continue
        z = a * sz[0] + b * sz[1] + c * sz[2]
        sub = zb[y0:y1 + 1, x0:x1 + 1]; m &= z < sub
        if not m.any(): continue
        # vertex colours: toon index per vertex, or plain lit/vertex colour
        if shade and enable:
            idx = [_toon_index(view_dir(v[1]), mat, lv, enable) for v in t['verts']]
            if mode == 2: vc = toon_rgb[idx]                                 # (3,3) toon table colours
            else: vc = np.array([[idx[i] / 31.0] * 3 for i in range(3)], np.float32)
        else: vc = np.array([v[3] for v in t['verts']], np.float32)
        rgb = a[..., None] * vc[0] + b[..., None] * vc[1] + c[..., None] * vc[2]
        ta = tex_arr(mat)
        if ta is not None:
            uv = [v[2] for v in t['verts']]
            u = a * uv[0][0] + b * uv[1][0] + c * uv[2][0]; v_ = a * uv[0][1] + b * uv[1][1] + c * uv[2][1]
            h, w = ta.shape[:2]; tx = np.floor(u).astype(int) % w; ty = np.floor(v_).astype(int) % h
            texel = ta[ty, tx]; m &= texel[..., 3] >= 128
            rgb = rgb * texel[..., :3].astype(np.float32) / 255.0
        else:
            rgb = rgb * 1.0
        if not m.any(): continue
        reg = col[y0:y1 + 1, x0:x1 + 1]; reg[m] = np.clip(rgb[m] * 255.0 + 0.5, 0, 255); sub[m] = z[m]; idb[y0:y1 + 1, x0:x1 + 1][m] = pid + 1
    if outline:
        ec = np.array([rgb555_to_f(c) for c in edge], np.float32) * 255.0
        mark = np.zeros((S, S), bool)
        for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
            nid = np.roll(idb, (dy, dx), axis=(0, 1)); nz = np.roll(zb, (dy, dx), axis=(0, 1))
            mark |= (idb > 0) & (nid != idb) & (zb <= nz + 1e-3)
        if edge_px > 1:
            for _ in range(edge_px - 1):                                     # grow the mark set into polygon pixels only
                g = mark.copy()
                for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)): g |= np.roll(mark, (dy, dx), axis=(0, 1)) & (idb > 0)
                mark = g
        ids = np.clip((idb.astype(np.int32) - 1) >> 3, 0, 7); col[mark] = ec[ids[mark]]
    img = Image.fromarray(np.clip(col, 0, 255).astype(np.uint8), 'RGB')
    return img.resize((size, size), Image.LANCZOS) if ss > 1 else img
