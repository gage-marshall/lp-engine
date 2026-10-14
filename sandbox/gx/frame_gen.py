"""Complete per-update GX streams (compositing included) for a heroine scene, from game data only.

    .venv/bin/python sandbox/gx/frame_gen.py [line26 | line30] [--updates N]

One game update emits, in this order (Main_UpdateFrame; code addresses in docs/RENDER_PRIMITIVES.md):
  Gfx3D_UpdateFrame        effect sprites via the ScreenFade 2D-in-3D renderer, only when phase == count - 1
  Gfx_RenderCapturedFrame  the captured-image quad (ScreenFade sprite, texture = capture bank) when draw byte != 0
  Mod2045ee8_ProcessessGraphics_10
      G3X_Reset, G3X_ResetMtxStack, then for the 20 objects in array order: compute pass (NNS_G3dDraw flags 3) when the
      object's compute phase == current phase, draw (FlushVP + SBC) when its draw phase == draw byte; NNS_G3dGlbFlushP
  Sprite_Proc_20490e0      UI sprite manager setup (ortho) and its (here empty) sprite lists
  G3_SwapBuffers(0)
Phases (Capture3D_OnVBlank): count 3; phase p = 0,1,2; draw byte = p - 1 (wrapping 2 -> -1).
"""
import os, struct, sys
import numpy as np
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'tools'))
import nnsgen  # noqa: E402
import scene_data as sd  # noqa: E402
import scene_gen  # noqa: E402

U = nnsgen.u32


def mtx_ortho_w(t, b, l, r, n, f, w):
    """NitroSDK MTX_OrthoW -> 4x4 rows"""
    inv1 = nnsgen.fx_div64(4096, r - l)
    inv2 = nnsgen.fx_div64(4096, t - b)
    if w != 4096:
        inv1 = nnsgen._cdiv(inv1 * w, 4096)
    m = [[0] * 4 for _ in range(4)]
    m[3][3] = w
    m[0][0] = nnsgen.fx_mul32x64c(8192, inv1)
    inv3 = nnsgen.fx_div64(4096, n - f)
    if w != 4096:
        inv2 = nnsgen._cdiv(inv2 * w, 4096)
    m[1][1] = nnsgen.fx_mul32x64c(8192, inv2)
    if w != 4096:
        inv3 = nnsgen._cdiv(inv3 * w, 4096)
    m[2][2] = nnsgen.fx_mul32x64c(8192, inv3)
    m[3][0] = nnsgen.fx_mul32x64c(-r - l, inv1)
    m[3][1] = nnsgen.fx_mul32x64c(-t - b, inv2)
    m[3][2] = nnsgen.fx_mul32x64c(f + n, inv3)
    return m


def flat(m):
    return [U(x) for row in m for x in row]


VIEWPORT = U(sd.w32(0x2045694))                                    # 0xbfff0000


def screenfade_setup():
    """ScreenFade_ProcessessGraphics: texture identity, viewport, ortho (0..192 x 0..256, n 0, f DAT 0x2045698, W 1024),
    store, then G3i_LookAt_(0, up 0x210164c, target 0x2101640)"""
    far = sd.w32(0x2045698)
    tgt = [sd.w32(sd.w32(0x204569c) + 4 * i) for i in range(3)]; up = [sd.w32(sd.w32(0x20456a0) + 4 * i) for i in range(3)]
    out = [(0x10, [3]), (0x15, []), (0x10, [0]), (0x60, [VIEWPORT]), (0x10, [0]),
           (0x16, flat(mtx_ortho_w(0, 0xc0000, 0, 0x100000, 0, far, 0x400000))), (0x13, [0]), (0x10, [2])]
    out += [(0x10, [2]), (0x17, flat(nnsgen.mtx_look_at([0, 0, 0], up, tgt)))]
    return out


