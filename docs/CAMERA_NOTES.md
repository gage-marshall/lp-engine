# Camera, heroine placement and scene appearance: what is known, what is not

Written at the end of a session in which several changes were made by comparing renders with how single recorded scenes looked.  That approach was wrong (it produced
compensating sign errors), so those changes were reverted.  This file separates **decoded from the game** (code / RAM) from **guesses**, and lists what was removed.

## Decoded from the game (reliable)
Sources: decompiles of `Camera_Reset`, `Camera_SetPreset`, `Camera_BeginTransition`, `Mod2045ee8_*` (camera module), `Transform_ApplyScaleVector`; the camera struct, projection and look-at matrices read from the emulator
(`tools/play_monitor.py` writes `tools/emu_scripts/states/logs/vram/v-<script>-f<frame>-{a,b}.cam`, `.bin` = BG VRAM + palettes + registers, `-a.ram` = main RAM).

* **Projection** (`Camera_Reset`, `Mod2045ee8_ProcessessGraphics_11`): `MTX_PerspectiveW(fovSin, fovCos, aspect, near, far, wScale)` with the half-angle from a sin/cos table at angle 1820 (10 degrees, tan = 706/4035 = 0.17497),
  aspect 4/3 (0x1555), near 1.0, far 400 (600 for presets 1011/1012/1019/1046, 2011.., 3011..), W-scale 10 (2 for the 600 case).  Then a zoom factor (+0x40) and a screen shift (+0x44/+0x48), applied in projected space; **zoom 1.0 and shift 0 in every
  recorded scene** (read from RAM).  The real projection matrix has [0][0] = 42.87 and [1][1] = 57.15 = (cot 10deg / aspect) x 10 and cot x 10.
* **Camera is rolled 90 degrees**: up vector (-1, 0, 0) (`Camera_Reset`; real look-at rows (0,-1,0), (0.99,0,0.13), (-0.13,0,0.99) for preset 3000).  So the 10 degree half-angle spans the **portrait width (192 px)**; the vertical angle is
  2 atan(0.175 h / w).  Implemented in `engine/src/player.rs` (`GAME_TAN_HALF_WIDTH`).  Eye/target come from the preset record (words 1-3 eye, 4-5 target x,y, target z forced to 0 by `Camera_BeginTransition`).
* **Preset record** (164 bytes): word 7 = a 16-bit angle (see below), 8-13 light angles, 14 a mode flag (0 or 2), 15-26 four alternate eye vectors and 27-38 four alternate target vectors (the D-pad manual views of `Camera_UpdateManualControl`), 39/40 four u16 angles.
  There is **no field of view and no zoom** in the record.
* **Backdrop** pictures: see `docs/SCENE.md` (selection decoded and reproduced for all recorded samples).  The backdrop is BG layer 1 (a 512x256 text map, behind the 3D layer, prio 3); the text window is BG2 (prio 0), another picture layer BG3 (prio 1).
  The NSCR holds a **48x32-tile (384x256 px) picture**: columns 0-31 in the first screen block, columns 32-47 packed 16 entries per row in the second block.  Entries are offset by a tile base (958 in the dump).
  The screen shows a **256x192 window scrolled by (HOFS, VOFS)**; for the Famires break-room frame registration against the picture gave (64, 33).  The lower rows (about 21+) of the picture decoded from the files differ from VRAM
  (the game supplies other tiles there; unexplained), and **the scroll rule is not known**.  Scroll registers cannot be read back from hardware (write-only); the shadow table address I used (0x27e25a0 from an ITCM literal) reads code at run time and is wrong.
* **World rotation observed once**: in the RAM of scene `08143402` (preset 3014, word 7 = 49664 = 272.8 deg) a 3x3 matrix at 0x212bdfc is a pure Y rotation by 87.2 deg = 360 - 272.8.  It sits next to the projection/look-at matrices in the renderer state.
  What it multiplies (the heroine only? the whole world?), its sign convention relative to our frame, and whether other presets behave the same were **not established**.
* Motion blend / queue semantics (`Gfx3D_InitializeModel`, `Gfx3D_UpdateModel_2`): see `docs/SCENE.md` (implemented, with tests).
* `Display_ApplyBgScrollRegs` copies shadow values (flag byte per BG, x at +8+8i, y at +0xc+8i) to the BG scroll registers; the shadow table address is still to be found at run time.
* Costume: the game opens `tm_model_label.arc` entry 0x195 only for the Nene Famires scenes (-1 elsewhere) and `tm_model_block_base` entry 0xfc (Nene), 0xa0 (Rinko), 0x4d (Manaka).  The label entry lists part block ids (e.g. 614-624 in n_block3d_01.arc, files for outfit 22/23 palettes and shade tables); what they select is **not decoded**.

