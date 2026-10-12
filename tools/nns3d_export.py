"""Export NitroSystem models (NSBMD + textures from NSBTX in the same archive directory) to glTF binary (.glb), bind pose.

    nns3d_export.py ASSETS_DIR OUT_DIR

One .glb per model. Materials use the decoded base-color texture (embedded PNG), alpha mode from the material, sampler wrap/mirror from
the material's texture parameters. Geometry is baked to model space in the bind pose (skeleton/animation export is a later step).
"""
import io, json, os, struct, sys, traceback, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns3d as N


import math


def inv3(R):
    a, b, c = R[0]; d_, e, f = R[1]; g, h, i = R[2]
    det = a * (e * i - f * h) - b * (d_ * i - f * g) + c * (d_ * h - e * g)
    if abs(det) < 1e-12: return [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d_ * i) / det, (a * i - c * g) / det, (c * d_ - a * f) / det],
            [(d_ * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d_) / det]]


def mat4_colmajor(m):
    R, t = m
    return [R[0][0], R[1][0], R[2][0], 0, R[0][1], R[1][1], R[2][1], 0, R[0][2], R[1][2], R[2][2], 0, t[0], t[1], t[2], 1]


def quat_from_matrix(R):
    """column-convention rotation matrix -> (x, y, z, w) quaternion (normalizes slightly non-orthonormal input)"""
    m = R; tr = m[0][0] + m[1][1] + m[2][2]
    if tr > 0:
        s_ = math.sqrt(tr + 1.0) * 2; w = 0.25 * s_; x = (m[2][1] - m[1][2]) / s_; y = (m[0][2] - m[2][0]) / s_; z = (m[1][0] - m[0][1]) / s_
    elif m[0][0] > m[1][1] and m[0][0] > m[2][2]:
        s_ = math.sqrt(1.0 + m[0][0] - m[1][1] - m[2][2]) * 2; w = (m[2][1] - m[1][2]) / s_; x = 0.25 * s_; y = (m[0][1] + m[1][0]) / s_; z = (m[0][2] + m[2][0]) / s_
    elif m[1][1] > m[2][2]:
        s_ = math.sqrt(1.0 + m[1][1] - m[0][0] - m[2][2]) * 2; w = (m[0][2] - m[2][0]) / s_; x = (m[0][1] + m[1][0]) / s_; y = 0.25 * s_; z = (m[1][2] + m[2][1]) / s_
    else:
        s_ = math.sqrt(1.0 + m[2][2] - m[0][0] - m[1][1]) * 2; w = (m[1][0] - m[0][1]) / s_; x = (m[0][2] + m[2][0]) / s_; y = (m[1][2] + m[2][1]) / s_; z = 0.25 * s_
    n = math.sqrt(x * x + y * y + z * z + w * w) or 1.0
    return (x / n, y / n, z / n, w / n)


def node_trs(node):
    R = [[node['rot'][j][i] for j in range(3)] for i in range(3)]       # stored rotations are row-vector form -> transpose
    return list(node['trans']), quat_from_matrix(R), list(node['scale'])


def skeleton_nodes(md, parent):
    """glTF node list for the model's joints (index = NNS node id) + root indices"""
    nodes = []; children = {i: [] for i in range(len(md['nodes']))}; roots = []
    for i in range(len(md['nodes'])):
        p = parent.get(i)
        if p is None: roots.append(i)
        else: children[p].append(i)
    for i, n in enumerate(md['nodes']):
        t, q, sc = node_trs(n)
        g = {'name': n['name'], 'translation': t, 'rotation': list(q), 'scale': sc}
        if children[i]: g['children'] = children[i]
        nodes.append(g)
    return nodes, roots


def _spec_frames(spec, nf):
    """frames at which a track has stored data (keyframes every step frames up to last_interp, then every frame)"""
    if spec[0] == 'const': return {0}
    info = spec[1]; step = N._step_of(info); last = (info & 0x1fff0000) >> 16
    if step == 1: return set(range(nf))
    return set(range(0, last + 1, step)) | set(range(last + 1, nf))


