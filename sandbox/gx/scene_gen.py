"""Heroine scene -> GX command stream -> image, from game data only.

    .venv/bin/python sandbox/gx/scene_gen.py [--compare CAPTURE.npz] [--ref REF3D.npy] [--out OUT.png]

Scene state (see scene_data.py for where the game takes each value from) for the recorded break-room frame:
    Nene, hair label 0x3d, outfit label 0xfc, accessory 405, time mode 0, camera preset 3014,
    motion key 587, 3 game updates since that motion started.
Output: the two polygon lists of one 4-frame cycle (leg pass, main pass), each as packets; optional packet comparison
with a capture (gxcap.py) and a render compared with an emulator 3D-only frame.
"""
import argparse, os, struct, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'tools'))
import nnsgen  # noqa: E402
import scene_data as sd  # noqa: E402

SCENE = dict(heroine=2, hair_label=0x3d, outfit_label=0xfc, hand_variant=0, face_variant=0, accessory=405, time_mode=0,
             preset=3014, motion_key=587, updates=3, mood=22, viseme=5, blink=False, skin=0)


def expression_textures(scene):
    """face placeholders -> selected textures (expressions are texture swaps): face_tables (code tables 0x2125168 moods,
    0x2127ae4 eye table, mouth rows, viseme map) + mood (script command 0x34) + viseme (lipsync data of the playing voice
    at the current time; 5 = silence) + blink state"""
    import json
    sys.path.insert(0, os.path.join(ROOT, 'tools')); import face_tables as ft
    t = json.load(open(os.path.join(ROOT, 'content', 'engine', 'face_tables.json')))
    f = ft.face_for(t, scene['mood'], viseme=scene['viseme'], blink=scene['blink'])
    p = sd.HPREFIX[scene['heroine']]
    if scene.get('eye_tex') is not None:                 # eye texture from the blink machine (sd.blink_step)
        f = dict(f, eye=scene['eye_tex'])
    return {'%s_eye.0' % p: '%s_eye.%d' % (p, f['eye']), '%s_brow.0' % p: '%s_brow.%d' % (p, f['brow']),
            '%s_cheek.0' % p: '%s_cheek.%d' % (p, f['cheek']), '%s_mouth.0' % p: '%s_mouth.%d' % (p, f['mouth']),
            '%s_face_00' % p: '%s_face_%02d' % (p, scene['skin'])}


EXPR_ARCHIVES = ('tm_model_block_base.arc', 'async3d.arc')     # eye 0-22 in async3d, brow/cheek/mouth/face in tm_model_block_base


def expression_sources(names):
    """name -> texture set path (archive entry or bundle member) holding it"""
    import nns3d
    found = {}
    for arcname in EXPR_ARCHIVES:
        for fn in sorted(os.listdir(os.path.join(sd.A, arcname))):
            path = arcname + '/' + fn; d = sd.arc(path)
            blobs = [(path, d)] if d[:4] == b'BTX0' else []
            if not blobs and len(d) > 8 and struct.unpack_from('<I', d, 4)[0] == 0x28 and struct.unpack_from('<I', d)[0] < 64:
                try:
                    blobs = [(path + ':' + k, v) for k, v in nnsgen.bundle_entries(d).items() if v[:4] == b'BTX0']
                except Exception:
                    blobs = []
            for src, b in blobs:
                t = nns3d.parse_file(b)['tex0']
                for x in (t or {}).get('textures', []):
                    if x['name'] in names and x['name'] not in found:
                        found[x['name']] = src
    return found
TRACKED = None   # bone names (DTCM table 0x27e033c), loaded below
DTCM = open(os.path.join(ROOT, 'work', 'arm9_dtcm.bin'), 'rb').read()


def tracked_bones():
    """MotionAnimTable_FindIndexByName name table: DTCM 0x27e033c (pointers into the ARM9 image)"""
    out = []
    for i in range(32):
        p = struct.unpack_from('<I', DTCM, 0x33c + 4 * i)[0]
        if p == 0:
            break
        src = DTCM if 0x27e0000 <= p < 0x27e4000 else sd.MAIN
        o = p - (0x27e0000 if src is DTCM else 0x2000000)
        out.append(src[o:o + 16].split(b'\0')[0].decode('latin1'))
    return out