## Guesses that were removed (and why)
All were derived from how a recorded scene looked, not from the game:
1. **Camera x negation** (`render.rs`: eye/target x sign).  Reverted to the original `(x, y, -z)`.  Only presets with x = 0 were ever verified; the sign of x for other presets (e.g. 3014: eye -18, target -13) is unknown.
2. **"Heroine stands at the preset target x"** (camera shifted by target.x).  Removed.  It centred a face in one scene and was refuted by the reference frame (heroine right of centre there).
3. **Heroine body yaw = preset word 7**, applied in the renderer, and **a head turn toward the camera clamped at 60 degrees** (sign flipped by eye).  Removed.  Together they produced a mirrored result (wrong side, hair swept the wrong way).  The head-turn sign flip hid a wrong body-yaw sign.
   The parsed value stays in `Preset.yaw_deg` as data only.
4. **Default outfit / hair per heroine and place** (Rinko hair 200, Nene hair 200, outfit 10 for Nene at a bg_center place).  Removed; defaults are outfit 0 / hair 0.  The observations behind them (kept as evidence, not as rules):
   Rinko's hair in recorded frames matched id 200; Nene in the Famires matched outfit 10 (waitress uniform, orange bow and name tag) with hair 200 plus a white-and-orange cap that **we do not render at all** (the bib is rendered; the cap is a missing piece, probably one of the `tm_model_label` parts).
5. The calibrated "VIEW_SCALE" zoom (earlier) was replaced by the decoded projection and is gone.

## What was kept (decoded, or layout only)
* The decoded projection (`GAME_TAN_HALF_WIDTH`) and the DS-aspect pillarbox for windows (`ds_fit`): the picture is the console's portrait screen, rendered 3:4 and centred.
* Backdrop selection and decoding (`engine/src/backdrop.rs`), the small learned place table for stage scenes (`engine/data/bg_observed.json`, marked as learned from play).
* Motion blend / queue / idle loop, the 7-track hair fix, menu modes 2-5, season/period/status queries.
* Tools: `tools/side_by_side.py` (recorded frame beside our render, aligned through the recorded voice/motion commands; needs trustworthy projection/placement before its output means anything), `tools/frame_metrics.py` (noisy),
  `tools/bg_pano.py`, `tools/bg_decode.py`, `engine/examples/{outfit_sheet,hair_follow,root_probe,joint_rot,clip_sheet}.rs`.

## Known wrong right now
* Backdrop: a fixed window of the picture, no scroll, wrong lower rows.
* Heroine placement and orientation: x sign for presets with x != 0 unknown, body yaw unknown, no head/eye behaviour, no per-scene costume; poses played from the clip regardless of seating.
* The Famires reference frame (Nene seated, body turned away from the player, head to the camera, eyes closed, cap) is **not** reproduced.

## Plan for a clean start (principle: only code/RAM evidence, never "looks right")
1. Rebuild our camera from the game's own matrices: from a preset record compute eye/target/up exactly as `Camera_BeginTransition` / `MTX_LookAt` do, and compare numerically with the dumped look-at matrix for several presets (3000, 3014, 2006 ...), including x != 0.
   Do the same for the world rotation: find which object it applies to (RAM snapshot + decompile of the overlay function the camera module calls, `func_0x02171f58`, resident overlay of the dialogue scene), then verify with a numeric projection of a known vertex.
2. Find the heroine's own transform and joint matrices in the RAM snapshot (`-a.ram`): the render object / joint result arrays of the body model, to read what the game really applies (head/neck look, seated offsets).  A search for exact joint rotations from our sampled pose found nothing, so the storage layout needs working out from the decompile of the 3D draw path (`Gfx3D_DrawModel_2`, `Mod2046354_*`).
3. Backdrop scroll: locate the real shadow table at run time (decompile what writes it: `Gfx3D_SetLayerVec2`, `Bg_*` state) and read it from the `.ram` snapshots; reproduce the NSCR/VRAM tile layout including the rows the files do not give.
4. Costume: decode the `tm_model_label` entry format against the parts it names; find the cap.
5. Only then compare against recorded frames, with `side_by_side.py`, and only as a check.
