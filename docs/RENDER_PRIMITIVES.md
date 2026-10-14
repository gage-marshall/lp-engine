# Render primitives (evidence-driven)

This file records how the game actually renders a heroine scene. A rule is listed here only with the
evidence behind it and the samples it has been checked on. Status values:

* **verified**: reproduced numerically from captured data.
* **observed**: read directly from hardware or emulator state, but not yet reproduced by our own code.
* **inferred**: consistent with the evidence, but not yet tested.
* **open**: not known yet.

The verified rules are ported to `engine/` (`sim/`, `camera::GameCamera`, `scene_inputs`, `save`; docs/ENGINE_PORT_PLAN.md §7). New rules are still worked out here first (see `docs/CAMERA_ATTEMPTS.md` for why).

## Method and tools (`sandbox/`)

* `tools/play_monitor.py --save-at <script>`: played by hand. It pins the emulator RTC to emulated time
  (`Emu.pin_rtc`; otherwise the host clock makes runs diverge from the first frame), logs every input change,
  and saves an anchor state when the script starts.
* `sandbox/rec/verify.py`: replays a session headlessly and compares it with the live screenshots.
  Session `20261013-084834` (Nene, break room, script `08143402`) matched on all 43 shots with a pixel
  difference of 0.
* `sandbox/rec/record.py`: per-frame main-RAM diffs plus screenshots for a whole session
  (`sandbox/rec/out/s08143402`, 5,564 frames).
* `sandbox/rec/gxcap.py`: the complete 3D command stream for a frame range, read from libdesmume's
  `gxFIFO` ring, which every GX write path goes through. It also saves the display registers, all VRAM
  banks, palettes and OAM per frame.
* `sandbox/gx/gx.py`: a reference geometry engine (GBATEK semantics) and a z-buffered coverage rasterizer.

## Verified: the whole frame generated from game data (bit-exact)

`sandbox/gx/scene_gen.py` takes **scene state only** and generates both polygon lists of a 4-frame cycle
(the leg pass and the main pass) as GX command packets. It then renders them.

* **Inputs:** heroine, wardrobe label ids, accessory id, time-of-day mode, camera preset, motion key,
  mood, lip-sync viseme, and the number of game updates since the motion started.
* **Sources:** content files plus code/data tables read from the ARM9 binary. No RAM, VRAM, register or
  capture value is used.

Results against recording `s08143402` (Nene, break room, preset 3014):

* **Whole updates, compositing included** (`sandbox/gx/frame_gen.py`): every packet the game sends between two
  `SWAP_BUFFERS`, for the 3 updates of a cycle (phase 0: compositing quad only; phase 1: leg pass; phase 2:
  main pass), is generated and compared with no exclusions:
  * moment `line26` (script position 26, swaps 6379/6380/6381): **26,216 / 26,216** packets exact;
  * moment `line30` (script position 30, mood 38, a queued motion, swaps 7198/7199/7201): **26,216 / 26,216**.
  Each update is: `Gfx3D_UpdateFrame` (ScreenFade 2D-in-3D setup, phase 2), `Gfx_RenderCapturedFrame` (the
  captured-image quad: ScreenFade sprite, direct-colour 256×192 texture at the capture bank's slot address,
  when the draw byte ≠ 0), `G3X_Reset` + `G3X_ResetMtxStack`, the object loop
  (`Mod2045ee8_ProcessessGraphics_10`), `NNS_G3dGlbFlushP`, the UI sprite manager's ortho setup
  (`Sprite_Proc_20490e0`), `SWAP_BUFFERS`. Phases come from `Capture3D_OnVBlank` (count 3, draw byte = phase − 1).
  * **VRAM addresses** come from the game's allocator (`VramBank_*`: bump allocation in load order, textures
    aligned to 0x80, palettes to 0x20, after the 47 `effect_3d` textures ending at 0x2380/0x5e0), not from
    captured VRAM.
  * **NODEMIX's 3 trailing words** are no longer excluded. `G3_LoadMtx43(sp+0xe0)` at `0x20054a0` loads a 3×3
    as 4×3, so words 9–11 are NODEMIX's saved r4–r6 (its frame is 0x104 bytes): r4 = the `NNSG3dRS` address
    (NNS_G3dDraw's local at its sp), r5 = `G3dDrawInternal_Loop_`'s literal `0x21164fc`, r6 = PG10's r6 (= 0,
    `0x2046944`). The rs address is computed from the binary: system-mode SP set by crt0
    (`0x2000820..0x2000864`: 0x27e3fc0 − 0x40 − 0x800 − 4, 8-byte aligned = 0x27e3778) minus the prologue
    frames of main `0x2000c38` → `Main_UpdateFrame` → PG10 → [PG9 for draws] → `NNS_G3dDraw` (push + 0x188)
    = **0x27e3598** (compute pass, called from PG10) / **0x27e3568** (draw pass, via PG9).
    `nnsgen.nodemix_stack_tail`; matches the runtime SPs at every level.
