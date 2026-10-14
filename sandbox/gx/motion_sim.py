"""Heroine motion controller, ported from the game: script motion commands -> per-part animation layers, per game update.

Ports (code addresses; decompilation in fid/decomp/motionctl.c):
  MapCtl_UpdateTouchSelection 0x204f664   per-update driver: start a requested transition once its hold (+0x90) has run out,
                                          the motion group is loaded and the previous blend is over; pending mood (+0x4, delay +0x94);
                                          when idle and the pose is ready: queued action (+0x74 = cmd 0x5d) or the default key (+0x9c)
  MapCtl_BeginTransitionByKey 0x204fd50   (cmd f via ..WithMode8 0x204fe9c: +0x88 = 8, +0x90 = hold)
  MapCtl_QueuePendingAction74 0x2050034   (cmd 0x5d)
  Gfx3D_UpdateActor/ProcessActor/UpdateModel_2/InitializeModel/ProcessModel (0x207d99c, 0x207da90, 0x207d5c8, 0x207d218, 0x207d6d0)
  Gfx3D_DrawActor 0x207d764               per update: blend weights (Motion_UpdateBlendWeights) or the group state machine
  Motion_UpdateStateMachine 0x1ffa03c     segment stepping per group type; Motion_IsPoseReady / AreAllLayersFinished
  Mod2045ee8_Process 0x2046670            per-layer frame advance: += 1 frame, wrap if the layer loops, else clamp (= finished)
Not modelled: async loading (assumed ready), motion ids in Motion_IsIdInSpecialRangeSet, touch input, random transitions (0x4d).
"""
import json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE)
import nnsgen  # noqa: E402
import scene_data as sd  # noqa: E402

SEGMENTS = [struct.unpack_from('<i', sd.MAIN, 0x2102e08 - 0x2000000 + 4 * i)[0] for i in range(9)]   # Motion_GetTableValueOrOne
PARTS = ('hair', 'body', 'rarm', 'larm', 'leg')
OWN = {'hair': 'hair0', 'body': 'body', 'rarm': 'rarm', 'larm': 'larm', 'leg': 'leg'}
_MT = None


def mtab():
    global _MT
    if _MT is None:
        _MT = json.load(open(os.path.join(ROOT, 'content', 'engine', 'motion_table.json')))['records']
    return _MT


def group(idx):                 # Motion_GetGroupIdForIndex (s16 table 0x212588c = async3d bundle)
    return mtab()[idx]['bundle']


def info(idx):                  # Motion_GetInfoRecord: 6 bytes at 0x2126028 + 6 * group {s8 type, s8 mood, s8, s8 flag, s16 number}
    return mtab()[idx]['info']


def group_start(idx):           # Motion_FindGroupStartIndex
    g = group(idx)
    rng = range(0, idx) if idx < 4 else range(idx - 4, idx)
    for j in rng:
        if group(j) == g:
            return j
    return idx


def offset_valid(idx):          # Motion_IsFrameOffsetValidForType
    t = info(idx)[0]; s = idx - group_start(idx)
    return (t in (2, 3) and s == 1) or (t == 4 and s in (1, 3)) or (t in (6, 7) and s == 2)


def special(idx):
    """Motion_IsIdInSpecialRangeSet 0x1ffaa74: idle-type groups (their key becomes the default key, MapCtl +0x9c)"""
    s = group_start(idx)
    if s in (200, 346, 363, 364, 365, 719, 733, 735, 941, 942, 943, 944) or 236 <= s <= 247:
        return True
    return info(idx)[0] == 4


IDLE_PRESETS = (1008, 1009, 1010, 2008, 2009, 2010, 3008, 3009, 3010)


def idle_key(preset):
    """MapCtl_StartRandomTransition 0x204fedc: idle key 0x28e for these camera presets, else 0x217"""
    return 0x28e if preset in IDLE_PRESETS else 0x217


