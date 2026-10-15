# The runtime: the game running on the engine (`engine/src/runtime`)

The game boots and plays as on the DS: power-on → Konami logo → title → title menu → file select → room, and in the
room it takes taps (寝る advances the day). Every function the engine has ported runs natively; every other function
runs as the game's own code in an ARM interpreter. The hardware is the engine's: memory map, 2D/3D graphics, card,
touch, keys, real-time clock, sound.

## Layers

| Layer | Files | What it is |
|---|---|---|
| Machine | `mod.rs` (`Machine`, `Game`) | RAM, input, clock (`tick`, OS ticks of 64 bus cycles), backup chip, PXI state, diagnostics |
| Memory | `ram.rs`, `vram.rs` | Main RAM (mirrored to 0x2ffffff), ITCM / DTCM, palettes, OAM, banked VRAM (VRAMCNT), I/O registers (divider, sqrt, DMA, GX FIFO, card bus, GXSTAT) |
| Startup | `ram.rs`, `Game::power_on` | the ARM9 program as the card holds it (`content/fs/arm9.bin`) loaded at the header's address; the firmware's header copy, boot type and card ID; then the game's own crt0 (unpacking, autoloads, bss) and `_fp_init`, `NitroStartUp`, the static initializers and `Main_Startup` run as game code (`tests/boot_crt0.rs`: crt0 unpacks exactly the extractor's image) |
| Frame | `Game::frame` | a port of `Main_UpdateFrame` 0x203d9d4's loop body (input, game state update outside 3D-capture phases, graphics, fades, display capture) |
| Registry | `ext.rs` | every function the engine answers: `Native` (ported, `natives.rs`), `Subsystem`, `Pending` (runs as game code) |
| Interpreter | `interp.rs`, `../arm.rs` | ARM / Thumb execution of game code; traps to the registry; BIOS SWIs; SDK threads (`OS_SaveContext` / `OS_LoadContext` on the interpreter's registers, `OS_Halt` → next alarm or VBlank); interrupts delivered through the game's own IRQ table |
| Hardware edge | `sdk.rs` | card ROM reads over `content/fs` (`VirtualRom`), backup chip, PXI, RTC, touch panel, keys, power manager |
| Graphics | `gx.rs`, `raster3d.rs`, `ppu.rs` | geometry engine, 3D rasterizer, both 2D engines, display capture |
| Sound | `snd7.rs`, `../snd.rs` | the ARM7's sound driver (below) |
| Checks | `check.rs`, `tests/runtime_live.rs` | natives replayed against the game's call logs |

Time: interpreted code advances `tick` by one per `INSTR_PER_TICK` (85) instructions. VBlank comes every
`TICKS_PER_FRAME` (8753) ticks; time slices end at the next VBlank so its interrupt can preempt busy code.

## Hardware facts measured, not assumed

- **Touch panel, pen up.** A released sample keeps the last pressed x/y with `touch` = 0; it is not (0, 0). Measured in
  the emulator: after a hold at (200, 100) the game's touch block (0x213cae4) shows raw x/y (201, 101) with touch 0
  on the following frames. It matters. `Input_SwitchMode` 0x2040b58 copies x/y whenever validity is 0, and the room's
  release hit test runs `TouchCtl_Update` a second time in `Ov35MainScene_UpdateMainSceneTouchInput`, on the
  position that sample gives. With (0, 0) every room tap missed. (`Ov35MainScene_NopStub_4` is mislabeled: it is
  the room's tap handler.)

## The ARM7 sound driver (`snd7.rs`)

The ARM7 binary is not run. Its sound driver is implemented against the protocol the game's ARM9 code uses:

- **Commands.** PXI tag 7 carries the address of an `SNDCommand {next, id, arg[4]}` list, or 0 for "run what you
  have". The driver runs each list and then adds one to `finishCommandTag`. Each command's argument packing is read
  off the game's builders (`SND_*` 0x2023c24..0x2024054). `SND_SetupChannelPcm` packs `chNo | timer << 16`, data,
  `loopLen | shift << 22 | volume << 24` and `loopStart | pan << 16 | format << 24 | loop << 26`. `SND_SetupAlarm`
  packs (alarm, tick, period, id). The timer argument is the sample period (`SND_TIMER_CLOCK / rate`), not the
  register value.
- **Alarms.** These come back to the ARM9 as `alarm | id << 8` through the game's PXI callback (`PxiFifoCallback` →
  `SNDi_CallAlarmHandler`). The audio up to an alarm's due time is rendered before its handler runs, which is the
  order on the hardware.
- **Data.** The driver reads sequences, banks and waves from main RAM where NNS loaded them. A bank (`SNDBankData`,
  the SBNK with wave archive links at +0x18 + 8k) and the waves its archives hold are copied when a sequence is
  prepared. A wave archive's offset table (+0x3c) holds 0 (not loaded), an offset from the archive, or the
  address of a wave loaded on its own (`SND_GetWaveDataAddress` 0x2024e9c). `INVALIDATE_SEQ / BANK / WAVE` stop the
  players using that memory and drop the copies.
- **Sequencer.** This is `snd::Driver` through its per-player interface (`start_player`, `set_player_param`,
  `set_track_param`, `set_allocatable`, …). Player parameter offsets are the SDK's `SNDPlayer` fields (prio +4,
  volume +5, extFader +6, tempo ratio +0x1a). Track parameter offsets are `SNDTrack` fields (pan +8, ext_pan +9,
  extFader +0xa, ext_pitch +0xc). Locked channels (`LOCK_CHANNEL`) are never allocated to sequences.