* **Image:** the render from these packets, with textures/palettes from the texture files and toon/edge
  tables from `.tmbsd`, against the emulator's BG0-only frame: IoU 0.975, median channel difference 4/255,
  44 % of pixels exact. Image: `export/scene/gx_from_data_6381.png`.

**Save file format** (from an in-game save made in session `20261013-110515`, exported by `play_monitor`):
`Save_WriteBlock` (`0x20f0fb8`) writes each packed block (overlay 0 `PackSave*ToBuffer`) as a 0x20-byte header
{u32 magic `0x10190573`, u32 body size, u16 generation, u16 version 3, 20-byte hash} + `MI_CompressLZ` body, at a
ping-pong card offset. That save: kind 1 → card `0xd4a0` (0x44fc bytes unpacked), kind 0 → `0xa20` (0x30c), kind 2 →
`0x19620` (0x1e6); RTC stamp at `0x33740`. Block 1 holds the heroine records at `0x1989 + 0x5b3·slot`, byte-identical
for 0x140 bytes to the RAM records at `0x2144250 + 0x688·slot` (all three slots checked). The hair label is not saved:
FlowState +0x24 is recomputed at event load from record +0xd.
Block 0 ('LDSD') is the settings record (`*(0x21544e8+4)`, 0x1f8 bytes) packed field by field by `Ov00Init_PackSaveA`
`0x21585b4`: sections of entries, each entry {u16 size, u16 count} from the descriptor list `*(0x2165e80 + 4·section)`
(entry counts `0x2164f00 + 4·section`), entry 0 of each section = its byte size; laid out back to back from offset 0.
Section 0 = header (0x25, 'LDSD', 1, total 0x30c, ...); section 1 = record +5, +6, +7 (music / sound / voice level) at
0x29–0x2b; section 2 = record +8, **+0xc (text speed) at 0x41**, +0xd, +0xe, +0xf at 0x40–0x44; section 3 = +0x1ec at
0x59, +0x10, +0x12 (... ). Checked against the RAM record in the savestates saved with all four backups (+5..+7, +0xc,
+0xf, +0x1ec agree).
Second save (session `20261013-110915`: day 10 played through Nene's 15h scenes, saved at 21h) against the pre-day
save: record +0xd is unchanged for all three heroines (2, 0, 1), and both generated moments stay exact using it. The day changed
39 bytes of block 1, 1 byte of block 0 and 1 byte of block 2. In Nene's record: +0x170, +0x1bd and +0x1dc went 1 → 2.
Slot 0 changed +0x1bc and +0x587; slot 1 changed +0x22a. Several global fields changed too (e.g. 0x14 → 0x205 at three
places, and the 9 bytes at 0x318f cleared). These are not decoded; they don't feed rendering.

How each scene-state input is produced by the game. `sandbox/gx/scene_inputs.py` resolves all of them from the
event script (run through `tools/event_trace.py`), the save record, the clock and data files; the resolved
scene equals the one the generator was verified with. All rules were traced at the event load (frames 3901–3902).

| Input | Source in the game | Resolved from |
|---|---|---|
| heroine | script id top byte; `Model_SetByte0AndNotifyGlobals` | event id |
| outfit label | script `12e 4 <label>` (flow_dispatch → FlowState) → `MapCtl_SetupContext` → `Model_SetPartSlotsByClass` (`0x204eed4`) | script |
| accessory | script `12e 10 <id×100+variant>` → `SceneState_UpdateFlaggedField` → `Model_SetPartSlot1FromId` | script |
| hair label | `SaveSlot_UpdateStateMetadata` `0x2060c7c`: MapCtl `+0x14` if set (cleared here), else FlowState `+0x24`, set by `Scene_UpdateRandomEventState` `0x20ecaf0` = `Overlay_UpdateMode` `0x20eee34`(slot, record`+0xd`, FlowState`+0x26`): first entry of `fashion/fashion_hair_param_<slot>.bin` (u32 n; n × {s16 label, s8 state, u8, u8 key, 3 u8}) with key = record `+0xd` and state = FlowState `+0x26`, else retry with state 0 | **save file**: record `+0xd` (Nene: 1) via `scene_inputs.record_0d_from_save`; random-event state = `scene_inputs.random_event_state` (0 while GameState `+0x78` = 0, save block 1 byte 0x14: every event load of the session writes 0) |
| time mode | `MapCtl_RemapModeCode` `0x204ee30` (0→2, 1→0, 2→1) of `Bg_ProcessTimedValue`: background kind 3/4 → bg value, else 1. Bg value = `Bg_UpdateTimedState` from `GameClock_RunTimeSequence`: season = first i with month ≤ [2,5,8,11] (`0x2101cfc`); phase = first i with hour < table[season][i] (`0x2101d4e`: [7,16,17] [6,17,18] [5,18,19] [6,17,18]), else 0 | clock (October, 15h → season 3, phase 1, mode 0) |
| backdrop time of day | `ScheduleText_UpdateState` 0x209dc34: phase = `TextRes_SelectTimePeriodText` (`TimePeriod_FromHour` 0x205ebf4: < 5 → 5, else first i with hour < [8,12,15,18,21] @0x2101d48, else 5; period 0–2 → 1, 3 → 2, 4–5 → 0) when GameState `+0x78` = 0 or flag 0xe0, else the season table above; flag 0x104: 1 → 2, flag 0x105 → 0; `ScheduleText_GetStateValue` 0→2, 1→0, 2→1 (day 0, evening 1, night 2). `scene_inputs.backdrop_tod` | game clock 15h → period 3 → **evening** (hooked in the game at the backdrop reload, frame 10559); bg 14 has one picture for all times |
| camera preset | script command 6 | script |
| motion key | script command `f`. `0x5d` only queues: at position 30 (`5d 694`) the stream still matches 587 exactly | script |
| animation frame | animation ticks since the `f` command, mod the clip length (31). Frame 0 is set when `f` runs (5977); one tick per completed Capture3D cycle (4 video frames, more on lag frames), i.e. the game's own update count. Measured on the body `NNSG3dAnmObj` (`0x232b5d4`): tick 96 → frame 3 (line26), tick 295 → frame 16 (line30). Not per video frame: an earlier per-frame reading matched only because 819 ≡ 199 (mod 31). Blend after `f 587`: ratio 0 for ticks 1–7, then +510.5 per tick to 4096 at tick 16 | game update count |
| mood | script command 0x34 | script |
| viseme | lip-sync of the playing voice; 5 once the clip is over. Clip length = `Siren14Voice.vtb` (offset, size) ×16, 40 bytes per 20 ms (30156: 3.94 s). The live mouth changes are `Model_SetMotionModeFromTable(chara, v, 0x14)` (stop at 6128 here) | voice table + playback time |

Rules (code location → what it does) used by the generator:

* **Wardrobe → files.** Label record (`tm_model_label.arc/NNNN`, decimal entry = id): u16 +0 = base
  archive entry, u16 +2 = motion class. `Mod207b444_Switch_207b444` gives the entry per slot:
  * hair and body: base + kind + 1;
  * right arm: base + kind + 4; left arm: base + kind + 3;
  * waist: base + kind + 6; leg: base + kind + 8 (garment category 0, `Mod2079638_Calc_2079978`);
  * face_m / face_l / hands: code tables `0x207b5e8`, `0x207b5e4`, `0x207b5ec`, `0x207b5f0`, indexed by
    [variant × 6 + kind × 3 + heroine].

  The archive layout per label base is: +0 `.tmbsd` bundle, then models and textures. Accessory:
  `(u16)(id × 100 + variant)` is looked up in the table at `0x21251fc` → `block3d` entry + kind (405 → 106/107).
* **Motion.** `Gfx3D_ResetActor`: key → `tm_model_block_base.arc/0101` record [key][class][heroine]
  (s16 at +2) → animation index → motion table (from code tables) → `async3d.arc/<bundle>` → stem.
  Each part plays `<stem>_<part>`.
* **Joint records.**
  * Compute passes (render-object flags 3 = RECORD | NOGECMD) sample each part's own animation.
  * The hands copy the arm records 0–7 (`Matrix_TransformVector3` / `Matrix_Multiply43`, NODEDESC
    callbacks on the arm's last node).
  * The waist copies leg records [0, 1, 4, 7, 8, 9, 10, 11] (`Matrix_Multiply33`, class ≠ 2).
  * The faces use the hair's record buffer.
  * BCA sampling uses NitroSystem's integer path (3:1 and 1:1 key blends, `VEC_Normalize`).
* **SBC.** NitroSystem's interpreter with the basic scaling rule:
  * NODEDESC emits RESTORE src, then MULT_4x3 / TRANS / MULT_3x3, then SCALE unless 1, then STORE dest.
  * NODEMIX uses the evp cache; it runs even in compute passes and uses the GE as a calculator
    (identity projection, read back).
  * POSSCALE, MTX, MAT and SHP (the display list, sent as is) behave as in the library.
* **Game callbacks.**
  * `Model_ApplyNamedBoneMtx` (hair, body, hands; timing C) reads the current matrix of 17 tracked bones
    (names: DTCM `0x27e033c`), emitting projection-mode PUSH/IDENTITY/POP, once per bone per update.
  * The accessory callback `Mod2045ee8_ProcessessGraphics_6` (timing A, skip) emits TRANS (dead),
    LOAD_4x3 of the Head bone, and MULT_4x3 by the accessory object matrix.
* **Materials.**
  * Lights 0+1 are enabled on every part material (`Gfx3D_SetModelMaterialColors`, not on accessories).
  * The polygon-mode mask bits are cleared before each draw and the global attribute is
    `NNS_G3dGlbPolygonAttr(0, TOON, CULL_NONE, 0, 31)` (`Mod2045ee8_ProcessessGraphics_8`).
  * Polygon IDs are set by material name (`brow`, `cheek`, `eye`, `face`, `mouth` / `facel` / `hair` /
    `neck` + `_m/_r/_n`) from the table `[1,48,2 | 48,48,48]` at `0x21064d8`, chosen by the facing test.
    Accessories get table[k=0].
  * Texture binding is by name (NitroSystem: `teximg |= param + set base`).
* **Facing test** (`Char_IsFacingTarget`): the camera eye lies within 15° of the Head's forward
  (0, sin 15°, cos 15°). It selects the polygon IDs and the edge table.
* **Expressions.** The face materials' `.0` textures are placeholders. Eye, brow, cheek and mouth come from
  the face tables (code `0x2125168`, `0x2127ae4`; `tools/face_tables.py`) for mood + viseme + blink; the skin
  from the base face bundle. Here mood 22 + silence gives eye 5, brow 5, cheek 0, mouth 8, all verified
  byte-for-byte in VRAM.
* **Global state (FlushVP).**
  * Projection: `MTX_PerspectiveW(sin/cos at index 1820 = 706/4035, aspect 0x1555, near 1, far 400, W 10)`.
  * Camera: `MTX_LookAt(eye, up (−1,0,0), target z = 0)` from the preset.
  * Lights: `Model_ApplyGlobalLights` (colours `0x2102bac`, vector (0,0,−4095) × RotX × RotY with the
    preset's light angles, light 3 = NitroSystem default).
  * Base: RotY(word 7) with height table `0x2102e2c`.
  * Material defaults: `0x4210` (`0x20469e0`).
  * All of it uses NitroSDK fixed point exactly (`FX_Div`, rounding multiplies, the sine table at
    `0x20fc090`).
* **Passes and visibility.**
  * Draw phase = (slot ≠ 9) (`Model_SetPartPaletteFromTable`), so the leg is the separate pass;
    accessories are phase 1.
  * Draw order = object array order.
  * Face models: `Model_ClassifyIdRangeToVariant` gives 0 (face_l) if the Head distance ≥ 210, else 1
    (face_m) for close-up presets or 2 (neither). The Head distance is computed per update by
    `Model_UpdateCharaScene`.
* **Shading tables** (`Mod207b910_Proc_207b994`).
  * Toon table: the base bundle (`n_base_<time>.tmbsd`).
  * Edge table: entry 0 from the base bundle, entries 1–5 from the outfit, 6–7 from the hair (ownership
    switch `0x207b910`); the not-facing table has entry 6 = `0x296d` (code constant).
  * Uploaded by `VramBank_ProcessessGraphics`, which also enables edge marking.
  * `DISP3DCNT = 0x39` (textures, blend, antialiasing, edge marking, toon); clear colour = transparent
    black, ID 63.
* **Backdrop scroll** (`Projection_UpdateScreenPosition`): project the camera target (z = 0), add camera
  shake, then scroll = (64 + (128 − sx) / 5, 32 + (95 − sy) / 5), clamped to [0,128]×[0,64]. The result
  goes to the BG1 shadow table (DTCM `0x27e25a0`). Reproduces the captured (64, 32).

Not generated yet:

* **Motion blending** (decoded; generated output near-exact). On `f` with a blend length B, `Gfx3D_InitializeModel` (`0x207d218`)
  puts the new motion on layer 0 at frame 0 and the previous motion on layer 1 at its current frame. It sets
  w = (0, 4096, 0) and `inc0 = dec1 = (int)(4096f / (B·0.7f))`; the third layer gets `inc2 = (int)(4096f / (B·0.4f))`,
  or 0 when chara+0x40 is −1. Per update, `Motion_UpdateBlendWeights` (`0x1ffa56c`) runs while remaining > 0:
  t = FX_Div(B − remaining, B); w0 += inc0 once t ≥ 0.3; w1 −= dec1; w2 ramps up while t ≤ 0.4 and down once
  t ≥ 0.6; remaining −= 1; the ratios are FX_Div(w_i, Σw), with the third taking the remainder.
  `Gfx3D_DrawActor` writes them to the NNS AnmObjs (new first, old chained via `next`), and NitroSystem's
  `NNSi_G3dAnmBlendJnt` mixes them. That includes renormalizing rows 0 and 2 and the rounded `VEC_CrossProduct`.
  * `nnsgen.motion_blend_ratios` reproduces the RAM weights of `f 587, 20` exactly (tick 7: 510/3586 … tick 14:
    4082/14, then 4096).
  * `nnsgen.AnimChain` ports `NNSi_G3dAnmBlendJnt`.
  * Against GX captures mid-blend (`gx_6005_6035.npz`, ticks 7, 8, 9): 233/261, 4,225/4,231 and 21,692/21,724
    packets exact. The residual is skinned (NODEMIX) loads involving the leg. The leg's compute pass (phase 0)
    matches the *previous* tick's ratio, while its draw (phase 1) needs the current one. When the leg's record
    buffer (`recJntAnm`, in ITCM) is refreshed within the cycle is **open**.
  * Early in the blend (5980/5986) the main pass has 21,691 packets instead of 21,724; not yet examined.
* **Antialiasing.** DISP3DCNT bit 4 is set by game code (0x39), so the game asks for it, but it is hardware
  rasterizer behaviour, not game data. The reference emulator (libdesmume's software rasterizer) does not
  emulate it, so a reference rasterizer implementation cannot be verified against these captures; it is
  left out on purpose. It needs a hardware capture or an emulator that implements it to check against.

## Verified: backdrop picture layout (BG1)

A backdrop is 48×32 tiles (384×256), 4 bpp. The character data (NCGR, 1,536 tiles) is the picture in **linear tile order**:
tile (tx, ty) = character `ty·48 + tx`. The screen map (NSCR, two blocks: 32 columns, then the remaining 16 columns at
`1024 + ty·16 + tx − 32`) stores that same index, but in the 10-bit field. Every tile from 1024 on (roughly rows 21–31)
is therefore stored as 0; only its palette nibble is valid. Decode = character from the position, palette from the map.
This was checked on `bg_center` entry 41: all 1,024 representable entries equal the linear index.

Reading the map's tile index literally draws tile 0 with assorted palettes in the lower rows, which shows as diagonal noise.
It appears behind the heroine's head when the window scrolls (HOFS 64, VOFS 32). The same mistake is in the engine:
`engine/src/nns2d.rs bg_picture` crops to 168 rows to hide it ("rows 21-23 hold other tile data"). The salvaged Codex
`bg_window` patch has it too. Fixed in `sandbox/gx/scene_video.py backdrop()`; the engine is unchanged pending agreement.

## Verified: scene drivers from game logic (whole scene 08143402)

`sandbox/gx/scene_video.py sim` builds one row per game update. The only inputs are the event id, the save file, the game
clock and calendar state, the player's tap releases, and the frame clock (Capture3D phase per frame). Every driver is a port
of the game's code and was checked update by update against RAM of the recording (1,177 updates):

| Driver | Port (sandbox) | Code | Match |
|---|---|---|---|
| Script pacing (when each command runs) | `script_sim.py` | VM stream (`event_trace`) + tasks: `0x44 n` = n updates; `0x1` yield resumes the update after the tasks free; fades as below, the wait task (`TaskList_FreeAfterFadesIdleThreeTicks` 0x206c6ec) frees on the 3rd update with both fades idle, counting the update the fade ends in; text: typewriter below, the first tap while typing completes the text, the next closes it and the script resumes 4 updates after the cycle containing the release | 70 / 70 commands in the exact update |
| Typewriter | `script_sim.typing_end` | `TextControl_ProcessText` 0x208bf4c, once per frame: 12.12 accumulator −0x1000; draws one character and reloads with interval[speed] / div when its integer part is ≤ 0. Intervals 0x21274e0 = 10, 8, 4, 2, 1 frames; speed = settings record `+0xc` (`SaveData_GetFieldc`, passed by `MenuText_SelectState` 0x208c740) = **save block 0 byte 0x41** (`Ov00Init_PackSaveA` section 2 entry 2); div = 0x213d2e4+0xa0 (`VramBank_Process` 0x2044ee8: 2 after a phase-2 pass, else 1) = 2 iff the previous frame's phase is 2. Typing starts 2 frames after the phase-0 run of update A + 2 (A = the line's update, or + 1 when voiced; see message box below) | reload/step 2,071 / 2,071 frames; typing start 19 / 19, end 17 / 17 (2 lines tap-completed) |
| Motion (clip, frame, blend ratio) and mood | `motion_sim.MotionSim` | `MapCtl_UpdateTouchSelection` 0x204f664 (hold +0x90, pose-ready, queued 0x5d, default key +0x9c), `Gfx3D_UpdateActor/ProcessActor/UpdateModel_2/InitializeModel`, `Gfx3D_DrawActor`, `Motion_UpdateStateMachine` (group types, segment counts 0x2102e08), `Motion_UpdateBlendWeights`, `Mod2045ee8_Process` (frame advance, loop/clamp); pending mood is passed to the start | 1,173 / 1,177 (4 = one-frame snapshot lag) |
| Eye texture / eye mode | `motion_sim.EyeSim` | blink 0x2094aa4; transition 0x2094cdc; requests `Model_RequestMotionState` 0x2079eb4 from the motion's `eye%02d` track; every mood set (0x2079c74) starts a type-2 reload; texture residency (ids 4, 5 fixed + last loaded set). Order in `Model_UpdateCharaScene` (traced): the eye step sees the mood from before this update's motion start (`Model_SetMotionIdAndInitFlags` runs later, in `Gfx3D_UpdateActor`); `Gfx3D_UpdateActorState` issues requests from the current frame's tracks before the frame advance. Mood 0x1a blinks draw `Rand_Next` (`scene_data.Rand64`, MATH_Rand64 seeded from the RTC at boot; its state at scene start is runtime state; unused here) | 1,135 / 1,177 at the end-of-cycle sample; 881 / 942 at the exact eye-step point (hooked) |
| Hand poses | `motion_sim.actor_state` | `Gfx3D_UpdateActorState` 0x207db00: lhand/rhand tracks of the motion's NSBVA (`async3d` bundle + 549), first active node ≥ 3 → model `async3d` (per-heroine base 0x2102e50/52) + pose | 1,148 / 1,177 |
| Mouth (lip sync) | `lipsync_sim.py` | `VoiceQueue_UpdateMode` 0x209ab74: stream peak (every 5th sample of decoded 640-sample blocks, `GameSnd_StreamAccumulatePeak` 0x2078088) vs threshold 1000; key = lipsync.dat[elapsed·n/duration], elapsed counts frames while speaking (VBlank), duration = Siren frames; rest pattern 0x21046f4; mode = NextModeIndex; hold 20 updates. Voice starts 4 updates after `0x5c` (stream pipeline). PCM from `tools/siren14dec.c` | 42 / 54 calls of line 30156 in order; misses are ±1 update at shape changes (decode-thread scheduling) |
| Screen fades | `script_sim.ScreenFade` | `ScreenFade_ToBlack` 0x20451f8 / `ToClear` 0x20452a4 / `ScreenFade_Process` 0x2045390, 16.16, once per main-loop pass (3 per update, from the update after the command): step = (target − cur) / max(n, 1) truncated (n = 20 takes 21 steps); to-clear waits `counter ≥ delay` (6 for `0xe`); master brightness = cur / 0x1000 toward zero. A row (end of the cycle) also includes the next cycle's phase-1/2 passes | brightness 1,177 / 1,177 |

Inputs that are not game data: the player's tap releases; the frame clock of this emulator run (the Capture3D phase per
frame; it sets the update boundaries and the typewriter's div, and lags under CPU load); the game clock (month, schedule hour:
`scene_inputs.session_clock` from the session log). From the save: text speed (block 0 byte 0x41), sound levels (block 0
0x29–0x2b), record `+0xd`, the calendar state GameState `+0x78` (block 1 byte 0x14, `Ov00Init_PackSaveB` section 0
entry 8; set to 1 when a heroine enters the lovers state), which selects the backdrop time-of-day path and the random-event
state (`scene_inputs.random_event_state`: 0 when `+0x78` is 0, else heroine record `+0x107`). Not data: `set_actor`'s
model load before the first update, and sub-update audio decode timing.

Message box and voice timing (traced by replaying the session with exec / write hooks; `script_sim` header):
* The script blocks with context byte `+0x10bd` = 0x83; `Script_ExecuteOpcode` 0x204d388 then handles the dialog
  Transform state (`*0x21418a8` +8) once per update.
* Open: text → state 3; the script's update runs `GameSnd_AdvanceStreamStartState` 0x2073990 (1 at once without a
  voice, else once `VoiceQueue_IsLoaded`) → 4 = slide-in (VBlank job: +10 per VBlank to 80 = 8 VBlanks) → 5 → 0 → 6 =
  typing. Typing starts after update A + 2 (19 / 19 lines).
* Close: `DialogState_Update` 0x208fce4 consumes the release in the first update starting after it (cycle + 1) → 10;
  the next update's script stops the voice (`GameSnd_StopStreamAndClearQueue`) → 16 = slide-out (job 0x208f500,
  8 VBlanks to flag 0x27e2614); `MenuList_RefreshState` 0x208e0f8 → 17 after that frame's script; the next update
  clears `+0x10bd` and runs the commands: cycle + 5 (19 / 19 closes; after the last one the commands themselves start a fade and wait, so `+0x10bd` goes 0x83 → 0x87 instead of 0).
