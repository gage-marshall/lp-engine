"""Scene state for the renderer, resolved from game data: event script, save record, clock, fashion tables, voice table.

    .venv/bin/python sandbox/gx/scene_inputs.py            # resolves the recorded break-room moment and checks it against scene_gen.SCENE

Nothing here is read from emulator RAM. The inputs are what a port has anyway:
  event id + script position   which event plays and which text line is on screen (index into the script's event stream)
  save file                    the heroine's record byte +0xd (SaveSlot_SetByte0xd_Range0to2), read from the game's own save
                               (record_0d_from_save: block header + LZ + packed record offset)
  random-event state           FlowState +0x26 (Scene_GetFieldE 0x20ecae0): `random_event_state`, from the calendar state
                               and the save. Not saved itself (FlowState +0x24/+0x26 are recomputed at event load).
  calendar state               GameState +0x78: set to 1 by the day refresh when a heroine enters the lovers state
                               (SaveData_RefreshDateTimeRecords -> GameClock_UpdateCalendar 0x205fc10, record flag bit 0) and
                               by the schedule sequence (ResourceTable_UpdateSchedule); saved: `calendar_state_from_save`.
                               Drives the random-event state and the backdrop time-of-day path.
  clock                        month (1-12) and hour of the game clock (GameClock_GetMonth / GetHour; `session_clock`)
  playback                     game update ticks since the current motion's f command, seconds since the line's voice started, blink

Rules (all traced on the recording at the event load, frame 3901-3902):
  heroine     script id top byte (0x08.. Nene = 2)
  preset      last script command 6 before the position
  motion key  last command f before the position. 0x5d only queues (verified: at position 30, frame 7201, `5d 694` has not
              replaced 587; the stream matches 587 exactly). When a queued motion takes over is not decoded.
  anim frame  = animation ticks since the f command, mod the clip length (sd.motion_frame). Frame 0 is set when f runs
              (5977 here); one tick per completed Capture3D cycle (4 video frames, longer on lag frames), i.e. the game's
              own update count. Measured on the body NNSG3dAnmObj (0x232b5d4): tick 96 -> frame 3 (line26),
              tick 295 -> frame 16 (line30). Blend after f 587: ratio 0 for ticks 1-7, then +510.5/tick to 4096 at tick 16.
  mood        last command 0x34
  outfit      command 0x12e kind 4 (flow_dispatch -> FlowState; MapCtl_SetupContext -> Model_SetPartSlotsByClass, 0x204eed4)
  accessory   command 0x12e kind 10: id*100+variant (SceneState_UpdateFlaggedField -> Model_SetPartSlot1FromId)
  hair        SaveSlot_UpdateStateMetadata 0x2060c7c: MapCtl field +0x14 if set, else FlowState +0x24, which
              Scene_UpdateRandomEventState 0x20ecaf0 sets to Overlay_UpdateMode 0x20eee34(slot, record[+0xd], FlowState+0x26):
              the first entry of fashion/fashion_hair_param_<slot>.bin (u32 n, then n x {s16 label, s8 state, u8 ?, u8 key, 3 x u8})
              with key == record[+0xd] and state == FlowState+0x26; none found and state != 0 -> retry with state 0
  time mode   MapCtl_RemapModeCode 0x204ee30 of Bg_ProcessTimedValue: background kind 3/4 -> bg value, else 1.
              bg value = Bg_UpdateTimedState 0x2085eac from GameClock_RunTimeSequence: season = Calendar_MonthToSeason
              (first i with month <= [2,5,8,11] @0x2101cfc, else 0); phase = first i with hour < table[season][i]
              (@0x2101d4e: [7,16,17] [6,17,18] [5,18,19] [6,17,18]), else 0; phase 0/1/2 -> bg value 0/1/2.
              remap: 0 -> 2, 1 -> 0, 2 -> 1 (overrides by game flags 0x104/0x105 are not modelled: both clear here)
  backdrop    time of day for the backdrop file: `backdrop_tod` (hour periods, ScheduleText_UpdateState)
  viseme      5 (rest) once the line's voice clip is over: clip length = Siren14Voice.vtb (offset, size) x16, 40 bytes per 20 ms
"""
import os, struct, sys
HERE = os.path.dirname(os.path.abspath(__file__)); ROOT = os.path.dirname(os.path.dirname(HERE))
sys.path.insert(0, HERE); sys.path.insert(0, os.path.join(ROOT, 'tools'))
import scene_data as sd  # noqa: E402

