#!/usr/bin/env python3
"""Build the script command reference (fid/script_command_names.csv, docs/SCRIPT_COMMANDS.md).

Inputs (all produced by earlier tools): fid/script_command_spec2.csv (effect strings recovered by running the real
handlers in Unicorn), fid/script_callsites.jsonl (literal arguments at every call site).  MANUAL holds the
hand-verified names/semantics; every other used command gets an automatic name from its first native call and is
marked confidence 'auto'.

Confidence: V = verified (emulator / content cross-check), H = high (handler + argument statistics agree),
M = medium (handler effect clear, argument meaning inferred), L = low, auto = mechanical label only.
"""
import collections
import csv
import json
import os
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CH = 0x80000000  # character reference: 0x80000000 | character id; 0x80000000 itself = the current heroine/default actor

# cmd: (name, category, args, description, confidence)
MANUAL = {
    0x1: ('yield', 'flow', '', 'Suspend the script until no task of the current kind is running (the script "yield"; 14.7% of all calls).', 'V'),
    0x44: ('wait', 'flow', 'ticks', 'Allocate a countdown task; the script resumes after `ticks` updates (values 1,2,5,8,10,15,20,30 dominate).', 'H'),
    0x3: ('wait_sound', 'flow', '[char]', 'Task kind 8: tick sub-state with sound; waits for the voice/sound sub-state to finish.', 'M'),
    0x4c: ('is_proc_active', 'flow', '', 'Query: result in acc is whether the 0x204fb44 process (character state machine) is still running.', 'L'),
    0xd: ('fade_out', 'screen', 'frames [, screen]', 'Fade to black over `frames` updates (20/15/30); optional second arg selects screen(s) (0 both, 1, -1).', 'H'),
    0xe: ('fade_in', 'screen', '[frames [, screen]]', 'Fade from black (to clear) on both screens; default 10 frames.', 'H'),
    0x2: ('set_actor', 'actor', 'char', 'Load/refresh the 3D heroine for character ref `char` (0x80000000 = current heroine) and redraw the date UI separator; opens a task (kind 7).', 'M'),
    0x16: ('get_char_slot', 'actor', 'char', 'Query: heroine slot pointer for the character ref (result in acc).', 'M'),
    0x17: ('clear_char_record', 'actor', 'char', 'Clear the per-heroine state record of the character ref.', 'M'),
    0x18: ('get_char_slot_b', 'actor', 'char', 'Query: heroine slot pointer variant.', 'L'),
    0xf: ('motion_blend', 'actor', 'motion [, blend [, hold]]', 'Switch the heroine to motion index `motion` (0-973, see models/motion_table.json) with a transition: task kind 0; blend/hold in updates (a1 15/20/0/25/30, a2 30/20/40/45).', 'H'),
    0x5d: ('motion_queue', 'actor', 'motion, timeout, blend', 'Queue motion `motion` (pending action 0x74): it starts when the current motion finishes; `timeout` = number of updates the current motion may keep playing before the controller forces it to its end (MapCtl +0x88/+0x8c; 3000/9999/999999 = effectively let it finish); `blend` = blend frames of the queued transition (20/15/30).', 'M'),
    0x34: ('face', 'actor', 'mood [, delay]', 'Set the face mood id 0..44 (engine/face_tables.json): applied at once through Model_SetMotionIdAndInitFlags when no transition runs and delay<=0, otherwise stored as the pending mood (MapCtl +4) with a delay in updates (+0x94).', 'V'),
    0x136: ('face_after_transition', 'actor', 'mood', 'Queue the face mood to apply when the current motion transition finishes (MapCtl +8; on completion the controller copies +8 into the pending mood +4 and clears +8). Typical order: f motion, 34 mood, 136 follow-up mood.', 'M'),
    0x5c: ('voice', 'sound', 'clip [, volume [, frames]]', 'Request voice clip index `clip` (Siren14Voice.vtb, 0..40554; -1 = none); a1 = channel/level (7 in 94%), a2 optional fade.', 'H'),
    0xfb: ('voice_slot', 'sound', 'channel, id, ...', 'Request voice clip for a slot (a0 = 7, id in a1: 420-500 = fixed system lines).', 'M'),
    0x68: ('bgm', 'sound', 'bgm_id [, fade]', 'Play BGM sequence `bgm_id` (fade in 100-300).', 'H'),
    0x69: ('se', 'sound', 'player, se_id [, volume [, frames]]', 'Request a sound effect/sequence on player a0 (2 = SE bank in 74%).', 'M'),
    0x6a: ('ambient', 'sound', 'player, seq_id, volume', 'Start an ambient sequence loop (a2 = volume, 90 in 87%).', 'M'),
    0x6c: ('stop_seq', 'sound', 'fade_frames', 'Stop the current sequence with a fade (120, 10, 20, ...).', 'H'),
    0x6d: ('stop_seq_player1', 'sound', 'player, id [, frames]', 'Stop a sequence on player 1 channel.', 'M'),
    0x6e: ('stop_ambient', 'sound', 'fade_frames', 'Fade out/stop the ambient channel.', 'H'),
    0x70: ('fade_bgm', 'sound', 'volume', 'Start a volume fade of the BGM channel.', 'M'),
    0x71: ('fade_channel1', 'sound', 'player, id, volume, frames', 'Start a volume fade on channel 1.', 'M'),
    0x72: ('fade_ambient', 'sound', 'volume [, frames]', 'Start a volume fade on the ambient channel.', 'M'),
    0x12f: ('scene_audio_lock', 'sound', 'mode 0..3', 'Lock/unlock scene audio and (re)play scene audio.', 'M'),
    0x6: ('camera_preset', 'camera', 'preset', 'Move the camera to preset id (1000/2000/3000 families, camera/presets.json); task kind 0xb.', 'H'),
    0x7: ('camera_preset_t', 'camera', 'preset, frames', 'Camera preset with transition time (a1 120/30/1).', 'M'),
    0xb: ('camera_preset_nosnap', 'camera', 'preset [, frames]', 'Camera preset without snapping.', 'M'),
    0x130: ('camera_mode2_a', 'camera', '', 'Camera mode 2 begin (variant 0).', 'L'),
    0x131: ('camera_mode2_b', 'camera', '', 'Camera mode 2 begin (variant 1).', 'L'),
    0x1df: ('camera_for_area', 'camera', 'preset', 'Camera preset for the current area.', 'L'),
    0x55: ('set_flag', 'state', 'flag, value', 'Set scoped game flag `flag` (ids 3719-16068) to 0/1 (value 1 in 94%).', 'H'),
    0x59: ('test_flag', 'state', 'flag', 'Query: scoped game flag (acc = 0/1).', 'H'),
    0x107: ('add_counter', 'state', 'id, amount', 'Add `amount` to the clamped scoped byte counter `id` (4097+).', 'M'),
    0xa7: ('rand', 'state', 'n', 'acc = Rand_Next(n) (n 2..100).', 'H'),
    0x42: ('get_season', 'clock', '', 'Query: season of the current month.', 'H'),
    0x147: ('get_time_period', 'clock', '', 'Query: time period from the clock hour.', 'H'),
    0x146: ('get_time_period_b', 'clock', '', 'Query: time period from the clock hour (variant).', 'H'),
    0xc1: ('get_time_period_c', 'clock', '', 'Query: time period from the clock hour (variant).', 'H'),
    0xc2: ('get_time_period_d', 'clock', '', 'Query: time period from the clock hour (variant).', 'H'),
    0x151: ('get_time_period_e', 'clock', '', 'Query: time period from the clock hour (variant).', 'H'),
    0x203: ('get_effective_day', 'clock', '', 'Query: calendar effective day/month (acc/out vars).', 'M'),
    0x1a7: ('get_effective_day_a', 'clock', 'char', 'Query: calendar effective day for a character.', 'L'),
    0x19d: ('get_effective_day_b', 'clock', 'char', 'Query: calendar effective day.', 'L'),
    0x19c: ('get_effective_day_c', 'clock', '[char]', 'Query: calendar effective day.', 'L'),
    0x12e: ('flow_dispatch', 'flow', 'kind, arg, arg2', 'Dispatch a game-flow command (task kind 0x147): a0 kind 0..18 (4, 6, 0, 9, 18 common), a1/a2 payload (scene or event ids).', 'M'),
    0x12: ('change_scene', 'scene', 'scene_id', 'Apply a map/scene change to `scene_id` (121 distinct values up to 13572); task kind 0x21.', 'M'),
    0x1d: ('char_stage_set', 'actor', 'char, n', 'Set heroine record flag/stage `n` (1..10) for char (SaveSlot_CommitRecordFlags).', 'M'),
    0x1e: ('char_state_update', 'actor', 'char, n', 'Update heroine record state `n` for char (SaveSlot_UpdateRecordState).', 'M'),
    0x27: ('char_add_stat_a', 'actor', 'char, amount', 'Add clamped stat 0x180 for char.', 'M'),
    0x29: ('char_add_stat_b', 'actor', 'char, amount', 'Add clamped stat 0x181 for char.', 'M'),
    0xca: ('decode_char_slot', 'actor', 'char_id', 'Decode the heroine slot from the high bits of a script id and set the map-arc slot (a0 = full event id 0x08xxxxxx etc.).', 'M'),
    0x1d1: ('stage_bgm', 'sound', 'char', 'Play the heroine-stage BGM for char.', 'M'),
    0xc5: ('get_char_bgm', 'sound', 'char', 'Query: current map-arc BGM id.', 'L'),
    0x13d: ('char_bgm_record', 'sound', 'char', 'Read BGM id and set the primary record flag.', 'L'),
    0x1bb: ('skinship_init', 'skinship', 'mode 0..12', 'Initialise a skinship (touch) session in overlay 2.', 'M'),
    0x1bc: ('skinship_ready', 'skinship', '', 'Query: skinship session ready.', 'M'),
    0x18e: ('skinship_reaction_line', 'skinship', 'type 13..19', 'Select the reaction voice line for touch type.', 'M'),
    0xe7: ('skinship_touch_record', 'skinship', '[...]', 'Read the current touch record (skinship state array).', 'L'),
    0x198: ('skinship_finish_gfx', 'skinship', 'flag', 'Finish skinship graphics setup.', 'L'),
    0x1dd: ('overnight_cell', 'overnight', '0/1', 'Overlay 41 (overnight scene): update graphics cell.', 'M'),
    0x1de: ('overnight_effect', 'overnight', 'effect 0..27', 'Overlay 41: update the animated effect.', 'M'),
    0x1dc: ('overnight_sprite', 'overnight', 'effect 0..12', 'Overlay 41: update the sprite effect.', 'M'),
    0x1c3: ('overnight_cb_b', 'overnight', 'n 0..6', 'Overlay 41: callback B (no-op when loaded).', 'L'),
    0x1c4: ('overnight_aux', 'overnight', 'n 0..4', 'Overlay 41: update the auxiliary effect.', 'M'),
    0x115: ('overnight_state', 'overnight', '', 'Overlay 41: set overnight state value.', 'L'),
    0x116: ('overnight_step', 'overnight', '', 'Overlay 41: advance the overnight sequence step.', 'L'),
    0x155: ('get_camera_preset', 'camera', '', 'Query: current camera preset id (when overlay 41 is not loaded).', 'L'),
    0xf5: ('bg_draw_primary', 'screen', '[frames]', 'Draw the primary BG layer with fade (task kind 0x19).', 'M'),
    0xf6: ('bg_update_primary', 'screen', '[frames]', 'Update the primary BG layer (kind 0x1a).', 'M'),
    0xf7: ('bg_draw_secondary', 'screen', '[v]', 'Draw the secondary BG layer (kind 0x1b).', 'M'),
    0xf8: ('bg_update_secondary', 'screen', '[v]', 'Update the secondary BG layer (kind 0x1c).', 'M'),
    0x123: ('load_event_assets', 'scene', 'res, n', 'Load primary event assets (kind 0x15a).', 'M'),
    0x14f: ('set_stream_mode', 'sound', 'mode', 'Set stream mode (kind 0x8b).', 'L'),
    0x124: ('vram_reset', 'screen', '', 'Reset VRAM bank override and date-UI header.', 'L'),
    0x1ec: ('set_flag_range', 'state', '', 'Set a flag bit range.', 'L'),
    0xa6: ('query_state_78', 'state', 'char', 'Query game-state field 0x78 / 0x110.', 'L'),
    0x120: ('flow_update', 'flow', '', 'GameFlow_UpdateState(1).', 'L'),
    0x1ba: ('get_data_ptr', 'state', '', 'Query: data table pointer (0x21466a8).', 'L'),
    0x1b9: ('meter_add_table', 'state', 'idx 0..14', 'Add a table-indexed slot meter value.', 'L'),
    0xc4: ('stop_seq_cb', 'sound', 'frames', 'Stop sequence on player 1 + callback (kind 0x96).', 'L'),
    0x1a8: ('event_meter_add_303', 'state', 'idx', 'Add event meter 0x303 by table entry.', 'M'),
    0x1a9: ('event_meter_add_304', 'state', 'idx', 'Add event meter 0x304 by table entry.', 'M'),
    0x1d4: ('get_camera_area', 'camera', '', 'Query: area of the current camera preset.', 'L'),
    0x20f: ('set_field_198', 'state', '0/1', 'Set game-flags field 0x198.', 'L'),
    0xc3: ('get_flags_4', 'state', '', 'Query: game-flags field 4 (task kind 0x95).', 'L'),
    0x62: ('save_transition', 'flow', 'n 2..18', 'GameFlow_HandleSaveTransition.', 'L'),
    0x138: ('clear_flag_fields', 'state', 'n 0..6', 'Clear game-flags fields 0x90/0x9c/0xa0.', 'L'),
    # Dream-scene overlay (ov37) commands: one shared stub (0x205d4c4 -> 0x2170ab0) switches on the command id (ctx+0x10c0);
    # ctx+0x10bd = 0x87 keeps the script suspended (re-called every update), 0 releases it; ctx+0x10c4 = acc (result).
    0x51: ('dream_step', 'dream', '', 'Dream scene: advance the scene step counter (state +0xc) and wait while the step animation runs (steps 3,4,7,12,16,17,20 release immediately).', 'M'),
    0x52: ('dream_sprite_anim', 'dream', 'anim', 'Dream scene: set the animation of the primary dream sprite (state +0x68).', 'M'),
    0x53: ('dream_record_anim', 'dream', 'anim', 'Dream scene: set the animation of the second dream sprite (state +0x6c) and reset its speed to 0.', 'M'),
    0x74: ('dream_wait_steps', 'dream', '', 'Dream scene: wait while the step counter is <= 6.', 'M'),
    0x75: ('dream_wait_input', 'dream', '', 'Dream scene: wait for touch/pad input according to the scene mode (modes 5, 0x15, 0x18); sets acc to the pressed choice.', 'L'),
    0x76: ('dream_fade_reset', 'dream', '', 'Dream scene: play sound 0xf0, fade both screens to black over 180 updates, reload the BG palette and fade back in.', 'M'),
    0x77: ('dream_set_state4', 'dream', '', 'Dream scene: set the step counter to 4.', 'M'),
    0x7c: ('dream_effect', 'dream', 'effect 0..14 [, frames]', 'Dream scene staging effect: 0/1 show layer flag 1, 2 flag 2, 3 set value 0x40, 4 fade to black, 5 fade to white, 6/7 black/white flash then reveal, 8 init scene (waits ~15 updates), 9 start sequence, 10-12 set up sequence, 13/14 show overlay flags; second arg = fade frames (default 60).', 'M'),
    0x7d: ('dream_wait_timer', 'dream', 'ticks', 'Dream scene: wait until the scene timer (state +0x1c) reaches `ticks`.', 'M'),
    0xe8: ('dream_wait_tap_rect', 'dream', 'x, y, w, h, snap', 'Dream scene: wait for a touch inside the rectangle (x, y, w, h in pixels); on hit acc = 1 and, if snap != 0, the cursor jumps to the centre; the hit-test uses the cursor position in 1/16 pixel.', 'M'),
    0xe9: ('dream_background', 'dream', 'step 0..4', 'Dream scene: run a dream background sequence step (0/1 animation 1/2, 2/3 draw background 1/2, 4 draw background 0).', 'M'),
    0xea: ('dream_sprite_row', 'dream', 'frame', 'Dream scene: set the row sprite frame (state +0x70); -1 hides the sprite.', 'M'),
    0xf0: ('dream_counter', 'dream', '', 'Dream scene: advance the scene counter; acc = new value.', 'L'),
    0x193: ('dream_flag', 'dream', '', 'Dream scene: query scene byte +0x4fc (acc).', 'L'),
    0x194: ('dream_nop', 'dream', '', 'No-op in the dream overlay.', 'M'),
    0x195: ('dream_input_state', 'dream', '', 'Dream scene: query the dream input state.', 'L'),
    0x197: ('dream_input_state_b', 'dream', '', 'Dream scene: query the dream input state (variant).', 'L'),
    # long-tail commands named from their recorded native effects (confidence L unless noted)
    0x137: ('rank_check', 'state', 'stat 0..6', 'Query: Stats_CheckRankCondition for the player stat (task kind 0xdc) -> acc.', 'L'),
    0x158: ('slot_workflow', 'actor', 'char, action 0..3', 'SlotWorkflow_DispatchActions for the heroine (task kind 0x148).', 'L'),
    0x1cd: ('set_flags_18c', 'state', 'n 0..7', 'GameFlags field 0x18c = n.', 'L'),
    0x18c: ('get_manual_date', 'clock', '', 'Read the manually set clock date (year, month, day) and weekday into script variables / flags.', 'L'),
    0x1f5: ('get_manual_date_b', 'clock', 'part 1..3', 'Read the manual clock date part (year/month/day).', 'L'),
    0xcd: ('get_flags_4', 'state', '', 'Query: GameFlags field 4 (current entry value).', 'L'),
    0x12d: ('get_flags_4_fade', 'state', '', 'Query GameFlags field 4 and update the fade state.', 'L'),
    0x21c: ('get_flags_4_b', 'state', '', 'Query: GameFlags field 4 (variant).', 'L'),
    0x14e: ('slot_action', 'actor', 'n 1..9', 'Current-slot action n (task kind 0xe5).', 'L'),
    0x15a: ('char_state_psi', 'actor', 'char', 'Select/refresh heroine state data for char (StateLogic_SelectDataPsi, hook 4 mode 2).', 'L'),
    0xcb: ('skinship_input_mode', 'skinship', 'mode [, v]', 'Skinship overlay: set the input mode and wait on the overlay predicate (task kind 0x100).', 'L'),
    0xcc: ('skinship_global', 'skinship', '', 'Query the skinship global state.', 'L'),
    0x1fb: ('get_current_slot', 'actor', 'n 1..5', 'Query: game-state current slot (acc).', 'L'),
    0x13b: ('scene_save_fields', 'actor', 'char, n', 'Update scene/save state fields for char (task kind 0xb0).', 'L'),
    0x1ae: ('projection_callback', 'camera', 'n 0..2', 'Set the 3D projection callback.', 'L'),
    0x182: ('char_record_a', 'actor', 'char, n 0..4', 'Heroine event-record operation (task kind 0x6c): reset record n and update the record state.', 'L'),
    0x188: ('char_record_msgbox', 'actor', 'char, n 0..4', 'Heroine event-record operation with message-box reset (task kind 0x6d); waits for the box (TaskList_FreeWhenMsgBoxClosed).', 'L'),
    0x189: ('char_record_flags', 'actor', 'char, n', 'Heroine record flags operation (task kind 0x6e).', 'L'),
    0x184: ('char_record_b', 'actor', 'char, n', 'Heroine record operation (task kind 0xb9).', 'L'),
    0x191: ('char_sync_slot', 'actor', 'char, n', 'Sync the current slot to the heroine (task kind 0xac).', 'L'),
    0xda: ('get_slot_ptr', 'actor', 'char', 'Query: heroine slot pointer.', 'L'),
    0x201: ('char_meta', 'actor', 'char', 'Update heroine state metadata and build the selected-entry record.', 'L'),
    0x9e: ('get_current_slot_b', 'actor', '', 'Query: game-state current slot.', 'L'),
    0xb6: ('get_current_slot_c', 'actor', '', 'Query: game-state current slot.', 'L'),
    0xdc: ('get_current_slot_d', 'actor', '', 'Query: game-state current slot.', 'L'),
    0x78: ('get_flow_state', 'flow', '', 'Query: GameFlow state field 4 (the text-route kind).', 'M'),
    0x3c: ('test_schedule_state', 'state', '', 'Query: schedule/special-day state tests (low-five flags, secondary state 2, primary state).', 'L'),
    0x60: ('schedule_update_hour', 'state', '', 'Refresh the schedule for the current month and hour.', 'L'),
    0x8c: ('fade_transition_b', 'screen', '', 'Run the event-screen fade transition B.', 'L'),
    0x90: ('slot_cache_update', 'actor', 'slot, a, b', 'Update the slot cache state and calendar (task kind 0xb6).', 'L'),
    0x20e: ('ov38_cmd_20e', 'menu', 'n 0..14', 'Overlay 38 (Love+ menu) command: calls the overlay extension function when loaded.', 'L'),
    0x1db: ('ov38_cmd_1db', 'menu', '', 'Overlay 38 command.', 'L'),
    0x1fa: ('ov38_cmd_1fa', 'menu', '', 'Overlay 38 command.', 'L'),
    0x20d: ('ov38_cmd_20d', 'menu', '', 'Overlay 38 command.', 'L'),
    0x219: ('ov38_cmd_219', 'menu', '', 'Overlay 38 command.', 'L'),
    0x222: ('schedule_period', 'state', 'n 1..4', 'Schedule_SelectTimePeriod.', 'M'),
    0x225: ('consume_timer4', 'state', '0/1', 'Consume mask-4 schedule timer.', 'L'),
    0x223: ('add_timer', 'state', 'amount', 'Add to the clamped schedule timer.', 'M'),
}

