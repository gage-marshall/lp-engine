# What still runs as the game's code (2026-10-14)

Measured, not recalled. `game_probe --profile OUT.csv` counts every instruction the interpreter runs per code
address (main RAM and ITCM) and attributes it to its function by `fid/labels_all.csv`; counts are set aside
whenever an overlay is loaded or unloaded, with the overlays then in memory, so overlay code is attributed to the
right module. Natives never run in the interpreter, so what the profile lists is exactly what still executes as
game code. `cargo run --release --example registry_dump` lists the registry.

Runs merged (7.1 billion instructions of game code):

| run | what it covers |
|---|---|
| title | power-on, crt0, title with the heroine (1320 frames) |
| newgame | the whole new-game script: clock, profile (handwriting), prologue choices, tutorial, room, save (42,100 frames) |
| room | demo save: title, file select, room on day 1 |
| sound | title options, sound settings, a bar dragged |
| sess1, sess3 | two later play-session saves loaded, dialogue tapped through (about 4,000 frames each) |

## The registry (what the engine answers)

| kind | entries | meaning |
|---|---|---|
| port | 81 | native ports of game functions, checked against the game while it runs (shadow check) |
| sdk-native | 46 | SDK / hardware edges: card, PXI, RTC, touch, power manager, DMA with interrupt, tick, microphone |
| subsystem | 65 | routed to an engine subsystem; `NoSubsystems` declines them all, so they run as game code |
| pending | 143 | named, not answered: game code |

Ported so far: the top-level flow (state machine, save select, screens, scene loop; `flow/`), GameFlags (11),
Screen (11), map archive (8), GameClock (8), Sprite (5), state stack (5), ScreenFade (4), GameState (4), overlays
(3), and single functions (save data, UI selection, projection, ...). Beyond the registry the engine does natively:
the frame loop (`Game::frame`), the ARM7 (sound driver `snd7`, touch, RTC, mic), the geometry engine (`gx`), the 2D
engines (`ppu`), the 3D rasteriser, the card and its FAT, overlay loading.

## What ran as game code

3,583 distinct functions: 2,836 static, 747 in overlays (of 15,499 labelled; 6,798 static, 8,701 in 44 overlays).

By label source, share of the game-code instructions:

| source | functions | share | what it is |
|---|---|---|---|
| sem (game logic, named by analysis) | 2,662 | 41.7% | the game's own code |
| sdk (TwlSDK) | 462 | 27.4% | OS (threads, IRQs, alarms, mutexes), MI (DMA, copies, fills), DC (cache), MTX/VEC/FX math, FS, GX/G3 command buffers, SND (ARM9 side) |
| nns (NitroSystem) | 206 | 17.3% | G3D model drawing and animation, G2D, sound library |
| runtime/donor/other | 133 | 11.3% | compiler runtime (`_ll_sdiv`, `_s32_div_f`, `strncmp`), LZ compression (`SearchLZ`, saving), OAM loading, glyph drawing |
| manual | 120 | 2.3% | hand-named game functions (text, sprites, clock) |

The heaviest single areas (share of instructions):

| area | functions | share | note |
|---|---|---|---|
| G7221 (voice decoder) | 27 | 17.3% | the engine already has a native decoder (`siren.rs`, identical output; `sound_live`) not used by the runtime |
| Sprite (+ CpuLoadOAM) | 77 | 13.1% | the game's sprite manager building OAM every frame |
| MIi / MI / DC / MemCopyBytes / Libc | ~50 | ~15.7% | memory copies, fills, cache maintenance |
| NNS / NNSi | 206 | 17.3% | G3D drawing (heroine and props), animation evaluation, G2D |
| OS / OSi | 112 | 10.5% | thread switching, interrupt dispatch, alarms |
| compiler runtime division | 2 | 4.5% | |
| LetterChar / DrawGlyphLine / GetGlyphIndex / Text | ~10 | 3.8% | text rendering into tiles |
| BgBuf / TileMap / Bg | ~35 | 2.7% | BG buffers |
| Input / Touch | 60 | 1.2% | input handling |
| GameSnd | 84 | 0.8% | the game's sound layer (sequences, voices, volumes) |
| Script | 42 | 0.4% | the script VM (the engine has a native one, `machine.rs`, trace-identical on 16,332 events, not used by the runtime) |