- **ARM9 channels.** Channels set up with `SETUP_CHANNEL_PCM` play from main RAM as the hardware does (PCM8,
  PCM16, IMA-ADPCM with the loop-start state). They start at `START_TIMER` with their phase aligned to the output
  clock, so a refill alarm (period = one ring quarter) always comes after the quarter has played.
- **Shared work.** `playerStatus`, `channelStatus`, player variables and tick counters, and global variables are
  written after every update.
- **Driver info.** `READ_DRIVER_INFO` fills the layout `SND_ReadTrackInfo` 0x20248cc reads: players at +0x540
  (0x24 each, tracks[] at +8), tracks at +0x780 (0x40 each), channels at +0 (0x54 each, nextLink +0x50),
  workAddress at +0x11c0.

What the game sends, from power-on to the room: prepared sequences (title and room music, sound effects), player
and channel parameters, track channel masks, invalidations, a driver-info read every update, and one stream. The
title's heroine voice is decoded by the game's own G.722.1 decoder (`G7221_*`) inside the alarm handler into a
PCM16 ring of 0x500 words per channel. It plays on channels 8 and 9 at period 0x200 (32,728 Hz), and the alarm fires
every 0x2800 ticks, which is 640 samples, one codec frame, a quarter of the ring.

**Checked** (`tests/sound_live.rs`, from power-on to the title with the session save):
- Every bank copied from RAM equals an SDAT bank, and every wave its instruments use decodes identically.
- The stream the driver played is a voice clip of `Siren14Voice` decoded with `siren.rs`, identical sample for
  sample and in order (e.g. clip 22584, 153,658 samples, until the game stopped it).

**Not done:** capture (`SETUP_CAPTURE`), PSG / noise channels set up by the ARM9, `SKIP_SEQ`, surround and output
selection, stopping sounding notes on `INVALIDATE_WAVE` (later notes cannot use the dropped data). The driver info
leaves out channel, alarm and per-channel fields the game does not read.

## Binaries

- `game CONTENT [--save BACKUP.sav] [--scale N] [--res N] [--smooth] [--cpu] [--mute]`: a window with both screens
  like a book (kept at the DS's proportions in any window size). `--res`, `--smooth` and the View menu are the
  presentation (below); `--cpu` draws its 3D on the CPU. The mouse is the
  stylus; keys are listed in the file. Sound plays on the default device at its rate. Frames are paced by the game's
  clock (`tick` against wall time), so overrun frames take longer, as on the DS, and audio is produced as fast as
  it plays. The battery save is written back when the game saves.
- `game_probe CONTENT FRAMES [...]`: headless. Time is counted in VBlanks (`tick / TICKS_PER_FRAME`), as the
  emulator counts frames: a game frame that overruns takes two, so a script, screenshot or peek at frame N lines up
  with an emulator run at frame N. `--script JSON` (emulator-driver steps: `wait`, `tap` held 4 then released 20,
  `drag` points held 2 each then released 8), `--save` / `--save-out`, `--rtc Y-M-D-h-m-s` (the clock starts there
  and runs with game time, for repeatable runs), `--shot`, `--peek ADDR,N,FROM,TO`, `--break ADDR`, `--break-frame
  F` with `--watch ADDR` (`LP_WATCH_SKIP=n` lets n watched writes through, to reach the n+1-th writer),
  `--snapshot`, `--trace`, `--wav PATH` (the driver's output) and `--check-sound` (the sound test's checks,
  printed). `LP_SND_LOG=1` logs sound commands (`all` includes driver-info reads). `--res N`, `--smooth`,
  `--look bilinear,toon,thick,aa` and `--gpu`: shots as the game binary presents them. `--gpu-check`: every swapped
  3D frame drawn by `raster3d` and `gpu3d` at the DS resolution and the pixels compared.
- `examples/save_diff A.sav B.sav`: every save structure of two battery saves unpacked and compared byte by byte.

## Pace

In the heroine scenes the game runs 3 main-loop passes per 4 VBlanks, in the emulator and in the engine. This is
the game's own pacing, not load: `VramBank_Process` 0x2044ee8 waits for VBlank interrupts until the count at
*(0x213d2e4 + 0xa1) reaches the target at +0xa0, which is 2 while +0xa3 is 2 (the pass that draws the heroine and
captures her) and 1 otherwise. That pass runs about 280k instructions, well inside one VBlank. The heroine is
therefore redrawn 15 times a second on the DS. The game binary paces by `tick`, so it plays at the DS's speed.

## Presentation (`present.rs`, `gpu3d.rs`)

What is shown beyond the DS, without touching the game's memory or timing (1x with no options is the DS's picture):

