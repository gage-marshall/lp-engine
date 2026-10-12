#!/usr/bin/env python3
"""Play LovePlus+ in a window while logging event-selection activity.

Usage (repo root, repo venv, ROM at rom/loveplus_plus_j.nds):
    .venv/bin/python tools/play_monitor.py [--state NAME] [--speed N] [--no-rotate]

The console is played sideways, so the window shows the two screens side by
side in reading orientation; the mouse is the stylus.

Keys
    arrows / Z X A S Q W   D-pad / A B X Y L R        Enter = START, Backspace = SELECT
    Tab (hold)             fast-forward (--speed x)
    F5 / F9                save / load quick state    (states/quick.dst)
    F6                     save a named state (name typed in the terminal)
    F1                     screenshot                 (states/logs/shots)
    M                      drop a marker in the log (type nothing; say what you did in chat)
    Esc                    quit

Log: tools/emu_scripts/states/logs/session-<time>.jsonl (git-ignored, user-local).
Each record: {"t": seconds, "f": frame, "ev": name, ...} with the last date/hour
the game clock reported.  Hooked: script starts, event-selection functions
(EventPick/EventLogic/RandEvent/Schedule), clock reads.  Noisy hooks are muted
automatically (a "muted" record says so).
"""
import argparse
import collections
import csv
import json
import os
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
sys.path.insert(0, HERE)
from emu_driver import Emu, KEYS  # noqa: E402

import pygame  # noqa: E402

STATES = os.environ.get('EMU_STATES', os.path.join(HERE, 'emu_scripts', 'states'))
LOGDIR = os.path.join(STATES, 'logs')

SCRIPT_START = 0x205e7c4   # ScriptMgr_Start(id, ...)
SCRIPT_BEGIN = 0x204d6c0   # Script_Begin
HOOK_PREFIXES = ('EventPick_', 'EventLogic_Select', 'EventLogic_Trigger', 'EventLogic_Commit',
                 'EventLogic_RunEventStateMachine', 'EventLogic_ProcessCalendarEvents',
                 'EventLogic_ProcessTimePeriodEvents', 'EventLogic_StartEventSession',
                 'EventRoll_', 'EventList_', 'RandEvent_RunSequence', 'GameState_SelectRandomEvent',
                 'GameState_TryRandomSaveEvent', 'Calendar_SetOverrideDay', 'FlowState_SetEventPair',
                 'Schedule_GetDateEntry', 'Scene_SelectRandomTransition', 'RandomEvent_SelectTransition',
                 'RandomEvent_ProcessSelection', 'MapArc_SetCurrentCharId', 'MapArc_ApplyIdByKind',
                 'MapArc_GetActiveCharIdAndMode', 'MapEvent_InvokeCallback', 'MapEvent_ApplyOutcome',
                 'MapEvent_GetByteField', 'MapSelector_RunTextStateMachine', 'Schedule_ResolveMapMessage',
                 'EventState_HandlePhaseChange', 'DayEvent_GetResultIdForSlotEvent', 'DayEvent_GetMonthEventId',
                 'DayEvent_BuildCalendarEventList', 'DayEvent_PickRandomUniqueSlots', 'SceneRoute_SelectTextRoute',
                 'TextEvent_TestMap')
# functions whose return value is worth logging (entry args + r0 on return)
RETURNING = ('EventPick_SelectEventId', 'EventPick_GetTimeVariantEventId', 'EventPick_BuildCandidateList',
             'EventPick_GetTimeCategory', 'EventRoll_RollOutcomeByWeights', 'Schedule_GetDateEntry',
             'GameState_SelectRandomEvent', 'GameState_TryRandomSaveEvent', 'EventLogic_SelectEligibleEvent',
             'EventLogic_DispatchTrigger', 'MapArc_GetActiveCharIdAndMode', 'MapEvent_InvokeCallback',
             'MapEvent_GetByteField', 'MapSelector_RunTextStateMachine', 'Schedule_ResolveMapMessage',
             'DayEvent_GetResultIdForSlotEvent', 'DayEvent_GetMonthEventId', 'SceneRoute_SelectTextRoute')