class TexVram:
    """synthetic texture / palette VRAM: each texture set gets a base (our allocator; the game's is the NNS VRAM manager)"""
    def __init__(self):
        self.tex = bytearray(0x80000); self.pal = bytearray(0x18000); self.tp = 0; self.pp = 0; self.sets = {}

    def load(self, path):
        if path in self.sets:
            return self.sets[path]
        import nns3d
        if ':' in path:
            f, member = path.split(':', 1); d = nnsgen.bundle_entries(sd.arc(f))[member]
        else:
            d = sd.arc(path)
        t = nns3d.parse_file(d)['tex0']
        if not t:
            self.sets[path] = None
            return None
        tb, pb = self.tp, self.pp
        raw = d[t['ofs_tex']:t['ofs_tex'] + t['size_tex']]
        self.tex[tb:tb + len(raw)] = raw; self.tp = (tb + len(raw) + 15) & ~15
        praw = d[t['ofs_pltt']:t['ofs_pltt'] + t['size_pltt']]
        self.pal[pb:pb + len(praw)] = praw; self.pp = (pb + len(praw) + 15) & ~15
        if t['ofs_tex4'] and t['ofs_idx4']:
            pass                                                # no 4x4-compressed textures in these sets (checked by census)
        self.sets[path] = (t, tb, pb)
        return self.sets[path]

    def binder(self, tex_sets, mat_names, swap=None, swap_src=None):
        def bind(m, idx, mt):
            tname, pname = mat_names[idx]
            if not tname:
                return mt['teximg'], mt['pltt']
            sets = tex_sets
            if swap and tname in swap:
                tname = swap[tname]; pname = tname + '_pl'; sets = [swap_src[tname]]
            for path in sets:
                ld = self.load(path)
                if ld is None:
                    continue
                t, tb, pb = ld
                tx = [x for x in t['textures'] if x['name'] == tname]
                if tx:
                    teximg = mt['teximg'] | (tx[0]['param'] + (tb >> 3))
                    pl = [p for p in t['palettes'] if p['name'] == pname]
                    pltt = ((pl[0]['offset'] + pb) >> (3 if tx[0]['fmt'] == 2 else 4)) if pl else 0
                    return teximg, pltt
            raise KeyError(tname)
        return bind