* Voice: cmd `0x5c` (update c) only enqueues; c + 1 `GameSnd_ProcessStreamRequestQueue` → `VoiceQueue_Enqueue` /
  `LoadEntry`; c + 2 `IsLoaded` seen (state 2); c + 3 reaped, and that cycle's phase-2 pass `GameSnd_UpdateStreamSlots`
  → `VoiceQueue_PlayEntry` + `GameSnd_StreamSetup`; c + 4 `VoiceQueue_UpdateMode` sees it playing (lip sync).
* Eye start state: `Model_ResetMotionFlagsEntry` / `Model_ClearLoaderSet` / `UiAsset_LoadVariantResources`
  (eye = 0x2103540[0] = 4) / the creation mood set (type-2 reload): `motion_sim.EyeSim`.
* Voice level 0: value 0 → −640 cB, which `GameSnd_StreamSetup` / `StreamStartIfReady` turn into −1024 (docs/AUDIO.md).
Not drawn: dialogue box / UI layers. Video: `sandbox/rec/out/video_08143402/scene_08143402_side_by_side_sim.mp4`.

## Verified

* **Geometry decoding.** Running the captured command stream through `gx.py` gives per-vertex clip
  coordinates equal to libdesmume's own `vertList`, within fixed-point rounding (at most about 2/1000), for
  all 5,393 vertices of the list swapped at frame 6381.
