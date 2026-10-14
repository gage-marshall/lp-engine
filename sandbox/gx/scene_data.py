"""Resolve a heroine scene's render inputs from game data only (content files + ARM9 code/data tables).

Scene/save state in (what the game itself reads from the save, the script and the clock):
    heroine (0 Manaka, 1 Rinko, 2 Nene)          script id top byte / Model byte 0
    hair_label, outfit_label                       tm_model_label ids (save: SaveSlot / ItemDb selection)
    hand_variant, face_variant                     slot variants (0 = default)
    accessory_id                                   e.g. 405 (scene: SceneState fields)
    time_mode                                      Model byte 1 (MapCtl_RemapModeCode): .tmbsd variant _0/_1/_2
    preset_id                                      camera preset (script command 6)
    motion_key, updates_since_motion_start         script command f; one animation frame per game update
Everything else (archive entries, animation, camera, lights, shading tables) is resolved here from game data, with
the code location of each rule.
"""
import os, struct
import nnsgen

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
A = os.path.join(ROOT, 'content', 'assets', 'arc')
MAIN = open(os.path.join(ROOT, 'work', 'arm9_main.bin'), 'rb').read()      # ARM9 static image (ROM-derived, local)
HPREFIX = 'mrn'


def arc(path):
    return open(os.path.join(A, path), 'rb').read()


def w32(addr):
    return struct.unpack_from('<i', MAIN, addr - 0x2000000)[0]


def code_table(literal, n):
    base = w32(literal)
    return [w32(base + 4 * i) for i in range(n)]


# ------------------------------------------------------------------ wardrobe -> archive entries (Mod207b444_Switch_207b444)
BLOCK3D = {0: 'X_block3d_00.arc', 1: 'X_block3d_01.arc'}


def label_base(label):
    """tm_model_label record: first u16 = base archive entry (Mod1ffa934_Proc_1ffa9e0 reads cache +4 = file +0)"""
    return struct.unpack_from('<H', arc('tm_model_label.arc/%04d.bin' % label), 0)[0]


def label_class(label):
    """first u16 at +2 = motion model class (MotionEntry_GetShort6 -> Gfx3D_ResetActor)"""
    return struct.unpack_from('<H', arc('tm_model_label.arc/%04d.bin' % label), 2)[0]


def garment_category(label):
    """Mod2079638_Calc_2079978: 0 full outfit, 1 top, 2 bottom, -1/-2 other (label id ranges, constants in code)"""
    d = [w32(a) for a in (0x2079b84, 0x2079b88, 0x2079b8c, 0x2079b90, 0x2079b94, 0x2079b98, 0x2079b9c, 0x2079ba0, 0x2079ba4)]
    b84, b88, b8c, b90, b94, b98, b9c, ba0, ba4 = d
    p = label
    if (0 <= p < 0x4d) or (b84 <= p <= b84 + 0x11) or (b88 <= p < 0x16d) or (b8c <= p <= b8c + 0x11):
        return -1
    if (p < 0x4d or p > 0x73) and (p < 0xa0 or p > 0xc4) and (p < 0xf2 or p > b90) and (p < b94 or p > b94 + 0xf) \
            and (p < b98 or p > b98 + 0xe) and (p < b9c or p > b9c + 0xf):
        if (0x73 < p < 0x90) or (0xc4 < p < 0xe4) or (ba0 <= p <= ba0 + 0x1d) or (b98 <= p <= b98 - 1) or (b9c <= p <= b9c - 1) or (ba4 <= p <= ba4 - 1):
            return 1
        if (p < 0x90 or p > 0x9f) and (p < 0xe4 or p > 0xf1) and (p < 0x138 or p > 0x148) and (p < b98 or p > b98 - 1) and (p < b9c or p > b9c - 1):
            if p < ba4 or p > ba4 - 1:
                return -2
        return 2
    return 0


def part_entry(slot, label, heroine, variant, kind):
    """archive entry of slot's model (kind 0) or texture (kind 1)"""
    base = label_base(label) if (label != -1 and slot not in (5, 7)) else None
    if slot in (0, 1):
        return base + kind + 1
    if slot == 2:
        return code_table(0x207b5e8, 3)[heroine]
    if slot == 3:
        return code_table(0x207b5e4, 12)[variant * 6 + kind * 3 + heroine]
    if slot == 4:
        return base + kind + 4
    if slot == 5:
        return code_table(0x207b5ec, 12)[variant * 6 + kind * 3 + heroine]
    if slot == 6:
        return base + kind + 3
    if slot == 7:
        return code_table(0x207b5f0, 12)[variant * 6 + kind * 3 + heroine]
    if slot == 8:
        return base + kind + 6 if garment_category(label) == 0 else None
    if slot == 9:
        return base + kind + 8 if garment_category(label) == 0 else None