def key_entry(key, heroine, cls):
    """tm_model_block_base 0101: {s8 mood, s8 eye, s16 index} for (key, class, heroine)"""
    t = sd.arc('tm_model_block_base.arc/0101.bin')
    return struct.unpack_from('<bbh', t, 4 + key * 36 + cls * 12 + heroine * 4)


class Layer:
    def __init__(self, idx, frame=0):
        self.idx, self.frame = idx, frame      # frame in fx32

    def length(self):
        return _nframe(self.idx)


_NF = {}


def _nframe(idx):
    if idx not in _NF:
        r = mtab()[idx]
        d = nnsgen.bundle_entries(sd.arc('async3d.arc/%04d.bin' % r['bundle']))[r['stem'] + '.nsbca']
        _NF[idx] = nnsgen.load_bca(d)['%s_body' % r['stem']].nframe
    return _NF[idx]


class MotionSim:
    def __init__(self, heroine, cls, default_key=-1):
        self.h, self.cls = heroine, cls
        # chara (Model) fields
        self.idx = -1; self.type = 0; self.seg = 0; self.rem = 0; self.total = 0
        self.w = [0, 0, 0]; self.n = [4096, 0, 0]; self.inc = [-1, -1, -1]; self.s30 = 0; self.s34 = 0; self.s38 = 0
        self.mood = 0; self.eye_mode = 9; self.on_mood = None
        self.layers = [None, None, None]; self.loop0 = False; self.loop1 = False
        # MapCtl fields
        self.busy = 0; self.target = -1; self.by_key = 0; self.key = -1; self.blend = 0xf; self.m6c = 0
        self.hold = 0; self.m88 = 0; self.m8c = 0; self.q74 = 0; self.q80 = -1; self.q84 = 0xf
        self.mood_pending = -1; self.mood_after = -1; self.mood_delay = 0; self.default_key = default_key

    # ---------------------------------------------------------------- script commands
    def cmd_f(self, key, blend=0xf, hold=0):
        self.begin_by_key(key, blend, 0)
        self.m88 = 8; self.hold = hold

    def cmd_set_actor(self, preset):
        """set_actor (cmd 2) -> MapCtl_SetupAndRefresh -> MapCtl_StartRandomTransition(0): the preset's idle motion"""
        self.begin_by_key(idle_key(preset), 0, 0)

    def cmd_5d(self, key, timeout=8, blend=0xf):
        self.q74 = 1; self.q80 = key; self.q84 = blend; self.m88 = timeout; self.m8c = 0

    def cmd_34(self, mood, delay=0):
        """face (task 0x34): at once when no transition runs and delay <= 0, else pending with a delay"""
        if not self.busy and delay <= 0:
            self.set_mood(mood)
        else:
            self.mood_pending = mood; self.mood_delay = delay

    def cmd_136(self, mood):
        self.mood_after = mood

    def begin_by_key(self, key, blend, m6c):
        idx = key_entry(key, self.h, self.cls)[2]
        if self.idx >= 0 and idx == group_start(self.idx):
            return
        self.busy = 1; self.target = idx; self.by_key = 1; self.key = key; self.blend = blend; self.m6c = m6c
        self.q74 = 0; self.hold = 0
        if special(idx):
            self.default_key = key                           # MapCtl +0x9c

    # ---------------------------------------------------------------- per update
    def pose_ready(self):
        if self.idx == -1:
            return True
        cnt = SEGMENTS[info(self.idx)[0]] if info(self.idx)[0] >= 0 else 1
        if self.idx - (cnt - 1) < 0 or group(self.idx - (cnt - 1)) != group(self.idx):
            return False
        return self.all_finished()

    def all_finished(self):
        L = self.layers[0]
        return L is not None and not self.loop0 and L.frame == L.length() << 12

    def update(self):
        """MapCtl_UpdateTouchSelection (without touch); Model_UpdateCharaScene -> Gfx3D_DrawActor first"""
        self.draw_actor()
        if not self.busy:
            if self.mood_delay > 0:
                self.mood_delay -= 1
            if self.mood_delay < 1 and self.mood_pending != -1:
                self.set_mood(self.mood_pending); self.mood_pending = -1
        else:
            if self.hold > 0:
                self.hold -= 1
            if self.hold < 1 and self.rem <= 0:              # group loaded (assumed) and the previous blend is over
                self.start(self.target, self.blend, self.m6c, self.key if self.by_key else None)
                self.busy = 0; self.by_key = 0
        if self.idx == 0 or self.busy:
            return
        if not self.pose_ready():
            if offset_valid(self.idx):
                self.m8c += 1
                if self.m8c >= self.m88:
                    self.s34 = 1
            return
        if self.q74:
            self.begin_by_key(self.q80, self.q84, 0)
        else:
            self.begin_by_key(self.default_key, 0xf, 0)
        self.mood_pending = self.mood_after; self.mood_after = -1
        self.m8c = 0; self.m88 = 0

    def start(self, idx, blend, m6c, key):
        """Gfx3D_UpdateActor (by key) / ProcessActor -> UpdateModel_2 -> InitializeModel; then the motion's mood"""
        t = info(idx)[0]; loop = m6c
        self.s30 = m6c
        if t == 1: loop = 1
        elif t == 8: loop = 0; self.s30 = 1
        elif t != 0: loop = 0
        self.type = t; self.seg = idx - group_start(idx)
        self.init_model(idx, blend if self.idx != -1 else 0, loop)
        mood = self.mood_pending                         # the controller passes the pending mood (+0x4) to UpdateActor
        if mood == -1 and key is not None:
            mood = key_entry(key, self.h, self.cls)[0]
        if mood == -1:
            mood = info(idx)[1]
        self.set_mood(0 if mood == -1 else mood)

    def set_mood(self, mood):
        """Model_SetMotionIdAndInitFlags 0x2079c74: mood (+0x454) and face bytes; always starts a type-2 eye reload"""
        self.mood = mood
        if self.on_mood:
            self.on_mood(mood)

    def init_model(self, idx, blend, loop, prev_override=None):
        prev = self.layers[0]; prev_idx = self.idx
        if blend < 1 or prev is None:
            self.rem = self.total = 0; self.inc = [-1, -1, -1]
            self.layers = [Layer(idx), None, None]
        else:
            self.rem = self.total = blend
            self.w = [0, 4096, 0]; self.n = [0, 4096, 0]          # layer ratios at init: +0x1c = 0, +0x1e = 0x1000
            self.inc = [nnsgen_inc(blend, 0.7), nnsgen_inc(blend, 0.7), 0]
            self.layers = [Layer(idx), Layer(prev_idx, prev.frame), None]
            self.loop1 = offset_valid(prev_idx) or info(prev_idx)[0] == 1
        self.loop0 = bool(loop)
        self.idx = idx

    def draw_actor(self):
        if self.rem < 1:
            self.state_machine()
        else:
            n = nnsgen.motion_blend_ratios(self.total, self.total - self.rem + 1)
            self.rem -= 1
            if self.rem < 1:
                self.n = [4096, 0, 0]; self.layers[1] = self.layers[2] = None
            else:
                self.n = n

    def state_machine(self):
        if self.idx == -1:
            return
        t = self.type; s = self.idx - group_start(self.idx); fin = self.all_finished

        def pm(step, loop):
            self.idx += step
            self.seg = (self.seg + 1) % SEGMENTS[t]
            self.init_model(self.idx, 0, loop)

        def clear():
            if self.s34 == 1:
                self.s34 = 0; self.loop0 = False
        if t == 1:
            if self.s38 == 1 and self.s34 == 1:
                self.s34 = 0; self.loop0 = False
        elif t == 2:
            if s == 0 and fin(): pm(1, 1)
            elif s == 1:
                clear()
                if self.s30 == 1 and fin(): pm(-1, 0)
        elif t == 3:
            if s == 0 and fin(): pm(1, 1)
            elif s == 1:
                clear()
                if fin(): pm(1, 0)
            elif s == 2 and self.s30 == 1 and fin(): pm(-2, 0)
        elif t == 4:
            if s == 0 and fin(): pm(1, 1)
            elif s == 1:
                clear()
                if fin(): pm(1, 0)
            elif s == 2 and fin(): pm(1, 1)
            elif s == 3:
                clear()
                if self.s30 == 1 and fin(): pm(-3, 0)
        elif t == 5:
            if s == 0 and fin(): pm(1, 0)
            elif s == 1 and self.s30 == 1 and fin(): pm(-1, 0)
        elif t == 6:
            if s == 0 and fin(): pm(1, 0)
            elif s == 1 and fin(): pm(1, 1)
            elif s == 2:
                clear()
                if self.s30 == 1 and fin(): pm(-3, 0)
        elif t == 7:
            if s == 0 and fin(): pm(1, 0)
            elif s == 1 and fin(): pm(1, 1)
            elif s == 2:
                clear()
                if fin(): pm(1, 0)
            elif s == 3 and self.s30 == 1 and fin(): pm(-3, 0)
        elif t == 8:
            if s == 0 and fin(): pm(1, 0)
            elif s == 1 and fin(): pm(-1, 0)

    def advance(self):
        """Mod2045ee8_Process for layers 0..2 (once per update; parts follow the body via Motion_CopyLayer0ToAllLayers)"""
        for k, L in enumerate(self.layers):
            if L is None:
                continue
            n = L.length() << 12
            L.frame += 0x1000
            if L.frame >= n:
                loops = self.loop1 if k == 1 else self.loop0
                L.frame = 0 if loops else n

    def chain(self):
        """AnmObj chain for rendering: [(stem@bundle, frame, ratio)], head first"""
        out = []
        for k, L in enumerate(self.layers):
            if L is None:
                continue
            r = mtab()[L.idx]
            out.append(('%s@%d' % (r['stem'], r['bundle']), L.frame >> 12, self.n[k] if self.rem > 0 else (4096 if k == 0 else 0)))
        return out