* **Coverage.** Rasterizing that list (minus the full-screen backdrop quad) against a 3D-only emulator
  frame gives IoU 0.975. The emulator was run with BG1–3 and OBJ disabled, and BG0 was captured into bank A
  at frame 6383. The residual is a 1-pixel silhouette rim (see *open: edge marking*) plus a few interior
  pixels.
* **Full one-frame render.** `sandbox/gx/frame.py` renders the decoded list against the captured VRAM
  (bank mapping from VRAMCNT, all texture formats, palettes) and the captured 3D registers (toon table at
  I/O `0x380`, edge colours at `0x330`, DISP3DCNT). Shading is toon, then modulate. Culling is front-only,
  with front = counter-clockwise in screen space (the clockwise convention measures far worse: median 40
  against 4). Edge marking is included. Compared with the BG0-only emulator frame: coverage IoU 0.975; on
  shared pixels the median channel difference is 4/255 and 45 % of pixels are exact. The residual sits on
  polygon edges and texture-sample boundaries (antialiasing is not implemented; the rasterizer uses
  approximate fill rules). Image: `export/scene/gx_render_6381.png`.
* **Camera lives in the projection matrix.** Each 3D pass loads the perspective matrix in matrix mode 0
  (`[0][0]=42.8677, [1][1]=57.1533, [2]=(-10.05,-10), [3][2]=-20.05`; this matches `CAMERA_NOTES.md`). It
  then multiplies a 4×3 look-at into that same projection matrix (`MTX_MULT_4x3` in mode 0, values
  `0x0, 0xfffff005, 0xffffff40, 0xfce, …` at preset 3014). Clip = position × (look-at × perspective).
  All model, joint and part transforms live in the position stack.