def generate(scene=SCENE):
    global TRACKED
    TRACKED = TRACKED or tracked_bones()
    h = scene['heroine']
    sc = nnsgen.SinCos(sd.MAIN)
    preset = None
    for d in sorted(os.listdir(sd.A)):
        if d.startswith('camera_data_'):
            for f in sorted(os.listdir(os.path.join(sd.A, d))):
                b = sd.arc(d + '/' + f)
                if len(b) == 164 and struct.unpack_from('<i', b)[0] == scene['preset']:
                    preset = struct.unpack('<41i', b)
    # globals: Camera_Reset projection, preset look-at (target z forced 0, up -X), lights, base
    far = 0x190000                                            # 0x258000 for presets selected by Model_ComputeAnimationTransform (not 3014)
    proj = nnsgen.mtx_perspective_w(sc.sin(1820), sc.cos(1820), 0x1555, 0x1000, far, 0xa000)
    cam = nnsgen.mtx_look_at(list(preset[1:4]), [-4096, 0, 0], [preset[4], preset[5], 0])
    lights = nnsgen.game_lights(sc, [(preset[11 + i], preset[8 + i]) for i in range(3)])
    base_rot = nnsgen.rot_y33(sc.sin(preset[7] & 0xffff), sc.cos(preset[7] & 0xffff))
    height = sd.code_table(0x207d95c, 3)[h]                  # Gfx3D_DrawActor per-heroine table
    glb = nnsgen.glb_from_code(sd.w32(0x20469e0) & 0x7fff)
    # wardrobe and motion
    ward = sd.wardrobe(h, scene['hair_label'], scene['outfit_label'], scene['hand_variant'], scene['face_variant'])
    idx, bundle, stem = sd.motion_stem(scene['motion_key'], h, sd.label_class(scene['outfit_label']))
    anims = nnsgen.load_bca(nnsgen.bundle_entries(sd.arc('async3d.arc/%04d.bin' % bundle))[stem + '.nsbca'])
    models = {p: nnsgen.Model(sd.arc(m)) for p, (m, t) in ward.items()}
    tex_sets = [t for p, (m, t) in ward.items() if t] + [p for p, (m, t) in ward.items()]   # sets loaded with the parts
    vram = TexVram()
    swap = expression_textures(scene); swap_src = expression_sources(set(swap.values()))
    import nns3d
    mat_names = {p: [(x['tex'], x['pal']) for x in nns3d.parse_file(sd.arc(m))['models'][0]['materials']] for p, (m, t) in ward.items()}
    # joint records: compute passes (own animations), then the game's copy callbacks
    own = {'hair': 'hair0', 'body': 'body', 'rarm': 'rarm', 'larm': 'larm', 'leg': 'leg'}
    rec = {}
    for p, a in own.items():
        an = anims['%s_%s' % (stem, a)]; m = models[p]
        fr = sd.motion_frame(scene['updates'], an.nframe) << 12
        rec[p] = [an.result(n, fr, m) if n < an.nnode else m.node_result(n) for n in range(m.nnode)]
    rec['rhand'] = [rec['rarm'][n] for n in range(8)]                          # Matrix_TransformVector3
    rec['lhand'] = [rec['larm'][n] for n in range(8)]                          # Matrix_Multiply43
    rec['waist'] = [rec['leg'][n] for n in (0, 1, 4, 7, 8, 9, 10, 11)]          # Matrix_Multiply33 (class != 2)
    bones = {}; captured = set(); state = {'facing': None}

    def bone_cb(draw, node, timing):                                            # Model_ApplyNamedBoneMtx
        if timing != 'C':
            return False
        nm = draw.m.node_names[node]
        if nm in TRACKED and nm not in captured:
            captured.add(nm)
            bones[nm] = ([r[:3] for r in draw.ge.vec[:3]], draw.ge.pos[3][:3])
            for c, ps in ((0x10, [0]), (0x11, []), (0x15, []), (0x12, [1]), (0x10, [2])):
                draw.emit_direct(c, ps)
        return False
    ge = nnsgen.GE()

    def part_draw(part, out):
        m = models[part]
        out += nnsgen.flush_vp(proj, cam, lights, glb, base_rot, [0, height, 0], [4096] * 3)
        for c, ps in out[-20:]:
            ge.run(c, ps)
        if state['facing'] is None and 'Head' in bones:
            state['facing'] = nnsgen.facing_test(bones['Head'][0], bones['Head'][1], list(preset[1:4]))[0]
        dr = nnsgen.Draw(m, ge, out, rec[part], glb=glb, node_cb=(bone_cb if part in ('hair', 'body', 'rhand', 'lhand') else None))
        dr.tex_bind = vram.binder(tex_sets, mat_names[part], swap, swap_src)
        dr.poly_id = nnsgen.polygon_ids(part, m.mat_names, h, bool(state['facing']))
        dr.run()

    def accessory_draw(out):
        mp = 'block3d.arc/%04d.bin' % sd.accessory_entry(scene['accessory'], 0, 0)
        tp = 'block3d.arc/%04d.bin' % sd.accessory_entry(scene['accessory'], 0, 1)
        m = nnsgen.Model(sd.arc(mp))
        d = sd.arc('block3d.arc/0202.bin')                                      # Model_ComputePartScaleOffsets
        ofs = None
        for k in range(struct.unpack_from('<i', d)[0]):
            if struct.unpack_from('<i', d, 4 + 40 * k)[0] == scene['accessory'] % 100:
                ofs = [int(v * 4096) for v in struct.unpack_from('<3f', d, 4 + 40 * k + 4 + 12 * h)]
        out += nnsgen.flush_vp(proj, cam, lights, glb, base_rot, ofs, [4096] * 3)
        for c, ps in out[-20:]:
            ge.run(c, ps)
        rows, hp = bones['Head']

        def acc_cb(draw, node, timing):                                         # Mod2045ee8_ProcessessGraphics_6
            if timing != 'A':
                return False
            draw.emit(0x1c, nnsgen.multvec33(ofs, rows)); draw.emit(0x17, [x for r in rows for x in r] + list(hp))
            draw.emit(0x19, [4096, 0, 0, 0, 4096, 0, 0, 0, 4096] + ofs)
            return True
        dr = nnsgen.Draw(m, ge, out, [None] * m.nnode, glb=glb, node_cb=acc_cb)
        dr.tex_bind = vram.binder([tp], [(x['tex'], x['pal']) for x in nns3d.parse_file(sd.arc(mp))['models'][0]['materials']])
        dr.light_patch = False
        k0 = nnsgen.POLYID_TABLE[0 + (0 if state['facing'] else 3)]
        dr.poly_id = {i: k0 for i in range(m.nmat)}
        dr.run()
    # bones/distances come from the previous update's draws; the Head bone is known before the frame (static pose here),
    # so evaluate it once from the hair chain for the visibility test (Model_UpdateCharaScene -> Model_ApplyVariantToParts)
    import math
    probe = []; part_draw('hair', probe); bones_prev = dict(bones); captured.clear(); bones.clear(); state['facing'] = None
    hp = bones_prev['Head'][1]; eye = list(preset[1:4])
    dist = int(math.sqrt(sum((eye[i] - hp[i]) ** 2 for i in range(3))))      # VEC_Distance (fx32)
    slots = sd.visible_slots(scene['preset'], dist)
    state['face_variant'] = sd.face_variant(scene['preset'], dist); state['head_distance'] = dist / 4096
    ge.__init__()
    rec['face_m'] = rec['hair'][:6]; rec['face_l'] = rec['hair'][:6]          # faces share the hair's record buffer
    lists = {}
    for name, phase in (('legs', 0), ('main', 1)):
        out = []
        for slot in slots:
            if sd.draw_phase(slot) == phase:
                part_draw(sd.SLOT_NAMES[slot], out)
        if phase == 1:
            accessory_draw(out)                                                  # accessories: draw phase 1
        lists[name] = out
    toon, edge = sd.shading_tables(h, scene['hair_label'], scene['outfit_label'], scene['time_mode'], bool(state['facing']))
    return lists, vram, toon, edge, state