def accessory_entry(acc_id, variant, kind):
    """Mod207b7a8: key (u16)(id*100+variant) in the table at 0x21251fc -> entry short + kind (block3d.arc)"""
    t = w32(0x207b7f4)
    for i in range(0x93):
        key, ent = struct.unpack_from('<Hh', MAIN, t + 4 * i - 0x2000000)
        if key == (acc_id * 100 + variant) & 0xffff:
            return ent + kind
    return None


def wardrobe(heroine, hair_label, outfit_label, hand_variant=0, face_variant=0):
    """slot -> (archive file of the model, archive file of the texture set); slots per Mod2079638_Clear_2079ba8"""
    h = HPREFIX[heroine]
    cat = garment_category(outfit_label)
    slots_of_outfit = {0: [1, 4, 6, 8, 9], 1: [1, 4, 6], 2: [8, 9]}[cat]
    labels = {0: hair_label, 2: -1, 3: -1, 5: -1, 7: -1}
    for s in slots_of_outfit:
        labels[s] = outfit_label
    names = {0: 'hair', 1: 'body', 2: 'face_m', 3: 'face_l', 4: 'rarm', 5: 'rhand', 6: 'larm', 7: 'lhand', 8: 'waist', 9: 'leg'}
    out = {}
    for s, lab in sorted(labels.items()):
        var = hand_variant if s in (5, 7) else face_variant
        if s in (2, 3, 5, 7):
            archive = 'tm_model_block_base.arc'
        else:
            archive = '%s_block3d_%02d.arc' % (h, 0 if s == 0 else 1)
        m = part_entry(s, lab, heroine, var, 0); t = part_entry(s, lab, heroine, var, 1)
        out[names[s]] = ('%s/%04d.bin' % (archive, m), '%s/%04d.bin' % (archive, t) if t is not None else None)
    return out


def shading_bundles(heroine, hair_label, outfit_label):
    """.tmbsd bundles (base + part labels, entry = label base + 0); base bundle per heroine (tm_model_block_base)"""
    h = HPREFIX[heroine]
    base = {0: 0, 1: 30, 2: 66}[heroine]                       # m_base / r_base / n_base bundles (Mod207b910_Proc_207b994 case 0)
    return {'base': 'tm_model_block_base.arc/%04d.bin' % base,
            'hair': '%s_block3d_00.arc/%04d.bin' % (h, label_base(hair_label)),
            'outfit': '%s_block3d_01.arc/%04d.bin' % (h, label_base(outfit_label))}


def shading_tables(heroine, hair_label, outfit_label, time_mode, facing):
    """toon table (32) and edge table (8) as uploaded by VramBank_ProcessessGraphics (Mod207b910_Proc_207b994)"""
    rgb = lambda b, i: b[3 * i] | (b[3 * i + 1] << 5) | (b[3 * i + 2] << 10)
    bund = shading_bundles(heroine, hair_label, outfit_label)

    def tmbsd(path):
        ents = nnsgen.bundle_entries(arc(path))
        return [v for k, v in sorted(ents.items()) if k.endswith('_%d.tmbsd' % time_mode)][0]
    base = tmbsd(bund['base'])
    toon = [rgb(base, i) for i in range(32)]
    edge = [w32(0x207bcf8) & 0xffff] * 8
    edge[0] = rgb(base, 32)
    owner = {'hair': (6, 7), 'outfit': (1, 2, 3, 4, 5)}                 # Mod207b910_Switch_207b910 (parts 1 and 2 of this wardrobe)
    for part, ks in owner.items():
        b = tmbsd(bund[part])
        for k in ks:
            edge[k] = rgb(b, 32 + k)
    if not facing:
        edge = edge[:6] + [0x296d] + edge[7:]                          # Seg20a0e90_Get_20a2558 constant -> chara+0xf40 (table 2)
    return toon, edge