## Verified: scene state → transforms (scene 08143402, list swapped at frame 6381)

`sandbox/gx/rebuild.py` replaces every projection, camera, base and joint value in the captured stream with a
value computed from files and scene inputs, then re-renders. Results against the emulator:

* IoU 0.9750 with rebuilt transforms, against 0.9747 with captured transforms.
* Median colour difference 4 in both cases.
* Largest difference between a captured and a rebuilt matrix element: 0.014, which is fixed-point rounding.

Image: `export/scene/gx_rebuilt_6381.png`.

The chain, all applied by NitroSystem G3D (`NNS_G3dGlbFlushP/VP`, `NNS_G3dDraw`):

1. **Projection** (matrix mode 0) = perspective × look-at.
   * Perspective: half-angle sin 706 / cos 4035, aspect 0x1555, near 1, far 400, W-scale 10 for preset 3014.
   * Look-at: preset eye (words 1–3), target (words 4–5, **z forced 0**), up (−1, 0, 0). The eye's x is used
     as is, not negated.
2. **Base**, per drawn object (`Mod2045ee8_ProcessessGraphics_9`): the rotation is RotY(camera+0xb4)
   combined with the object's own 3×3 at `obj+0` (identity here). The angle at camera+0xb4 equals preset
   word 7; `Transform_DrawYawMatrix` writes it to `0x213e5bc` each frame. NNS RotY convention: row0 =
   (cos, 0, −sin). Translation `obj+0x24`, scale `obj+0x30`.
   * **Body parts** (`Gfx3D_UpdateAnimation`): translation = (0, `chara+0x44`, 0). `chara+0x44` is set
     every cycle by `Gfx3D_DrawActor` from the per-heroine table at `0x2102e2c`: Manaka 0, Rinko −21488,
     Nene 8380 (2.046). Camera presets 1013, 2013 and 3013 add per-heroine offsets from `0x21254c0`:
     (0, 6963, 41861), (0, 23757, 28590), (0, −4096, 29491). Their effect is not yet checked on screen.
   * **Accessories**: the base is set but overwritten. The accessory's matrix is the **Head joint world
     matrix** (from the compute-only `NNS_G3dDraw` pass, flags 3), then × the offset for
     (accessory id % 100, heroine) from `block3d.arc/0202`, then × the model position scale. The table
     holds 49 records of {id, 3 heroines × (x, y, z) float}, loaded by `Model_ComputePartScaleOffsets`.
     Here: accessory 405 → `block3d/0106` `i_head_05_00` (`cap_01`), Nene (0, 19.543, 5.540).
