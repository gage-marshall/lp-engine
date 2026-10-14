# Engine port plan: verified scene drivers → `engine/`

Status: **implemented** (2026-10-13, approved by the user). Section 7 lists what was built, how each gate was met, and the deviations.

The sandbox (`sandbox/gx`, `sandbox/audio_mix`) now reproduces a whole heroine scene (Nene, break room, event `08143402`)
from game data, checked update by update against the recording (docs/RENDER_PRIMITIVES.md, docs/AUDIO.md):

| Driver | Parity |
|---|---|
| script pacing (every command in its exact update) | 70 / 70 |
| typewriter (start / end of every line) | 19 / 19, 17 / 17 (2 tap-completed) |
| screen fade brightness | 1,177 / 1,177 |
| motion clip, frame, blend ratio, mood | 1,173 / 1,177 |
| eye texture + mode | 1,135 / 1,177 |
| hand poses | 1,148 / 1,177 |
| lip-sync mouth calls (line 30156) | 42 / 54 in order |
| box open/close, voice start, voice volume, eye start state | traced to code |

The engine got here first, by estimation. Its architecture (formats → host → effects → presentation) is sound,
but its **time model is wrong at the root**, and most presentation rules are placeholders. This plan replaces the
estimates with the verified drivers and keeps the layers clean.

---

## 1. What is wrong in the engine today

### 1.1 Time (the root problem)
* `host.rs` runs the whole event offline and stamps effects with **estimated ticks** (`TICK_SECONDS = 3.8/60`,
  `voice_end`, `fade_end`), then `audio::pace_with` **re-times** them with reading-speed heuristics
  (`0.5 + 0.07·chars`, `voice + 0.4 s`). The game does not work like this: pacing comes from per-update tasks,
  the player's taps, the typewriter, and the message-box and voice state machines.
* `scene.rs` assumes **30 updates per second** (`frame = t·30`, blend lengths `/30`). The game runs one logic
  update per Capture3D cycle: 4 frames at 59.8261 Hz (phases 0, 1, 2, 2), so about 15 Hz.
* The typewriter reveals **14 characters per second**. The game uses the interval table 0x21274e0 indexed by the
  save's text speed (speed 1 = 8 frames/char), with a per-frame accumulator and a VBlank divisor.
* Fades are **linear in seconds**. The game uses ScreenFade: 16.16, three passes per update, a delay counter,
  a truncated step (n = 20 takes 21 steps), and master brightness = cur/0x1000 rounded toward zero.

### 1.2 Heroine state
* **Motion:** time-based. It has no hold (+0x90) and no pose-ready gating. The queue semantics of `0x5d` are
  wrong: it replaces at clip end and loops. The game's group types and segment counts (0x2102e08) are missing,
  and the blend is linear rather than `Motion_UpdateBlendWeights` + `NNSi_G3dAnmBlendJnt`. The idle key is
  "the first record of the heroine" instead of `idle_key(preset)` (535 / 0x28e via
  `Motion_IsIdInSpecialRangeSet`). The per-motion default mood is not the game's pending-mood rule.
* **Blink:** a periodic `interval/hold` table cycle. The game runs the eye machine (blink 0x2094aa4, transition
  0x2094cdc, request states from the motion's `eye` track, type-2 reload on every mood set, texture
  residency), and it draws `Rand_Next` under mood 0x1a.
* **Lip-sync:** the key is chosen by `t/duration`. The game uses a stream-peak threshold (1000) over decoded
  640-sample blocks, elapsed counted in VBlanks while speaking, rest pattern 0x21046f4, and a 20-update hold.
  The voice starts 4 updates after `0x5c`.
* **Hands:** pose from the visibility track (roughly right), but the async3d base per heroine
  (0x2102e50/52) and the first-active-node rule need checking against `motion_sim.actor_state`.

### 1.3 Scene inputs
* `default_outfit` / `default_hair` return 0. The game takes the outfit label from script `12e 4`, the motion
  class from `tm_model_label`, and the hair from `fashion_hair_param_<slot>` (record `+0xd`, random-event state).
* The clock is `Clock::default()` (April, 12 h) or a CLI override. It should be the game clock (month and
  schedule hour; the manual hour while 0x2142c6c is set).
