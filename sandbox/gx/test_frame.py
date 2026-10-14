"""Generate every heroine part draw of one captured polygon list from game data and diff it against the capture.

    .venv/bin/python sandbox/gx/test_frame.py CAPTURE.npz SWAP_FRAME

Inputs that are not yet derived from game rules are listed in WARDROBE / SCENE below with their provenance.
Ignored in the comparison (documented in docs/RENDER_PRIMITIVES.md): the 3 trailing words of NODEMIX's 3x3 load
(stack/pointer garbage, dead), and VRAM base addresses inside TEXIMAGE_PARAM / PLTT_BASE (VRAM manager output).
"""
import os, struct, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'tools'))
from gx import packets, NAME, s32  # noqa: E402
import nnsgen  # noqa: E402
from test_gen import TRACKED  # noqa: E402

A = os.path.join(ROOT, 'content', 'assets', 'arc')
rd = lambda p: open(os.path.join(A, p), 'rb').read()

# ---- wardrobe for this scene: content files (archive entry ids from the load trace; selection rule = tm_model_label, open)
WARDROBE = [  # (part, model file, animation sub-name or record source)
    ('hair', 'n_block3d_00.arc/0071.bin', 'hair0'),
    ('body', 'n_block3d_01.arc/0141.bin', 'body'),
    ('rarm', 'n_block3d_01.arc/0144.bin', 'rarm'),
    ('rhand', 'tm_model_block_base.arc/0082.bin', ('rarm', 8)),     # Matrix_TransformVector3 copies rarm records 0..7
    ('larm', 'n_block3d_01.arc/0143.bin', 'larm'),
    ('lhand', 'tm_model_block_base.arc/0080.bin', ('larm', 8)),     # Matrix_Multiply43 copies larm records 0..7
    ('waist', 'n_block3d_01.arc/0146.bin', ('leg', [0, 1, 4, 7, 8, 9, 10, 11])),   # Matrix_Multiply33 (class != 2)
    ('leg', 'n_block3d_01.arc/0148.bin', 'leg'),
]
HEROINE = 2
HEIGHT = [0, -21488, 8380]          # Gfx3D_DrawActor table @0x2102e2c
EYE = [-73728, 622592, 430080]   # camera preset 3014 words 1-3 (Camera_SetPreset)
FACING = None    # computed below from the Head bone matrix and the preset eye
SCENE = {'motion_bundle': 'async3d.arc/0092.bin', 'motion': 'm_0230_10', 'frame': 3}


def index_textures():
    """texture name -> [(source, TEX0 parse)] over every BTX0/BMD0 file and bundle entry in content"""
    sys.path.insert(0, os.path.join(ROOT, 'tools')); import nns3d
    idx = {}
    for root, _, fs in os.walk(A):
        for f in fs:
            p = os.path.join(root, f); d = open(p, 'rb').read()
            blobs = []
            if d[:4] in (b'BTX0', b'BMD0'):
                blobs.append((os.path.relpath(p, A), d))
            elif len(d) > 8 and struct.unpack_from('<I', d, 4)[0] == 0x28 and struct.unpack_from('<I', d)[0] < 64:
                try:
                    for nm, blob in nnsgen.bundle_entries(d).items():
                        if blob[:4] in (b'BTX0', b'BMD0'):
                            blobs.append((os.path.relpath(p, A) + ':' + nm, blob))
                except Exception:
                    pass
            for src, blob in blobs:
                try:
                    t = nns3d.parse_file(blob)['tex0']
                except Exception:
                    continue
                if t:
                    for tx in t['textures']:
                        idx.setdefault(tx['name'], []).append((src, t))
    return idx


def mat_tex_names(model_bytes):
    import nns3d
    m = nns3d.parse_file(model_bytes)['models'][0]
    return [(x['tex'], x['pal']) for x in m['materials']]


