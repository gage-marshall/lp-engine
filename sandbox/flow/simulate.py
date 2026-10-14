"""Run the ported flow machines without the game: a scripted world plays a new game from power-on.
    .venv/bin/python sandbox/flow/simulate.py [--calls]

The world here is a script, not a simulation of the subsystems: screens finish with the results a player's choices
would give, the scene overlay plays each scene for a few frames, fades end at once. What happens between those
answers - which state follows, which screen is requested with which entry argument, which overlay is loaded - is the
game's code (lpflow). Overlay functions are named by address; the overlay loaded at the time decides which one runs."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from lpflow.names import F, NAME
from lpflow.ports import ports
from lpflow.sim import SimCtx, World, SB
from lpflow import screens

SCREEN_NAMES = {0: 'title menu', 1: 'file select', 2: 'file delete', 11: 'profile (handwriting)', 0xc: 'ov22',
                0x11: 'room', 4: 'schedule', 20: 'time setting', 0x10: 'display settings', 0xf: 'sound settings'}


class NewGame(World):
    """a new game: start -> new file -> clock -> profile -> the opening (scenes) -> the room -> quit to the title menu"""
    def __init__(self, scenes=2, room_result=4):
        super().__init__()
        self.flags = [0, 0]
        self.log = []
        self.screen_frames = 0
        self.scene_t = 0
        self.scenes_left = scenes + 1
        # what each screen does: (frames it runs, ('result', n) or ('next', screen id))
        self.screens = {0: (5, ('result', 0)),                     # title menu: ゲームを開始します
                        1: (5, ('next', 20)),                      # file select: a new file -> the clock
                        20: (5, ('next', 11)),                     # time setting -> the profile
                        11: (8, ('result', 2)),                    # profile done (sets the new-game flag)
                        0x11: (6, ('result', room_result))}        # the room: 4 = back to the title menu
        on = self.on
        on(0x2161528, lambda cx: 1)                                # boot overlay passes finish at once
        on(F.Transform_GetInputContextStatus, lambda cx: 1)
        self.title_frames = 0
        on(0x216a9a8, self.title)                                  # ov13 title screen: "press start" after 4 frames
        on(F.ScreenFade_IsActive, lambda cx, which: 0)
        on(F.SaveSelect_Init, lambda cx: 1)                        # the save data is readable
        on(F.GameFlags_Test, self.test)
        on(F.GameFlags_Assign, self.assign)
        on(F.Screen_RunCurrent, self.run_screen)
        on(F.GameState_GetField0x7a, lambda cx: 0)
        on(F.Overlay_Load, lambda cx, ov: self.log.append(('load', ov)))
        on(F.Overlay_Unload, lambda cx, ov: self.log.append(('unload', ov)))
        # the scene: one active character; a scene "plays" while the clock's manual time is held
        on(F.MapArc_GetActiveCharIdAndMode, self.active_char)
        on(F.GameClock_ClearManualTime, lambda cx, t: 1 if self.scene_t < 10 else 0)
        on(F.MapArc_SetBgmId, self.scene_start)
        on(F.MapArc_GetCurrentCharId, lambda cx: 1 if self.scenes_left > 1 else 0)   # another scene follows
        on(F.GameFlow_GetStateField4, lambda cx: 0)

    def title(self, cx):
        self.title_frames += 1
        return 1 if self.title_frames > 4 else 0

    def test(self, cx, a, b):
        return int(bool((self.flags[0] & a) or (self.flags[1] & b)))

    def assign(self, cx, a, b, on):
        for i, m in enumerate((a, b)):
            self.flags[i] = (self.flags[i] | m) if on else (self.flags[i] & ~m)

    def run_screen(self, cx):
        sid = cx.load(screens.REQ + 0xc, signed=True)
        frames, (what, v) = self.screens.get(sid, (1 << 30, ('result', 0)))
        self.screen_frames += 1
        if self.screen_frames < frames:
            return 1
        self.screen_frames = 0
        if what == 'next':
            cx.store(SB + 4, v)
            return 3
        if sid == 11:                                              # the profile marks the new game
            self.flags[0] |= 0x20000000
        cx.store(SB + 0x14, v)
        return 4

    def active_char(self, cx, zero, kind, ptr):
        kind[0], ptr[0] = 1, 0
        return 1

    def scene_start(self, cx, bgm):
        """a scene begins (the scene machine's state 0x16 sets its music): it plays for 10 frames"""
        self.scene_t = 0
        self.scenes_left -= 1

    def frame(self, cx):
        self.scene_t += 1


if __name__ == '__main__':
    w = NewGame()
    cx = SimCtx(ports(), w)
    seen_screens = []
    last_req = None
    w.on(F.Screen_Request, None)
    reqs = []
    real_request = cx.ports[F.Screen_Request][1]

    def request(cx_, kind, screen, arg):
        reqs.append((cx_.frame, screen, arg))
        return real_request(cx_, kind, screen, arg)
    cx.ports[F.Screen_Request] = ('Screen_Request', request)
    for _ in range(400):
        cx.step()
        if any(t[2] == 'Top_Run' and t[3] == 0x28 and t[4] == 0x1f for t in cx.trace):
            break
    names = {'Top_Run': 'top', 'SaveSelect_Run': 'saveselect', 'Scene_Run': 'scene', 'SceneLoopA_Run': 'loopA',
             'SceneLoopB_Run': 'loopB', 'Top_BootOverlays': 'boot'}
    print('state changes (frame, machine, old -> new):')
    for f, d, m, old, new in cx.trace:
        print('  %4d  %s%-11s %4x -> %x' % (f, '  ' * (d - 2), names.get(m, m), old & 0xffff, new & 0xffff))
    print('screens requested (frame: screen, entry argument):')
    print('  ' + ', '.join('%d: %s %d' % (f, SCREEN_NAMES.get(s, s), a - (1 << 32) if a >= 1 << 31 else a) for f, s, a in reqs))
    print('overlays:', ' '.join(('+%d' if k == 'load' else '-%d') % v if v >= 0 else '-all' for k, v in w.log))
    if '--calls' in sys.argv:
        print('externals answered by default (0):')
        for n, k in sorted(cx.unhandled.items(), key=lambda x: -x[1]):
            print('  %6d %s' % (k, n))
