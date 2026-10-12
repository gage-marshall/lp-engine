"""NitroSystem G3D (NSBMD/NSBTX) parser and texture decoder, written from the G3D binary format layouts (read-only reference).

Container: header{sig 'BMD0'/'BTX0', byteOrder 0xFEFF, version, fileSize, headerSize=16, nBlocks} + u32 block offsets; blocks start with
{kind 'MDL0'/'TEX0', size}. Dictionaries: {revision u8, numEntry u8, sizeDictBlk u16, dummy u16, ofsEntry u16, tree nodes[numEntry+1]*4B},
entry header at ofsEntry {sizeUnit u16, ofsName u16, data[numEntry*sizeUnit], names[numEntry]*16B}.
"""
import struct
from PIL import Image

U8, U16, U32, S16, S32 = (lambda d, o: d[o]), (lambda d, o: struct.unpack_from('<H', d, o)[0]), (lambda d, o: struct.unpack_from('<I', d, o)[0]), \
    (lambda d, o: struct.unpack_from('<h', d, o)[0]), (lambda d, o: struct.unpack_from('<i', d, o)[0])


def fx(v, shift=12): return v / (1 << shift)


def read_dict(d, o, nameless=False):
    """-> list of (name, data bytes) for the dictionary at offset o"""
    n = d[o + 1]; ofs_entry = U16(d, o + 6)
    e = o + ofs_entry; unit = U16(d, e); ofs_name = U16(d, e + 2)
    out = []
    for i in range(n):
        data = d[e + 4 + i * unit:e + 4 + (i + 1) * unit]
        name = d[e + ofs_name + i * 16:e + ofs_name + i * 16 + 16].split(b'\0')[0].decode('latin1')
        out.append((name, data))
    return out


def file_blocks(d):
    sig = d[:4][::-1].decode('latin1'); hsz, nb = U16(d, 12), U16(d, 14)
    offs = [U32(d, hsz + 4 * i) for i in range(nb)]
    return sig, {d[o:o + 4].decode('latin1'): (o, U32(d, o + 4)) for o in offs}


# ------------------------------------------------------------------ textures
def parse_tex0(d, o):
    """TEX0 block at o -> {'textures': [...], 'palettes': {name: (offset,size)}} with raw data refs"""
    base = o
    # NNSG3dResTex: header(8) texInfo(16) tex4x4Info(20) plttInfo(16) dict
    ti = base + 8
    size_tex, ofs_dict, _fl, _d, ofs_tex = struct.unpack_from('<HHHHI', d, ti + 4)
    t4 = base + 24
    size4, ofs_dict4, _f4, _d4, ofs_tex4, ofs_idx4 = struct.unpack_from('<HHHHII', d, t4 + 4)
    pi = base + 44
    size_pl, flag_pl, ofs_dict_pl, _d5, ofs_pl = struct.unpack_from('<HHHHI', d, pi + 4)
    texs = []
    for name, data in read_dict(d, base + ofs_dict):
        param, extra = struct.unpack('<II', data)
        texs.append({'name': name, 'param': param, 'extra': extra, 'addr': (param & 0xffff) << 3, 'w': 8 << ((param >> 20) & 7),
                     'h': 8 << ((param >> 23) & 7), 'fmt': (param >> 26) & 7, 'color0_transp': (param >> 29) & 1})
    plts = []
    for name, data in read_dict(d, base + ofs_dict_pl):
        off, flag = struct.unpack('<HH', data); plts.append({'name': name, 'offset': off << 3, 'flag': flag})
    return {'textures': texs, 'palettes': plts, 'ofs_tex': base + ofs_tex, 'ofs_tex4': base + ofs_tex4, 'ofs_idx4': base + ofs_idx4, 'ofs_pltt': base + ofs_pl,
            'size_pltt': size_pl << 3, 'size_tex': size_tex << 3, 'data': d}