TINY_SKIP = ('EventPick_GetTimeCategory',)   # called every frame by the map scene; entry not logged
# GameClock_Get{Year,Month,Day,Weekday,Hour} return sites (r0 = value)
CLOCK_EXITS = {'y': (0x205f4cc, 0x205f4d4), 'm': (0x205f558, 0x205f560), 'd': (0x205f5ec, 0x205f5e4),
               'w': (0x205f694, 0x205f69c), 'h': (0x205f2c8, 0x205f2d0)}
SCRIPT_CTX_PTR = 0x21415b8   # *this = ScriptContext*: +0x10c0 cmd, +0x10cc sp, +0x10d4 stack (stack[sp] = argc, stack[sp+argc] = a0)
EVENT_SESSION = 0x2156898     # event-selection session: +4 heroine slot, +5 activity code, +8 result, +0xc stage kind, +0x10 ctx mask, +0x14 stage index, +0x16 period
EVENT_CTX_PTR = 0x20a40a8     # literal pool word -> &ctx pointer; *ctx = event kind (EventLogic_SelectEvent)


def load_hooks():
    hooks = {}
    with open(os.path.join(ROOT, 'fid', 'labels_all.csv')) as f:
        for r in csv.reader(f):
            if len(r) > 2 and r[0] == 'static' and r[2].startswith(HOOK_PREFIXES):
                hooks[int(r[1], 16)] = r[2]
    return hooks