CATEGORY_BY_PREFIX = (('Gfx', 'screen'), ('Camera', 'camera'), ('GameSnd', 'sound'), ('GameFlags', 'state'),
                      ('Schedule', 'state'), ('SaveSlot', 'actor'), ('GameClock', 'clock'), ('Calendar', 'clock'),
                      ('Ov02Skinship', 'skinship'), ('Ov41Overnight', 'overnight'), ('TASK', 'task'),
                      ('MapCtl', 'actor'), ('ScreenFade', 'screen'), ('Bg_', 'screen'), ('MsgBox', 'text'),
                      ('MsgScript', 'text'), ('GameState', 'state'), ('Stats', 'state'), ('EventLogic', 'state'))


def auto_label(effect):
    parts = [p.strip() for p in effect.split(';') if p.strip()]
    for p in parts:
        n = p.split('(')[0]
        if n in ('str', 'charid') or n.startswith('TASK') or n.startswith('GATE'):
            continue
        return n
    return parts[0].split('(')[0] if parts else 'unknown'


def load():
    spec = {int(r['cmd'], 16): r for r in csv.DictReader(open(os.path.join(ROOT, 'fid/script_command_spec2.csv')))}
    stats = collections.defaultdict(lambda: collections.defaultdict(collections.Counter))
    argc = collections.defaultdict(collections.Counter)
    for line in open(os.path.join(ROOT, 'fid/script_callsites.jsonl')):
        x = json.loads(line)
        argc[x['cmd']][x['k']] += 1
        for i, v in enumerate(x['args'] or []):
            stats[x['cmd']][i][v] += 1
    return spec, stats, argc


