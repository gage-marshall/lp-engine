"""Script pacing per game update: the event's VM stream (tools/event_trace.py) + the player's taps -> the update in which
each script command runs.

Rules (measured on session 20261013-084834 and tied to the code paths named):
  cmd 0x44 n        countdown task, n updates (TaskList_FreeAfterCountdown)
  cmd 0xd/0xe n[,s] screen fade: ScreenFade steps once per 3D frame (3 per update with the 3-phase capture) after the
                    delay argument (0xe default delay 6), n steps; the task frees after both fades are idle 3 updates
                    (TaskList_FreeAfterFadesIdleThreeTicks)
  cmd 0x2           set_actor: model load (I/O; `load_updates`, 0 in a port)
  cmd 0x1           yield: the script continues in the first update after no task runs
  text commit       shown in the update after the burst; typed by the typewriter (`typing_end`, from update u+2 or the
                    voice start if later); the first tap release while typing only completes the text; after typing, a release closes the box and the script continues 4
                    updates after the update cycle containing the release (12 3D steps from the end of that cycle)
"""
import json, os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, os.path.join(ROOT, 'tools'))

CLOSE_FRAMES = 19
# Message box (the Transform context *0x21418a8, state field +8; the script blocks with ctx +0x10bd = 0x83 and
# Script_ExecuteOpcode 0x204d388 handles the state once per update): traced in the replayed session.
#  open:  text op -> state 3. The script's update handles 3 with GameSnd_AdvanceStreamStartState 0x2073990: 1 at once
#         when no voice is pending, else once VoiceQueue_IsLoaded (the request is processed by ProcessStreamRequestQueue the
#         update after cmd 0x5c, the load is seen the update after that) -> state 4. State 4 = slide-in: a VBlank job adds 10
#         per VBlank to 0x27e2608 up to 80 (8 VBlanks = 2 updates of the 4-frame cycle) -> 5, then 0 and 6 on the next two
#         passes; 6 = TextControl typing (first character 2 frames after the update's phase-0 run).
#         => typing starts after update A + 2, A = the text's update u, or u + 1 with a voice (19 / 19 lines).
#  close: DialogState_Update 0x208fce4 consumes the tap release in the first update whose phase-0 run starts after it
#         (U = cycle_of(release) + 1) -> state 10. Update U + 1: the script handles 10 (GameSnd_StopStreamAndClearQueue, ->
#         16, slide-out job 0x208f500: 8 VBlanks to 0x27e2614 = 1). MenuList_RefreshState 0x208e0f8 -> 17 on the pass that
#         sees the flag, after that frame's script; update U + 4 handles 17 (SceneRoute_UpdateSchedule, ctx +0x10bd = 0) and
#         runs the next commands. => resume at cycle_of(release) + 5 (19 / 19 closes).
TYPE_START = 2          # A + 2 (slide-in: 2 updates)
VOICE_LOADED = 2        # cmd 0x5c in update c: request processed in c + 1, IsLoaded seen in c + 2
VOICE_START = 4         # the stream itself plays from update c + 4 (UpdateStreamSlots phase-2 pass after c + 3; lip sync)
CLOSE_UPDATES = 5       # release consumed in cycle + 1, slide-out, resume in cycle + 5


def _div(a, b):
    """_s32_div_f: signed division truncating toward zero"""
    q = abs(a) // abs(b)
    return q if (a < 0) == (b < 0) else -q


class ScreenFade:
    """ScreenFade 0x213e4cc (one entry; the scene fades both screens alike): cur 16.16 (-1.0 black, 0 clear), step, state
    (0 to black, 2 to clear, 3 idle), delay counter. ScreenFade_ToBlack 0x20451f8: step = (-1.0 - cur) / max(n, 1).
    ScreenFade_ToClear 0x20452a4: step = -cur / max(n, 1), counter 0. ScreenFade_Process 0x2045390, once per main-loop pass
    (3 per update): to black steps and clamps at -1.0; to clear counts up and steps once counter >= delay, idle when the
    sign of cur reaches the step's. The truncated step makes n = 20 take 21 steps (RAM: idle after 26 = 5 + 21 calls)."""
    def __init__(self, cur=-0x10000):
        self.cur = cur; self.step = 0; self.state = 3; self.counter = 0; self.delay = 0

    def to_black(self, n):
        self.state = 0; self.step = _div(-0x10000 - self.cur, max(n, 1))

    def to_clear(self, n, delay):
        self.state = 2; self.step = _div(-self.cur, max(n, 1)); self.counter = 0; self.delay = delay

    def process(self):
        if self.state == 0:
            self.cur += self.step
            if self.cur <= -0x10000: self.cur = -0x10000; self.state = 3
        elif self.state == 2:
            self.counter += 1
            if self.counter >= self.delay:
                self.cur += self.step
                if (self.step >= 0 and self.cur >= 0) or (self.step < 0 and self.cur <= 0):
                    self.cur = 0; self.state = 3

    def active(self):
        return self.state != 3


