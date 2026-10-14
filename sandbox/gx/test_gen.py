"""Compare nnsgen output (game data only) with a captured part draw, packet by packet.

    .venv/bin/python sandbox/gx/test_gen.py CAPTURE.npz SWAP_FRAME
"""
import os, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
from gx import packets, NAME, s32  # noqa: E402
import nnsgen  # noqa: E402

A = os.path.join(ROOT, 'content', 'assets', 'arc')
TRACKED = ['Head', 'Spine2', 'Hips', 'RightUpLeg', 'LeftUpLeg', 'RightLeg', 'LeftLeg', 'Neck', 'RightArm', 'LeftArm', 'RightForearm',
           'LeftForearm', 'RightHand', 'LeftHand', 'rBreast', 'lBreast', 'hair_f']   # DTCM 0x27e033c (MotionAnimTable_FindIndexByName)


def captured_parts(z, swap):
    pk = packets(z['cmd'], z['param'], z['frame'])
    sw = [i for i, p in enumerate(pk) if p[0] == 0x50]
    end = next(sw[i] for i in range(1, len(sw)) if pk[sw[i]][2] == swap); start = sw[sw.index(end) - 1] + 1
    seg = [(c, ps) for c, ps, f, i in pk[start:end]]
    # part draws begin with FlushVP: MTX_MODE 0, LOAD_4x4 (perspective), MULT_4x3 (camera), MTX_MODE 2, IDENTITY, LIGHT_VECTOR x4 ...
    starts = [j for j in range(len(seg) - 5) if seg[j][0] == 0x10 and seg[j + 1][0] == 0x16 and seg[j + 2][0] == 0x19 and seg[j + 4][0] == 0x15
              and seg[j + 5][0] == 0x32]
    parts = []
    for k, j in enumerate(starts):
        e = starts[k + 1] if k + 1 < len(starts) else len(seg)
        parts.append(seg[j:e])
    return seg, parts


def flushvp_len():
    return 2 + 1 + 1 + 1 + 4 + 4 + 4 + 3      # mode,load44 | mult43 | mode | identity | lightvec*4 | dif,spe,poly,vp | lightcol*4 | mult43,scale,teximage


def diff(gen, cap, label, show=12):
    n = min(len(gen), len(cap)); bad = []
    for i in range(n):
        if gen[i][0] != cap[i][0] or [s32(x) for x in gen[i][1]] != [s32(x) for x in cap[i][1]]:
            bad.append(i)
    same_cmds = sum(gen[i][0] == cap[i][0] for i in range(n))
    print('%s: generated %d packets, captured %d; command match %d/%d; exact (cmd+params) %d/%d' % (
        label, len(gen), len(cap), same_cmds, n, n - len(bad), n))
    for i in bad[:show]:
        g, c = gen[i], cap[i]
        print('   #%d gen %s %s | cap %s %s' % (i, NAME.get(g[0], hex(g[0])), [s32(x) for x in g[1]][:12], NAME.get(c[0], hex(c[0])), [s32(x) for x in c[1]][:12]))
    return bad


if __name__ == '__main__':
    z = np.load(sys.argv[1]); swap = int(sys.argv[2])
    seg, parts = captured_parts(z, swap)
    print('captured part draws in list', swap, ':', len(parts))
    hair = nnsgen.Model(open(os.path.join(A, 'n_block3d_00.arc', '0071.bin'), 'rb').read())
    bundle = nnsgen.bundle_entries(open(os.path.join(A, 'async3d.arc', '0092.bin'), 'rb').read())
    anims = nnsgen.load_bca(bundle['m_0230_10.nsbca'])
    vp = parts[0][:flushvp_len()]
    glb = nnsgen.glb_from_code(0x4210)   # DAT_020469e0
    cap_glb = {'diffamb': vp[9][1][0], 'specemi': vp[10][1][0], 'polyattr': vp[11][1][0]}
    print('globals from code', {k: hex(v) for k, v in glb.items()}, 'captured FlushVP', {k: hex(v) for k, v in cap_glb.items()})
    cap = parts[0][flushvp_len():]
    ge = nnsgen.GE(); out = []
    for c, ps in vp:                      # matrices set by FlushVP (to be generated from game data next)
        ge.run(c, ps)
    captured_bones = set()

    def bone_cb(draw, node, timing):
        if timing != 'C' or draw.record or draw.nogecmd:
            return
        nm = draw.m.node_names[node]
        if nm in TRACKED and nm not in captured_bones:
            captured_bones.add(nm)
            draw.emit_direct(0x10, [0]); draw.emit_direct(0x11, []); draw.emit_direct(0x15, []); draw.emit_direct(0x12, [1]); draw.emit_direct(0x10, [2])
    nnsgen.Draw(hair, ge, out, None, anim=anims['m_0230_10_hair0'], frame_fx=3 << 12, node_cb=bone_cb, glb=glb).run()
    diff(out, cap, 'hair part (n_00_100_hair) draw')