By number of functions the game logic is spread wide: profile and handwriting (ov06, 224), boot and save (ov00,
124), title menu (ov21, 117), Model (94), file select (ov27, 85), GameSnd (84), main scene (ov35, 80), Sprite (77),
GameState (73), GameClock (66), Gfx3D (64), StateLogic (56), DateUi (53), Input (50), GameFlags (47), Script (42),
time setting (ov42, 42), Schedule (41), MicUi (34), SaveSlot (31), VramBank (29), EventLogic (28), and more.

## Not reached by these runs

11 of 44 overlays ran (ov00, 01, 02, 06, 13, 21, 24, 27, 35, 42, 43); 33 never loaded: ov03-05, 07-12, 14-20,
22-23, 25-26, 28-34, 36-41 (dates and outings, mini-games, communication, gallery, card exchange, the large scene
overlays ov14/17/18/26 among them). Static code that only those scenes call did not run either. Everything there is
game code too: nothing in it is ported.

## Native code the engine has but the runtime does not use

From the earlier engine work: the Siren14 / G.722.1 voice decoder (`siren.rs`), the script VM (`machine.rs`,
`script.rs`), the NNS 2D / 3D loaders and the heroine assembly (`nns2d.rs`, `nns3d.rs`, `heroine.rs`), the
motion / eye / lip-sync / schedule simulation (`sim/`), the save format (`save.rs`), the SDAT player (`sdat.rs`,
`snd.rs`, used by `snd7`). Wiring these in as ports (each checked against the game's own code while it runs, as
the existing ports are) would take over the voice decoder (17% of game-code instructions) and the script VM.

## Ported since (same day)

| function(s) | how it is checked | time charged |
|---|---|---|
| G7221_DecodeFrame, G7221_ResetDecoderBuffers | the engine's decoder (`siren`), one per decoder state and channel; `sound_live` (the game's PCM ring equals a clean decode) | 171,744 instructions per 640-sample frame (measured) |
| MIi_CpuCopy16, MIi_CpuClear16, MIi_CpuSend32, MIi_CpuClearFast, MemCopyBytes, Libc_Bzero, DC_FlushRange, DC_InvalidateRange, strncmp, _s32_div_f, _ll_sdiv | `tests/natives_exact.rs`: the game's function and the native on the same memory and arguments, 200 randomised cases each (odd / unaligned sizes, null pointers, NULs, zero and extreme divisors): results and every byte equal | the game's loop, from its disassembly; within 10% of the instructions it ran (most within 2%) |

| NNSi_G3dFuncSbc_NODEMIX (the envelope / skinning matrices) | `natives_exact`: 300 made-up render states (1-6 joints, some cached, random weights, stack slots and matrices): memory, the envelope cache and every geometry matrix equal; `LP_CHECK_NATIVE=2004dcc` while the game runs: 57,631 of 57,631 calls in the room run identical | 83 + 256 a joint + 93 a joint not cached yet: exact |
| MTX_Concat44 | `natives_exact`: 200 cases (small, large, extreme elements; the result apart, onto a, onto b) | 216, 239 into b: exact |