CONTENT = os.path.join(ROOT, 'content')
HEROINE_BY_TOP = {0xa0: 0, 0xa4: 0, 0x58: 1, 0x5c: 1, 0x5f: 1, 0x08: 2, 0x0f: 2}


def event_stream(event_id):
    import event_trace
    return event_trace.trace(CONTENT, event_trace.parse_event_id('0x%08x' % event_id), 4000)['events']


def hair_label(slot, key, state):
    d = open(os.path.join(CONTENT, 'assets', 'fashion', 'fashion_hair_param_%d.bin' % slot), 'rb').read()
    n = struct.unpack_from('<I', d)[0]
    for i in range(n):
        label, st = struct.unpack_from('<hb', d, 4 + 8 * i)
        if d[4 + 8 * i + 4] == key and st == state:
            return label
    return hair_label(slot, key, 0) if state != 0 else -1


def lz10(d):
    """NitroSDK LZ77 (MI_CompressLZ / type 0x10)"""
    n = d[1] | d[2] << 8 | d[3] << 16; out = bytearray(); p = 4
    while len(out) < n:
        flags = d[p]; p += 1
        for b in range(8):
            if len(out) >= n:
                break
            if flags & (0x80 >> b):
                x = d[p] << 8 | d[p + 1]; p += 2
                for _ in range((x >> 12) + 3):
                    out.append(out[-((x & 0xfff) + 1)])
            else:
                out.append(d[p]); p += 1
    return bytes(out)


def save_block(sav, card_offset):
    """one save block as Save_WriteBlock (0x20f0fb8) writes it: header {u32 magic 0x10190573, u32 body size, u16 generation,
    u16 version 3, 20-byte hash}, then MI_CompressLZ of the packed buffer (or the raw buffer when compression fails)"""
    magic, size = struct.unpack_from('<II', sav, card_offset)
    assert magic == 0x10190573, 'no save block at %#x' % card_offset
    body = sav[card_offset + 0x20:card_offset + 0x20 + size]
    return lz10(body) if body[:1] == b'\x10' else body


def record_0d_from_save(path, slot, card_offset=0xd4a0):
    """heroine record byte +0xd from a backup image. Block kind 1 (card 0xd4a0 in the recorded save, 0x44fc bytes packed)
    holds the three heroine records at 0x1989 + 0x5b3 * slot (verified against RAM records 0x2144250 + 0x688 * slot for
    all three slots of save backup-session-20261013-110515-f1445)"""
    return save_block(open(path, 'rb').read(), card_offset)[0x1989 + 0x5b3 * slot + 0xd]


def time_mode(month, hour, bg_kind=3):
    seasons = [sd.w32(0x2101cfc + 4 * i) for i in range(4)]
    season = next((i for i, m in enumerate(seasons) if month <= m), 0)
    t = sd.MAIN[0x101d4e + 3 * season:0x101d4e + 3 * season + 3]
    phase = next((i for i in range(3) if hour < t[i]), 0)
    value = phase if bg_kind in (3, 4) else 1
    return {0: 2, 1: 0, 2: 1}[value]


def time_period(hour):
    """TimePeriod_FromHour 0x205ebf4: 5 below hour MAIN[0x2101d4d] (5); else first i with hour < [8,12,15,18,21]
    (0x2101d48), else 5"""
    if hour < sd.MAIN[0x101d4d]: return 5
    return next((i for i in range(5) if hour < sd.MAIN[0x101d48 + i]), 5)