def nnsgen_inc(blend, k):
    import numpy as np
    f = np.float32
    return int(f(4096.0) / (f(blend) * f(k)))


# ------------------------------------------------------------------ visibility tracks, eye machine, hands
EYE_CODE = [struct.unpack_from('<i', sd.MAIN, 0x2102e64 - 0x2000000 + 4 * i)[0] for i in range(22)]   # Gfx3D_AdvanceActorLoop
HAND_BASE = {'l': [struct.unpack_from('<h', sd.MAIN, 0x2102e50 - 0x2000000 + 4 * h)[0] for h in range(3)],
             'r': [struct.unpack_from('<h', sd.MAIN, 0x2102e52 - 0x2000000 + 4 * h)[0] for h in range(3)]}
_BVA = {}


def bva(idx):
    """the motion's visibility tracks: async3d bundle (group + 549) '<stem>.nsbva' (MotionQueue slot B)"""
    if idx not in _BVA:
        sys.path.insert(0, os.path.join(ROOT, 'tools'))
        import nns3d
        r = mtab()[idx]
        d = nnsgen.bundle_entries(sd.arc('async3d.arc/%04d.bin' % (r['bundle'] + 549)))[r['stem'] + '.nsbva']
        _BVA[idx] = {a['name'][len(r['stem']) + 1:]: a for a in nns3d.parse_bva(d)}
    return _BVA[idx]