def captured_draws(z, swap):
    pk = packets(z['cmd'], z['param'], z['frame'])
    sw = [i for i, p in enumerate(pk) if p[0] == 0x50]
    end = next(sw[i] for i in range(1, len(sw)) if pk[sw[i]][2] == swap); start = sw[sw.index(end) - 1] + 1
    seg = [(c, ps) for c, ps, f, i in pk[start:end]]
    starts = [j for j in range(len(seg) - 5) if seg[j][0] == 0x10 and seg[j + 1][0] == 0x16 and seg[j + 2][0] == 0x19 and seg[j + 4][0] == 0x15 and seg[j + 5][0] == 0x32]
    return seg, starts


def same(g, c):
    if g[0] != c[0]:
        return False
    gp, cp = [s32(x) for x in g[1]], [s32(x) for x in c[1]]
    if g[0] == 0x2a:
        return (gp[0] & ~0xffff) == (cp[0] & ~0xffff)          # VRAM address bits are allocator output
    if g[0] == 0x2b:
        return True
    return gp == cp


MAIN = open(os.path.join(ROOT, 'work', 'arm9_main.bin'), 'rb').read()
SC = nnsgen.SinCos(MAIN)
PRESET = struct.unpack('<41i', rd('camera_data_2.bin/0014.bin'))          # camera preset 3014 (script command 6)
PROJ = nnsgen.mtx_perspective_w(SC.sin(1820), SC.cos(1820), 0x1555, 0x1000, 0x190000, 0xa000)   # Camera_Reset constants
CAM = nnsgen.mtx_look_at(list(PRESET[1:4]), [-4096, 0, 0], [PRESET[4], PRESET[5], 0])
LIGHTS = nnsgen.game_lights(SC, [(PRESET[11 + i], PRESET[8 + i]) for i in range(3)])
BASE_ROT = nnsgen.rot_y33(SC.sin(PRESET[7] & 0xffff), SC.cos(PRESET[7] & 0xffff))


ACCESSORY = {'model': 'block3d.arc/0106.bin', 'id': 405}     # chara accessory slot 0 (id from the wardrobe, open)


def accessory_offset(acc_id, heroine):
    """Model_ComputePartScaleOffsets: block3d.arc/0202 float table, _ffix(x * 4096.0) (truncating)"""
    d = rd('block3d.arc/0202.bin')
    for k in range(struct.unpack_from('<i', d)[0]):
        if struct.unpack_from('<i', d, 4 + 40 * k)[0] == acc_id % 100:
            return [int(v * 4096) for v in struct.unpack_from('<3f', d, 4 + 40 * k + 4 + 12 * heroine)]


def main():
    z = np.load(sys.argv[1])
    plan = [(6380, ['leg']), (6381, ['hair', 'body', 'rarm', 'rhand', 'larm', 'lhand', 'waist', 'cap'])]
    state = {}
    grand = [0, 0]
    for swap, parts in plan:
        e, t = gen_list(z, swap, parts, state)
        grand[0] += e; grand[1] += t
    print('ALL LISTS: exact %d / %d generated packets' % tuple(grand))