def rgb555(c): return ((c & 31) * 255 // 31, ((c >> 5) & 31) * 255 // 31, ((c >> 10) & 31) * 255 // 31)


def decode_texture(tex0, tex, pal):
    """RGBA image for texture entry `tex` using palette entry `pal` (or None for direct color)"""
    d = tex0['data']; w, h, fmt = tex['w'], tex['h'], tex['fmt']
    pcols = None
    if pal is not None and 'colors' in pal:                 # explicit RGB555 palette (colour-variant override)
        pcols = list(pal['colors'])
    elif pal is not None:
        po = tex0['ofs_pltt'] + pal['offset']
        n = {2: 4, 3: 16, 4: 256, 1: 32, 6: 8}.get(fmt, 256)
        pcols = [struct.unpack_from('<H', d, po + 2 * i)[0] for i in range(n) if po + 2 * i + 2 <= len(d)]
    img = Image.new('RGBA', (w, h)); px = img.load(); base = tex0['ofs_tex'] + tex['addr']
    transp0 = tex['color0_transp']
    if fmt == 7:    # direct color A1B5G5R5
        for i in range(w * h):
            c = struct.unpack_from('<H', d, base + 2 * i)[0]; r, g, b = rgb555(c); px[i % w, i // w] = (r, g, b, 255 if c & 0x8000 else 0)
    elif fmt == 1:  # A3I5
        for i in range(w * h):
            v = d[base + i]; idx, a = v & 31, v >> 5; r, g, b = rgb555(pcols[idx]) if idx < len(pcols) else (255, 0, 255); px[i % w, i // w] = (r, g, b, (a * 255) // 7)
    elif fmt == 6:  # A5I3
        for i in range(w * h):
            v = d[base + i]; idx, a = v & 7, v >> 3; r, g, b = rgb555(pcols[idx]) if idx < len(pcols) else (255, 0, 255); px[i % w, i // w] = (r, g, b, (a * 255) // 31)
    elif fmt == 4:  # 256 colors
        for i in range(w * h):
            idx = d[base + i]; r, g, b = rgb555(pcols[idx]) if idx < len(pcols) else (255, 0, 255); px[i % w, i // w] = (r, g, b, 0 if (transp0 and idx == 0) else 255)
    elif fmt == 3:  # 16 colors
        for i in range(w * h):
            v = d[base + i // 2]; idx = (v >> 4) if i & 1 else (v & 15)
            r, g, b = rgb555(pcols[idx]) if idx < len(pcols) else (255, 0, 255); px[i % w, i // w] = (r, g, b, 0 if (transp0 and idx == 0) else 255)
    elif fmt == 2:  # 4 colors
        for i in range(w * h):
            v = d[base + i // 4]; idx = (v >> (2 * (i & 3))) & 3
            r, g, b = rgb555(pcols[idx]) if idx < len(pcols) else (255, 0, 255); px[i % w, i // w] = (r, g, b, 0 if (transp0 and idx == 0) else 255)
    elif fmt == 5:  # 4x4 compressed
        tb = tex0['ofs_tex4'] + tex['addr']; ib = tex0['ofs_idx4'] + (tex['addr'] >> 1)
        po = tex0['ofs_pltt'] + (pal['offset'] if pal else 0)
        for ty in range(h // 4):
            for tx in range(w // 4):
                bi = ty * (w // 4) + tx
                texel = struct.unpack_from('<I', d, tb + 4 * bi)[0]; pi_ = struct.unpack_from('<H', d, ib + 2 * bi)[0]
                mode, poff = pi_ >> 14, (pi_ & 0x3fff) * 4
                def col(i): return struct.unpack_from('<H', d, po + poff + 2 * i)[0]
                c0, c1 = rgb555(col(0)), rgb555(col(1))
                if mode == 0: cols = [c0 + (255,), c1 + (255,), rgb555(col(2)) + (255,), (0, 0, 0, 0)]
                elif mode == 1: cols = [c0 + (255,), c1 + (255,), tuple((a + b) // 2 for a, b in zip(c0, c1)) + (255,), (0, 0, 0, 0)]
                elif mode == 2: cols = [c0 + (255,), c1 + (255,), rgb555(col(2)) + (255,), rgb555(col(3)) + (255,)]
                else:
                    cols = [c0 + (255,), c1 + (255,), tuple((5 * a + 3 * b) // 8 for a, b in zip(c0, c1)) + (255,), tuple((3 * a + 5 * b) // 8 for a, b in zip(c0, c1)) + (255,)]
                for y in range(4):
                    for x in range(4):
                        px[tx * 4 + x, ty * 4 + y] = cols[(texel >> (2 * (y * 4 + x))) & 3]
    return img


# ------------------------------------------------------------------ model
def parse_mdl0(d, o):
    """MDL0 block (model set) at o -> list of models (dicts)"""
    models = []
    for name, data in read_dict(d, o + 8):
        mo = o + struct.unpack('<I', data)[0]   # offsets are relative to the MDL0 block start
        models.append(parse_model(d, mo, name))
    return models


def parse_model(d, mo, name):
    size, ofs_sbc, ofs_mat, ofs_shp, ofs_evp = struct.unpack_from('<5I', d, mo)
    ii = mo + 20
    sbc_type, scaling, texmtx, nnode, nmat, nshp, first_unused, _ = struct.unpack_from('<8B', d, ii)
    pos_scale, inv_pos_scale = struct.unpack_from('<ii', d, ii + 8)
    info = {'num_node': nnode, 'num_mat': nmat, 'num_shp': nshp, 'pos_scale': fx(pos_scale), 'scaling_rule': scaling, 'tex_mtx_mode': texmtx,
            'first_unused_mtx': first_unused, 'num_vertex': U16(d, ii + 16), 'num_polygon': U16(d, ii + 18)}
    ni = ii + 44   # node dictionary follows the fixed info: 8 (u8x8) + 8 + 8 (4 u16) + 12 (6 fx16) + 8
    nodes = []
    for nm, data in read_dict(d, ni):
        off = struct.unpack('<I', data)[0]; nodes.append(parse_node(d, ni + off, nm))
    mats = parse_materials(d, mo + ofs_mat)
    shapes = parse_shapes(d, mo + ofs_shp)
    return {'name': name, 'info': info, 'nodes': nodes, 'materials': mats, 'shapes': shapes, 'sbc_off': mo + ofs_sbc, 'mdl_off': mo,
            'sbc_end': mo + ofs_mat, 'evp_off': mo + ofs_evp if ofs_evp else None}


def parse_node(d, o, name):
    flag = U16(d, o); m00 = S16(d, o + 2); p = o + 4
    t = (0.0, 0.0, 0.0); s = (1.0, 1.0, 1.0)
    if not flag & 1: t = tuple(fx(S32(d, p + 4 * i)) for i in range(3)); p += 12
    rot = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
    if not flag & 2:
        if not flag & 8:
            v = [fx(S16(d, p + 2 * i)) for i in range(8)]; p += 16
            rot = [[fx(m00), v[0], v[1]], [v[2], v[3], v[4]], [v[5], v[6], v[7]]]
        else:
            A, B = fx(S16(d, p)), fx(S16(d, p + 2)); p += 4
            piv = (flag & 0xf0) >> 4; minus = -1.0 if flag & 0x100 else 1.0
            C = -B if flag & 0x200 else B; D = -A if flag & 0x400 else A
            rot = [[0.0] * 3 for _ in range(3)]
            pr, pc = divmod(piv, 3); rot[pr][pc] = minus
            rows = [r for r in range(3) if r != pr]; cols = [c for c in range(3) if c != pc]
            rot[rows[0]][cols[0]] = A; rot[rows[0]][cols[1]] = B; rot[rows[1]][cols[0]] = C; rot[rows[1]][cols[1]] = D
    if not flag & 4: s = tuple(fx(S32(d, p + 4 * i)) for i in range(3)); p += 24
    return {'name': name, 'flag': flag, 'trans': t, 'rot': rot, 'scale': s}


def parse_materials(d, o):
    ofs_t2m, ofs_p2m = U16(d, o), U16(d, o + 2)
    mats = []
    for name, data in read_dict(d, o + 4):
        mo = o + struct.unpack('<I', data)[0]
        tag, size, diffamb, specemi, polyattr, polymask, teximg, texmask, plttbase, flag, ow, oh, magw, magh = struct.unpack_from('<HHIIIIIIHHHHii', d, mo)
        mats.append({'name': name, 'diffuse': diffamb & 0x7fff, 'ambient': (diffamb >> 16) & 0x7fff, 'polyattr': polyattr, 'alpha': (polyattr >> 16) & 31,
                     'cull': (polyattr >> 6) & 3, 'tex_param': teximg, 'pltt_base': plttbase, 'flag': flag, 'orig_w': ow, 'orig_h': oh, 'tex': None, 'pal': None})
    # texture/palette bindings: name -> list of material indices
    for key, ofs in (('tex', ofs_t2m), ('pal', ofs_p2m)):
        for nm, data in read_dict(d, o + ofs):
            off, num, _fl = struct.unpack('<HBB', data)
            for i in range(num):
                mats[d[o + off + i]][key] = nm
    return mats


def parse_shapes(d, o):
    out = []
    for name, data in read_dict(d, o):
        so = o + struct.unpack('<I', data)[0]
        tag, size, flag, ofs_dl, size_dl = struct.unpack_from('<HHIII', d, so)
        out.append({'name': name, 'flag': flag, 'dl_off': so + ofs_dl, 'dl_size': size_dl})
    return out


def parse_file(d):
    sig, blocks = file_blocks(d)
    res = {'sig': sig, 'models': [], 'tex0': None}
    if 'MDL0' in blocks: res['models'] = parse_mdl0(d, blocks['MDL0'][0])
    if 'TEX0' in blocks: res['tex0'] = parse_tex0(d, blocks['TEX0'][0])
    return res


# ------------------------------------------------------------------ geometry: display lists + SBC
PARAMS = {0x00: 0, 0x10: 1, 0x11: 1, 0x12: 1, 0x13: 1, 0x14: 1, 0x15: 0, 0x16: 16, 0x17: 12, 0x18: 16, 0x19: 12, 0x1a: 9, 0x1b: 3, 0x1c: 3,
          0x20: 1, 0x21: 1, 0x22: 1, 0x23: 2, 0x24: 1, 0x25: 1, 0x26: 1, 0x27: 1, 0x28: 1, 0x29: 1, 0x2a: 1, 0x2b: 1, 0x30: 1, 0x31: 1, 0x32: 1,
          0x33: 1, 0x34: 32, 0x40: 1, 0x41: 0, 0x50: 1, 0x60: 1, 0x70: 3, 0x71: 2, 0x72: 1}


def s10(v): return v - 1024 if v & 0x200 else v


def decode_dl(dl, restore=None):
    """Decode a GX display list -> list of polygons. Each vertex: dict(pos=(x,y,z) model units, nrm, uv, col, mtx=stack index or None).
    restore: unused hook. Primitive types: 0 tris, 1 quads, 2 tri strip, 3 quad strip (output as triangles)."""
    polys = []; p = 0; n = len(dl)
    cur = {'nrm': (0.0, 0.0, 1.0), 'uv': (0.0, 0.0), 'col': (1.0, 1.0, 1.0), 'mtx': None}
    pos = [0.0, 0.0, 0.0]; mode = None; verts = []
    def emit():
        nonlocal verts
        if mode is None or not verts: verts = []; return
        v = verts
        if mode == 0:
            for i in range(0, len(v) - 2, 3): polys.append([v[i], v[i + 1], v[i + 2]])
        elif mode == 1:
            for i in range(0, len(v) - 3, 4): polys.append([v[i], v[i + 1], v[i + 2]]); polys.append([v[i], v[i + 2], v[i + 3]])
        elif mode == 2:
            for i in range(len(v) - 2): polys.append([v[i], v[i + 1], v[i + 2]] if i % 2 == 0 else [v[i + 1], v[i], v[i + 2]])
        elif mode == 3:
            for i in range(0, len(v) - 3, 2):
                a, b, c, d = v[i], v[i + 1], v[i + 3], v[i + 2]; polys.append([a, b, c]); polys.append([a, c, d])
        verts = []
    def vtx():
        verts.append({'pos': tuple(pos), 'nrm': cur['nrm'], 'uv': cur['uv'], 'col': cur['col'], 'mtx': cur['mtx']})
    while p + 4 <= n:
        cmds = dl[p:p + 4]; p += 4
        for c in cmds:
            if c == 0 and False: continue
            k = PARAMS.get(c)
            if k is None: continue
            if c == 0: continue
            if p + 4 * k > n: p = n; break
            w = struct.unpack_from('<%dI' % k, dl, p) if k else (); p += 4 * k
            if c == 0x14: cur['mtx'] = w[0] & 31
            elif c == 0x20: v = w[0]; cur['col'] = ((v & 31) / 31, ((v >> 5) & 31) / 31, ((v >> 10) & 31) / 31)
            elif c == 0x21: v = w[0]; cur['nrm'] = (s10(v & 0x3ff) / 512, s10((v >> 10) & 0x3ff) / 512, s10((v >> 20) & 0x3ff) / 512)
            elif c == 0x22:
                s, t = struct.unpack('<hh', struct.pack('<I', w[0])); cur['uv'] = (s / 16, t / 16)
            elif c == 0x23:
                x, y = struct.unpack('<hh', struct.pack('<I', w[0])); z = struct.unpack('<h', struct.pack('<I', w[1])[:2])[0]
                pos[:] = [x / 4096, y / 4096, z / 4096]; vtx()
            elif c == 0x24:
                v = w[0]; pos[:] = [s10(v & 0x3ff) / 64, s10((v >> 10) & 0x3ff) / 64, s10((v >> 20) & 0x3ff) / 64]; vtx()
            elif c == 0x25:
                x, y = struct.unpack('<hh', struct.pack('<I', w[0])); pos[0], pos[1] = x / 4096, y / 4096; vtx()
            elif c == 0x26:
                x, z = struct.unpack('<hh', struct.pack('<I', w[0])); pos[0], pos[2] = x / 4096, z / 4096; vtx()
            elif c == 0x27:
                y, z = struct.unpack('<hh', struct.pack('<I', w[0])); pos[1], pos[2] = y / 4096, z / 4096; vtx()
            elif c == 0x28:
                v = w[0]; pos[0] += s10(v & 0x3ff) / 4096; pos[1] += s10((v >> 10) & 0x3ff) / 4096; pos[2] += s10((v >> 20) & 0x3ff) / 4096; vtx()
            elif c == 0x40: emit(); mode = w[0] & 3
            elif c == 0x41: emit(); mode = None
    emit()
    return polys


# matrices: (R(3x3 row-major list of lists), t(3)) meaning p' = R*p + t
IDENT = ([[1, 0, 0], [0, 1, 0], [0, 0, 1]], [0, 0, 0])


def mat_mul(a, b):
    Ra, ta = a; Rb, tb = b
    R = [[sum(Ra[i][k] * Rb[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    t = [sum(Ra[i][k] * tb[k] for k in range(3)) + ta[i] for i in range(3)]
    return (R, t)


def mat_apply(m, p):
    R, t = m; return tuple(R[i][0] * p[0] + R[i][1] * p[1] + R[i][2] * p[2] + t[i] for i in range(3))


def mat_apply_dir(m, p):
    R, _ = m; return tuple(R[i][0] * p[0] + R[i][1] * p[1] + R[i][2] * p[2] for i in range(3))


def node_matrix(node, transpose_rot=False):
    R = node['rot']; S = node['scale']
    if transpose_rot: R = [[R[j][i] for j in range(3)] for i in range(3)]
    RS = [[R[i][j] * S[j] for j in range(3)] for i in range(3)]
    return (RS, list(node['trans']))


def run_sbc(d, model, transpose_rot=True, nodes=None, info=None, vis=None):
    """Execute the model's structure byte code; returns draw list
    [(shape_idx, mat_idx, node_idx, stack_matrices, cur_matrix, stack_prov, cur_prov)].
    nodes: optional per-node SRT overrides (animation); info (dict) receives 'world' {node: matrix} and 'parent' {node: parent}.
    prov = list of (node, weight): which joints (and how strongly) the matrix follows."""
    p = model['sbc_off']; end = model['sbc_end']
    stack = [IDENT] * 32; sprov = [[] for _ in range(32)]; cur = IDENT; cprov = []; node = 0; mat = 0; draws = []
    nodes = nodes or model['nodes']; evp = model.get('evp')
    world = {}; parent = {}; node_vis = {}
    while p < end:
        b = d[p]; cmd = b & 31; opt = b >> 5; p += 1
        if cmd == 0: continue
        if cmd == 1: break
        if cmd == 2: node = d[p]; node_vis[node] = bool(d[p + 1] & 1); p += 2
        elif cmd == 3: i_ = d[p] & 31; cur = stack[i_]; cprov = sprov[i_]; p += 1
        elif cmd == 4: mat = d[p]; p += 1
        elif cmd == 5:
            shown = vis(node) if vis is not None else node_vis.get(node, True)
            if shown: draws.append((d[p], mat, node, list(stack), cur, list(sprov), cprov))
            p += 1
        elif cmd == 6:
            nid, pid, fl = d[p], d[p + 1], d[p + 2]; p += 3
            dest = src = None
            if opt in (1, 3): dest = d[p] & 31; p += 1
            if opt in (2, 3): src = d[p] & 31; p += 1
            if src is not None: cur = stack[src]
            cur = mat_mul(cur, node_matrix(nodes[nid], transpose_rot)); cprov = [(nid, 1.0)]
            world[nid] = cur; parent[nid] = pid if pid != nid and pid < len(nodes) else None
            if dest is not None: stack[dest] = cur; sprov[dest] = cprov
        elif cmd in (7, 8):
            p += 1
            if opt in (1, 3): p += 1
            if opt in (2, 3): p += 1
        elif cmd == 9:
            dest, num = d[p], d[p + 1]; p += 2; blend = None; pr = []
            for _ in range(num):
                src, nid, ratio = d[p], d[p + 1], d[p + 2]; p += 3
                m = stack[src & 31]
                if evp is not None: m = mat_mul(m, evp[nid])
                r = ratio / 256; pr.append((nid, r))
                part = ([[m[0][i][j] * r for j in range(3)] for i in range(3)], [m[1][i] * r for i in range(3)])
                blend = part if blend is None else ([[blend[0][i][j] + part[0][i][j] for j in range(3)] for i in range(3)], [blend[1][i] + part[1][i] for i in range(3)])
            stack[dest & 31] = blend or IDENT; sprov[dest & 31] = pr
            cur = stack[dest & 31]; cprov = pr                       # the blend is also loaded as the engine's current matrix
        elif cmd == 0x0a: p += 8
        elif cmd == 0x0b: pass
        elif cmd in (0x0c, 0x0d): p += 2
        else: break
    if info is not None: info['world'] = world; info['parent'] = parent
    return draws


def read_evp(d, model):
    """envelope inverse matrices (MtxFx43 invM + MtxFx33 invN per node): invM as (R, t) in p' = R*p + t form"""
    if not model['evp_off']: return None
    o = model['evp_off']; out = []
    if o + 84 * model['info']['num_node'] > len(d): return None
    for i in range(model['info']['num_node']):
        v = struct.unpack_from('<12i', d, o + i * 84)           # 4x3 matrix: 12 fx32, followed by 3x3 invN (9 fx32)
        m = [x / 4096 for x in v]
        R = [[m[0], m[3], m[6]], [m[1], m[4], m[7]], [m[2], m[5], m[8]]]
        out.append((R, [m[9], m[10], m[11]]))
    return out


def build_mesh(d, model, tex0=None, transpose_rot=True, nodes=None, skin=False, vis=None):
    """Triangles baked to model space for the given node pose (bind pose when nodes is None):
    list of dict(material=idx, verts=[(pos, nrm, uv, col, joints)*3]) (joints only filled when skin=True)."""
    model['evp'] = read_evp(d, model)
    ps = model['info']['pos_scale']; tris = []
    for shp_i, mat_i, node_i, stack, cur, sprov, cprov in run_sbc(d, model, transpose_rot, nodes, None, vis):
        shp = model['shapes'][shp_i]
        dl = d[shp['dl_off']:shp['dl_off'] + shp['dl_size']]
        for poly in decode_dl(dl):
            tri = []
            for v in poly:
                m = stack[v['mtx']] if v['mtx'] is not None else cur
                pos = mat_apply(m, tuple(c * ps for c in v['pos'])); nrm = mat_apply_dir(m, v['nrm'])
                joints = (sprov[v['mtx']] if v['mtx'] is not None else cprov) if skin else None
                tri.append((pos, nrm, v['uv'], v['col'], joints))
            tris.append({'material': mat_i, 'verts': tri})
    return tris


# ------------------------------------------------------------------ texture binding + preview renderer
def index_textures(dirpath):
    """all BTX0 textures in a directory -> {texture_name: (tex0, tex_entry, {pal_name: pal_entry})}"""
    import os
    idx = {}
    for f in sorted(os.listdir(dirpath)):
        p = os.path.join(dirpath, f)
        if not os.path.isfile(p): continue
        d = open(p, 'rb').read()
        if d[:4] != b'BTX0': continue
        try: t = parse_file(d)['tex0']
        except Exception: continue
        if not t: continue
        pals = {x['name']: x for x in t['palettes']}
        for tx in t['textures']: idx.setdefault(tx['name'], (t, tx, pals))
    return idx


def material_image(mat, tex_index, cache):
    if mat.get('image') is not None: return mat['image']        # resolved override (variant texture / recoloured palette)
    if not mat.get('tex') or mat['tex'] not in tex_index: return None
    key = (mat['tex'], mat.get('pal'))
    if key in cache: return cache[key]
    t, tx, pals = tex_index[mat['tex']]
    pal = pals.get(mat.get('pal')) if mat.get('pal') else None
    try: img = decode_texture(t, tx, pal)
    except Exception: img = None
    cache[key] = img; return img


def render_preview(tris, materials, tex_index, size=400, yaw=0.0, pitch=0.0, bg=(40, 40, 50)):
    """Tiny painter's-algorithm software renderer for visual checks (orthographic)."""
    import math
    cache = {}; pts = [v[0] for t in tris for v in t['verts']]
    if not pts: return Image.new('RGB', (size, size), bg)
    cx = sum(p[0] for p in pts) / len(pts); cy = sum(p[1] for p in pts) / len(pts); cz = sum(p[2] for p in pts) / len(pts)
    cyw, syw, cp, sp = math.cos(yaw), math.sin(yaw), math.cos(pitch), math.sin(pitch)
    def proj(p):
        x, y, z = p[0] - cx, p[1] - cy, p[2] - cz
        x, z = x * cyw + z * syw, -x * syw + z * cyw
        y, z = y * cp - z * sp, y * sp + z * cp
        return x, -y, z
    pp = [proj(p) for p in pts]
    ext = max(max(abs(p[0]) for p in pp), max(abs(p[1]) for p in pp)) or 1; sc = size * 0.45 / ext
    img = Image.new('RGB', (size, size), bg); px = img.load()
    order = []
    for t in tris:
        pr = [proj(v[0]) for v in t['verts']]; order.append((sum(p[2] for p in pr) / 3, t, pr))
    order.sort(key=lambda a: -a[0])
    for _, t, pr in order:
        mat = materials[t['material']]; tim = material_image(mat, tex_index, cache); tp = tim.load() if tim else None
        sx = [size / 2 + p[0] * sc for p in pr]; sy = [size / 2 + p[1] * sc for p in pr]
        x0, x1 = max(0, int(min(sx))), min(size - 1, int(max(sx)) + 1); y0, y1 = max(0, int(min(sy))), min(size - 1, int(max(sy)) + 1)
        den = (sy[1] - sy[2]) * (sx[0] - sx[2]) + (sx[2] - sx[1]) * (sy[0] - sy[2])
        if abs(den) < 1e-9: continue
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                a = ((sy[1] - sy[2]) * (x - sx[2]) + (sx[2] - sx[1]) * (y - sy[2])) / den
                b = ((sy[2] - sy[0]) * (x - sx[2]) + (sx[0] - sx[2]) * (y - sy[2])) / den; c = 1 - a - b
                if a < -1e-4 or b < -1e-4 or c < -1e-4: continue
                if tp:
                    u = a * t['verts'][0][2][0] + b * t['verts'][1][2][0] + c * t['verts'][2][2][0]; v = a * t['verts'][0][2][1] + b * t['verts'][1][2][1] + c * t['verts'][2][2][1]
                    ix = int(u) % tim.width; iy = int(v) % tim.height; col = tp[ix, iy]
                    if col[3] < 128: continue
                    px[x, y] = col[:3]
                else:
                    cc = [int(255 * (a * t['verts'][0][3][k] + b * t['verts'][1][3][k] + c * t['verts'][2][3][k])) for k in range(3)]
                    px[x, y] = tuple(cc)
    return img


# ------------------------------------------------------------------ joint animation (NSBCA / 'BCA0')
def _fx_arr(d, off, n, fx16):
    return list(struct.unpack_from('<%d%s' % (n, 'h' if fx16 else 'i'), d, off))


def _key_index(frame, step, last_interp):
    """-> (i, t, hold): which key(s) to read for integer frame; hold=True means use data[i] only"""
    if step == 1: return frame, 0.0, True
    if frame > last_interp: return (last_interp // step) + (frame & (step - 1)), 0.0, True
    return frame // step, (frame % step) / step, frame % step == 0


def _step_of(info):
    return 2 if info & 0x40000000 else (4 if info & 0x80000000 else 1)


def _num_keys(info, nframes):
    step = _step_of(info); last = (info & 0x1fff0000) >> 16
    return nframes if step == 1 else (last // step) + 1 + (nframes - 1 - last)


def _rot_from_idx(d, ja, ofs_rot3, ofs_rot5, idx):
    if idx & 0x8000:                                  # compressed pivot form (6 bytes)
        o = ja + ofs_rot3 + (idx & 0x7fff) * 6
        info, A, B = struct.unpack_from('<Hhh', d, o); A /= 4096; B /= 4096
        piv = info & 15; r = [[0.0] * 3 for _ in range(3)]
        pr, pc = divmod(piv, 3); r[pr][pc] = -1.0 if info & 0x10 else 1.0
        rows = [x for x in range(3) if x != pr]; cols = [x for x in range(3) if x != pc]
        r[rows[0]][cols[0]] = A; r[rows[0]][cols[1]] = B
        r[rows[1]][cols[0]] = -B if info & 0x20 else B; r[rows[1]][cols[1]] = -A if info & 0x40 else A
        return r
    o = ja + ofs_rot5 + (idx & 0x7fff) * 10               # 5-value form; third row by cross product
    v = struct.unpack_from('<5h', d, o)
    m12 = v[4] & 7; m11 = v[4] >> 3
    m12 = ((m12 << 3) | (v[0] & 7)); m00 = v[0] >> 3
    m12 = ((m12 << 3) | (v[1] & 7)); m01 = v[1] >> 3
    m12 = ((m12 << 3) | (v[2] & 7)); m02 = v[2] >> 3
    m12 = ((m12 << 3) | (v[3] & 7)); m10 = v[3] >> 3
    m12 = m12 & 0x1fff
    if m12 & 0x1000: m12 -= 0x2000                        # signed 13-bit (low 13 bits of the assembled value)
    r0 = [m00 / 4096, m01 / 4096, m02 / 4096]; r1 = [m10 / 4096, m11 / 4096, m12 / 4096]
    r2 = [r0[1] * r1[2] - r0[2] * r1[1], r0[2] * r1[0] - r0[0] * r1[2], r0[0] * r1[1] - r0[1] * r1[0]]
    return [r0, r1, r2]


def parse_bca(d):
    """-> list of animations: {'name', 'frames', 'tracks': {node: dict(...)}} ; sample with sample_anim()"""
    sig, blocks = file_blocks(d); o = blocks['JNT0'][0]; anims = []
    for name, data in read_dict(d, o + 8):
        ja = o + struct.unpack('<I', data)[0]
        nframes, nnode, flag, ofs3, ofs5 = struct.unpack_from('<HHIII', d, ja + 4)
        tags = []
        for i in range(nnode):
            to = ja + U16(d, ja + 20 + 2 * i); tag = U32(d, to); p = to + 4; tr = {'node': tag >> 24, 'flag': tag}
            if not tag & 1:
                for ax, (idn, bs, cst) in zip('xyz', ((0x2, 0x4, 0x8), (0x2, 0x4, 0x10), (0x2, 0x4, 0x20))):
                    pass
                if not tag & 0x6:
                    tr['t'] = []
                    for cbit in (0x8, 0x10, 0x20):
                        if tag & cbit: tr['t'].append(('const', S32(d, p) / 4096)); p += 4
                        else: tr['t'].append(('anim', U32(d, p), U32(d, p + 4))); p += 8
                if not tag & 0xc0:
                    if tag & 0x100: tr['r'] = ('const', U32(d, p)); p += 4
                    else: tr['r'] = ('anim', U32(d, p), U32(d, p + 4)); p += 8
                if not tag & 0x600:
                    tr['s'] = []
                    for cbit in (0x800, 0x1000, 0x2000):
                        if tag & cbit: tr['s'].append(('const', S32(d, p) / 4096)); p += 8
                        else: tr['s'].append(('anim', U32(d, p), U32(d, p + 4))); p += 8
            tags.append(tr)
        anims.append({'name': name, 'frames': nframes, 'tags': tags, 'ja': ja, 'ofs3': ofs3, 'ofs5': ofs5, 'd': d})
    return anims


def _sample_scalar(d, ja, spec, frame, nframes, scale_pair=False):
    if spec[0] == 'const': return spec[1]
    _, info, off = spec; fx16 = bool(info & 0x20000000); step = _step_of(info); last = (info & 0x1fff0000) >> 16
    n = _num_keys(info, nframes) * (2 if scale_pair else 1)
    arr = _fx_arr(d, ja + off, n, fx16); stride = 2 if scale_pair else 1
    frame = min(frame, nframes - 1)
    i, t, hold = _key_index(frame, step, last)
    a = arr[i * stride] / 4096
    if hold or i * stride + stride >= len(arr): return a
    b = arr[(i + 1) * stride] / 4096
    return a + (b - a) * t


def _sample_rot(d, ja, ofs3, ofs5, spec, frame, nframes):
    if spec[0] == 'const': return _rot_from_idx(d, ja, ofs3, ofs5, spec[1] & 0xffff)
    _, info, off = spec; step = _step_of(info); last = (info & 0x1fff0000) >> 16
    n = _num_keys(info, nframes); idxs = list(struct.unpack_from('<%dH' % n, d, ja + off))
    frame = min(frame, nframes - 1); i, t, hold = _key_index(frame, step, last)
    a = _rot_from_idx(d, ja, ofs3, ofs5, idxs[i])
    if hold or i + 1 >= len(idxs): return a
    b = _rot_from_idx(d, ja, ofs3, ofs5, idxs[i + 1])
    return [[a[r][c] + (b[r][c] - a[r][c]) * t for c in range(3)] for r in range(3)]


def sample_anim(anim, model_nodes, frame):
    """Per-node SRT dicts (same shape as model nodes) at integer frame; nodes without a track keep the model's pose."""
    d = anim['d']; ja = anim['ja']; nf = anim['frames']
    out = [dict(n) for n in model_nodes]
    for tr in anim['tags']:
        nid = tr['node']
        if nid >= len(out): continue
        tag = tr['flag']; node = out[nid]
        if tag & 1:
            node['trans'] = (0.0, 0.0, 0.0); node['rot'] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]; node['scale'] = (1.0, 1.0, 1.0); continue
        if tag & 0x2: node['trans'] = (0.0, 0.0, 0.0)
        elif 't' in tr: node['trans'] = tuple(_sample_scalar(d, ja, sp, frame, nf) for sp in tr['t'])
        if tag & 0x40: node['rot'] = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
        elif 'r' in tr: node['rot'] = _sample_rot(d, ja, anim['ofs3'], anim['ofs5'], tr['r'], frame, nf)
        if tag & 0x200: node['scale'] = (1.0, 1.0, 1.0)
        elif 's' in tr: node['scale'] = tuple(_sample_scalar(d, ja, sp, frame, nf, scale_pair=True) for sp in tr['s'])
    return out


# ------------------------------------------------------------------ visibility animation (NSBVA / 'BVA0') + name-based retargeting
def parse_bva(d):
    """-> list of {'name', 'frames', 'nodes', 'bits'}; visible(anim, frame, node) reads bit frame*nodes+node (little-endian)"""
    sig, blocks = file_blocks(d); o = blocks['VIS0'][0]; out = []
    for name, data in read_dict(d, o + 8):
        va = o + struct.unpack('<I', data)[0]
        nframes, nnodes, size = struct.unpack_from('<HHH', d, va + 4)
        nwords = max(1, (size - 12) // 4) if size >= 16 else 1 + ((nframes * nnodes) >> 5)
        nwords = min(nwords, (len(d) - (va + 12)) // 4)
        out.append({'name': name, 'frames': nframes, 'nodes': nnodes, 'bits': struct.unpack_from('<%dI' % nwords, d, va + 12)})
    return out


def visible(anim, frame, node):
    if node >= anim['nodes']: return True
    i = min(frame, anim['frames'] - 1) * anim['nodes'] + node
    return bool((anim['bits'][i >> 5] >> (i & 31)) & 1)


def sample_anim_named(anim, src_names, model_nodes, frame):
    """Like sample_anim, but the animation's tracks are matched to model joints by NAME (src_names = joint names of the
    skeleton the animation was authored for). Joints without a matching track keep the model's pose."""
    d = anim['d']; ja = anim['ja']; nf = anim['frames']; out = [dict(n) for n in model_nodes]
    by_name = {n['name']: i for i, n in enumerate(model_nodes)}
    for tr in anim['tags']:
        if tr['node'] >= len(src_names): continue
        tgt = by_name.get(src_names[tr['node']])
        if tgt is None: continue
        sub = dict(anim); sub['tags'] = [dict(tr, node=tgt)]
        res = sample_anim(sub, model_nodes, frame)
        out[tgt] = res[tgt]
    return out