def vis_value(anim, frame, base, eye):
    import nns3d
    for node in range(base, anim['nodes']):
        if (not eye or node not in (0, 2, 3, 4)) and nns3d.visible(anim, frame, node):
            return node - base
    return -1


class EyeSim:
    """eye state at chara+0x488 (UiAsset_UpdateSceneVariants 0x2094e90): blink (sd.blink_step) when +0x21 == 0, else the
    mode transition (0x2094cdc); requests via Model_RequestMotionState 0x2079eb4"""
    def __init__(self, rand=None):
        # rand: scene_data.Rand64 with the RNG state at the scene start (runtime state; only blinks under mood 0x1a draw)
        self.rand = rand
        # state after model creation (traced, write watch on chara+0x488.. at set_actor): Model_ResetMotionFlagsEntry
        # 0x207a004 -> phase +0 = 0, counter +1 = 1, idx +2 = 0; Model_ClearLoaderSet 0x207a06c -> loaders +4/+8 = -1,
        # transition +0x21, mode +0x28 and the rest 0; UiAsset_LoadVariantResources -> eye +3 = UiAsset_GetVariantByte(0) =
        # 0x2103540[0] (4); the creation mood set (Model_SetMotionIdAndInitFlags 0x2079ca8) -> +0x21 = 2, +0x23 = 0,
        # +0x2c = +0x28: a type-2 transition that reloads the current mode before blinking starts
        self.st = dict(counter=1, idx=0, eye=sd._s8(0x2103540), phase=0, hold=0, extra=0, mode=0)
        self.trans = 2; self.sub = 0; self.req = 0
        # eye texture streaming: ids 0x2103540[0..1] (4, 5) are resident; the loaders hold what the last transition loaded
        self.fixed = (sd._s8(0x2103540), sd._s8(0x2103541)); self.cache = set(); self.pending = set()

    def mood_set(self, mood):                      # Model_SetMotionIdAndInitFlags: +0x4a9 = 2, +0x4ab = 0, +0x4b4 = +0x4b0
        self.trans = 2; self.sub = 0; self.req = self.st['mode']

    def request(self, code):                       # Model_RequestMotionState
        if code in (12, 13, 14):
            code = 2
        if self.trans != 0:
            return
        self.req = code
        if code != self.st['mode']:
            self.trans = 1; self.sub = 0

    def update(self, mood):
        if self.trans == 0:
            sd.blink_step(self.st, mood, rand=self.rand)
        elif self.trans in (1, 2):
            if self.sub == 0:
                r = self.req                          # load table(req), table(modebyte(req)), special(req) for this mood
                self.pending = {sd.eye_table(r, mood), sd.eye_table(sd._s8(sd.EYE_MODE_BYTE + r), mood)}
                sp = sd.eye_special(r, mood)
                if sp != -1: self.pending.add(sp)
                self.sub = 1                          # texture requests
            elif self.sub == 1:
                self.sub = 3                          # loaded (assumed at once)
            elif self.sub == 3:
                self.st['eye'] = sd.eye_table(2, mood); self.sub = 2
            elif self.sub == 2:
                self.st['mode'] = self.req; self.req = -1; self.trans = 0; self.sub = 0
                self.st['eye'] = sd.eye_table(sd._s8(sd.EYE_MODE_BYTE + self.st['mode']), mood)
                self.cache = set(self.pending) | set(self.fixed)
        # residency check (0x2094ee8..0x2094fe8): a non-resident eye texture forces a reload of the current mode
        e = self.st['eye']
        if e != -1 and e not in self.cache and self.trans != 2:
            self.sub = 0; self.trans = 2; self.req = self.st['mode']


def actor_state(sim, eye, heroine, field4a4=0, eye_variant=0):
    """Gfx3D_UpdateActorState: eye request + hand poses from the visibility tracks at layer 0's frame"""
    L = sim.layers[0]
    if L is None:
        return None, None
    tr = bva(L.idx)
    any_anim = next(iter(tr.values()))
    fr = L.frame >> 12
    if any_anim['frames'] and any_anim['frames'] <= fr:
        fr = any_anim['frames'] - 1
    vals = [-1, -1, -1]
    for k, (name, base) in enumerate((('eye%02d' % eye_variant, 0), ('lhand', 3), ('rhand', 3))):
        if k == 0 and field4a4 != 0:
            continue
        a = tr.get(name)
        if a is not None:
            vals[k] = vis_value(a, fr, base, k == 0)
    if vals[0] < 0:
        if field4a4 == 0:
            eye.request(9)
    else:
        eye.request(EYE_CODE[vals[0]])
    hands = (vals[1] if vals[1] >= 0 else None, vals[2] if vals[2] >= 0 else None)
    return hands