class Monitor:
    def __init__(self, emu, path):
        self.emu = emu
        self.e = emu.e
        self.t0 = time.time()
        os.makedirs(LOGDIR, exist_ok=True)
        os.makedirs(os.path.join(LOGDIR, 'shots'), exist_ok=True)
        self.f = open(path, 'a', buffering=1)
        self.date = None
        self.hour = None
        self.names = {}
        self.pending = collections.defaultdict(list)   # return address -> [(name, args)]
        self.ret_hooked = set()
        self.rate = collections.defaultdict(list)
        self.muted = set()
        self.last_clock = None

    def log(self, ev, **kw):
        rec = {'t': round(time.time() - self.t0, 2), 'f': self.emu.frames, 'ev': ev}
        if self.date: rec['date'] = self.date
        if self.hour is not None: rec['hour'] = self.hour
        rec.update(kw)
        self.f.write(json.dumps(rec) + '\n')

    def regs(self):
        r = self.e.memory.register_arm9
        return r.r0, r.r1, r.r2, r.r3, r.lr

    def throttled(self, name):
        if name in self.muted:
            return True
        now = time.time()
        q = self.rate[name]
        q.append(now)
        while q and now - q[0] > 1.0:
            q.pop(0)
        if len(q) > 60:
            self.muted.add(name)
            self.log('muted', name=name)
            return True
        return False

    def hook(self, addr, name):
        self.names[addr] = name
        self.e.memory.register_exec(addr, lambda a, s, n=name: self.on_entry(n))

    def on_entry(self, name):
        r0, r1, r2, r3, lr = self.regs()
        if name in RETURNING:
            self.pending[lr & ~1].append((name, [r0, r1, r2, r3]))
            if (lr & ~1) not in self.ret_hooked:
                self.ret_hooked.add(lr & ~1)
                self.e.memory.register_exec(lr & ~1, lambda a, s: self.on_return(a))
        if name in TINY_SKIP or self.throttled(name):
            return
        extra = {}
        if name == 'EventLogic_SelectEvent':
            try:
                ctx = self.emu.read32(self.emu.read32(EVENT_CTX_PTR))
                extra['kind'] = self.emu.read32(ctx)
            except Exception:
                pass
        self.log('call', fn=name, args=['%x' % v for v in (r0, r1, r2, r3)], lr='%x' % lr, **extra)

    def on_return(self, addr):
        st = self.pending.get(addr)
        if not st:
            return
        name, args = st.pop()
        if name in self.muted or name in TINY_SKIP:
            return
        r0 = self.e.memory.register_arm9.r0
        self.log('ret', fn=name, args=['%x' % v for v in args], ret='%x' % r0)

    def install_cmds(self):
        """Hook every script command handler (fid/script_commands.csv) and log (cmd, args) at dispatch."""
        self.last_cmd = None
        seen = set()
        with open(os.path.join(ROOT, 'fid', 'script_commands.csv')) as f:
            for r in csv.DictReader(f):
                h = int(r['handler'], 16) & ~1
                if h not in seen:   # several command ids share one handler: read the id from the context instead
                    seen.add(h)
                    self.e.memory.register_exec(h, lambda a, s: self.on_cmd())

    def on_cmd(self):
        rd = self.emu.read32
        try:
            ctx = rd(SCRIPT_CTX_PTR)
            cmd = rd(ctx + 0x10c0)
            sp, stk = rd(ctx + 0x10cc), rd(ctx + 0x10d4)
            argc = rd(stk + sp * 4)
            if argc > 12:
                return
            args = [rd(stk + (sp + argc - i) * 4) for i in range(argc)]
        except Exception:
            return
        key = (cmd, tuple(args))
        if key == self.last_cmd:
            return
        self.last_cmd = key
        self.log('cmd', cmd='%x' % cmd, args=args)

    def install(self):
        self.install_cmds()
        for addr, name in load_hooks().items():
            self.hook(addr, name)
        self.e.memory.register_exec(SCRIPT_START, lambda a, s: self.on_script('script_start'))
        self.e.memory.register_exec(SCRIPT_BEGIN, lambda a, s: self.on_script('script_begin'))
        self.dt = {}
        for k, addrs in CLOCK_EXITS.items():
            for a in addrs:
                self.e.memory.register_exec(a, lambda a_, s, k=k: self.on_clock(k))

    def on_script(self, ev):
        r0, r1, r2, r3, lr = self.regs()
        rd, b = self.emu.read32, EVENT_SESSION
        sess = {'slot': rd(b + 4) & 0xff, 'act': (rd(b + 4) >> 8) & 0xff, 'res': '%x' % rd(b + 8), 'kind': rd(b + 0xc) & 0xffff,
                'mask': '%x' % rd(b + 0x10), 'stage': (rd(b + 0x14)) & 0xffff, 'period': (rd(b + 0x14) >> 16) & 0xff}
        self.log(ev, id='%08x' % r0, args=['%x' % v for v in (r1, r2, r3)], lr='%x' % lr, sess=sess)
        if ev == 'script_start':
            self.shot('start-%08x-f%d' % (r0, self.emu.frames))

    def on_clock(self, k):
        v = self.e.memory.register_arm9.r0
        if not 0 <= v < 4000 or self.dt.get(k) == v:
            return
        self.dt[k] = v
        if k == 'h':
            self.hour = v
        d = self.dt
        if all(x in d for x in 'ymd'):
            nd = '%04d-%02d-%02d%s' % (d['y'], d['m'], d['d'], '/w%d' % d['w'] if 'w' in d else '')
            if nd != self.date:
                self.date = nd
                self.log('clock', date=nd)

    def shot(self, name):
        try:
            self.emu.shot(os.path.join(LOGDIR, 'shots', name + '.png'), rotate=True)
        except Exception as ex:
            print('shot failed', ex)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('--rom', default=os.path.join(ROOT, 'rom', 'loveplus_plus_j.nds'))
    ap.add_argument('--state', help='state name (states/NAME.dst) or path to load at start')
    ap.add_argument('--speed', type=int, default=8, help='fast-forward factor while Tab is held')
    ap.add_argument('--scale', type=int, default=2)
    ap.add_argument('--no-rotate', action='store_true', help='show the raw upright layout (top over bottom)')
    a = ap.parse_args()

    state = None
    if a.state:
        state = a.state if os.path.exists(a.state) else os.path.join(STATES, a.state + '.dst')
    emu = Emu(a.rom, state)
    path = os.path.join(LOGDIR, 'session-%s.jsonl' % time.strftime('%Y%m%d-%H%M%S'))
    os.makedirs(LOGDIR, exist_ok=True)
    mon = Monitor(emu, path)
    mon.install()
    mon.log('start', state=a.state or 'fresh-boot')
    print('logging to', path)

    pygame.init()
    rot = not a.no_rotate
    w, h = (384, 256) if rot else (256, 384)
    win = pygame.display.set_mode((w * a.scale, h * a.scale))
    pygame.display.set_caption('LovePlus+ monitor  (F5 save, F9 load, M marker, Tab fast)')
    clock = pygame.time.Clock()
    keymap = {pygame.K_UP: 'UP', pygame.K_DOWN: 'DOWN', pygame.K_LEFT: 'LEFT', pygame.K_RIGHT: 'RIGHT',
              pygame.K_z: 'A', pygame.K_x: 'B', pygame.K_a: 'X', pygame.K_s: 'Y', pygame.K_q: 'L', pygame.K_w: 'R',
              pygame.K_RETURN: 'START', pygame.K_BACKSPACE: 'SELECT'}
    from desmume.controls import keymask
    held = set()
    touching = False
    quick = os.path.join(STATES, 'quick.dst')
    shot_n = 0
    running = True
    max_frames = int(os.environ.get('LPM_MAX_FRAMES', '0'))
    while running:
        if max_frames and emu.frames >= max_frames:
            break
        for ev in pygame.event.get():
            if ev.type == pygame.QUIT:
                running = False
            elif ev.type == pygame.KEYDOWN:
                if ev.key == pygame.K_ESCAPE:
                    running = False
                elif ev.key in keymap:
                    held.add(keymap[ev.key])
                elif ev.key == pygame.K_F5:
                    emu.save(quick); snap = os.path.join(STATES, 'quick-f%d.dst' % emu.frames); emu.save(snap)
                    mon.log('save', path=snap); print('saved', quick, 'and', snap)
                elif ev.key == pygame.K_F9:
                    if os.path.exists(quick):
                        emu.load(quick); mon.log('load', path=quick); print('loaded', quick)
                elif ev.key == pygame.K_F6:
                    name = input('state name: ').strip()
                    if name:
                        p = os.path.join(STATES, name + '.dst'); emu.save(p); mon.log('save', path=p)
                elif ev.key == pygame.K_F1:
                    shot_n += 1; mon.shot('manual-%d-f%d' % (shot_n, emu.frames))
                elif ev.key == pygame.K_m:
                    mon.log('marker'); mon.shot('marker-f%d' % emu.frames); print('marker at frame', emu.frames)
            elif ev.type == pygame.KEYUP and ev.key in keymap:
                held.discard(keymap[ev.key])
        emu.e.input.keypad_update(sum(keymask(KEYS[k]) for k in held))
        mx, my = pygame.mouse.get_pos()
        mx //= a.scale; my //= a.scale
        down = pygame.mouse.get_pressed()[0]
        if down:
            if rot:
                # reading orientation: bottom screen is the right half (x>=192), page 192 x 256
                if mx >= 192:
                    emu.e.input.touch_set_pos(255 - my, mx - 192); touching = True
                elif touching:
                    emu.e.input.touch_release(); touching = False
            else:
                if my >= 192:
                    emu.e.input.touch_set_pos(mx, my - 192); touching = True
                elif touching:
                    emu.e.input.touch_release(); touching = False
        elif touching:
            emu.e.input.touch_release(); touching = False
        fast = pygame.key.get_pressed()[pygame.K_TAB]
        for _ in range(a.speed if fast else 1):
            emu.step(1)
        im = emu.image()
        if rot:
            top = im.crop((0, 0, 256, 192)).rotate(90, expand=True)
            bot = im.crop((0, 192, 256, 384)).rotate(90, expand=True)
            from PIL import Image
            page = Image.new('RGB', (384, 256)); page.paste(top, (0, 0)); page.paste(bot, (192, 0)); im = page
        surf = pygame.image.frombuffer(im.tobytes(), im.size, 'RGB')
        if a.scale != 1:
            surf = pygame.transform.scale(surf, (w * a.scale, h * a.scale))
        win.blit(surf, (0, 0))
        pygame.display.flip()
        clock.tick(60 * (a.speed if fast else 1))
    mon.log('end')
    pygame.quit()


if __name__ == '__main__':
    main()