3. **Joints**: per part, NNS SBC composes (restore parent, multiply local, store). The locals are BCA samples
   of the part's own animation, at an **integer frame**, with matrices used as stored (not transposed).
   Skinned vertices also multiply the model's envelope inverse-bind matrices.
   * Here: motion file `m_0230_10` (in RAM at `0x2266588`, 8 sub-animations named by part: `_body`,
     `_hair0..2`, `_larm`, `_rarm`, `_leg`, `_item`), frame 3 when drawn (RAM after the frame shows 4),
     ratio 1, one animation per part.
4. **Parts** (`chara` at `0x229154c`; part object pointers at `chara+0x208+16·i`, objects 0x340 bytes,
   render object at `+0x2cc`): hair `n_00_100`, body/rarm/larm/waist/leg of `n_01_010`, base hands
   `n_base_{r,l}hand_00`, faces `n_base_face_m/l`, and accessory slots at `chara+0x2ac+16·k`. Their
   models are identified by matching RAM against content files; they differ only by load-time patches.

All items that were open in this chain are now resolved. See the generator section at the top: the
SBC-driven op order, hand/face record sharing, face visibility, motion selection and timing, and NODEMIX's
matrix-only blocks. Blending is not exercised by this recording.

## Observed: frame pipeline of a heroine dialogue scene (preset 3014, frames 6370–6390)