* **Backdrop time of day:** `tod_of_period` uses a `period_tod` table. The game uses `ScheduleText_UpdateState`
  (period path when GameState +0x78 is 0, season table otherwise, flags 0x104/0x105/0xe0).
* **Toon time mode** is not separated from the backdrop's time of day. They are different rules: 15 h is day for
  shading and evening for the backdrop.
* There is no save model. Text speed (block 0 0x41), sound levels (block 0 0x29–0x2b), calendar state
  GameState +0x78 (block 1 0x14) and heroine records (block 1 0x1989 + 0x5b3·slot) are all needed.

### 1.4 2D / backdrop
* `nns2d::bg_picture` reads tiles through the NSCR and crops to 168 rows to hide garbage rows. The BG picture is
  linear (`tile = ty·48 + tx`) and the NSCR supplies only the palette; indices ≥ 1024 are stored as 0. This is
  the source of the noise behind the heroine's head.

### 1.5 Audio (docs/AUDIO.md "Engine differences")
* The slot volume `105·level/15` is missing.
* Voice is mixed at a flat 0.7 per side instead of `2·(127/128)·pan`.
* The fader is linear in gain instead of `(v/127)²`.
* There is no voice level curve (`trunc(500·(level+15)/30)` → cB, with ≤ −640 → −1024).
* There is no stream-start pipeline: the voice should start at `0x5c` + 4 updates.