# ------------------------------------------------------------------ motion (Gfx3D_ResetActor + motion tables)
def motion_stem(motion_key, heroine, model_class):
    """script key -> animation index (tm_model_block_base.arc/0101: 911 x 9 x {u8 mood, u8 eye, s16 index}),
    index -> bundle/stem via the motion table extracted from code (@0x212588c / 0x2126028)"""
    import json
    t = arc('tm_model_block_base.arc/0101.bin')
    o = 4 + motion_key * 36 + model_class * 12 + heroine * 4
    idx = struct.unpack_from('<h', t, o + 2)[0]
    rec = json.load(open(os.path.join(ROOT, 'content', 'engine', 'motion_table.json')))['records'][idx]
    return idx, rec['bundle'], rec['stem']


def motion_frame(updates, nframes, loops=True):
    """one animation frame per game update; loop at the clip length"""
    return (updates % nframes) if loops else min(updates, nframes - 1)


# ------------------------------------------------------------------ which objects are drawn in which capture pass
SLOT_NAMES = ['hair', 'body', 'face_m', 'face_l', 'rarm', 'rhand', 'larm', 'lhand', 'waist', 'leg']


def draw_phase(slot, single_pass=False):
    """Model_SetPartPaletteFromTable: draw phase = (slot != 9) (the leg slot is the separate pass), accessories 1"""
    return 1 if single_pass else int(slot != 9)


def closeup_preset(p):
    """Model_ClassifyIdRangeToVariant's preset test (constants 0x20797fc=2022, 0x2079800=1037, 0x2079804=3013)"""
    D1, D2, D3 = w32(0x20797fc), w32(0x2079800), w32(0x2079804)
    if p == D1 or p == D2 or p == D3:
        return True
    if p > D1:
        if p > D3:
            if p > D3 + 0xf:
                x = D3 + 0x18
                if x < p:
                    x = D3 + 0x24
                return p == x
            if p == D3 + 0xf:
                return True
            x = D3 + 8
            if x < p:
                x = D3 + 9
            return p == x
        if p > D1 + 0x19:
            x = D3 - 0xd
            if p <= x:
                return p == x
            return p == 0xbc0
        if p < D1 + 0x19:
            x = D1 + 6
            if x < p:
                x = D3 - 0x3d0
            return p == x
        return True
    if p > D2:
        if p > D1 - 0xe:
            x = D2 + 0x3d0
            if x < p:
                x = D2 + 0x3d8
            return p == x
        if p < D1 - 0xe:
            x = D2 + 0xb
            if p <= x:
                return p == x
            return p == 2000
        return True
    if p > D2 - 0x18:
        if p > D2 - 0xf:
            return p == D2 - 9
        return p in (D2 - 0x10, D2 - 0xf)
    if p < D2 - 0x18:
        return p == 1000 if p < 0x3e9 else p == 0x3f0
    return True


def face_variant(preset, head_distance_fx):
    """0: face_l drawn, 1: face_m drawn, 2: neither (body face); head distance = |camera eye - Head bone| (fx32),
    computed each update by Model_UpdateCharaScene"""
    if head_distance_fx >= 0xd2000:
        return 0
    return 1 if closeup_preset(preset) else 2


def visible_slots(preset, head_distance_fx):
    v = face_variant(preset, head_distance_fx)
    hidden = {2: [2, 3], 0: [2], 1: [3]}[v]
    return [s for s in range(10) if s not in hidden]


# ------------------------------------------------------------------ backdrop (BG1) scroll
def _cdiv(a, b):
    q = abs(a) // abs(b)
    return q if (a >= 0) == (b > 0) else -q


def bg_scroll(proj, cam, target_xy, shake=(0, 0)):
    """Projection_UpdateScreenPosition: project the camera target (x, y, z=0) with the current camera/projection
    (NNS_G3dWorldPosToScrPos, viewport 0,0,255,191), add camera shake (Projection_ProjectWorldPosition; 0 when the camera
    struct's mode byte +0x34 is 0), then scroll = (64 + (128 - sx) * 20 / 100, 32 + (95 - sy) * 20 / 100), clamped to
    [0, 128] x [0, 64].  Written to the BG1 shadow table (DTCM 0x27e25a0) via Bg_ProcessAuxiliary/BgTransition."""
    v = [target_xy[0] / 4096, target_xy[1] / 4096, 0.0, 1.0]
    C = [[x / 4096 for x in row] + [0.0] for row in cam]; C[3][3] = 1.0          # 4x3 -> 4x4 (row-vector)
    P = [[x / 4096 for x in row] for row in proj]
    e = [sum(v[k] * C[k][j] for k in range(4)) for j in range(4)]
    c = [sum(e[k] * P[k][j] for k in range(4)) for j in range(4)]
    sx = int(round((c[0] / c[3] + 1) * 128)) + shake[0]
    sy = int(round((1 - c[1] / c[3]) * 96)) + shake[1]
    x = _cdiv((0x80 - sx) * 0x14, 100) + 0x40
    y = _cdiv((0x5f - sy) * 0x14, 100) + 0x20
    return min(max(x, 0), 0x80), min(max(y, 0), 0x40)