The work repeats every 4 frames. Engine A drives the lower screen (`POWCNT1 = 0x020f`).

| Cycle frame | GX submitted (SWAP) | DISPCNT A | Capture (DISPCAPCNT) | VRAM A / D |
|---|---|---|---|---|
| 1 | list of 1 poly: full-screen ortho quad textured from bank D (`TEXIMAGE 0x7ed08000`: direct colour, 256×256, slot offset 0x40000) | normal mode, BG1–3 + OBJ, **BG0 off** | 3D only → bank D (`0x81330010`) | A = OBJ, D = LCDC |
| 2 | list of 520 polys, all at screen x −383..−62 (off screen in this shot) | same | same | same |
| 3 | list of 3,130 polys: the visible heroine, plus a full-screen D-textured backdrop quad | same | same, completes (enable bit clears) | same |
| 4 | 92 entries, no SWAP: ortho projection, viewport, D-textured quad setup | **VRAM display (bank A)**, BG0–3 on, OBJ off | graphics (BG0+BG1–3) → bank A (`0x80300010`) | A = LCDC, D = texture slot 2 |

* Frames 1–3 show the last composite through 12 bitmap OBJs (OAM 115–126, 64×64 each, tiling 256×192 at
  (0,0), no affine transform). Frame 4 shows bank A as a VRAM bitmap. Bank A changes only on cycle frame 1,
  which is when the composite captured during frame 4 lands. The visible picture changes once per cycle
  (15 Hz).