| LetterChar, NNSi_G2dBitReaderRead (glyphs into characters) | `natives_exact`: 300 glyph parts (4 / 8 bpp, 1-7 bits a dot, clipped at every edge) and 300 bit reads; live: 442 of 442 calls in the room run | 55 + 25 a line + 11 a dot (4 bpp); 51 + 23 a line + 8 a dot + 10 a dot set (8 bpp); reads 13, 24 + the recursive read when reloading: exact |
| NNS_G2dApplyOamManagerToHW, NNS_G2dResetOamManagerBuffer, NNS_G2dEntryOamManagerOam, NNS_G2dEntryOamManagerAffine, CpuLoadOAMMain_ / Sub_ | `natives_exact`: 300 manager states (both engines, empty and full areas, the game's 'no affine' state, fast transfer), the buffers and OAM compared; live: 4,256 / 4,260 / 11,001 / 554 of as many calls | from the game's code per attribute and affine set, including the 3 instructions an empty area's check takes: exact |
| Sprite_CopyCellOamsWithFlip (the game's cell to OAM, flips, affine, screen clip) | `natives_exact`: 400 cells; live: 11,001 of 11,001 | 28 (+4) + 34 an entry; a moved one 73 kept or 48 / 50 / 53 / 55 by the screen test failing, 14 the move, 117 / 136 (double size) the matrix, +10 flipped: exact |
| Sprite_BuildPriorityLists, Sprite_DrawAll | `natives_exact` (lists, 100 slot tables); DrawAll live: 35,207 of 35,207 calls over the whole new-game run (`newgame_live` with LP_CHECK_NATIVE) | 2124 + 14 listed + 3 hidden; DrawAll 2449 + 5 a listed sprite + 5 an active one + 2 an active slot: exact |
| MI_UncompressLZ8 (standard and extended lengths), MIi_UncompressBackward (the packed overlays, in place) | `natives_exact`: 300 made-up LZ8 streams (every length tier, copies reaching before the output, a last copy past the size) and 300 backward decodes of any bytes (header lengths, growth, null, empty); live: LZ8 1,025 of 1,025 calls over the new-game run, the backward decoder 1 of 1 (the one packed overlay either run loads) | from the game's code per flag byte, bit, byte and copied byte, the cache lines after: exact |

`LP_CHECK_NATIVE=<address>` checks any native with side effects while the game runs: on every call the game's own
function runs first from a snapshot of the memory and the geometry engine, then the native from the same snapshot;
memory (the callee's stack frame aside) and the geometry engine's matrices are compared and the instructions
counted, and the native's run is kept (`game_probe` prints the tally). The vector matrix's translation row is told
apart: NODEMIX loads it from three words of stack garbage in the game (the native sends 0), and the hardware never
uses it.

The game's behaviour where the SDK documents none was taken from its code: a zero divisor gives `_s32_div_f`
(dividend, 0) and leaves `_ll_sdiv`'s dividend as the result.

Natives charge the emulated CPU time the game's code would have taken (`Machine::spend`), so threads keep their
pace: on the new-game run thread switches and interrupts stay within 0.7% of the interpreted run, and
`newgame_live` (the save against the emulator's, every port shadow-checked) still passes.

| new-game run (42,100 VBlanks) | game-code instructions | wall time |
|---|---|---|
| before | 4.63 billion | 140 s |
| + voice decoder | 3.82 billion | 117 s |
| + memory, cache, division, strncmp | 3.10 billion | 107 s |

The game's time on the room run (demo save, title to room, 2600 VBlanks): 172.6 million instructions of game code
now; `newgame_live` runs in 70 s (102 s before the G2D ports). `newgame_live` prints and asserts the tally when
LP_CHECK_NATIVE is set.

G3D after these: the rest is a long tail (NODEDESC 0.56%, MAT 0.34%, the draw loop 0.28%, VEC_Normalize 0.25%,
the GE buffer 0.22%, each other function below 0.2%); NODEMIX was 2.9% of all game code and 37% of the G3D share.

Left in G2D and the sprite layer: Sprite_BuildCellOamsTransformed (about 2% of the room run), NNS_G2dTickAnimCtrl,
NNS_G2dMapScrToChar256x16Pltt, Sprite_AdvanceAnimTime / UpdateAnim (each under 1%).

The room run after the decoders: 152.2 million instructions of game code (172.1 million before).

Next by weight (room run): the OS thread and interrupt machinery (about 17%), BgBuf_ClearScreenRect (4.6%),
MIi_DmaSetParameters (3.6%), MotionAnimTable_FindIndexByName (3.5%), the G3D tail (NODEDESC, MAT, the draw loop, the GE
buffer, joint animation: about 12% together now that the rest has gone). `game --profile OUT.csv` records what a played session runs, for the modes no script
reaches yet (dates, kiss, skinship, gift, mini-games, communication).