- **Resolution** N x: the 3D layer is drawn at N x. Each display capture the game makes also gets a finer version,
  made from the same registers and the finer 3D layer. Wherever the game shows captured pixels 1:1 (bitmap OBJ,
  VRAM display, unscaled direct-colour BG) and they are still what it captured, the finer version is drawn; the
  heroine is shown this way. 2D art is scaled up.
- **Smooth motion**: when two successive captures come from the same polygons, the halfway geometry is drawn and
  shown while the game holds its draw pass, so the heroine moves 30 times a second, 2 VBlanks behind.
- **3D options** (`raster3d::Look`): bilinear textures, smooth toon shading, outlines at DS thickness, 2 x 2
  anti-aliasing blended against what lies behind.
- **GPU** (`gpu3d`): the same polygons (`raster3d::prepare`: ordered, clipped, projected and culled on the CPU) drawn
  by a shader that follows `raster3d` line by line (texture formats decoded from the texture slots, polygon modes,
  toon / highlight, alpha test). Translucency over pixels with alpha 0 is written, not blended, as on the DS: the
  stencil marks pixels drawn on, and a translucent polygon is drawn in three parts (blended over drawn pixels, its
  full-alpha pixels, its pixels over nothing). Edge marking, anti-aliasing reduction and RGB555 packing are one pass.
  The game's own captures are always drawn by `raster3d`.
- Checked (`--gpu-check`, title with the heroine, 508 frames, 25M pixels): coverage differs at 122 pixels and colour
  at 3,545 (1,649 by more than one step), all single pixels on shared triangle edges, where the GPU's fill rule
  gives the pixel to the other triangle. Cost per pass at 3x with all options: CPU 77 ms on average, GPU 14 ms.

## New game against the real game

`tests/newgame_live.rs` (ignored by default; about 3 minutes) plays a whole new game from an empty battery save:
clock setup, the profile written by hand (the game's own handwriting recognizer reads the strokes), the
prologue with its two choices, the tutorial, then a save from the room. The input is
`tests/data/newgame/script.json`, timed in VBlanks. The emulator ran the same script from power-on and showed the
same screens. Its save matches the engine's in every structure (settings, game state, header) except GameState
+0x180, where `GameState_Load` 0x20632f0 stores `OS_GetTick()`. The emulator shows `GameState_Load` running at the same
frames as on the engine (33, 1513, 2040). The OS tick starts when the game's `OS_InitTick` starts hardware timer 0,
after crt0 has unpacked the program. That takes about 21 frames on the DS and 13.4 on the engine, whose flat
instructions-per-tick estimate makes crt0's memory-heavy unpacking fast, so the engine's stamp is about 10 VBlanks
higher.

## Native ports checked against the game while it runs

With `LP_SHADOW` set (always in `tests/newgame_live.rs`), every call of a native getter (the pure `*Get*`, `*Is*`,
`GameFlags_Test`, `TimePeriod_FromHour` ports) is followed by the game's own function, interpreted on the same
state, and the results are compared. Over the new game that is about 690,000 calls of 26 ports, and all agree.
Calls during which an interrupt was taken are set aside. The replay check (`tests/runtime_live.rs`) prints
disagreements too, but it runs calls outside their real context (one power-on log alone shows 1,586 faults), so
its counts are mostly replay artifacts. The shadow check is the one the ports are held to.

## Bugs found by running the game

- **Arguments passed by address.** The flow port passed `Arg::Any` (0 when live) where the game passes a stack
  struct. That was harmless in the log replay, which does not compare those arguments, but live it crashed the
  first scene (`Mod2049a04_ApplyMapSelection` writing through 0). All such calls now pass the struct as an
  `Arg::Out` buffer built from the values the game puts there: the map selection {u16 ident, s8, s8}, the date for
  `Schedule_TestSecondaryDateAndFlag`, the clock block {year, month, day, weekday, hour, minute, second} for
  `Schedule_GetDateEntry` / `Schedule_GetFilteredCount`, and the colours for the `GX(S)_LoadBGPltt` calls.
- **The clock getters.** Following a mislabeled decompile, the ports read the RTC copy at the wrong offsets and
  skipped a branch. Each getter (hour 0x205f2ac, year, month, day, weekday) picks the manual date, the clock
  block's own date (when its +0xc flag is set, or when flag 0x8000000 is set and GameFlags field 0xe0 is 0), or
  the RTC copy. On the RTC path the hour is shifted by an offset in seconds and the weekday by a day offset. The
  shadow check caught the first call of the boot, where the old port answered month 0 and the game answered 10.
- **The OS tick's origin.** `OS_GetTick` counted from power-on. On the DS it counts from `OS_InitTick` (timer 0
  started at bus / 64), and the SDK's alarm times are in that unit. It now counts from the timer start, and the
  idle loop converts alarm times back.
- **Interrupts with IME or IE off.** The engine still ran the SDK's thread wake-up when the master enable or the
  source was off. The DS takes no interrupt then. Running crt0 exposed this: a VBlank arrived while the program was
  still packed.
- **Saving.** `MI_CompressLZ` runs in one call for well over 20 million instructions; the per-call budget is now
  4 billion.
