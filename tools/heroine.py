"""Assemble an animated heroine from her costume parts (reference/preview tooling).

Parts: body, larm, rarm, waist, leg (m_01_NNN_*), hair (m_00_NNN_hair), face + hands (tm_model_block_base), item. Motions live in
async3d.arc as named bundles {X.nsbca, X.nsbva}; each BCA holds one animation per part track (body, larm, rarm, leg, hair0/1/2, item).
Animation tracks are matched to each part's joints by name (parts use different joint orders).
"""
import os, struct, sys, re
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import nns3d as N

A = 'work/assets/arc'


def load_bundles(path=os.path.join(A, 'async3d.arc')):
    """-> {motion_name: {'bca': [anims], 'bva': [anims]}} for bundle files {name.nsbca, name.nsbva}"""
    out = {}
    for f in sorted(os.listdir(path)):
        b = open(os.path.join(path, f), 'rb').read()
        if len(b) < 8 or b[:4] in (b'BTX0', b'BMD0'): continue
        n, esz = struct.unpack_from('<II', b, 0)
        if not (1 <= n <= 16 and esz == 0x28): continue
        for k in range(n):
            off, size = struct.unpack_from('<II', b, 8 + k * esz); name = b[8 + k * esz + 8:8 + k * esz + 40].split(b'\0')[0].decode('latin1')
            data = b[off:off + size]; stem, ext = name.rsplit('.', 1)
            ent = out.setdefault(stem, {'bca': [], 'bva': [], 'bundle': f})
            if data[:4] == b'BCA0': ent['bca'] = N.parse_bca(data)
            elif data[:4] == b'BVA0': ent['bva'] = N.parse_bva(data)
    return out


def load_models(arch):
    d = os.path.join(A, arch); out = {}
    for f in sorted(os.listdir(d)):
        b = open(os.path.join(d, f), 'rb').read()
        if b[:4] == b'BMD0':
            for m in N.parse_file(b)['models']: out[m['name']] = (b, m)
    return out