def compare(lists, capture):
    from gx import packets, s32
    z = np.load(capture)
    pk = packets(z['cmd'], z['param'], z['frame'])
    sw = [i for i, p in enumerate(pk) if p[0] == 0x50]
    res = {}
    for name, swap in (('legs', 6380), ('main', 6381)):
        end = next(sw[i] for i in range(1, len(sw)) if pk[sw[i]][2] == swap); start = sw[sw.index(end) - 1] + 1
        cap = [(c, ps) for c, ps, f, i in pk[start:end]]
        gen = lists[name]
        # walk the capture, skipping only compute-pass NODEMIX blocks (they sit between part draws) and the
        # composition commands; every generated packet must appear in order
        ci = next(j for j in range(len(cap) - 5) if cap[j][0] == 0x10 and cap[j + 1][0] == 0x16 and cap[j + 2][0] == 0x19
                  and cap[j + 4][0] == 0x15 and cap[j + 5][0] == 0x32)        # first part draw (FlushVP); before it: composition pass
        exact = 0; matched = 0
        for g in gen:
            while ci < len(cap) and cap[ci][0] != g[0]:
                ci += 1
            if ci >= len(cap):
                break
            c = cap[ci]; ci += 1; matched += 1
            gp, cp = [s32(x) for x in g[1]], [s32(x) for x in c[1]]
            if g[0] == 0x2a:
                ok = (gp[0] & ~0xffff) == (cp[0] & ~0xffff)
            elif g[0] == 0x2b:
                ok = True
            elif g[0] == 0x17 and gp[:9] == cp[:9] and gp[9:] != cp[9:] and len(set(cp[9:])) > 1 and abs(cp[9]) > 1 << 24:
                ok = True                                    # NODEMIX 3x3 load: trailing stack words
            else:
                ok = gp == cp
            exact += ok
        res[name] = (len(gen), matched, exact)
    return res


def render(lists, vram, toon, edge, disp3dcnt=0x39):
    from gx import Geo
    from render import render as rnd, to_rgb8
    g = Geo()
    g.run([(c, ps, 0, i) for i, (c, ps) in enumerate(lists['main'] + [(0x50, [0])])])
    polys = g.lists[-1][2]
    col, alpha, _, _ = rnd(polys, np.frombuffer(bytes(vram.tex), np.uint8), np.frombuffer(bytes(vram.pal), np.uint8), toon, edge, disp3dcnt)
    img = to_rgb8(col); img[alpha == 0] = 0
    return img, alpha