def gen_list(z, swap, plan_parts, state):
    seg, starts = captured_draws(z, swap)
    print('--- list swapped at', swap, ':', len(starts), 'part draws captured')
    bundle = nnsgen.bundle_entries(rd(SCENE['motion_bundle']))
    anims = nnsgen.load_bca(bundle[SCENE['motion'] + '.nsbca'])
    frame_fx = SCENE['frame'] << 12
    tex_index = state.setdefault('tex_index', None) or index_textures(); state['tex_index'] = tex_index
    glb = nnsgen.glb_from_code(0x4210)
    records = state.setdefault('records', {})
    captured_bones = state.setdefault('captured_bones', set())

    bones = state.setdefault('bones', {})

    def bone_cb(draw, node, timing):
        if timing != 'C':
            return False
        nm = draw.m.node_names[node]
        if nm in TRACKED and nm not in captured_bones:
            captured_bones.add(nm)
            bones[nm] = ([r[:3] for r in draw.ge.vec[:3]], draw.ge.pos[3][:3])   # NNS_G3dGetCurrentMtx: vector rows + position translation (projection = identity)
            for c, ps in ((0x10, [0]), (0x11, []), (0x15, []), (0x12, [1]), (0x10, [2])):
                draw.emit_direct(c, ps)
    for part, mfile, src in WARDROBE:            # compute passes (flags 3) record every animated part first
        if part in records:
            continue
        if isinstance(src, str):
            model = nnsgen.Model(rd(mfile)); an = anims['%s_%s' % (SCENE['motion'], src)]
            records[part] = [an.result(n, frame_fx, model) if n < an.nnode else model.node_result(n) for n in range(model.nnode)]
    ge = nnsgen.GE()
    pos = starts[0]; total = exact = 0; report = []
    wd = {p: (m, s) for p, m, s in WARDROBE}
    for part in plan_parts:
        if part == 'cap':
            res = draw_accessory(seg, starts, pos, ge, glb, bones, tex_index)
            exact += res[0]; total += res[1]; report.append(res[2]); pos = res[3]
            continue
        mfile, src = wd[part]
        mb = rd(mfile); model = nnsgen.Model(mb)
        # joint results: own animation, or copied records of another part (the game's NODEDESC callbacks)
        rec = [None] * model.nnode
        if isinstance(src, str):
            rec = records[part]
        else:
            other, sel = src
            sel = list(range(sel)) if isinstance(sel, int) else sel
            for n in range(model.nnode):
                rec[n] = records[other][sel[n]] if n < len(sel) else model.node_result(n)
        # texture binding by name (texture set chosen = unique file containing the material's texture)
        names = mat_tex_names(mb)

        def tex_bind(m, idx, mt, names=names):
            tname, pname = names[idx]
            if not tname:
                return mt['teximg'], mt['pltt']
            srcs = tex_index.get(tname, [])
            t = srcs[0][1]; tx = [x for x in t['textures'] if x['name'] == tname][0]
            return mt['teximg'] | tx['param'], mt['pltt']
        # FlushVP generated from game data (camera preset, Camera_Reset constants, light tables, base = RotY(word 7) + height)
        vp = nnsgen.flush_vp(PROJ, CAM, LIGHTS, glb, BASE_ROT, [0, HEIGHT[HEROINE], 0], [4096] * 3)
        vp_ok = sum(g[0] == c[0] and [s32(x) for x in g[1]] == [s32(x) for x in c[1]] for g, c in zip(vp, seg[pos:pos + 20]))
        exact += vp_ok; total += len(vp)
        for c, ps in vp:
            ge.run(c, ps)
        out = []
        draw = nnsgen.Draw(model, ge, out, rec, glb=glb, node_cb=(bone_cb if part in ('hair', 'body', 'rhand', 'lhand') else None))  # cbInitFunc 0x2078294 = Model_ApplyNamedBoneMtx
        draw.tex_bind = tex_bind
        global FACING
        if 'Head' in bones and FACING is None:
            rows, hp = bones['Head']
            FACING, dot = nnsgen.facing_test(rows, hp, EYE)
            print('facing test: Head at', [round(x / 4096, 2) for x in hp], 'dot %.4f -> facing=%s' % (dot / 4096, FACING))
        draw.poly_id = nnsgen.polygon_ids(part, [n for n in nnsgen.Model(mb).mat_names], HEROINE, FACING if FACING is not None else False)
        draw.run()
        cap = seg[pos + 20:pos + 20 + len(out)]
        ok = [same(g, c) for g, c in zip(out, cap)]
        cmd_ok = sum(g[0] == c[0] for g, c in zip(out, cap))
        nodemix_junk = sum(1 for (g, c), o in zip(zip(out, cap), ok) if not o and g[0] == 0x17 and [s32(x) for x in g[1]][:9] == [s32(x) for x in c[1]][:9])
        exact += sum(ok) + nodemix_junk; total += len(out)
        bad = [k for k, o in enumerate(ok) if not o and not (out[k][0] == 0x17 and [s32(x) for x in out[k][1]][:9] == [s32(x) for x in cap[k][1]][:9])]
        report.append(('%s+vp%d' % (part, vp_ok), mfile, len(out), cmd_ok, sum(ok) + nodemix_junk, bad[:3], [(NAME.get(out[k][0]), [s32(x) for x in out[k][1]][:6], [s32(x) for x in cap[k][1]][:6]) for k in bad[:2]]))
        # skip what follows this draw in the capture until the next FlushVP (calc-pass NODEMIX of the next part)
        nxt = [s for s in starts if s >= pos + 20 + len(out)]
        for c, ps in seg[pos + 20 + len(out):(nxt[0] if nxt else len(seg))]:
            ge.run(c, ps)
        pos = nxt[0] if nxt else len(seg)
    for r in report:
        print('%-11s %-34s generated %5d  cmd-match %5d  exact %5d  first mismatches %s %s' % r)
    print('list %d: exact %d / %d generated packets' % (swap, exact, total))
    return exact, total