def screenfade_sprite(sp, sc):
    """ScreenFade_ProcessessGraphics_2 for one sprite record (fields as in Seg2041394_Clear / Gfx_RenderCapturedFrame)"""
    rot = sp['rot'] & 0xffff
    s, c = sc.sin(rot), sc.cos(rot)
    w, h = sp['w'], sp['h']
    out = [(0x11, []), (0x1c, [0, 0, 0]), (0x1c, [U(sp['x'] >> 4), U(sp['y'] >> 4), U(sp['z'])]),
           (0x1a, [U(c), U(s), 0, U(-s), U(c), 0, 0, 0, 4096]),
           (0x1c, [U(sp['cx'] << 12), U(sp['cy'] << 12), 0]),
           (0x1b, [U(w << 12), U(h << 12), 4096]), (0x1b, [U(sp['sx']), U(sp['sy']), 4096])]
    fmt = sp['fmt']
    tex = (fmt << 26) | (sp['tex'] >> 3) | 0x40000000 | (sp['ss'] << 20) | (sp['st'] << 23) | ((0 if sp['col0'] else 1) << 29)
    out += [(0x2a, [U(tex)]), (0x2b, [U(sp['pltt'] >> (4 - (fmt == 2)))]),
            (0x29, [U((sp['id'] << 24) | 0xc0 | (sp['alpha'] << 16))]), (0x20, [sp['color']]), (0x40, [1])]
    u0 = (((sp['u'] << 20) & 0xffffffff) >> 16) & 0xffff
    v1 = (((h * 4096 + sp['v'] * 4096) * 0x100) >> 16)
    v0 = (((sp['v'] << 20) & 0xffffffff) >> 16)
    u1 = (((w * 4096 + sp['u'] * 4096) * 0x100) >> 16) & 0xffff
    for tc, vt in ((u0 | (v1 << 16), 0x10000), (u1 | (v1 << 16), 0x10040), (u1 | (v0 << 16), 0x40), (u0 | (v0 << 16), 0)):
        out += [(0x22, [U(tc)]), (0x24, [vt])]
    out += [(0x41, []), (0x12, [1])]
    return out


def captured_frame_sprite():
    """Gfx_RenderCapturedFrame's record: Seg2041394_Clear defaults + 256x192 direct-colour image at the capture bank's
    texture address (state struct 0x213d2e4 +0xc = bank D as texture slot 2 = 0x40000), z = DAT 0x2042df0, alpha 31"""
    return dict(x=0, y=0, z=sd.w32(0x2042df0), rot=0, cx=0, cy=0, w=0x100, h=0xc0, sx=0x1000, sy=0x1000, fmt=7, tex=0x40000,
                ss=5, st=5, col0=False, pltt=0, id=0, alpha=0x1f, color=sd.w32(0x2045920) & 0xffff, u=0, v=0)


def reset_mtx_stack(ge):
    """G3X_ResetMtxStack (pops by the current stack levels)"""
    out = [(0x10, [3]), (0x15, []), (0x10, [0])]
    if ge.pj_level:
        out.append((0x12, [ge.pj_level]))
    out += [(0x10, [2]), (0x12, [U(ge.sp) & 63]), (0x15, [])]
    return out


def g3x_reset(ge):
    """G3X_Reset: stack reset, then POLYGON_ATTR(no lights, modulate, cull back, id 0, alpha 31), TEXIMAGE 0, PLTT 0"""
    return reset_mtx_stack(ge) + [(0x29, [(2 << 6) | (31 << 16)]), (0x2a, [0]), (0x2b, [0])]


def sprite_manager():
    """Sprite_Proc_2049064 + Sprite_Proc_20490e0 with no active sprites"""
    return [(0x60, [U(sd.w32(0x20490d8))]), (0x10, [0]),
            (0x16, flat(mtx_ortho_w(0, 0xc0000, 0, 0x100000, nnsgen.s32(0xff000000), 0, 0x400000))),
            (0x13, [0]), (0x10, [1]), (0x29, [0x1f00c0]), (0x20, [0x7fff]), (0x11, []), (0x12, [1])]


def flush_p(proj, cam, lights, glb, base_rot, base_trans, base_scale, teximage=0):
    """NNS_G3dGlbFlushP"""
    vecs, cols = lights
    out = [(0x10, [0]), (0x16, flat(proj)), (0x10, [2]), (0x17, flat(cam))]
    out += [(0x32, [v]) for v in vecs]
    out += [(0x30, [glb['diffamb']]), (0x31, [glb['specemi']]), (0x29, [glb['polyattr']]), (0x60, [VIEWPORT])]
    out += [(0x33, [c]) for c in cols]
    out += [(0x19, [U(x) for row in base_rot for x in row] + [U(x) for x in base_trans]), (0x1b, [U(x) for x in base_scale]), (0x2a, [teximage])]
    return out