def char_intervals():
    """TextControl_ProcessText 0x208bf4c interval table 0x21274e0: 5 x 12.12 frames per character, indexed by the text
    speed setting"""
    import scene_data as sd
    return [sd.w32(0x21274e0 + 4 * i) for i in range(5)]


def text_speed(save_block0):
    """settings record +0xc (SaveData_GetFieldc 0x2093d78), passed by MenuText_SelectState 0x208c740 to
    TextLayout_BuildGlyphs (forced to 1 when the menu's +0x3874 is set). Saved by Ov00Init_PackSaveA 0x21585b4 as section 2
    entry 2 = save block 0 byte 0x41 (layout from the descriptor tables 0x2165e80 / counts 0x2164f00)"""
    return struct.unpack_from('<b', save_block0, 0x41)[0]


def typing_end(f0, nch, speed, phase_of):
    """frame in which the last of nch characters is drawn, typing from frame f0. TextControl_ProcessText runs once per
    frame: the 12.12 accumulator loses 0x1000; while its integer part stays > 0 nothing is drawn, otherwise it reloads
    with interval[speed] / div (_s32_div_f, truncating) and one character is drawn. div = 0x213d2e4+0xa0, which
    VramBank_Process 0x2044ee8 sets to 2 after a phase-2 pass, else 1: div = 2 iff the previous frame's Capture3D phase
    is 2 (2071 / 2071 recorded reloads and steps; phase_of(frame) = Capture3D phase byte 0x213d2e4+0xa3)."""
    if speed < 0: return f0                      # -1: no typewriter, the text is drawn at once
    iv = char_intervals()[speed]; acc = 0; f = f0; drawn = 0
    while True:
        if acc >> 12 > 0: acc -= 0x1000
        if acc >> 12 <= 0:
            acc = int(iv / (2 if phase_of(f - 1) == 2 else 1)); drawn += 1
            if drawn >= nch: return f
        f += 1


def stream(event_id):
    import event_trace
    return event_trace.trace(os.path.join(ROOT, 'content'), event_trace.parse_event_id('0x%08x' % event_id), 4000)['events']


def schedule(event_id, update_frames, releases, start_update=0, load_updates=0, steps_per_update=3, speed=1,
             cycle_of=None, phase_of=None):
    """update_frames[u] = frame of update u; cycle_of(frame) = index of the update cycle containing a frame (the player's
    tap timing in game updates); phase_of(frame) = Capture3D phase of a frame; releases = tap release frames; speed = text
    speed setting (`text_speed`).
    Returns [(update, kind, payload)] for commands and texts in stream order."""
    ev = stream(event_id)
    out = []; u = start_update; i = 0
    tasks = []                                   # [kind, remaining]
    rel = sorted(releases); ri = 0
    voice_u = None                               # update in which the pending voice starts

    fade = ScreenFade()                          # the scene starts black

    def tick_tasks():
        """one update: 3 ScreenFade_Process passes, then the task list"""
        for _ in range(steps_per_update): fade.process()
        for t in tasks:
            if t[0] in ('count', 'load'):
                t[1] -= 1
            elif t[0] == 'fade' and not fade.active():
                t[1] += 1                            # TaskList_FreeAfterFadesIdleThreeTicks 0x206c6ec: frees when > 2
        tasks[:] = [t for t in tasks if not ((t[0] in ('count', 'load') and t[1] <= 0) or (t[0] == 'fade' and t[1] > 2))]

    while i < len(ev):
        e = ev[i]
        if e['kind'] == 'command':
            c = int(e['id'], 16); a = e['args']
            out.append((u, 'cmd', (c, a)))
            if c == 0x44:
                tasks.append(['count', a[0] if a else 1])
            elif c in (0xd, 0xe):
                n = a[0] if a else 10
                fade.to_black(n) if c == 0xd else fade.to_clear(n, 6)
                tasks.append(['fade', 0])
            elif c == 0x5c and a and a[0] >= 0:
                voice_u = u + VOICE_LOADED          # A for a voiced line
            elif c == 0x2:
                if load_updates: tasks.append(['load', load_updates])
            elif c == 0x1 and tasks:
                while tasks:                     # yield: the task list frees at the end of an update; the script sees it next
                    u += 1; tick_tasks()
                u += 1; tick_tasks()
            i += 1
        elif e['kind'] == 'text':
            u += 1; tick_tasks()                 # the message box takes the text in the next update
            out.append((u, 'text', e.get('text')))
            shown = update_frames[min(u, len(update_frames) - 1)]
            nch = len([ch for ch in (e.get('text') or '') if ch != '\n'])
            t0 = max(u, voice_u if voice_u is not None else 0) + TYPE_START; voice_u = None
            typed = typing_end(update_frames[min(t0, len(update_frames) - 1)] + 2, nch, speed, phase_of)
            while ri < len(rel) and rel[ri] <= shown: ri += 1
            if ri < len(rel) and rel[ri] <= typed: ri += 1             # the first tap while typing only completes the text
            if ri >= len(rel): break
            target = cycle_of(rel[ri]) + CLOSE_UPDATES; ri += 1
            while u < target:
                u += 1; tick_tasks()
            i += 1
        else:
            i += 1
    return out