def draw_accessory(seg, starts, pos, ge, glb, bones, tex_index):
    """accessory draw: FlushVP with base translation = table offset, then the cap SBC with the game's NODEDESC callback
    (Mod2045ee8_ProcessessGraphics_6, timing A): TRANS(offset x HeadN), LOAD_4x3(Head bone), MULT_4x3(obj rot + offset), skip"""
    ofs = accessory_offset(ACCESSORY['id'], HEROINE)
    mb = rd(ACCESSORY['model']); model = nnsgen.Model(mb)
    vp = nnsgen.flush_vp(PROJ, CAM, LIGHTS, glb, BASE_ROT, ofs, [4096] * 3)
    cap_vp = seg[pos:pos + 20]
    vp_ok = sum(g[0] == c[0] and [s32(x) for x in g[1]] == [s32(x) for x in c[1]] for g, c in zip(vp, cap_vp))
    for c, ps in vp:
        ge.run(c, ps)
    rows, hp = bones['Head']

    def acc_cb(draw, node, timing):
        if timing != 'A':
            return False
        draw.emit(0x1c, nnsgen.multvec33(ofs, rows))
        draw.emit(0x17, [x for r in rows for x in r] + list(hp))
        draw.emit(0x19, [4096, 0, 0, 0, 4096, 0, 0, 0, 4096] + ofs)
        return True
    out = []
    draw = nnsgen.Draw(model, ge, out, [None] * model.nnode, glb=glb, node_cb=acc_cb)
    names = mat_tex_names(mb)

    def tex_bind(m, idx, mt):
        tname, pname = names[idx]
        if not tname:
            return mt['teximg'], mt['pltt']
        t = tex_index[tname][0][1]; tx = [x for x in t['textures'] if x['name'] == tname][0]
        return mt['teximg'] | tx['param'], mt['pltt']
    draw.tex_bind = tex_bind
    draw.light_patch = False
    k0 = nnsgen.POLYID_TABLE[0 + (0 if FACING else 3)]           # accessories: NNS ...AmbAll(mdl, local_30) = table[k=0]
    draw.poly_id = {i: k0 for i in range(model.nmat)}
    draw.run()
    cap = seg[pos + 20:pos + 20 + len(out)]
    ok = [same(g, c) for g, c in zip(out, cap)]
    bad = [k for k, o in enumerate(ok) if not o]
    rep = ('cap+vp%d' % vp_ok, ACCESSORY['model'], len(out), sum(g[0] == c[0] for g, c in zip(out, cap)), sum(ok), bad[:3],
           [(NAME.get(out[k][0]), [s32(x) for x in out[k][1]][:6], [s32(x) for x in cap[k][1]][:6]) for k in bad[:2]])
    nxt = [s for s in starts if s >= pos + 20 + len(out)]
    return vp_ok + sum(ok), len(vp) + len(out), rep, (nxt[0] if nxt else len(seg))


if __name__ == '__main__':
    main()