def generate(scene):
    """one full 3-update cycle (after a warm-up cycle) for `scene`: list of 3 updates (phase 0, 1, 2), each a packet list"""
    sc = nnsgen.SinCos(sd.MAIN)
    S = scene_gen.SceneRenderer(scene)
    # two full cycles: the first warms up the geometry engine state (stack slots read by compute-pass NODEMIX)
    updates = []
    for cyc in range(2):
        for phase in range(3):
            draw = phase - 1
            out = []
            if phase == 2:
                out += screenfade_setup() + [(0x10, [1])]                         # Gfx3D_UpdateFrame, no active effects
            if draw != 0:
                out += screenfade_setup() + screenfade_sprite(captured_frame_sprite(), sc) + [(0x10, [1])]
            for c, ps in out:
                S.ge.run(c, ps)
            part = []
            for c, ps in g3x_reset(S.ge):
                S.ge.run(c, ps); part.append((c, ps))
            for c, ps in reset_mtx_stack(S.ge):
                S.ge.run(c, ps); part.append((c, ps))
            out += part
            out += S.object_loop(phase, draw)
            fp = flush_p(S.proj, S.cam, S.lights, S.glb, S.last_base[0], S.last_base[1], [4096] * 3)
            sm = sprite_manager()
            for c, ps in fp + sm + [(0x50, [0])]:
                S.ge.run(c, ps)
            out += fp + sm + [(0x50, [0])]
            updates.append(out)
    return updates[3:]


def captured_updates(path):
    """captured updates keyed by the frame of the SWAP_BUFFERS that ends them"""
    from gx import packets
    z = np.load(path)
    pk = packets(z['cmd'], z['param'], z['frame'])
    sw = [i for i, p in enumerate(pk) if p[0] == 0x50]
    return {pk[sw[k + 1]][2]: [(c, ps) for c, ps, f, i in pk[sw[k] + 1:sw[k + 1] + 1]] for k in range(len(sw) - 1)}


def compare(gen, cap):
    """(exact packets, total, first differing indices)"""
    from gx import s32
    n = min(len(gen), len(cap)); ok = 0; bad = []
    for i in range(n):
        same = gen[i][0] == cap[i][0] and [s32(x) for x in gen[i][1]] == [s32(x) for x in cap[i][1]]
        ok += same
        if not same:
            bad.append(i)
    return ok, max(len(gen), len(cap)), bad


MOMENTS = {
    # capture, swap frames of phases 0/1/2, scene resolve() arguments (script position, playback)
    'line26': ('sandbox/rec/out/gx_6370_6390.npz', (6379, 6380, 6381),
               dict(event_id=0x08143402, position=26, updates=96, voice_t=(6380 - 5981) / 60)),     # 96 animation ticks since `f 587`
    'line30': ('sandbox/rec/out/gx_7190_7210.npz', (7198, 7199, 7201),
               dict(event_id=0x08143402, position=30, updates=295, voice_t=(7199 - 6637) / 60)),   # 295 ticks since `f 587`
}
import scene_inputs as _si  # noqa: E402
SAVE = os.path.join(ROOT, 'tools/emu_scripts/states/logs/backup-session-20261013-110915-f7721.sav')   # in-game save, same day
SAVE_CLOCK = dict(record_0d=_si.record_0d_from_save(SAVE, 2), random_state=0, month=10, hour=15)  # scheduled event; Oct 15h


def main():
    import argparse, scene_inputs
    from gx import NAME, s32
    ap = argparse.ArgumentParser()
    ap.add_argument('moment', nargs='?', default='line26', choices=sorted(MOMENTS))
    ap.add_argument('--updates', type=int, help='animation ticks since the f command (overrides the moment; searched over one clip if the moment has none)')
    a = ap.parse_args()
    path, swaps, kw = MOMENTS[a.moment]
    kw = dict(kw, **SAVE_CLOCK)
    if a.updates is not None:
        kw['updates'] = a.updates
    caps = captured_updates(os.path.join(ROOT, path))
    if kw['updates'] is None:
        best = None
        for u in range(31):
            scene, _ = scene_inputs.resolve(**dict(kw, updates=u))
            ok, tot, _ = compare(generate(scene)[2], caps[swaps[2]])
            print('updates %2d: main-pass update %d / %d exact' % (u, ok, tot))
            if best is None or ok > best[1]:
                best = (u, ok)
        kw['updates'] = best[0]
        print('best: updates', best[0])
    scene, src = scene_inputs.resolve(**kw)
    print('scene:', scene)
    total = exact = 0
    for phase, (gen, swapf) in enumerate(zip(generate(scene), swaps)):
        cap = caps.get(swapf)
        if cap is None:
            print('no captured update ending at', swapf); continue
        ok, tot, bad = compare(gen, cap)
        total += tot; exact += ok
        print('update phase %d (swap @%d): generated %d, captured %d, exact %d' % (phase, swapf, len(gen), len(cap), ok))
        for i in bad[:4]:
            print('   #%d gen %s %s | cap %s %s' % (i, NAME.get(gen[i][0]), [s32(x) for x in gen[i][1]][:8], NAME.get(cap[i][0]), [s32(x) for x in cap[i][1]][:8]))
    print('ALL: %d / %d packets exact' % (exact, total))


if __name__ == '__main__':
    main()