def backdrop_tod(month, hour, calendar_78=0, flag_e0=0, flag_104=0, flag_105=0):
    """backdrop time of day (day 0 / evening 1 / night 2), ScheduleText_UpdateState 0x209dc34: phase =
    TextRes_SelectTimePeriodText 0x209e8f4 (period 0-2 -> 1, 3 -> 2, 4-5 -> 0) when GameState +0x78 (calendar state, set by
    GameClock_UpdateCalendar / ResourceTable_UpdateSchedule) is 0 or game flag 0xe0 is set, else GameClock_RunTimeSequence
    (the season hour table of `time_mode`); flag 0x104 turns phase 1 into 2, flag 0x105 forces 0; then
    ScheduleText_GetStateValue 0x209e888 remaps 0 -> 2, 1 -> 0, 2 -> 1. (RAM over the recorded scene: +0x78 = 0, flags 0.
    Hooked in the replayed session at the backdrop reload, frame 10559: hour 15 -> phase 2 -> evening, as predicted; the
    season-table path would give day.)"""
    if calendar_78 == 0 or flag_e0:
        phase = {3: 2, 4: 0, 5: 0}.get(time_period(hour), 1)
    else:
        seasons = [sd.w32(0x2101cfc + 4 * i) for i in range(4)]
        season = next((i for i, m in enumerate(seasons) if month <= m), 0)
        t = sd.MAIN[0x101d4e + 3 * season:0x101d4e + 3 * season + 3]
        phase = next((i for i in range(3) if hour < t[i]), 0)
    if flag_104 and phase == 1: phase = 2
    if flag_105: phase = 0
    return {0: 2, 1: 0, 2: 1}[phase]


def calendar_state_from_save(path, card_offset=0xd4a0):
    """GameState +0x78 from a backup image: Ov00Init_PackSaveB 0x2159104 writes GameState +0x7b, +0x77, +0x79, +0x7a, +0x78
    as section 0 entries 4-8 of block kind 1 (bytes 0x10-0x14; verified against RAM for all four session backups)"""
    return save_block(open(path, 'rb').read(), card_offset)[0x14]


def slot_byte_107_from_save(path, slot, card_offset=0xd4a0):
    """heroine record +0x107 (SaveSlot_GetSlotPointerC 0x2060c9c; record = GameState_GetSlotPtr = 0x2144250 + 0x688 slot,
    saved byte-identical for 0x140 bytes at block 1 0x1989 + 0x5b3 slot)"""
    return struct.unpack_from('<b', save_block(open(path, 'rb').read(), card_offset), 0x1989 + 0x5b3 * slot + 0x107)[0]


def random_event_state(calendar_78, slot_byte_107=0):
    """FlowState +0x26 at event load: Scene_ProcessRandomEvent 0x20eca3c stores RandomEvent_UpdateStateMachine(1)
    0x20ebe68, which returns 0 when GameState +0x78 is 0, else the active slot's saved byte (SaveSlot_GetSlotPointerC
    0x2060c9c: slot +0x107). Hooked over the replayed session from its start: every event load (0f4c3400, 08143400,
    08143401, 08143402) and every 0x12e kind 4 writes 0 with +0x78 = 0. (0x12e kind 0xd sets it directly from
    Scene_GetOverlayFieldA; not used by this chain.)"""
    return 0 if calendar_78 == 0 else slot_byte_107


def session_clock(session_path, frame):
    """(month, hour) of the game clock at a frame of a play session: the last 'clock' / 'anchor' event of the session log
    (tools/play_monitor.py logs GameClock reads) at or before the frame. GameClock_GetHour 0x205f2ac returns the manual
    (schedule) hour while *0x2142c6c is set, else the RTC hour; in this save it is the schedule hour (8, 12, 15, 18 across
    the session; shifting the RTC by +1 h / -2 h leaves it at 15)"""
    import json
    month = hour = None
    for line in open(session_path):
        x = json.loads(line)
        if x['f'] > frame: break
        if x.get('ev') in ('clock', 'anchor') and 'date' in x:
            month = int(x['date'].split('/')[0].split('-')[1]); hour = x['hour']
    return month, hour