def main():
    spec, stats, argc = load()
    rows = []
    for c, r in sorted(spec.items(), key=lambda kv: -int(kv[1]['uses'])):
        uses = int(r['uses'])
        if uses == 0 and c not in MANUAL:
            continue
        eff = r['effect']
        if c in MANUAL:
            name, cat, args, desc, conf = MANUAL[c]
        else:
            lab = auto_label(eff)
            name = 'cmd_%03x' % c
            cat = next((v for k, v in CATEGORY_BY_PREFIX if lab.startswith(k)), 'misc')
            args = ''
            desc = ('FAULT in emulation (handler not executable standalone)' if eff == 'FAULT'
                    else 'auto: ' + lab)
            conf = 'auto'
        top = {i: [(v, n) for v, n in h.most_common(3)] for i, h in sorted(stats[c].items())}
        rows.append((c, uses, name, cat, args, desc, conf, dict(argc[c].most_common(3)), top, eff))
    out = os.path.join(ROOT, 'fid/script_command_names.csv')
    with open(out, 'w', newline='') as f:
        w = csv.writer(f)
        w.writerow(['cmd', 'uses', 'name', 'category', 'args', 'description', 'confidence', 'effect'])
        for c, uses, name, cat, args, desc, conf, _, _, eff in rows:
            w.writerow(['0x%x' % c, uses, name, cat, args, desc, conf, eff[:300]])
    total = sum(r[1] for r in rows)
    cover = collections.Counter()
    for r in rows:
        cover[r[6]] += r[1]
    lines = ['# Script command reference (generated by tools/cmd_ref.py)', '',
             'Command ids are the `0x87` operand of the SBIN VM (see SCRIPT_VM.md). `char` = character ref `0x80000000 | id` '
             '(`0x80000000` alone = the current heroine/default actor). Confidence: V verified, H high, M medium, L low, auto = mechanical label.', '',
             'Coverage by confidence (share of the %d script calls): %s.' % (total, ', '.join(
                 '%s %.1f%%' % (k, 100 * v / total) for k, v in sorted(cover.items(), key=lambda kv: -kv[1]))), '']
    bycat = collections.defaultdict(list)
    for r in rows:
        bycat[r[3]].append(r)
    for cat in sorted(bycat, key=lambda k: -sum(r[1] for r in bycat[k])):
        lines += ['## %s' % cat, '', '| cmd | name | args | uses | conf | meaning |', '|---|---|---|---|---|---|']
        for c, uses, name, _, args, desc, conf, _, _, _ in bycat[cat]:
            if conf == 'auto' and uses < 20:
                continue
            lines.append('| 0x%x | %s | %s | %d | %s | %s |' % (c, name, args, uses, conf, desc.replace('|', '/')))
        n_rare = sum(1 for r in bycat[cat] if r[6] == 'auto' and r[1] < 20)
        if n_rare:
            lines.append('| ... | %d rarely used auto-labelled commands in this category (see fid/script_command_names.csv) | | | | |' % n_rare)
        lines.append('')
    open(os.path.join(ROOT, 'docs/SCRIPT_COMMANDS.md'), 'w').write('\n'.join(lines))
    print(len(rows), 'commands;', {k: round(100 * v / total, 1) for k, v in cover.items()})


if __name__ == '__main__':
    main()