def animation_json(anim, md, add_view, accessors, fps=30.0):
    """glTF animation for a BCA animation targeting joints 0..N-1. Keys are written only where the file stores data; glTF interpolates
    (quaternion lerp) between them, which avoids the non-orthonormal matrices the engine's element-wise rotation lerp produces."""
    nf = anim['frames']; tcache = {}; samplers = []; channels = []
    def times_acc(frames):
        key = tuple(frames)
        if key not in tcache:
            ts = [f / fps for f in frames]; vi = add_view(struct.pack('<%df' % len(ts), *ts))
            accessors.append({'bufferView': vi, 'componentType': 5126, 'count': len(ts), 'type': 'SCALAR', 'min': [ts[0]], 'max': [ts[-1]]}); tcache[key] = len(accessors) - 1
        return tcache[key]
    cache = {}
    def pose(f):
        if f not in cache: cache[f] = N.sample_anim(anim, md['nodes'], f)
        return cache[f]
    for tr in anim['tags']:
        nid = tr['node']
        if nid >= len(md['nodes']): continue
        tag = tr['flag']
        kf = {'translation': {0}, 'rotation': {0}, 'scale': {0}}
        if not tag & 1:
            if 't' in tr and not tag & 0x6: kf['translation'] = set().union(*[_spec_frames(sp, nf) for sp in tr['t']])
            if 'r' in tr and not tag & 0xc0: kf['rotation'] = _spec_frames(tr['r'], nf)
            if 's' in tr and not tag & 0x600: kf['scale'] = set().union(*[_spec_frames(sp, nf) for sp in tr['s']])
        for path, comp, typ in (('translation', 3, 'VEC3'), ('rotation', 4, 'VEC4'), ('scale', 3, 'VEC3')):
            frames = sorted(f for f in kf[path] if f < nf) or [0]
            vals = []; prev = None
            for f in frames:
                t, q, sc = node_trs(pose(f)[nid])
                if path == 'translation': vals += t
                elif path == 'scale': vals += sc
                else:
                    if prev is not None and sum(a * b for a, b in zip(q, prev)) < 0: q = tuple(-x for x in q)
                    prev = q; vals += list(q)
            vi = add_view(struct.pack('<%df' % len(vals), *vals))
            accessors.append({'bufferView': vi, 'componentType': 5126, 'count': len(frames), 'type': typ}); out_idx = len(accessors) - 1
            samplers.append({'input': times_acc(frames), 'output': out_idx, 'interpolation': 'LINEAR'})
            channels.append({'sampler': len(samplers) - 1, 'target': {'node': nid, 'path': path}})
    return {'name': anim['name'], 'samplers': samplers, 'channels': channels}