* Bank D is all zeros throughout this window, both through the struct offset and through its LCDC
  mapping. With list 2 entirely off screen, a 3D capture of it would be empty, so this is consistent and
  not necessarily an emulator fault.
* **Verified pass contents:** pass 2 (the 520-polygon list) is the **leg part alone**: draw phase 0 for
  slot 9. Pass 3 holds every other visible part plus accessories (phase 1). Both lists are generated
  bit-exact (see the top section). The leg render is captured into bank D and drawn behind pass 3 as a
  full-screen textured quad; in this framing the legs are off screen, so D is empty. The final 3D image is
  captured with the 2D layers into bank A and shown through OBJ or VRAM display. The register constants
  live in `Capture3D_*`. The per-update streams, compositing included, are generated exactly (top section).

## Observed: BG scroll values are readable

DeSmuME's raw ARM9 I/O array sits at `MMU+0x100c000` and holds the values the game wrote to write-only
registers. In this scene, engine A: BG1 (backdrop) HOFS = 64, VOFS = 32; BG0 (3D) 0, 0.
`BG0CNT..BG3CNT = 0x0006, 0x4107, 0x050c, 0x460d`. The (64, 33) estimate in `CAMERA_NOTES.md` was
registered by eye; the actual written value is (64, 32). The scroll rule is decoded (top section,
*Backdrop scroll*) and reproduces it. The real shadow table is in DTCM (`0x27e25a0`); the earlier main-RAM
address was wrong.

## libdesmume offsets used (bundled arm64 0.9.12 build)

`MMU`: main RAM `+0xc000`, raw ARM9 I/O `+0x100c000`, palettes `+0x2014000`, VRAM banks A–I (LCDC order)
`+0x2014800`, OAM `+0x20d8808`. `gxFIFO`: `cmd[200000]` `+0`, `param[200000]` `+0x30d40`, head/tail/size
`+0xf4240/4/8`. `vertList` pointer at lib base `+0x314a0a8` (16 floats per vertex, clip xyzw first), count
at `+0x63568b8`. RTC patch: `tools/emu_driver.py:pin_rtc`.

## Open

* **Other scenes.** Two moments of one scene (different mood, animation frame, queued motion) are exact. Still
  unchecked: other presets (a close-up with face models, legs on screen), other heroines and wardrobes.
* **Motion timing:** when a queued (`0x5d`) motion takes over; how `f`'s blend/hold arguments map to the measured ratio ramp.
* **Motion blending residual:** the leg record timing within the cycle; the early-blend packet-count difference (top section).
* **Antialiasing** (see the top section: no reference to verify against).
