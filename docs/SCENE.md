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

Still open on the player side (script side is closed): backdrops (no bg command in the script; the place comes from the event session / camera preset family 1000 / 2000 / 3000 and the BG archives, whose NCGR/NSCR sets do not decode to pictures with `nscr_image` yet),
costume and hair choice per scene, time-of-day toon state, motion `blend`/`hold` semantics, per-scene string variables from the save.
