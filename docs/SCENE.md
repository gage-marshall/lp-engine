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
