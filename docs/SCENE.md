# Scene player

`engine/src/{scene,player,font}.rs` + `bin/scene_player` run a script event as a scene: the heroine, camera, fades, the game's own dialogue font, voice, music and sound.

```
scene_player CONTENT EVENT_ID [--outfit N] [--hair N] [--look soft-s|clean-s|authentic|...] [--size WxH] [--ss N] [--choose N,N] [--fov F]
                              [--shot T1,T2,... --out PREFIX]   headless stills at scene times (seconds)
                              [--no-audio]
```
Window controls: Esc quit, Space pause, Left/Right seek 2 s, R restart.  Example: `scene_player content a44c1b00` (a hallway encounter with Manaka).

## Pipeline
script event -> `Host::run_event` (typed effects, script ticks) -> `audio::pace_with` (dialogue pacing: a line lasts its voice clip + 0.4 s or an estimated reading time, its voice starts with it)
-> `SceneState::apply` per effect (motion index -> stem via `engine/motion_table.json`, face mood, camera preset with easing, fades, dialogue)
-> `ScenePlayer::frame(t)`: heroine composed at the current motion frame (30 updates/s) with mood, blink (tables) and lip sync (`lipsync.dat` keys over the voice clip),
perspective camera from the preset's eye/target (vertical fov 30 by default) and its three light angles, supersampled enhanced look, dialogue box drawn with the NFTR font (typewriter 14 chars/s).
Renderer layers: gradient backdrop -> 3D scene (transparent background) -> UI overlay -> fade.  Audio effects fire from the same timeline (`AudioDirector`).

## Facts used / found
- The camera presets give eye and target in the game's world; the viewer's frame mirrors z (the model is turned half a turn to face the camera): eye = (x, y, -z).  Preset 1000 frames her bust-up, matching the game's own screenshots.
- Motions carry root movement: a walking motion brings the heroine in from the edge of the frame, as in the game.
- A heroine without a motion command yet stands in the first motion of her range (index 0 / 739 / 841); string variables `{vN}` (names, how she addresses the player) come from the save, so
  v0/v1 = "あなた", v26 = "あなた君", v3 = her family name, v8 = her given name are placeholders; other `{vN}` stay visible.
- Heroine selection: the slot comes from the event id's heroine byte (0xa0.. Manaka, 0x58.. Rinko, 0x08.. Nene); outfit 0 = winter uniform, hair 0.