### 1.6 Kept as-is (verified or out of scope)
* Kept: formats (`nns3d`, `nns2d` except `bg_picture`, `sdat`, `siren`, `script`, `machine`, `font`), the
  command table, `game::Rng` (already the game's MATH_Rand64), camera presets and the decoded projection, and
  the wgpu toon/edge renderer. Our higher render quality is a feature; we keep it.
* **Not verified, so left alone and flagged:** camera easing for `0xb`/`0x7` (only `cmd 6` snap is verified),
  the dialogue window graphics, and other heroines and presets.

---

## 2. Target layering

```
content/engine/*.json   ← tools/export_sim_tables.py (new): code/data tables from the ARM9/overlays, extracted once
        │
formats (existing)       nns2d nns3d sdat siren script machine font bytes
        │
game state               game.rs (flags, counters, Rng, clock)      save.rs (new: block decode + packed layouts A/B)
        │
scene inputs (new)       scene_inputs.rs: hair, outfit class, toon time mode, backdrop tod, random-event state,
                         text speed, sound levels: pure functions of (event, save, clock)
        │
simulation (new, pure)   sim/ : one call = one game update; no wall clock, no IO, no GPU
        │                  clock.rs     Capture3D cycle: phases, update boundaries, VBlank divisor
        │                  sched.rs     drives `Machine` per update: tasks (countdown, fade-wait, load), yield,
        │                               text commit; owns Host command decoding
        │                  fade.rs      ScreenFade (16.16, delay, truncated step, brightness)
        │                  msgbox.rs    dialog Transform states 3→4→5→0→6→7→10→16→17, slide jobs (8 VBlanks),
        │                               typewriter, tap consumption
        │                  voice.rs     request → load → play pipeline (+4), level → cB
        │                  motion.rs    MotionSim (MapCtl hold/queue/idle key, group types, blend weights, advance)
        │                  eyes.rs      EyeSim (blink, transitions, requests, residency, Rand_Next under 0x1a)
        │                  hands.rs     visibility tracks → async3d hand models
        │                  lipsync.rs   peak-threshold mouth driver (PCM peaks supplied by the audio layer)
        │                  scene.rs     SceneSim: the traced per-update order
        │                               (script → MapCtl → eye step → UpdateActorState requests → frame advance)
        │                               → `SceneFrame` snapshot + `AudioEvent`s
        │
presentation             heroine.rs   compose from SceneFrame (per-part clips, frames, blend ratios, eye texture
                                      id, mouth variant, hands); no time or blink logic left
                         render.rs    unchanged pipeline; fade = master brightness level 0..16
                         backdrop.rs  picture choice (tod from scene_inputs) + nns2d linear-tile fix
                         audio.rs     driver fixes; consumes AudioEvents stamped with game frames
        │
front ends               player.rs + bin/scene_player.rs: fixed-step tick accumulator (59.8261 Hz) drives the sim;
                         the display runs at its own rate, interpolating continuous state between the last two
                         SceneFrames (§5.3); touch input is latched per tick. Headless: bin/scene_replay (session taps → frames/WAV)
```

Rules for the boundaries:
* **`sim/` is deterministic and pure.** Its inputs are scene inputs, game data tables, the frame clock, and taps
  per frame. Its outputs are `SceneFrame`s and `AudioEvent`s. It has no `f64` seconds, no file IO and no
  rendering.
* **Tables come from `content/engine/*.json`** written by the extractor, never hard-coded numbers in Rust.
  Each constant keeps its address in a comment, as the sandbox does.
* **`host.rs` keeps command decoding** (args → typed `Effect`). The time bookkeeping (`t`, `voice_end`,
  `fade_end`, `TICK_SECONDS`) moves to `sim/sched.rs`. The offline `run_event` remains for tools
  (`host_survey`, `script_trace`) as an untimed transcript.
* **Presentation never decides game state.** It maps a `SceneFrame` to pixels and samples.

---

## 3. Phases

Each phase is one reviewable change set with its parity test. It merges only after your review.

| # | Change | Replaces | Gate (test) |
|---|---|---|---|
| 0 | `tools/export_sim_tables.py` → `content/engine/sim_tables.json`. Parity fixtures from the recording → `engine/tests/data/s08143402/*.json` (numbers only: per-update expectations, tap release frames, phase clock) | n/a | fixtures regenerate identically; no engine behaviour change |
| 1 | `save.rs` (LZ10 blocks, packed layouts A/B from the descriptor tables) and `scene_inputs.rs` | `default_outfit/hair`, `Clock::default` use, `tod_of_period`, CLI-only month/hour | hair 0x3d, outfit class 1, time mode 0, backdrop evening, text speed 1, levels 15/15/15, calendar 0, random state 0, all from the save of session 110915 |
| 2 | `sim/clock`, `sched`, `fade`, `msgbox`, `voice` | `pace_with`, `TICK_SECONDS`, host time accounting, `revealed()` 14 cps, `fade_at` | 70/70 command updates, 19/19 typing starts, 17/17 ends, 1,177/1,177 brightness |
| 3 | `sim/motion`, `eyes`, `hands`, `lipsync`, `scene` | `SceneState` motion/blink/viseme logic, `FaceTables::blink_closed`, time-based `LipSync` | motion 1,173, eyes 1,135, hands 1,148 /1,177; lip-sync 42/54 |
| 4 | Presentation from `SceneFrame`: exact blend (`AnimChain` = `NNSi_G3dAnmBlendJnt`), face textures from eye/mouth state, brightness fade, `nns2d` linear tiles | `compose_opts` time math, `bg_picture` crop | render of `line26`/`line30` against the sandbox frames (same pose and expression; pixels may differ, since render quality is ours) |
| 5 | Audio: slot volume, voice gain/pan, `(v/127)²` fader, level curve + −1024 clamp, frame-stamped events, peak feed to `lipsync` | flat 0.7 voice, linear fader | soundtrack event times identical to `scene_video.py audio`; mix within rounding of `audio_mix` |
| 6 | Front end: fixed-step tick loop (59.8261 Hz) decoupled from the display rate; render-side interpolation between `SceneFrame`s (continuous quantities only, switchable off); touch, `bin/scene_replay`; delete dead code (`pace`, `pace_with`, `TICK_SECONDS`, `blink_closed`, `gradient_backdrop` fallback when a picture exists) | `ScenePlayer::frame(t)` seconds API | the replayed session renders the same video as `scene_video.py video` with interpolation off (same SceneFrames); with it on, tick-aligned frames are unchanged |

Phase 2 is the largest. Phases 2 and 3 can be reviewed independently because `SceneSim` exposes both halves.

---

## 4. Testing

* **Parity tests** live in `engine/tests/scene_parity.rs` and run against `content/` when present (skipped
  otherwise, like the existing content tests). The fixtures hold only derived numbers (update indices, states,
  levels); there are no ROM or RAM bytes.
* **Unit tests per sim module** use the small cases already established: ScreenFade n = 20 → 21 steps;
  typewriter reload with div 2 after a phase-2 frame; voice level 0 → −1024; close at cycle + 5; and so on.
* **The sandbox stays the reference.** `scene_video.py` can dump its per-update `SceneFrame` equivalents, and
  the engine replay must produce the same JSON.

## 5. Decisions taken in this plan (say if you disagree)

1. **Renderer:** keep the wgpu mesh renderer and feed it the game-exact pose, blend, face and camera. Do not port
   the GX packet generator (`nnsgen`/`frame_gen`) into the engine; it stays in the sandbox as the verification
   oracle.
2. **Frame clock:** the engine runs the nominal 4-frame cycle (no emulator lag). Lag-dependent effects
   (typewriter div, slide timing) follow from that clock. Recorded-session replays inject the recorded phase
   sequence so the tests compare like with like.
3. **Higher frame rate = emulated ticks plus interpolated presentation.** The simulation keeps the game's
   timebase exactly: a fixed-step accumulator emulates 59.8261 Hz frame ticks (VBlanks, which the typewriter,
   slide jobs, audio fader and lip-sync elapsed count) and the 4-frame Capture3D cycle (logic updates). The
   renderer runs at the display rate and draws between the last two `SceneFrame`s at `alpha` = the fraction of
   the current tick:
   * **Interpolated:** continuous quantities only. These are the animation frame (the game's clips advance one
     frame per update, so sub-frame sampling of the NSBCA curves), blend ratios, the camera, and fade
     brightness (the 16.16 `cur`, not the 0..16 level).
   * **Snapped at their tick:** discrete game state. These are eye/mouth texture ids, hand pose models, revealed
     characters, box state and motion switches. A mid-tick blink half-way between textures would be an
     invention.

   Logic results (which update anything happens in) stay identical to the game at any display rate, so the
   parity tests run on ticks and are unaffected. Interpolation is a pure presentation feature (like the render
   quality), switchable off for 1:1 comparison with the emulator.
4. **Runtime state that is not game data** is an explicit input: taps, RNG state at the scene start, and the game
   clock outside a save.
5. **Unverified rules are flagged, not tuned:** camera easing (`0xb`/`0x7`), the dialogue window art, and other
   heroines/presets. They keep their current behaviour behind a clearly named placeholder until each is
   recorded and checked (docs/CAMERA_ATTEMPTS.md rule).

## 6. Risks

* **Coverage:** one scene and one heroine are verified. Phase gates use this scene. Before declaring the port
  general, record at least one Manaka and one Rinko scene with a camera move and a choice.
* **Lip-sync** depends on the Siren decoder's PCM matching the game's (it did for this clip). The remaining
  ±1-update misses come from decode-thread scheduling and are acceptable.
* **Eye residual:** 42 updates are not explained, at blink-timing level. They were judged imperceptible, and
  the port should not try to close them by tuning.


---

## 7. Implementation status

| # | Built | Gate result |
|---|---|---|
| 0 | `engine/src/rom.rs`: code/data tables read **by game address** from the extracted image in `content/engine/` (`static.bin`, `ovNN.bin`, `memory_map.json`); fixtures `engine/tests/data/s08143402/*.json` from `sandbox/gx/export_fixtures.py` | fixtures regenerate; no behaviour change |
| 1 | `save.rs` (blocks found by magic + tag, newest generation, LZ10, packed layouts from the overlay-0 descriptor tables), `scene_inputs.rs` | `phase1_save_and_scene_inputs`: levels 15/15/15, text speed 1, calendar 0, hair key 1 → hair 0x3d = `n_00_100`, outfit 0xfc = `n_01_010` (class 1), shading day, backdrop evening |
| 2 | `sim/clock.rs`, `sim/fade.rs`, `sim/sched.rs` (tasks, yield, text commit, typewriter, box open/close, voice load) | 70 / 70 commands in the recorded update; every text commit; typing start 19 / 19 and end 17 / 17; brightness 1,177 / 1,177 |
| 3 | `sim/motion.rs`, `sim/eyes.rs` (incl. `Rand_Next` under mood 0x1a via `game::Rng`), `sim/lipsync.rs`, `sim/scene.rs` (`SceneSim`, `SceneFrame`, the traced update order) | identical to the sandbox simulation on every update (chain, mood, eye, hands, mouth, fade, preset); RAM: motion 1,173, eyes 1,135, hands 1,148 / 1,177 |
| 4 | `camera::GameCamera` (LookAt up (−1,0,0), target z 0, base RotY(word 7) + height, PerspectiveW, world-space lights, portrait = landscape turned 90° ccw), `render.rs` game-camera path, `nns3d::blend_layers` (`NNSi_G3dAnmBlendJnt`), `heroine::compose_pose`, `nns2d::bg_panorama` / `bg_window` / `rotate_ccw`, accessory at the Head joint | `phase4_game_camera_and_backdrop_scroll`: scroll (64, 32), projection = the game chain within 0.6 px; replayed frames vs the sandbox render: same pose, expression, hands, framing and backdrop (mean abs difference ≈ 17/255, render-quality and shading differences) |
| 5 | `audio.rs` (NitroSystem fader per game frame, (v/127)², slot volume 105·level/15, voice gains 2·(127/128)·pan × 10^(cB/200) with the level curve and the −1024 clamp), `snd::Driver::play_clip` stereo gains | `phase5_voices_start_four_updates_after_their_request`; replayed soundtrack vs `scene_video.py audio`: 50 ms envelope correlation 0.97, median level difference 0.0 dB, 90 % of audible windows within 0.6 dB |
| 6 | `player.rs` (fixed-step ticks, latched taps, auto-advance input policy, interpolation of layers and fade between updates), `bin/scene_player` (tick accumulator against wall time), `bin/scene_replay` (recorded session → frames + WAV), `bin/scene_audio`; removed `pace`, `pace_with`, `render_timeline`, `TICK_SECONDS`, `SceneState`, the time-based `LipSync`, `FaceTables::blink_closed`, the host's time bookkeeping (transcripts are untimed) | the replay produces the same SceneFrames as the sandbox (phase 2/3 test); all engine test suites pass |
| save | `save.rs` rewritten (2026-10-13, docs/SAVE_FORMAT.md): all six block kinds A–F, `Card` (SHA-1 headers, ping-pong copy / generation as `Save_WriteBlock`), game-identical `lz10_compress`, packed block ↔ game struct via the bit maps in `data/save_maps.json` (derived with the game's own packers / unpackers as the oracle), `Save` (struct images, `store`), `SaveFacts` from the struct images, `Save::game_state` (player name, scoped flags / counters, heroine stats) | `tests/save_content.rs`: on all four recorded backups the unpacked images equal the game's own unpack (SHA-1), pack(unpack) and LZ are byte-identical, an edit survives write + reload; a Rust-written save boots in the emulator and the game's GameState equals the Rust image (15448 / 15448 bytes) |

Deviations from the plan:
* Tables are read by address from the extracted code image instead of a JSON export: the image is already extracted once into `content/`, and reading by address keeps each rule next to the address it was decoded from.
* The renderer's scene pipelines cull for the old (mirrored) orbit camera; the game camera gives the DS's unmirrored screen, so material cull faces are swapped in game-camera mode (`render.rs`).
* Text length for the typewriter counts the rendered line (unset `{vN}` variables stay as their 4-character placeholder; the recorded session's name happened to have 4 characters).

Known differences left (presentation only, not game state): the accessory's shading looks paler than the sandbox's (texture/material treatment not yet compared); the face close-up's cheek shading is softer (renderer quality); camera moves `0xb`/`0x7` remain an unverified linear ease; the dialogue window art is a placeholder; face_l / face_m selection by head distance is not ported (the engine draws `face_m`).

Follow-ups noted (2026-10-13):
* Face textures at high output resolutions: the eye / brow / cheek textures are small and palettized (4/16 colours), so at 4K
  their texels and palette steps show as blocks and bands around the eyes (texture magnification, not toon stepping).
  Candidate: a one-time edge-aware upscale (xBR / ScaleFX style) plus gentle debanding of palette gradients for the face
  textures at load time (check `engine/src/upscale.rs` first). Plenty of performance headroom.