def model_to_glb(d, md, tex_index, anims=None):
    info = {}; N.run_sbc(d, md, True, None, info) if False else None
    md['evp'] = N.read_evp(d, md); winfo = {}; N.run_sbc(d, md, True, None, winfo)
    tris = N.build_mesh(d, md, skin=True)
    cache = {}; materials = []; images = []; textures = []; samplers = []
    mat_images = {}
    for mi, m in enumerate(md['materials']):
        gm = {'name': m['name'], 'doubleSided': m['cull'] != 2 and m['cull'] != 1 or True, 'pbrMetallicRoughness': {'metallicFactor': 0.0, 'roughnessFactor': 1.0}}
        img = N.material_image(m, tex_index, cache)
        if img is not None:
            key = (m['tex'], m.get('pal'))
            if key not in mat_images:
                b = io.BytesIO(); img.save(b, 'PNG'); images.append(b.getvalue()); mat_images[key] = len(images) - 1
                tp = m['tex_param']; wrap = lambda rep, flip: 33648 if (rep and flip) else (10497 if rep else 33071)
                samplers.append({'wrapS': wrap((tp >> 16) & 1, (tp >> 18) & 1), 'wrapT': wrap((tp >> 17) & 1, (tp >> 19) & 1), 'magFilter': 9728, 'minFilter': 9728})
                textures.append({'source': mat_images[key], 'sampler': len(samplers) - 1})
            gm['pbrMetallicRoughness']['baseColorTexture'] = {'index': mat_images[key]}
            if img.getextrema()[3][0] < 255: gm['alphaMode'] = 'MASK'; gm['alphaCutoff'] = 0.5
        materials.append(gm)
    # primitives per material
    by = collections.defaultdict(list)
    for t in tris: by[t['material']].append(t)
    bin_data = bytearray(); views = []; accessors = []; prims = []
    def add_view(data, target=None):
        while len(bin_data) % 4: bin_data.append(0)
        views.append({'buffer': 0, 'byteOffset': len(bin_data), 'byteLength': len(data), **({'target': target} if target else {})}); bin_data.extend(data); return len(views) - 1
    for mi, ts in sorted(by.items()):
        mat = md['materials'][mi]; tw = mat['orig_w'] or 1; th = mat['orig_h'] or 1
        pos = []; nrm = []; uv = []; col = []; jnt = []; wgt = []
        for t in ts:
            for v in t['verts']:
                pos += list(v[0]); nrm += list(v[1]); uv += [v[2][0] / tw, v[2][1] / th]; col += list(v[3])
                pr = sorted(v[4] or [(0, 1.0)], key=lambda x: -x[1])[:4]; tot = sum(w for _, w in pr) or 1.0
                pr = [(j, w / tot) for j, w in pr] + [(0, 0.0)] * (4 - len(pr)); jnt += [j for j, _ in pr]; wgt += [w for _, w in pr]
        n = len(pos) // 3
        mn = [min(pos[i::3]) for i in range(3)]; mx = [max(pos[i::3]) for i in range(3)]
        def acc(vals, comp, typ, bounds=None):
            vi = add_view(struct.pack('<%df' % len(vals), *vals), 34962)
            a = {'bufferView': vi, 'componentType': 5126, 'count': len(vals) // comp, 'type': typ}
            if bounds: a['min'], a['max'] = bounds
            accessors.append(a); return len(accessors) - 1
        attrs = {'POSITION': acc(pos, 3, 'VEC3', (mn, mx)), 'NORMAL': acc(nrm, 3, 'VEC3'), 'TEXCOORD_0': acc(uv, 2, 'VEC2'), 'COLOR_0': acc(col, 3, 'VEC3')}
        ji = add_view(bytes(jnt), 34962); accessors.append({'bufferView': ji, 'componentType': 5121, 'count': n, 'type': 'VEC4'}); attrs['JOINTS_0'] = len(accessors) - 1
        attrs['WEIGHTS_0'] = acc(wgt, 4, 'VEC4')
        idx = list(range(n))
        vi = add_view(struct.pack('<%dI' % n, *idx), 34963)
        accessors.append({'bufferView': vi, 'componentType': 5125, 'count': n, 'type': 'SCALAR'})
        prims.append({'attributes': attrs, 'indices': len(accessors) - 1, 'material': mi, 'mode': 4})
    # image buffer views
    img_defs = []
    for im in images:
        vi = add_view(im); img_defs.append({'bufferView': vi, 'mimeType': 'image/png'})
    jn, roots = skeleton_nodes(md, winfo['parent'])
    ibm = []
    for i in range(len(md['nodes'])):
        w = winfo['world'].get(i, N.IDENT); R = inv3(w[0]); t = [-sum(R[r][c] * w[1][c] for c in range(3)) for r in range(3)]
        ibm += mat4_colmajor((R, t))
    ibv = add_view(struct.pack('<%df' % len(ibm), *ibm)); accessors.append({'bufferView': ibv, 'componentType': 5126, 'count': len(md['nodes']), 'type': 'MAT4'})
    nj = len(jn)
    gltf = {'asset': {'version': '2.0', 'generator': 'lp tools nns3d_export'}, 'scene': 0, 'scenes': [{'nodes': roots + [nj]}],
            'nodes': jn + [{'mesh': 0, 'skin': 0, 'name': md['name']}],
            'skins': [{'joints': list(range(nj)), 'inverseBindMatrices': len(accessors) - 1, 'skeleton': roots[0]}],
            'meshes': [{'name': md['name'], 'primitives': prims}], 'materials': materials, 'accessors': accessors, 'bufferViews': views,
            'buffers': [{'byteLength': len(bin_data)}]}
    if anims:
        gltf['animations'] = [animation_json(a, md, add_view, accessors) for a in anims]
        gltf['buffers'][0]['byteLength'] = len(bin_data)
    if img_defs: gltf['images'] = img_defs; gltf['textures'] = textures; gltf['samplers'] = samplers
    js = json.dumps(gltf, separators=(',', ':')).encode()
    while len(js) % 4: js += b' '
    while len(bin_data) % 4: bin_data.append(0)
    total = 12 + 8 + len(js) + 8 + len(bin_data)
    return struct.pack('<4sII', b'glTF', 2, total) + struct.pack('<I4s', len(js), b'JSON') + js + struct.pack('<I4s', len(bin_data), b'BIN\0') + bytes(bin_data), len(tris)


def anim_to_glb(md, parent, anim):
    """skeleton-only glb (joint hierarchy in bind pose + one animation); node names/indices match the model's joints"""
    bin_data = bytearray(); views = []; accessors = []
    def add_view(data, target=None):
        while len(bin_data) % 4: bin_data.append(0)
        views.append({'buffer': 0, 'byteOffset': len(bin_data), 'byteLength': len(data), **({'target': target} if target else {})}); bin_data.extend(data); return len(views) - 1
    jn, roots = skeleton_nodes(md, parent)
    animation = animation_json(anim, md, add_view, accessors)
    gltf = {'asset': {'version': '2.0', 'generator': 'lp tools nns3d_export'}, 'scene': 0, 'scenes': [{'nodes': roots}], 'nodes': jn,
            'accessors': accessors, 'bufferViews': views, 'buffers': [{'byteLength': len(bin_data)}], 'animations': [animation]}
    js = json.dumps(gltf, separators=(',', ':')).encode()
    while len(js) % 4: js += b' '
    while len(bin_data) % 4: bin_data.append(0)
    total = 12 + 8 + len(js) + 8 + len(bin_data)
    return struct.pack('<4sII', b'glTF', 2, total) + struct.pack('<I4s', len(js), b'JSON') + js + struct.pack('<I4s', len(bin_data), b'BIN\0') + bytes(bin_data)


_GLOBAL_REFS = {}


def global_refs(src_root):
    """reference skeletons by joint count from every model in the tree (fallback for animations whose skeleton lives in another archive)"""
    if _GLOBAL_REFS: return _GLOBAL_REFS
    for r, _, fs in os.walk(src_root):
        for f in sorted(fs):
            p = os.path.join(r, f)
            if not os.path.isfile(p) or open(p, 'rb').read(4) != b'BMD0': continue
            dd = open(p, 'rb').read()
            for md in N.parse_file(dd)['models']:
                n = md['info']['num_node']
                if n not in _GLOBAL_REFS:
                    info = {}; md['evp'] = N.read_evp(dd, md); N.run_sbc(dd, md, True, None, info); _GLOBAL_REFS[n] = (md, info['parent'], f, md['name'])
    return _GLOBAL_REFS


def export_tree(src_root, out_root):
    stats = collections.Counter(); fails = []
    for r, _, fs in os.walk(src_root):
        bmds = [f for f in sorted(fs) if os.path.isfile(os.path.join(r, f)) and open(os.path.join(r, f), 'rb').read(4) == b'BMD0']
        if not bmds: continue
        idx = N.index_textures(r)
        bcas = [f for f in sorted(fs) if os.path.isfile(os.path.join(r, f)) and open(os.path.join(r, f), 'rb').read(4) == b'BCA0']
        if bcas:
            # reference skeletons per node count (first model in the directory with that many nodes)
            refs = {}
            for f in bmds:
                dd = open(os.path.join(r, f), 'rb').read()
                for md in N.parse_file(dd)['models']:
                    if md['num_nodes' if False else 'info']['num_node'] not in refs:
                        info = {}; md['evp'] = N.read_evp(dd, md); N.run_sbc(dd, md, True, None, info); refs[md['info']['num_node']] = (md, info['parent'], f, md['name'])
            adir = os.path.join(out_root, os.path.relpath(r, src_root), 'animations'); os.makedirs(adir, exist_ok=True); index = {}
            for f in bcas:
                try:
                    for an in N.parse_bca(open(os.path.join(r, f), 'rb').read()):
                        nn = max([t['node'] for t in an['tags']] + [0]) + 1
                        pool = refs if [k for k in refs if k >= nn] else global_refs(src_root)
                        cand = [k for k in pool if k >= nn]
                        if not cand: stats['anim_no_skeleton'] += 1; continue
                        md, parent, rf, rname = pool[min(cand, key=lambda k: abs(k - len(an['tags'])))]
                        name = '%s.glb' % an['name'].replace('/', '_'); open(os.path.join(adir, name), 'wb').write(anim_to_glb(md, parent, an))
                        index[an['name']] = {'file': name, 'frames': an['frames'], 'nodes': len(an['tags']), 'skeleton_nodes': md['info']['num_node'], 'reference_model': rname}; stats['animations'] += 1
                except Exception:
                    fails.append((os.path.join(r, f), traceback.format_exc()[-160:]))
            json.dump(index, open(os.path.join(adir, 'index.json'), 'w'), indent=0)
        for f in bmds:
            d = open(os.path.join(r, f), 'rb').read()
            try:
                for md in N.parse_file(d)['models']:
                    glb, ntri = model_to_glb(d, md, idx)
                    rel = os.path.relpath(r, src_root); name = '%s_%s.glb' % (os.path.splitext(f)[0], md['name'].replace('/', '_'))
                    od = os.path.join(out_root, rel); os.makedirs(od, exist_ok=True); open(os.path.join(od, name), 'wb').write(glb)
                    stats['models'] += 1; stats['triangles'] += ntri
            except Exception:
                fails.append((os.path.join(r, f), traceback.format_exc()[-160:]))
    return stats, fails


if __name__ == '__main__':
    st, fl = export_tree(sys.argv[1], sys.argv[2])
    print(dict(st), len(fl), 'failures')
    for f in fl[:8]: print(' ', f)


# ------------------------------------------------------------------ heroine motions (async3d bundles) -> compact JSON
def _r(x, n=5): return round(float(x), n)


def anim_to_dict(anim, src_names=None):
    """Compact keyframe dict for one BCA animation: per joint, keys only where the file stores data. A channel set to null means
    'use the target model's own value' (BASE flags); identity channels are written explicitly."""
    nf = anim['frames']; dummy = [{'trans': (0.0, 0.0, 0.0), 'rot': [[1, 0, 0], [0, 1, 0], [0, 0, 1]], 'scale': (1.0, 1.0, 1.0)}] * (max([t['node'] for t in anim['tags']] + [0]) + 1)
    cache = {}
    def pose(f):
        if f not in cache: cache[f] = N.sample_anim(anim, dummy, f)
        return cache[f]
    joints = []
    for tr in anim['tags']:
        nid = tr['node']; tag = tr['flag']; j = {'j': nid}
        if src_names and nid < len(src_names): j['name'] = src_names[nid]
        if tag & 1: j.update({'t': [[0, 0, 0, 0]], 'r': [[0, 0, 0, 0, 1]], 's': [[0, 1, 1, 1]]}); joints.append(j); continue
        spec = {}
        spec['t'] = None if tag & 0x4 else ({0} if tag & 0x2 else set().union(*[_spec_frames(sp, nf) for sp in tr.get('t', [('const', 0)] * 3)]))
        spec['r'] = None if tag & 0x80 else ({0} if tag & 0x40 else _spec_frames(tr['r'], nf) if 'r' in tr else {0})
        spec['s'] = None if tag & 0x400 else ({0} if tag & 0x200 else set().union(*[_spec_frames(sp, nf) for sp in tr.get('s', [('const', 1)] * 3)]))
        for ch, frames in spec.items():
            if frames is None: j[ch] = None; continue
            fl = sorted(f for f in frames if f < nf) or [0]; rows = []; prev = None
            for f in fl:
                t, q, sc = node_trs(pose(f)[nid])
                if ch == 't': rows.append([f] + [_r(x) for x in t])
                elif ch == 's': rows.append([f] + [_r(x) for x in sc])
                else:
                    if prev is not None and sum(a * b for a, b in zip(q, prev)) < 0: q = tuple(-x for x in q)
                    prev = q; rows.append([f] + [_r(x) for x in q])
            if len(rows) > 1 and all(r[1:] == rows[0][1:] for r in rows): rows = rows[:1]        # constant channel
            j[ch] = rows
        joints.append(j)
    return {'frames': nf, 'joints': joints}


def export_motions(async_dir, out_dir, refs=None):
    """async3d bundles -> out_dir/<motion>.json ; returns (count, bytes)"""
    import base64, struct as st
    os.makedirs(out_dir, exist_ok=True); n = 0; total = 0; names = {}
    for f in sorted(os.listdir(async_dir)):
        b = open(os.path.join(async_dir, f), 'rb').read()
        if len(b) < 8 or b[:4] in (b'BTX0', b'BMD0'): continue
        cnt, esz = st.unpack_from('<II', b, 0)
        if not (1 <= cnt <= 16 and esz == 0x28): continue
        for k in range(cnt):
            off, size = st.unpack_from('<II', b, 8 + k * esz); name = b[8 + k * esz + 8:8 + k * esz + 40].split(b'\0')[0].decode('latin1'); data = b[off:off + size]
            stem = name.rsplit('.', 1)[0]; ent = names.setdefault(stem, {'name': stem, 'fps': 30, 'parts': {}})
            if data[:4] == b'BCA0':
                for a in N.parse_bca(data):
                    part = a['name'][len(stem) + 1:] if a['name'].startswith(stem + '_') else a['name']
                    ent['parts'].setdefault(part, {})['anim'] = anim_to_dict(a)
            elif data[:4] == b'BVA0':
                for v in N.parse_bva(data):
                    part = v['name'][len(stem) + 1:] if v['name'].startswith(stem + '_') else v['name']
                    ent['parts'].setdefault(part, {})['visibility'] = {'frames': v['frames'], 'nodes': v['nodes'], 'bits': base64.b64encode(st.pack('<%dI' % len(v['bits']), *v['bits'])).decode()}
    for stem, ent in names.items():
        p = os.path.join(out_dir, stem + '.json'); js = json.dumps(ent, separators=(',', ':')); open(p, 'w').write(js); n += 1; total += len(js)
    return n, total


def build_wardrobe(models_root):
    """Index of the exported heroine part models: models/heroines.json with per-heroine outfits/hairs/faces/hands and the part->motion-track rules."""
    import re
    found = {}
    for r, _, fs in os.walk(models_root):
        for f in fs:
            m = re.match(r'^\d+_(.+)\.glb$', f)
            if m and 'animations' not in r: found[m.group(1)] = os.path.relpath(os.path.join(r, f), models_root)
    out = {'heroines': {}, 'track_rules': {
        'body': ['body', 'face'], 'larm': ['larm', 'lhand'], 'rarm': ['rarm', 'rhand'], 'leg': ['leg', 'waist'],
        'hair0': ['hair layout with lpig/rpig (19 joints)'], 'hair1': ['hair layout with ribon (20 joints)'], 'hair2': ['hair layout with hairT_* joints (19 joints)'], 'item': ['item'],
        'note': 'animation joints are matched to each part model by joint NAME (parts use different joint orders); visibility tracks (incl. eye00, lhand, rhand) are bit arrays indexed frame*nodes+node'}}
    for pre, name in (('m', 'Manaka'), ('r', 'Rinko'), ('n', 'Nene')):
        outfits = {}; hairs = []
        for mn, path in sorted(found.items()):
            m = re.match(r'^%s_01_(\d+)_(body|larm|rarm|waist|leg)$' % pre, mn)
            if m: outfits.setdefault(int(m.group(1)), {})[m.group(2)] = path
            m = re.match(r'^%s_00_(\d+)_hair$' % pre, mn)
            if m: hairs.append({'id': int(m.group(1)), 'model': path})
        base = {k: v for k, v in found.items() if k.startswith(pre + '_base_')}
        out['heroines'][name] = {'prefix': pre, 'outfits': [dict(id=k, **v) for k, v in sorted(outfits.items())], 'hairs': hairs, 'base': base}
    json.dump(out, open(os.path.join(models_root, 'heroines.json'), 'w'), indent=1)
    return {k: (len(v['outfits']), len(v['hairs'])) for k, v in out['heroines'].items()}


def _bundle_entries(b):
    """game bundle container: {count u32, entry_size u32 (=0x28)}, entries {offset u32, size u32, name[32]} -> [(name, bytes, entry_fields)] or []"""
    import struct as st
    if len(b) < 8: return []
    n, esz = st.unpack_from('<II', b, 0)
    if esz != 0x28 or not 0 < n < 4096 or 8 + n * esz > len(b): return []
    out = []
    for k in range(n):
        off, size = st.unpack_from('<II', b, 8 + k * esz)
        name = b[8 + k * esz + 8:8 + k * esz + 40].split(b'\0')[0].decode('latin1')
        if off + size > len(b): return []
        out.append((name, b[off:off + size]))
    return out


def export_shading(arc_root, out_dir):
    """.tmbsd (120 B = 40 RGB triplets of 5-bit channels: 32 toon-table entries then 8 edge/outline colors, stored as RGB555) and
    .tmbpl (16 RGB555 palette = hair/colour variants) found inside the bundle files of every archive -> out_dir/<arc>.json."""
    import struct as st
    os.makedirs(out_dir, exist_ok=True); n_sd = n_pl = 0
    for arc in sorted(os.listdir(arc_root)):
        p = os.path.join(arc_root, arc)
        if not os.path.isdir(p): continue
        doc = {'tmbsd': {}, 'tmbpl': {}}
        for f in sorted(os.listdir(p)):
            for name, d in _bundle_entries(open(os.path.join(p, f), 'rb').read()):
                if name.endswith('.tmbsd') and len(d) == 120:
                    c = [d[i] | d[i + 1] << 5 | d[i + 2] << 10 for i in range(0, 120, 3)]
                    doc['tmbsd'][name[:-6]] = {'toon': c[:32], 'edge': c[32:]}; n_sd += 1
                elif name.endswith('.tmbpl') and len(d) % 2 == 0 and d:
                    doc['tmbpl'][name[:-6]] = list(st.unpack('<%dH' % (len(d) // 2), d)); n_pl += 1
        if doc['tmbsd'] or doc['tmbpl']:
            json.dump(doc, open(os.path.join(out_dir, arc.replace('.arc', '') + '.json'), 'w'), separators=(',', ':'))
    return n_sd, n_pl


def export_expressions(arc_root, out_dir):
    """Face expression textures. The face model's materials are named <p>_eye.0 / <p>_brow.0 / <p>_cheek.0 / <p>_mouth.0 (+ <p>_face_NN skin);
    an expression is selected by rebinding a slot to texture <p>_<slot>.<n> (palette '<name>_pl'). Writes PNGs + out_dir/expressions.json."""
    import re
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import nns3d as N
    os.makedirs(out_dir, exist_ok=True); index = {}; count = 0
    srcs = [os.path.join(arc_root, 'tm_model_block_base.arc'), os.path.join(arc_root, 'async3d.arc')]
    for s in srcs:
        if not os.path.isdir(s): continue
        for name, (tex0, tx, pals) in N.index_textures(s).items():
            m = re.match(r'^([mrn])_(eye|brow|cheek|mouth)\.(\d+)$', name) or re.match(r'^([mrn])_(face)_(\d+)$', name)
            if not m: continue
            pal = next((q for pn, q in pals.items() if pn.startswith(name + '_')), None)
            try: img = N.decode_texture(tex0, tx, pal)
            except Exception: continue
            pre, slot, var = m.group(1), m.group(2), int(m.group(3))
            d = os.path.join(out_dir, pre); os.makedirs(d, exist_ok=True)
            fn = '%s.png' % name.replace('.', '_'); img.save(os.path.join(d, fn)); count += 1
            index.setdefault(pre, {}).setdefault(slot, {})[var] = {'png': '%s/%s' % (pre, fn), 'w': tx['w'], 'h': tx['h']}
    json.dump({'note': 'expression = (eye, brow, cheek, mouth) variant numbers; rebind the face material named <p>_<slot>.0 to the variant texture',
               'heroines': index}, open(os.path.join(out_dir, 'expressions.json'), 'w'), indent=1)
    return count


def export_variants(arc_root, out_dir):
    """Variant (sub)textures stored in the per-model bundles as {offset,size,name[16],tex_off,pal_off,tex_size,pal_size} entries.
    Name = '<texture>_<group><variant>' (outfits: group 1/2 + variant; hair accessory '<tex>_0<n>'); the geometry/format comes from the base
    texture of the same model. -> PNGs in out_dir/<arc>/<name>.png and out_dir/variants.json"""
    import re
    os.makedirs(out_dir, exist_ok=True); index = {}; count = 0
    for arc in sorted(os.listdir(arc_root)):
        if not re.match(r'^[mrn]_block3d_0[01]\.arc$', arc): continue
        p = os.path.join(arc_root, arc); base = N.index_textures(p)
        for f in sorted(os.listdir(p)):
            b = open(os.path.join(p, f), 'rb').read()
            if len(b) < 8: continue
            n, esz = struct.unpack_from('<II', b, 0)
            if esz != 0x28 or not 0 < n < 4096 or 8 + n * esz > len(b): continue
            for k in range(n):
                e = 8 + k * esz; name32 = b[e + 8:e + 40].split(b'\0')[0].decode('latin1')
                if '.' in name32: continue
                name = name32[:16]; t_off, p_off, t_size, p_size = struct.unpack_from('<4I', b, e + 24)
                m = re.match(r'^(.+)_(\d)(\d)$', name)
                if not m or not t_size or not p_size or t_off + t_size > len(b) or p_off + p_size > len(b): continue
                bt = base.get(m.group(1))
                if bt is None: continue
                tex0 = {'data': b[t_off:t_off + t_size] + b[p_off:p_off + p_size], 'ofs_tex': 0, 'ofs_pltt': t_size}
                try: img = N.decode_texture(tex0, dict(bt[1], addr=0), {'offset': 0})
                except Exception: continue
                d = os.path.join(out_dir, arc.replace('.arc', '')); os.makedirs(d, exist_ok=True)
                img.save(os.path.join(d, name + '.png')); count += 1
                index.setdefault(m.group(1), {})['%s%s' % (m.group(2), m.group(3))] = '%s/%s.png' % (arc.replace('.arc', ''), name)
    json.dump({'note': "texture variant '<tex>_<group><variant>' replaces the base texture '<tex>'; fallback order: <group><variant>, 10, 1<variant%10>", 'textures': index},
              open(os.path.join(out_dir, 'variants.json'), 'w'), indent=1)
    return count


def export_camera_presets(arc_root, out_path):
    """camera_data_N.bin entries (164-byte records) -> JSON. id = (N+1)*1000 + index. Decoded from Camera_SetPreset: eye, target, light angles
    (3 lights: yaw/pitch degrees, view space); the vec3 blocks are the pan-limit variants (kept raw)."""
    import re
    out = {}
    for d in sorted(os.listdir(arc_root)):
        if not re.match(r'^camera_data_\d+\.bin$', d) or not os.path.isdir(os.path.join(arc_root, d)): continue
        for f in sorted(os.listdir(os.path.join(arc_root, d))):
            b = open(os.path.join(arc_root, d, f), 'rb').read()
            if len(b) != 164: continue
            w = struct.unpack('<41i', b); fx = lambda x: x / 4096
            out[str(w[0])] = {'eye': [fx(x) for x in w[1:4]], 'target': [fx(x) for x in w[4:7]], 'u16_7': w[7] & 0xffff,
                              'lights': [{'yaw': w[11], 'pitch': w[8]}, {'yaw': w[12], 'pitch': w[9]}, {'yaw': w[13], 'pitch': w[10]}],
                              'field_234': w[14], 'eye_block': [[fx(x) for x in w[15 + 3 * i:18 + 3 * i]] for i in range(4)],
                              'target_block': [[fx(x) for x in w[27 + 3 * i:30 + 3 * i]] for i in range(4)], 'u16_block': [w[39] & 0xffff, w[39] >> 16, w[40] & 0xffff, w[40] >> 16]}
    json.dump({'note': 'light yaw/pitch in degrees; see docs/CONTENT_LAYOUT.md', 'presets': out}, open(out_path, 'w'), indent=1)
    return len(out)


def export_motion_table(arc_root, out_path, arm9=None):
    """Script motion index (cmd 0x5d argument) -> heroine, async3d bundle, motion stems, default mood.
    Decoded from the game: category = index range (0-738 Manaka, 739-840 Rinko, 841-973 Nene); bundle file = s16 table at 0x212588c[index]
    (= async3d file number); the variant = index - first index sharing that bundle, stem '<p>_<NNNN>_<info0*10+variant>'; info record (6 bytes at 0x2126028 + 6*file) holds the motion number NNNN (short at +4) so the stem is
    '<m|r|n>_<NNNN>_<variant>'; default face mood = tm_model_block_base entry 101 (u32 count + 36-byte records: 3 groups x 3 heroines x
    {s8 mood, s8 -1, s16 value}; the mood of the record's own heroine is used)."""
    base = os.path.join(arc_root, 'tm_model_block_base.arc'); asy = os.path.join(arc_root, 'async3d.arc')
    if arm9 is None or not (os.path.isdir(base) and os.path.isdir(asy)): return 0
    tbl = None
    for f in sorted(os.listdir(base)):
        b = open(os.path.join(base, f), 'rb').read()
        if len(b) >= 40 and struct.unpack_from('<I', b, 0)[0] * 36 + 4 == len(b): tbl = b; break
    if tbl is None: return 0
    stems = {}
    for f in sorted(os.listdir(asy)):
        b = open(os.path.join(asy, f), 'rb').read()
        bb = _bundle_entries(b) if len(b) >= 8 and b[:4] not in (b'BTX0', b'BMD0') else []
        if bb: stems[int(f.split('.')[0])] = sorted({n.rsplit('.', 1)[0] for n, _ in bb})
    B = 0x2000000; grp = struct.unpack_from('<974h', arm9, 0x212588c - B); n = struct.unpack_from('<I', tbl, 0)[0]; recs = []; first = {}
    for i in range(974):
        cat = 0 if i <= 0x2e2 else 1 if i <= 0x348 else 2
        g = grp[i]; info = struct.unpack_from('<4bh', arm9, 0x2126028 - B + 6 * g)
        first.setdefault(g, i); v = i - first[g]
        rec = {'index': i, 'heroine': 'mrn'[cat], 'bundle': g, 'number': info[4], 'variant': v, 'info': list(info[:4]), 'stems': stems.get(g, []),
               'stem': '%s_%04d_%02d' % ('mrn'[cat], info[4], info[0] * 10 + v)}
        if i < n: rec['mood'] = struct.unpack_from('<bbh', tbl, 4 + 36 * i + 4 * cat)[0]
        recs.append(rec)
    json.dump({'note': 'index = script motion id (cmd 0x5d a0); mood = default face expression id (engine/face_tables.json), -1/absent = none',
               'records': recs}, open(out_path, 'w'), separators=(',', ':'))
    return len(recs)