def voice_seconds(clip):
    v = open(os.path.join(CONTENT, 'assets', 'sys', 'Siren14Voice.vtb'), 'rb').read()
    _, size = struct.unpack_from('<II', v, 8 * clip)
    return size // 16 / 40 * 0.02


def resolve(event_id, position, record_0d, random_state, month, hour, updates, voice_t, blink=False, bg_kind=3):
    """scene dict for scene_gen.SceneRenderer + where each value came from"""
    ev = event_stream(event_id)[:position + 1]
    src = {}
    scene = dict(heroine=HEROINE_BY_TOP[event_id >> 24], hand_variant=0, face_variant=0, skin=0, updates=updates, blink=blink)
    src['heroine'] = 'script id top byte %#x' % (event_id >> 24)
    voice = None
    for i, e in enumerate(ev):
        if e['kind'] != 'command':
            continue
        c, a = int(e['id'], 16), e['args']
        if c == 6:
            scene['preset'] = a[0]; src['preset'] = 'cmd 6 @%d' % i
        elif c == 0xf:
            scene['motion_key'] = a[0]; src['motion_key'] = 'cmd f @%d' % i
        elif c == 0x5d:                                  # queued: not current until the playing motion ends
            scene.setdefault('queued', []).append(a[0])
        elif c == 0x34:
            scene['mood'] = a[0]; src['mood'] = 'cmd 34 @%d' % i
        elif c == 0x12e and a[0] == 4:
            scene['outfit_label'] = a[1]; src['outfit_label'] = 'cmd 12e 4 @%d' % i
        elif c == 0x12e and a[0] == 10:
            scene['accessory'] = a[1] // 100; src['accessory'] = 'cmd 12e 10 @%d (variant %d)' % (i, a[1] % 100)
        elif c == 0x5c:
            voice = a[0]
    scene['hair_label'] = hair_label(scene['heroine'], record_0d, random_state)
    src['hair_label'] = 'fashion_hair_param_%d[key %d, state %d]' % (scene['heroine'], record_0d, random_state)
    scene['time_mode'] = time_mode(month, hour, bg_kind); src['time_mode'] = 'clock month %d hour %d' % (month, hour)
    dur = voice_seconds(voice) if voice is not None else 0
    scene['viseme'] = 5 if voice_t >= dur else None
    src['viseme'] = 'voice %s %.2f s, t %.2f s' % (voice, dur, voice_t)
    return scene, src


if __name__ == '__main__':
    import scene_gen
    # recorded moment: event 08143402, the line after `5c 30156` (stream position 26); Nene's record +0xd = 1, no random event;
    # clock 2026-10-12 15h; motion started 3 updates before the captured list; voice started ~400 frames earlier
    sav = os.path.join(ROOT, 'tools/emu_scripts/states/logs/backup-session-20261013-110515-f1445.sav')
    key = record_0d_from_save(sav, 2)
    print('hair key from the save file (Nene, record +0xd):', key)
    scene, src = resolve(0x08143402, 26, record_0d=key, random_state=0, month=10, hour=15, updates=96, voice_t=(6380 - 5981) / 60)
    for k in sorted(scene):
        ok = scene[k] == scene_gen.SCENE.get(k) or (k == 'updates' and scene[k] % 31 == scene_gen.SCENE[k])
        print('%-13s %-8s %-4s %s' % (k, scene[k], 'ok' if ok else 'DIFF(%s)' % scene_gen.SCENE.get(k), src.get(k, 'playback / default')))
    assert all(scene[k] == scene_gen.SCENE[k] or (k == 'updates' and scene[k] % 31 == scene_gen.SCENE[k]) for k in scene_gen.SCENE), 'mismatch'
    print('queued (not current):', scene.get('queued'))
    print('resolved scene == scene_gen.SCENE')