class Wardrobe:
    def __init__(self, prefix='m'):
        self.prefix = prefix; self.parts = {}; self.tex = {}
        for arch in ('%s_block3d_01.arc' % prefix, '%s_block3d_00.arc' % prefix, 'tm_model_block_base.arc'):
            self.parts.update(load_models(arch)); self.tex.update(N.index_textures(os.path.join(A, arch)))
        self.motions = {k: v for k, v in load_bundles().items() if k.startswith(prefix + '_')}
        # hand pose models live in async3d (<p>_base_lhand_NN / rhand_NN); a motion's lhand/rhand visibility track selects the pose
        self.parts.update({k: v for k, v in load_models('async3d.arc').items() if k.startswith(prefix + '_base_')})
        # expression textures: eyes live in async3d (m_eye.N), brow/cheek/mouth packs in tm_model_block_base (already indexed above)
        self.tex.update({k: v for k, v in N.index_textures(os.path.join(A, 'async3d.arc')).items() if k.startswith(prefix + '_eye.')})

    EXPR_SLOTS = ('eye', 'brow', 'cheek', 'mouth')

    # ------------------------------------------------------------ colour/texture variants and shading tables (from the per-model bundles)
    def _scan(self):
        """index every named entry of the model bundles: sd tables, palettes, variant (sub)textures"""
        if hasattr(self, 'sd'): return
        self.sd, self.pl, self.vt = {}, {}, {}
        for arch in ('%s_block3d_00.arc' % self.prefix, '%s_block3d_01.arc' % self.prefix, 'tm_model_block_base.arc'):
            d = os.path.join(A, arch)
            for f in sorted(os.listdir(d)):
                b = open(os.path.join(d, f), 'rb').read()
                if len(b) < 8: continue
                n, esz = struct.unpack_from('<II', b, 0)
                if esz != 0x28 or not 0 < n < 4096 or 8 + n * esz > len(b): continue
                for k in range(n):
                    e = 8 + k * esz; off, size = struct.unpack_from('<II', b, e); name = b[e + 8:e + 40].split(b'\0')[0].decode('latin1')
                    if name.endswith('.tmbsd') and size == 120:
                        c = b[off:off + 120]; self.sd[name[:-6]] = ([c[i] | c[i + 1] << 5 | c[i + 2] << 10 for i in range(0, 96, 3)],
                                                                    [c[i] | c[i + 1] << 5 | c[i + 2] << 10 for i in range(96, 120, 3)])
                    elif name.endswith('.tmbpl') and size == 32:
                        self.pl[name[:-6]] = list(struct.unpack_from('<16H', b, off))
                    elif '.' not in name:
                        name = name[:16]
                        t_off, p_off, t_size, p_size = struct.unpack_from('<4I', b, e + 24)
                        if t_size and p_size and t_off + t_size <= len(b) and p_off + p_size <= len(b): self.vt[name] = (b[t_off:t_off + t_size], b[p_off:p_off + p_size])

    def shading(self, outfit, hair, state=0, outfit_color=0, hair_color=0):
        """-> (toon[32], edge[8]) RGB555 tables for time-of-day `state` (0 day, 1 evening, 2 night).
        toon + edge[0] come from the heroine base bundle; hair owns edges 6-7, the outfit owns 1-5 (the colour variant 'plt_NN' bundle replaces the base one)."""
        self._scan(); p = self.prefix; toon, e = self.sd['%s_base_%d' % (p, state)]; edge = list(e)
        def pick(stem, color):
            return (self.sd.get('%s_plt_%02d_%d' % (stem, color, state)) if color else None) or self.sd.get('%s_%d' % (stem, state))
        h = pick('%s_00_%03d' % (p, hair), hair_color)
        if h:
            for i in (6, 7): edge[i] = h[1][i]
        o = pick('%s_01_%03d' % (p, outfit), outfit_color)
        if o:
            for i in range(1, 6): edge[i] = o[1][i]
        return toon, edge

    def _variant_image(self, mat, name):
        """RGBA image of the variant sub-texture `name` using the base texture's geometry/format"""
        self._scan()
        if name not in self.vt or mat.get('tex') not in self.tex: return None
        t, tx, _ = self.tex[mat['tex']]; tb, pb = self.vt[name]
        tex0 = {'data': tb + pb, 'ofs_tex': 0, 'ofs_pltt': len(tb)}
        return N.decode_texture(tex0, dict(tx, addr=0), {'offset': 0})

    def apply_variants(self, mats, outfit, hair, outfit_group=1, outfit_variant=0, hair_color=0, ribbon=0):
        """outfit textures: sub-texture '<tex>_<group><variant>' replaces the base (fallbacks as in the game: '<tex>_10', then '<tex>_1<variant%10>');
        hair: palette '<tex>_<NN>' replaces the base palette (colour variant NN >= 1), ribbon sub-texture '<hairtex2>_0<ribbon>'."""
        self._scan(); p = self.prefix
        for m in mats:
            tn = m.get('tex') or ''
            if tn.startswith('%s_01_%03d_' % (p, outfit)):
                for cand in ('%s_%d%d' % (tn, outfit_group, outfit_variant), '%s_10' % tn, '%s_1%d' % (tn, outfit_variant % 10)):
                    im = self._variant_image(m, cand)
                    if im is not None: m['image'] = im; break
            elif tn.startswith('%s_00_%03d_' % (p, hair)):
                if tn.endswith('ha2'):
                    im = self._variant_image(m, '%s_0%d' % (tn, ribbon))
                    if im is not None: m['image'] = im
                elif hair_color and ('%s_%02d' % (tn, hair_color)) in self.pl:
                    t, tx, _ = self.tex[tn]
                    m['image'] = N.decode_texture(t, tx, {'colors': self.pl['%s_%02d' % (tn, hair_color)], 'offset': 0})

    def hand_pose(self, motion, side, frame):
        """pose index of the left/right hand: the visibility track '<motion>_lhand|rhand' keeps nodes 0-2 on and exactly one node >= 3 visible; pose = node - 3"""
        if not motion: return 0
        for v in self.motions[motion]['bva']:
            if v['name'].endswith('_%shand' % side):
                for i in range(3, v['nodes']):
                    if N.visible(v, min(frame, v['frames'] - 1), i): return i - 3
        return 0

    def compose(self, outfit=17, hair=0, motion=None, frame=0, mood=0, viseme=5, eye_column=0, blink=False, state=0, tables=None,
                outfit_group=1, outfit_variant=0, outfit_color=0, hair_color=0, ribbon=0):
        """assembled heroine -> (tris, materials, toon, edge); `tables` = face_tables.extract(...) result (or None to leave the face as authored)"""
        import face_tables as F
        parts = []
        for kind, name in self.part_sets(outfit, hair):
            if kind in ('lhand', 'rhand'):
                alt = '%s_base_%s_%02d' % (self.prefix, kind, self.hand_pose(motion, kind[0], frame))
                if alt in self.parts: name = alt
            parts.append((kind, name))
        tris, mats = self.build(parts, motion, frame)
        if tables is not None: self.apply_expression(mats, F.face_for(tables, mood, viseme, eye_column, blink))
        self.apply_variants(mats, outfit, hair, outfit_group, outfit_variant, hair_color, ribbon)
        toon, edge = self.shading(outfit, hair, state, outfit_color, hair_color)
        return tris, mats, toon, edge

    def apply_expression(self, mats, expr):
        """expr = {'eye': n, 'brow': n, 'cheek': n, 'mouth': n}: face materials are named <p>_<slot>.<n>; a slot's texture (and its
        '<name>_pl' palette) is swapped for variant n. Palette names are truncated to 16 chars by the format."""
        for m in mats:
            mt = re.match(r'^%s_(eye|brow|cheek|mouth)\.(\d+)' % self.prefix, m.get('tex') or '')
            if not mt or mt.group(1) not in expr: continue
            tn = '%s_%s.%d' % (self.prefix, mt.group(1), expr[mt.group(1)])
            if tn in self.tex:
                m['tex'] = tn; m['pal'] = next((p for p in self.tex[tn][2] if p.startswith(tn + '_')), m.get('pal'))
        return mats

    def part_sets(self, outfit=0, hair=0):
        """[(part_kind, model_name)] in draw order"""
        p = self.prefix
        return [('body', '%s_01_%03d_body' % (p, outfit)), ('larm', '%s_01_%03d_larm' % (p, outfit)), ('rarm', '%s_01_%03d_rarm' % (p, outfit)),
                ('waist', '%s_01_%03d_waist' % (p, outfit)), ('leg', '%s_01_%03d_leg' % (p, outfit)), ('hair', '%s_00_%03d_hair' % (p, hair)),
                ('face', '%s_base_face_m' % p), ('lhand', '%s_base_lhand_00' % p), ('rhand', '%s_base_rhand_00' % p)]

    def anim_track(self, motion, kind, nn, names=None):
        ent = self.motions[motion]; want = {'body': 'body', 'face': 'body', 'larm': 'larm', 'lhand': 'larm', 'rarm': 'rarm', 'rhand': 'rarm',
                                            'waist': 'leg', 'leg': 'leg', 'hair': ('hair1' if nn == 20 else 'hair2' if names and 'hairT_lb_01' in names else 'hair0')}[kind]
        for a in ent['bca']:
            if a['name'].endswith('_' + want): return a, want
        return None, want

    def src_names(self, kind_track, anim):
        """joint names of the skeleton an animation track was authored for (first part model with that suffix and joint count)"""
        nn = max([t['node'] for t in anim['tags']] + [0]) + 1; n_tags = len(anim['tags'])
        suffix = {'body': '_body', 'larm': '_larm', 'rarm': '_rarm', 'leg': '_leg', 'hair0': '_hair', 'hair1': '_hair', 'hair2': '_hair', 'item': 'item'}[kind_track]
        marker = {'hair0': 'lpig', 'hair1': 'ribon', 'hair2': 'hairT_lb_01'}.get(kind_track)
        for name, (b, m) in self.parts.items():
            if name.endswith(suffix) and len(m['nodes']) == n_tags and (marker is None or marker in [n['name'] for n in m['nodes']]): return [n['name'] for n in m['nodes']]
        return None

    def build(self, parts, motion, frame, expr=None):
        """merged triangles (materials list is combined) for the heroine at `frame` of `motion`; expr = see apply_expression"""
        tris = []; mats = []
        for kind, name in parts:
            if name not in self.parts: continue
            d, md = self.parts[name]; nodes = md['nodes']; vis = None
            a, tr = self.anim_track(motion, kind, len(nodes), [n['name'] for n in nodes]) if motion else (None, None)
            if a is not None:
                src = self.src_names(tr, a)
                if src:
                    nodes = N.sample_anim_named(a, src, md['nodes'], frame)
                    bv = [v for v in self.motions[motion]['bva'] if v['name'].endswith('_' + tr)]
                    if bv:
                        by_name = {s: i for i, s in enumerate(src)}
                        names = [n['name'] for n in md['nodes']]
                        vis = lambda nid, bv=bv[0], by_name=by_name, names=names: N.visible(bv, frame, by_name.get(names[nid], 10 ** 6)) if names[nid] in by_name else True
            t = N.build_mesh(d, md, nodes=nodes, vis=vis); base = len(mats)
            for x in t: x['material'] += base
            mats += [dict(m) for m in md['materials']]; tris += t
        if expr: self.apply_expression(mats, expr)
        return tris, mats