## Not done
Backgrounds (the game's background images are not yet mapped to scenes; a dusk gradient stands in), the other characters/props of a scene, the game's real text UI layout (box position/size are approximations,
no AUTO button/advance icon), tap-to-advance (lines advance by pacing), choices UI, motion blending (`blend` frames) and hold, hair/skirt dynamics, effects (3D map effects, overlays), interactive scenes.

## Verification against the real game
`tools/play_monitor.py` (natural play, no forcing) logs every script command with its arguments, the opcode trace, script start/end with the event session and periodic screenshots.
`tools/scene_diff.py LOG [--full] [--only ID]` runs the Rust host for each script the game ran (at the game clock's month/hour) and compares:
default mode = motion / camera / face / voice / fade; `--full` = every command and argument (commands the monitor cannot see, i.e. without a native handler in `fid/script_commands.csv`, are ignored).
On the first three-day log, **all 12 scripts are identical command for command** (including 38-70 command Famires and school scenes).  Gaps found and closed this way: `clear_char_record` (0x17) returns the slot status
(`SaveSlot_ProcessStorageStatus`; 3 for a dating slot), `get_season` (0x42) follows `Calendar_MonthToSeason` (months 2/5/8/11), cmd 0x171 follows `TimePeriod_FromHour` (hours 5/8/12/15/18/21),
and motion indices 0-738 are a common set every heroine plays (Rinko scene 5c4c1b00 uses 292/608/535).  The camera has no pose-following mode (the 4 alternate eye/target blocks of a preset are D-pad manual views).

### Backdrops (decoded)
`engine/src/backdrop.rs` (reference `tools/bg_model.py`, extractor `tools/bg_select.py` -> `engine/bg_select.json`): event id -> `arc/tmx_bbin.arc` record -> background id -> archive by id range (`BgTransition_DispatchState`; ids >= 10001 are file numbers of archive id/10000)
-> `ScheduleText_UpdateState` file number from a 20-byte record + season index (spring 0 .. winter 3) + time of day (period 0-2 day, 3 evening, 4-5 night) -> position in `arc/bg_file_no_0N.bin` -> the (NCGR, NCLR, NSCR) triple.
`tools/bg_decode.py` / `nns2d::bg_picture` decode a picture (first 32x24 tiles minus the last 3 rows, rotated for the sideways console).  All 30 distinct backdrops of the recorded play are reproduced (`tests/scene_content.rs`).
Events without a record (e.g. `0f4c3400`, `a74e0200`: classroom/stage scenes) get their place from the event-selection stage; they fall back to the gradient until event selection is ported.
The same time of day picks the heroine's toon table (`state` 0 day / 1 evening / 2 night).

### Costume, hair, framing
Screenshots from the emulator were red/blue swapped until `emu_driver` was fixed (the "maroon uniform" was Rinko's navy one): outfit 0 is right for the school scenes.  Rinko's hair in the recorded play is id 200 (verified against frames; Manaka 0 / Nene 0 unverified).
Nene wears the Famires waitress uniform (outfit 10, hair 200 with the headband) in the restaurant scenes: `default_outfit(slot, backdrop archive)` encodes that (verified against frames); how the game decides the costume in general (save wardrobe + job state; `tm_model_label` entry 0x195 is opened only for those scenes) is not decoded.  The view angle is calibrated against recorded frames (about 15 degrees across the portrait width).
The camera records' 4 alternate eye/target blocks are D-pad manual views (no pose-following camera).

### Menus and end-of-scene hooks
4-option menus (text mode 4) take choices like mode 2; `scene_diff --search` finds the choices that reproduce a recorded scene (two scenes needed the heroine-name option).  Commands 0x147/0xc1/0xc2/0x146/0x151 call a game-state callback that returns a script id to run next (the event queue), so scenes chained that way continue in the game but stop in the host until event selection is ported.
Remaining voice-clip differences between host and game are the RNG rolls (`0xa7`).

Still open on the player side: place for events without a record (needs event selection), costume per scene (job uniforms etc.), motion `blend`/`hold` (the monitor now logs `Model_SetMotionIdAndInitFlags` calls to measure them), per-scene save strings (`--player NAME` sets the player's name).

### Motion blend (decoded)
`Gfx3D_UpdateModel_2` / `Gfx3D_InitializeModel`: a motion change with blend > 0 plays the old and the new animation together and weights them linearly (0 -> 0x1000 over `blend` updates; 30 updates per second); blend <= 0 (including the missing-argument -1) snaps.
The player does this per node (translation/scale lerp, rotation slerp).  The third argument (`hold`) is stored in the model (+0x30) and its use is not decoded; motion info byte 0 (1 = loops, 8 = one-shot) decides looping.

### Save strings
The player's name lives at game state +0x18 (`GameState_GetPtr_Ofs0x18`; read from RAM of a savestate): in the recorded save it is `一`, which is what the name tabs show.  The engine takes it with `--player NAME` (v0/v1; v26 = name + 君 is a guess).  Heroine family names come from the static table at 0x212426c.
### Framing
The view scale (`VIEW_SCALE`) is calibrated on one frame; against the Famires close-up (preset 3014) our heroine sits about 5-7% of the frame height too high, as in the library frame (about 4%), so a small constant vertical offset in the real projection is still unexplained.  Camera records also carry an angle word (u16 at word 7) that is not applied.

### Where host and game still differ (all recorded logs, `scene_diff --full --search`)
38 distinct scripts match command for command.  The rest are explained: menus the player answered differently (`--search` finds the choices; two scenes need option 2), RNG rolls in voice clips (`0xa7`), logs cut short by the player skipping, and scenes whose end-of-scene period callbacks (0x147/0xc1/0xc2/0x146/0x151) call a follow-up event the game scheduled earlier
(e.g. `80286400` -> `0x59e03900`, `a74e0200` -> `0xa2440200`, `0xa3fc3400`: the heroine-offset + stage-number family); those follow-ups come from event selection / the schedule and are not modelled.