if __name__ == '__main__':
    ap = argparse.ArgumentParser()
    ap.add_argument('--compare'); ap.add_argument('--ref'); ap.add_argument('--out')
    a = ap.parse_args()
    lists, vram, toon, edge, state = generate()
    print('generated: leg pass %d packets, main pass %d packets; facing=%s; face variant %s (head distance %.1f)' % (
        len(lists['legs']), len(lists['main']), state['facing'], state['face_variant'], state['head_distance']))
    if a.compare:
        for k, (n, m, e) in compare(lists, a.compare).items():
            print('  %s: %d generated, %d found in order in the capture, %d exact' % (k, n, m, e))
    if a.ref:
        from PIL import Image
        from render import to6, rgb555
        img, alpha = render(lists, vram, toon, edge)
        ref = np.load(a.ref); refrgb = (to6(rgb555(ref & 0x7fff)) * 255 // 63).astype(np.uint8); refm = (ref & 0x7fff) != 0
        m = alpha > 0; both = m & refm
        d = np.abs(img.astype(int) - refrgb.astype(int)).max(-1)
        print('render vs emulator 3D-only frame: IoU %.4f, median diff %d, exact %.1f%%' % ((m & refm).sum() / (m | refm).sum(), np.median(d[both]), 100 * (d[both] == 0).mean()))
        if a.out:
            panels = [Image.fromarray(x).rotate(90, expand=True) for x in (refrgb, img, np.stack([np.clip(d * 4, 0, 255).astype(np.uint8)] * 3, -1))]
            out = Image.new('RGB', (sum(p.width for p in panels) + 40, panels[0].height), (40, 40, 40))
            for k, p in enumerate(panels):
                out.paste(p, (k * (p.width + 20), 0))
            out.resize((out.width * 2, out.height * 2), Image.NEAREST).save(a.out)
            print('wrote', a.out)


TAIL_CALC = nnsgen.nodemix_stack_tail(sd.MAIN, via_pg9=False)
TAIL_DRAW = nnsgen.nodemix_stack_tail(sd.MAIN, via_pg9=True)


class SceneRenderer:
    """Stateful per-update renderer: the game's object loop (Mod2045ee8_ProcessessGraphics_10) for the heroine's parts and
    accessory, sharing one geometry-engine state across updates."""
    CALC_PHASE = None

    def __init__(self, scene):
        import nns3d
        global TRACKED
        TRACKED = TRACKED or tracked_bones()
        self.scene = scene; h = scene['heroine']; self.h = h
        sc = nnsgen.SinCos(sd.MAIN); self.sc = sc
        for d in sorted(os.listdir(sd.A)):
            if d.startswith('camera_data_'):
                for f in sorted(os.listdir(os.path.join(sd.A, d))):
                    b = sd.arc(d + '/' + f)
                    if len(b) == 164 and struct.unpack_from('<i', b)[0] == scene['preset']:
                        self.preset = struct.unpack('<41i', b)
        pr = self.preset
        self.proj = nnsgen.mtx_perspective_w(sc.sin(1820), sc.cos(1820), 0x1555, 0x1000, 0x190000, 0xa000)
        self.cam = nnsgen.mtx_look_at(list(pr[1:4]), [-4096, 0, 0], [pr[4], pr[5], 0])
        self.lights = nnsgen.game_lights(sc, [(pr[11 + i], pr[8 + i]) for i in range(3)])
        self.base_rot = nnsgen.rot_y33(sc.sin(pr[7] & 0xffff), sc.cos(pr[7] & 0xffff))
        self.height = sd.code_table(0x207d95c, 3)[h]
        self.glb = nnsgen.glb_from_code(sd.w32(0x20469e0) & 0x7fff)
        self.ward = sd.wardrobe(h, scene['hair_label'], scene['outfit_label'], scene['hand_variant'], scene['face_variant'])
        if scene.get('hands'):                           # hand poses from the motion's lhand/rhand tracks (Gfx3D_UpdateActorState):
            import motion_sim                            # model = async3d entry (per-heroine base 0x2102e50/52) + pose
            for part, k, v in (('lhand', 'l', scene['hands'][0]), ('rhand', 'r', scene['hands'][1])):
                if v is not None:
                    self.ward[part] = ('async3d.arc/%04d.bin' % (motion_sim.HAND_BASE[k][h] + v), self.ward[part][1])
        idx, bundle, stem = sd.motion_stem(scene['motion_key'], h, sd.label_class(scene['outfit_label']))
        self.stem = stem
        self.anims = nnsgen.load_bca(nnsgen.bundle_entries(sd.arc('async3d.arc/%04d.bin' % bundle))[stem + '.nsbca'])
        # motion cross-fade (scene['blend'] = {prev_key, prev_updates, blend}): layer 1 = the previous motion, which keeps
        # counting from its own frame; ratios from the game's schedule (nnsgen.motion_blend_ratios)
        self.blend = None
        bl = scene.get('blend')
        if bl:
            ratios = nnsgen.motion_blend_ratios(bl['blend'], scene['updates'])
            if ratios is not None:
                _, pb, pstem = sd.motion_stem(bl['prev_key'], h, sd.label_class(scene['outfit_label']))
                self.blend = (ratios, pstem, nnsgen.load_bca(nnsgen.bundle_entries(sd.arc('async3d.arc/%04d.bin' % pb))[pstem + '.nsbca']),
                              bl['prev_updates'] + scene['updates'])
        self.models = {p: nnsgen.Model(sd.arc(m)) for p, (m, t) in self.ward.items()}
        self.mat_names = {p: [(x['tex'], x['pal']) for x in nns3d.parse_file(sd.arc(m))['models'][0]['materials']] for p, (m, t) in self.ward.items()}
        order = [self.ward[sd.SLOT_NAMES[s]][1] for s in range(10) if self.ward[sd.SLOT_NAMES[s]][1]]
        order.append('block3d.arc/%04d.bin' % sd.accessory_entry(scene['accessory'], 0, 1))
        self.tex_sets = order
        self.vram = GameVram(order)
        self.swap = expression_textures(scene); self.swap_src = expression_sources(set(self.swap.values()))
        self.vram.place_expressions(self.ward['body'][1], self.swap, self.swap_src)
        tbl = sd.w32(sd.w32(0x2078d7c))                                       # Model_SetPartPaletteFromTable compute phases
        self.calc_phase = [x - 256 if x > 127 else x for x in sd.MAIN[tbl - 0x2000000:tbl - 0x2000000 + 11]]
        self.rec = {}
        self.ge = nnsgen.GE(); self.bones = {}; self.captured = set(); self.facing = False
        self.last_base = (self.base_rot, [0, self.height, 0])
        self.own = {'hair': 'hair0', 'body': 'body', 'rarm': 'rarm', 'larm': 'larm', 'leg': 'leg'}
        # face visibility: Head distance from the previous update (static pose: evaluate once)
        self.slots = list(range(10))

    def frame_fx(self, part):
        an = self.anims['%s_%s' % (self.stem, self.own[part])]
        return sd.motion_frame(self.scene['updates'], an.nframe) << 12

    def sets_for(self, part):
        """a part binds to its own slot's texture set first (the model/texture pair the game loads for the slot)"""
        own = self.ward[part][1]
        return ([own] if own else []) + [x for x in self.tex_sets if x != own]

    def _bone_cb(self, draw, node, timing):
        if timing != 'C' or draw.record or draw.nogecmd:
            return False
        nm = draw.m.node_names[node]
        if nm in TRACKED and nm not in self.captured:
            self.captured.add(nm)
            self.bones[nm] = ([r[:3] for r in draw.ge.vec[:3]], draw.ge.pos[3][:3])
            for c, ps in ((0x10, [0]), (0x11, []), (0x15, []), (0x12, [1]), (0x10, [2])):
                draw.emit_direct(c, ps)
        return False

    def _copy_records(self, part):
        """the arm/leg NODEDESC callbacks (last node) copy records into the hands / waist"""
        if part == 'rarm':
            self.rec['rhand'] = [self.rec['rarm'][n] for n in range(8)]
        if part == 'larm':
            self.rec['lhand'] = [self.rec['larm'][n] for n in range(8)]
        if part == 'leg':
            self.rec['waist'] = [self.rec['leg'][n] for n in (0, 1, 4, 7, 8, 9, 10, 11)]
        if part == 'hair':
            self.rec['face_m'] = self.rec['hair'][:6]; self.rec['face_l'] = self.rec['hair'][:6]

    _bca_cache = {}

    def layer_anim(self, key, part):
        if key not in self._bca_cache:
            if isinstance(key, str):                   # 'stem@bundle' (clip identified directly)
                stem, b = key.split('@'); b = int(b)
            else:
                _, b, stem = sd.motion_stem(key, self.h, sd.label_class(self.scene['outfit_label']))
            self._bca_cache[key] = (stem, nnsgen.load_bca(nnsgen.bundle_entries(sd.arc('async3d.arc/%04d.bin' % b))[stem + '.nsbca']))
        stem, an = self._bca_cache[key]
        return an['%s_%s' % (stem, self.own[part])]

    def part_anim(self, part):
        layers = self.scene.get('anim_layers', {}).get(part)
        if layers:                                     # explicit AnmObj chain [(motion key, frame, ratio), ...], head first
            return nnsgen.AnimChain([(self.layer_anim(k, part), f << 12, r) for k, f, r in layers])
        an = self.anims['%s_%s' % (self.stem, self.own[part])]
        if self.blend is None:
            return an
        ratios, pstem, panims, pupd = self.blend
        old = panims['%s_%s' % (pstem, self.own[part])]
        return nnsgen.AnimChain([(an, self.frame_fx(part), ratios[0]),
                                 (old, sd.motion_frame(pupd, old.nframe) << 12, ratios[1])])

    def calc(self, part, out):
        """compute pass: NNS_G3dDraw with RECORD | NOGECMD (only NODEMIX reaches the geometry engine)"""
        m = self.models[part]; an = self.part_anim(part)
        rec = [None] * m.nnode
        dr = nnsgen.Draw(m, self.ge, out, rec, anim=an, frame_fx=self.frame_fx(part), flags=3, glb=self.glb)
        dr.stack_tail = TAIL_CALC                     # PG10 -> NNS_G3dDraw (0x204696c)
        dr.tex_bind = self.vram.binder(self.sets_for(part), self.mat_names[part])
        dr.run()
        self.rec[part] = rec
        self._copy_records(part)

    def draw(self, part, out):
        m = self.models[part]
        vp = nnsgen.flush_vp(self.proj, self.cam, self.lights, self.glb, self.base_rot, [0, self.height, 0], [4096] * 3)
        for c, ps in vp:
            self.ge.run(c, ps)
        out += vp
        self.last_base = (self.base_rot, [0, self.height, 0])
        if 'Head' in self.bones:
            self.facing = nnsgen.facing_test(self.bones['Head'][0], self.bones['Head'][1], list(self.preset[1:4]))[0]
        dr = nnsgen.Draw(m, self.ge, out, self.rec[part], glb=self.glb, node_cb=(self._bone_cb if part in ('hair', 'body', 'rhand', 'lhand') else None))
        dr.stack_tail = TAIL_DRAW                     # PG10 -> PG9 -> NNS_G3dDraw (0x2046844)
        dr.tex_bind = self.vram.binder(self.sets_for(part), self.mat_names[part])
        dr.poly_id = nnsgen.polygon_ids(part, m.mat_names, self.h, bool(self.facing))
        dr.run()
        self._copy_records(part)

    def draw_accessory(self, out):
        import nns3d
        sc = self.scene
        mp = 'block3d.arc/%04d.bin' % sd.accessory_entry(sc['accessory'], 0, 0)
        tp = 'block3d.arc/%04d.bin' % sd.accessory_entry(sc['accessory'], 0, 1)
        m = nnsgen.Model(sd.arc(mp)); d = sd.arc('block3d.arc/0202.bin'); ofs = None
        for k in range(struct.unpack_from('<i', d)[0]):
            if struct.unpack_from('<i', d, 4 + 40 * k)[0] == sc['accessory'] % 100:
                ofs = [int(v * 4096) for v in struct.unpack_from('<3f', d, 4 + 40 * k + 4 + 12 * self.h)]
        vp = nnsgen.flush_vp(self.proj, self.cam, self.lights, self.glb, self.base_rot, ofs, [4096] * 3)
        for c, ps in vp:
            self.ge.run(c, ps)
        out += vp
        self.last_base = (self.base_rot, ofs)
        rows, hp = self.bones['Head']

        def acc_cb(draw, node, timing):
            if timing != 'A':
                return False
            draw.emit(0x1c, nnsgen.multvec33(ofs, rows)); draw.emit(0x17, [x for r in rows for x in r] + list(hp))
            draw.emit(0x19, [4096, 0, 0, 0, 4096, 0, 0, 0, 4096] + ofs)
            return True
        dr = nnsgen.Draw(m, self.ge, out, [None] * m.nnode, glb=self.glb, node_cb=acc_cb)
        dr.stack_tail = TAIL_DRAW
        dr.tex_bind = self.vram.binder(self.tex_sets, [(x['tex'], x['pal']) for x in nns3d.parse_file(sd.arc(mp))['models'][0]['materials']])
        dr.light_patch = False
        dr.poly_id = {i: nnsgen.POLYID_TABLE[0 + (0 if self.facing else 3)] for i in range(m.nmat)}
        dr.run()

    def object_loop(self, phase, draw_byte):
        """Mod2045ee8_ProcessessGraphics_10's loop over the object array (parts in slot order, then the accessory)"""
        out = []
        if phase == 0:
            self.captured.clear()                                             # Model_UpdateCharaScene resets the bone table
            if 'Head' in self.bones:
                import math
                hp = self.bones['Head'][1]; eye = list(self.preset[1:4])
                dist = int(math.sqrt(sum((eye[i] - hp[i]) ** 2 for i in range(3))))
                self.slots = sd.visible_slots(self.scene['preset'], dist)
        for slot in range(10):
            part = sd.SLOT_NAMES[slot]
            if self.calc_phase[slot] == phase:
                self.calc(part, out)
            if slot in self.slots and sd.draw_phase(slot) == draw_byte and part in self.rec:
                self.draw(part, out)
        if draw_byte == 1:
            self.draw_accessory(out)
        return out


class GameVram(TexVram):
    """The game's own texture/palette VRAM layout (VramBank_Alloc: bump allocators, textures aligned to 0x80, palettes to
    0x20). Entering the two-phase 3D mode clears the regions and Gfx3D_LoadResources loads the 47 effect_3d textures
    (entry ids from the table at 0x209f9e8; GTD header +0xc texture size, +10 palette size); that point is the mark the
    scene rolls back to. The character's texture sets are then allocated in slot order (Model_LoadPartResources via
    GfxRes_ProcessessGraphics, sets without TEX0 skipped), then accessories. Expression textures do not allocate: the
    game copies them over the face placeholders (VRAM transfers)."""
    def __init__(self, load_order):
        TexVram.__init__(self)
        import glob
        tbl = sd.w32(0x209f9e8)
        ids = [b - 256 if b > 127 else b for b in sd.MAIN[tbl - 0x2000000:tbl - 0x2000000 + 0x2f]]
        files = {int(os.path.basename(p)[:4]): p for p in glob.glob(os.path.join(sd.A, 'effect_3d.arc', '*'))}
        t = p = 0
        for e in ids:
            d = open(files[e], 'rb').read()
            tsz = struct.unpack_from('<I', d, 0xc)[0]; psz = struct.unpack_from('<H', d, 10)[0]
            t = (t + 0x7f) & ~0x7f; p = (p + 0x1f) & ~0x1f
            toff, poff = struct.unpack_from('<H', d, 10)[0], struct.unpack_from('<H', d, 8)[0]
            self.tex[t:t + tsz] = d[poff + toff:poff + toff + tsz]; self.pal[p:p + psz] = d[poff:poff + psz]
            t += tsz; p += psz
        self.tp, self.pp = t, p
        self.mark = (t, p)
        for path in load_order:
            self.load(path)

    def load(self, path):
        if path in self.sets:
            return self.sets[path]
        import nns3d
        if ':' in path:
            f, member = path.split(':', 1); d = nnsgen.bundle_entries(sd.arc(f))[member]
        else:
            d = sd.arc(path)
        t = nns3d.parse_file(d)['tex0']
        if not t or not t['size_tex']:
            self.sets[path] = None
            return None
        tb = (self.tp + 0x7f) & ~0x7f; pb = (self.pp + 0x1f) & ~0x1f
        self.tex[tb:tb + t['size_tex']] = d[t['ofs_tex']:t['ofs_tex'] + t['size_tex']]
        self.pal[pb:pb + t['size_pltt']] = d[t['ofs_pltt']:t['ofs_pltt'] + t['size_pltt']]
        self.tp, self.pp = tb + t['size_tex'], pb + t['size_pltt']
        self.sets[path] = (t, tb, pb)
        return self.sets[path]

    def place_expressions(self, owner_set, swap, swap_src):
        """copy each selected expression texture (and palette) over its placeholder in the owner set"""
        import nns3d
        t, tb, pb = self.sets[owner_set]
        for ph, real in swap.items():
            x = [q for q in t['textures'] if q['name'] == ph]
            pl = [q for q in t['palettes'] if q['name'] == ph + '_pl']
            if not x:
                continue
            src = swap_src[real]
            f, member = (src.split(':', 1) + [None])[:2]
            d = nnsgen.bundle_entries(sd.arc(f))[member] if member else sd.arc(f)
            st = nns3d.parse_file(d)['tex0']
            sx = [q for q in st['textures'] if q['name'] == real][0]
            n = {1: 1, 2: 0.25, 3: 0.5, 4: 1, 5: 0.25, 6: 1, 7: 2}[sx['fmt']] * sx['w'] * sx['h']
            self.tex[tb + x[0]['addr']:tb + x[0]['addr'] + int(n)] = d[st['ofs_tex'] + sx['addr']:st['ofs_tex'] + sx['addr'] + int(n)]
            spl = [q for q in st['palettes'] if q['name'] == real + '_pl']
            if pl and spl:
                psz = {1: 64, 2: 8, 3: 32, 4: 512, 5: 8, 6: 16, 7: 0}[sx['fmt']]
                self.pal[pb + pl[0]['offset']:pb + pl[0]['offset'] + psz] = d[st['ofs_pltt'] + spl[0]['offset']:st['ofs_pltt'] + spl[0]['offset'] + psz]