# ------------------------------------------------------------------ blink / eye texture (0x2094aa4, per update)
BLINK_INTERVAL = 0x2103554          # s8[10]
BLINK_HOLD = 0x210355e              # s8[10]
EYE_TABLE = 0x2127ae4               # s8[46][18]: eye texture per (mood, column)
EYE_MODE_BYTE = 0x2103542           # s8[]: column after a blink, per eye mode


def _s8(addr):
    v = MAIN[addr - 0x2000000]
    return v - 256 if v > 127 else v


def eye_table(col, mood):            # 0x2094898
    return _s8(EYE_TABLE + (0 if mood == -1 else mood) * 18 + col)


def eye_special(mode, mood):         # 0x20949f8: special eye for (mode, mood), -1 if none
    if mood == -1:
        return -1
    if mode == 8:
        return 0x13 if mood == 0x13 else (3 if mood == 0x1b else -1)
    if mode == 9:
        if mood in (9, 0x1b, 0xf): return 1
        if mood in (0xc, 0xd): return 3
        if mood in (0x13, 0x29): return 0x11
    return -1


class Rand64:
    """Rand_Next 0x2040258 (NITRO MATH_Rand64): state 0x2139da4+0xc, x = x * 0x5d588b656c078965 + 0x269ec3 (mod 2^64),
    returns (x >> 32) * n >> 32 (or x >> 32 when n = 0). Seeded once at boot by Input_Initialize 0x2040204 with the RTC
    timestamp; every draw anywhere in the game advances it, so its value at a scene start is runtime state."""
    MUL, ADD = 0x5d588b656c078965, 0x269ec3

    def __init__(self, state):
        self.x = state & (1 << 64) - 1

    def __call__(self, n):
        self.x = (self.x * self.MUL + self.ADD) & (1 << 64) - 1
        hi = self.x >> 32
        return (hi * n) >> 32 if n else hi


def blink_step(st, mood, rand=None):
    """one update of the eye state machine (0x2094aa4) on st = dict(counter, idx, eye, phase, hold, extra, mode);
    rand(n) = Rand_Next(n) (only used for mood 0x1a)"""
    mode = st['mode']
    if 0 <= mode - 10 <= 1:
        return
    step = 2 if mood == 0x18 else 1
    ph = st['phase']
    if ph == 0:
        st['eye'] = eye_table(mode, mood)
        if st['counter'] >= 0:
            st['counter'] -= step
        else:
            i = st['idx']
            st['counter'] = _s8(BLINK_INTERVAL + i)
            st['extra'] = (rand(30) + 0x2d) if (mood == 0x1a and i in (1, 2, 4, 5)) else 0
            st['phase'] = 1
            st['idx'] = 0 if i + 1 >= 10 else i + 1
        if eye_special(mode, mood) == -1:
            return
        if st['hold'] >= 0:
            st['hold'] -= 1
            return
        i = st['idx']; st['idx'] = 0 if i + 1 >= 10 else i + 1
        st['phase'] = 0x65; st['hold'] = _s8(BLINK_HOLD + i)
    elif ph == 1:
        st['eye'] = eye_table(2, mood)
        st['phase'] = st['phase'] + 1 if st['extra'] == 0 else 10
    elif ph == 2:
        st['eye'] = eye_table(_s8(EYE_MODE_BYTE + mode), mood); st['phase'] = 0
    elif ph == 10:
        if st['extra'] <= 0: st['phase'] = 2; st['extra'] = 0
        else: st['extra'] -= 1
    elif ph == 0x65:
        st['eye'] = eye_special(mode, mood); st['phase'] += 1
    elif ph == 0x66:
        st['eye'] = eye_special(mode, mood); st['phase'] = 0
