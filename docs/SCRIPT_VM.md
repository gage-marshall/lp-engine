# LovePlus+ script VM (SBIN) — engine core spec

Status: VM fully specified and verified against all 1,304 scripts (92% of entries parse cleanly to a `ret`).
Command layer: all 526 commands traced by emulation (2 overlays loaded, guards/gates explored both ways); 99.7% of script calls
have a traced effect. Function *names* come from automatic labels and are sometimes wrong (e.g. `Gfx3D_*RenderSequence` are really map commands). Source of truth for each fact is noted in brackets.

## 1. Container
- `arc/tmx_event.arc` entries are `SBIN` files. Cipher (all of TXRC/SBIN): `plain[i] = c[i] ^ ((hdr[7]*(i//3) + hdr[4+i%3]) & 0xff)`, i = absolute file offset, applied from byte 8. [TextRes_GetEntryField4 @0x209e93c, Script_HashByteAtIndex @0x204c0ac]
- Decrypted header is big-endian: `+8` entry count N, `+0xc..+0x30` section offsets, `+0x40` N x u32 entry offsets. Padding between entries is junk. String table: length-prefixed (1 byte, or 2 if bit 7 set), addressed by `value & 0x7fffffff` relative to a section. [Script_ParseFileHeader, Script_ReadLengthPrefixedString]
- Tools: `tools/sbin.py`, `sbin_scan.py`, `sbin_dis.py` (disassembler).

## 2. Instruction set
Encoding: `[op u8][nargs u8][nargs x u16 BE]`. State: accumulator `acc`, flag, pc, value stack, registers, global vars, local vars. [Script_ExecuteOpcode @0x204cb48]

| op | meaning | op | meaning |
|---|---|---|---|
| 00 | nop | 40/41/42 | acc = reg/var/local[arg] |
| 01 | push acc | 43/44/45 | reg/var/local[arg] = acc |
| 02 | pop -> acc | 50-56 | b=pop,a=pop; acc = a+b, a-b, a*b, a/b (0 if b==0), a&b, a\|b, a^b |
| 03 | drop N | 57-5c | b=pop,a=pop; acc = a==b, !=, <u, <=u, >u, >=u |
| 10/11/12 | jmp / jump if flag / jump if !flag, rel s16 from next pc | 80 | string buffer[arg] clear |
| 13 | return (acc) | 81 | buffer[a] append table string (offset u32) |
| 14 | return by id | 82 | buffer[a] append string var |
| 16 | call by id (pushes return) | 83 | text commit / wait for text |
| 20 | flag = acc != 0 | 84 | mode-based text commit / route wait |
| 21 | flag = (acc == top of stack) | 85,86 | native no-ops |
| 22-2f,36-3f,46-4f | native no-ops | 87 | **CALL COMMAND** (id = u32 from 2 args) |
| 30/31 | acc = 0 / 1 | 88 | yield until fade idle |
| 32/33 | acc = imm16 (u16) / imm32 | 5d-7f | no-ops |
| 34/35 | acc = !!acc / -acc | | |

## 3. Command ABI (opcode 87)
Caller pushes args in order a0..a(k-1), pushes `k`, executes `87 cmd`, then `03 k+1`. Handlers read `stack[sp]=k`, `stack[sp+1]=a(k-1)` ... `stack[sp+k]=a0`; trailing args are optional (handlers branch on k). String args have bit 31 set (`0x80000000|offset`).
Dispatch: `GameFlow_RunMainLoop @0x205a8f4`, jump table base `0x205a928`, 553 slots, 526 implemented; every handler is a thin thunk.

## 3b. Script host + gates
The same VM/command table serves several scenes; handlers gate on `Overlay_IsLoaded(id)`: id 0x29 = ov41 Overnight, 0x26 = ov38 LoveplusMenu; with neither, scripts run in ov02 Skinship (external calls land in 0x2158280.. = ov02: 38/72 clean function entries vs <=4 in ov0/1/3/4/5). Spec shows `GATE ovNN: loaded=[..] | not-loaded=[..]`.

## 4. Decoded top commands (by use)
| cmd | uses | effect |
|---|---|---|
| 0x01 | 36.6k | wait: yield until no task of current kind is running (`TaskList_IsNoTaskOfCurrentKind`) |
| 0x0f | 34.0k | map transition by key a0 (`MapCtl_BeginTransitionByKeyWithMode8`) [task kind 0] |
| 0x34 | 31.1k | request place a0[,a1] (`MapCtl_RequestPlace`), guarded by a scene-state predicate |
| 0x5c | 27.2k | play voice clip (a0,a1) (`GameSnd_RequestVoiceClip`, third arg 500) |
| 0x44 | 20.3k | countdown wait a0 (task kind 0x22 -> `TaskList_FreeAfterCountdown`) |
| 0x5d | 15.7k | queue pending map action a0 (`MapCtl_QueuePendingAction74`) [kind 3] |
| 0x136 | 14.0k | set map field8 = a0 (`MapCtl_SetField8`), guarded |
| 0x55 | 11.3k | set game flag a0 (`GameFlags_SetScopedBit(a0,1)`) |
| 0x0e / 0x0d | 5.6k / 5.3k | fade in / fade to black, then wait for fades idle (kinds 0x18 / 0x17) |
| 0x06 | 4.2k | `Camera_SetPreset(a0)` [kind 0xb] |
| 0x02 | 3.0k | object/character setup by name (CHAR lookup, map setup) [kind 7] |
| 0x68 / 0x69 / 0x6a / 0x6c / 0x6e | 0.8-2.5k | BGM play / sound request / ambient seq / stop seq / stop ambient |
| 0x12 | 1.2k | `Scene_ApplyMapChange(a0)` [kind 0x21] |
Full per-command table: `fid/script_command_spec2.csv`; task kinds: `fid/task_kinds.csv` (116 kinds, 103 with traced subsystem calls).

## 4b. Command families (weighted by script usage, 249k calls)
| family | cmds | share | notes |
|---|---|---|---|
| TASK descriptors | 125 | 39.7% | handler builds `{kind, count, (type,value) pairs}` and calls `TaskList_TickTaskMain @0x206ce34` (11KB, 195 kinds): scene/map/camera/fade/wait actions, e.g. cmd 0xf = kind 0 map transition, 0x5d = kind 3 map action, 0x0d/0x0e = fades, 0x44 = countdown |
| guarded map-state commands | 9 | 18.1% | 0x34 `MapCtl_RequestPlace`, 0x136 `MapCtl_SetField8` (guard = scene-state predicate @0x20d34d8) |
| sound (GameSnd_*) | 20 | 16.1% | 0x5c voice clip (a0,a1), 0x68 BGM, 0x69 request, 0x6a ambient seq, 0x6c/0x6e stop |
| wait on tasks | 2 | 14.7% | 0x1 = wait until no task of current kind (the script `yield`) |
| game flags | 25 | 5.0% | 0x55 = set flag a0 (scoped bit), 0x59 = test flag |
| overlay-gated / overlay calls | 167 | ~4.3% | handlers that depend on overlay code |
| other (calendar, rand, clock, event, save) | rest | <1% | |

## 5. Reproducing / tooling
- `tools/script_cmds.py` (capstone) -> `fid/script_commands.csv` command table (handler addr/size/callees).
- `tools/cmd_usage.py` -> `fid/script_cmd_usage.csv` (uses, files, argc histogram).
- `tools/emu_cmds.py` (unicorn) -> `fid/emu_cmds.json`: runs the real handler with tagged args (a_i = 0x1001+i; strings 0x80001001+i), recording calls and the task descriptor. `tools/cmd_spec.py` -> `fid/script_command_spec.csv`.
- Pipeline: `tools/emu_core.py` (Emu class, overlay loading), `tools/emu_run.py OUT.json 2,41` (stage 1 command handlers x argc 0-8, guard/gate variants; stage 2 task descriptors through TaskList_TickTaskMain), `tools/cmd_spec2.py` -> `fid/script_command_spec2.csv`, `fid/task_kinds.csv`.
- Known gaps: 22 commands (0.28% of uses) still fault in emulation; arg *types/ranges* (e.g. what a1 of the voice clip is) and the per-frame tick functions' internals are not yet decoded; ov38 as host not emulated.

## 6. Content layer (dialogue, voice, text variables)
- **Dialogue lives in each script's string table** (header `+0xc`, length-prefixed UTF-8; 1-byte length, or `0x80|hi` + `lo`). Op `80 b` clears text buffer b; `81 b off32` appends a table string; `82 b var` appends string-variable `var`; `83` commits the box (waits for input), `84 arg` commits with a mode arg. Buffer 0 = message text, buffer 2 = speaker name, buffer 1 = sub label. [Script_ExecuteOpcode cases 0x80-0x84]
- Corpus (tools/export_dialogue.py -> work/dialogue/*.txt): **42,355 lines in 1,236 scripts, 23,763 voiced**. Speakers: player `{v0}` 16.8k, 姉ヶ崎 8.1k, 高嶺 8.1k, 小早川 8.0k, then a long tail (店員, 仲居, 女子Ａ ...).
- String variables `{vN}` (27 distinct ids; v0 = player name) are runtime strings in the shared data block (set by commands via TextVar_* code). A port needs an id -> string table plus the game's own `[p-name2]`-style placeholders inside TXRC text.
- Variables/regs: ops 40-45; global and local variable defs are typed cells (`type<<28 | size/bit<<24 | offset`) into backing data blocks. The extracted corpus has 33 local declarations across 12 scripts (other local indices read as zero and ignore writes); the tracer models cell widths and bits rather than inventing a dictionary. Most branching uses query commands that return into `acc` (206 commands, e.g. flag tests, rand, date).
- **Voice**: cmd 0x5c(id, a1) / 0xfb. `sys/Siren14Voice.vtb` = 40,555 x (offset u32, size u32) entries, **both scaled x16** (real = value/16; sizes sum to exactly 16x the .spp size); clips are Siren14 (G.722.1C) data in `sys/Siren14Voice.spp` (221MB). `lipsync/lipsync_add.dat` = 40,554 x (offset,len) into `lipsync/lipsync.dat` (mouth-shape key bytes per clip). Player lines are unvoiced.
- **Reference VM**: `tools/lpvm.py` (Script.run(entry) generator yielding cmd/text/wait/end to a host). Verified: all 16,332 entries run under 2 query policies with 0 bad opcodes, 0 faults, ~99.4% reach `ret` (rest are polling loops); 41,026/41,042 jumps land on instruction boundaries.

## 7. Script directory and event addressing
- `tmx_event.arc` entry 899 (130KB) is the **master directory**: its header id table (`+0x2c`, count `+0x30` = 16,332 = total entries across all scripts) holds sorted `(event_id u32, ref u32)` records. `ref = slot<<24 | script_id<<12 | entry_index` with slots 1/2/3 (1,854 / 692 / 13,786 entries). The loader maps `(slot, script_id)` through four static ARM9 pool records at `0x02128868`; each is a `(u16 tmx_event.arc_entry)[]`. The extractor writes this as `scripts/resource_map.json`, which resolves all 16,332 directory records to valid SBIN entries. `tmx_bbin.arc` is unrelated to this indirection and remains undecoded.
- `tools/event_trace.py CONTENT_DIR EVENT_ID` is the intentionally minimal reference host: it resolves this mapping, follows opcode-16 calls through the master directory even when they enter another SBIN resource, models opcode-14 deferred return-by-ID handoffs and root-return packed-reference handoffs, logs bytecode-visible commands/dialogue/fade waits, and reports repeated native-command loops instead of pretending those commands are implemented. `--reply COMMAND=VALUE` supplies an explicit, recorded command result; `--text-reply MODE=VALUE` supplies the one-based result placed in register 0 by a mode-based text choice. For example, `0x08000200` resolves to `1303.sbin:0` and reaches two player dialogue boxes before returning; `0xa063f380 --reply 0x3e=1` follows its accepted timed-event path, including a call to `0378.sbin:2`.
- Event ids look hash-like (bits 7-30 high entropy; bit 7 and the low 0-6 bits are small selectors; top nibble in {0,5,8,9,10}). They are computed by native code (EventPick/CalendarRule) from date/heroine/state. **Open work for a port: reproduce that selection logic from the overlays**; scripts themselves are pure content.
- Scripts contain almost no choices/stat changes (choice cmds ~41 uses): affection/stat rules are native.
- Catalogue: `fid/script_index.json` (per script: entries, bytes, dialogue lines, voiced, top speakers).

## 8. Where event selection lives (native, not scripts)
`EventPick_*` @0x20ba1ac-0x20bb9c8 (BeginForSlot, ApplyElapsedTime, ChooseByTimeOfDay, RouteByKind -> RunNormal/RunSpecial, ApplyDefaultEvent, SelectEventId, BuildCandidateList, GetTimeVariantEventId, EventId_IsSpecialEvent/IsPoolExcluded) choose small native event numbers (e.g. 0x4d/0x4e/0xf2/0xf3 per-slot defaults, 0x69/0xbb/0x110 slot tracks) from date, hour, season, slot and flags, then `FlowState_SetEventPair`; `Script_Begin` receives the resolved ids. Reimplementing event choice for a port means porting this module (about 7KB of ARM) plus the Calendar/Schedule/RandEvent helpers it calls; the 32-bit ids in the directory are derived from these numbers via tables (not yet traced).

## 9. Native event lifecycle (verified end to end)
1. **Select**: `EventLogic_SelectEvent @0x20a3dd0` switches on the event *kind* (0..10) and stores a 32-bit script id into the event context (`*ctx+4`). Kinds read helpers or static tables: 0 `CheckScheduleSlot` (sets flag 0x20, records month/day), 1 `CheckScheduleSlotSimple` (0x40), 2 schedule slot (0x20|0x80), 3 id table @0x21067f0 indexed by current heroine slot (0x40|0x80), 4 `CheckTimeEligibility`, 5/6 tables @0x21067fc / 0x2106808, 7 current slot, 8/9 `CheckEventFlagA/B`, 10 `GetSlotScheduleValue`. Other triggers: `EventLogic_Trigger*Event` (Saturday/Sunday night 0x3f, day 16 0x29c, day 29 0x2a1, seasonal, holiday, milestone) and `EventPick_*` choose the kind/number from date, hour, season, slot, flags and weighted random rolls (`EventRoll_RollOutcomeByWeights`).
2. **Start**: `GameClock_SaveState @0x205e7c4` (really `ScriptMgr_Start(id,...)`) begins the script now or queues it behind the running one; `GameClock_LoadState @0x205e5a0` (really `ScriptMgr_Update`) runs `Script_Run` each frame and starts queued scripts.
3. **Resolve**: the id is looked up in the master directory (script 899): ids in the native tables are exactly its keys (checked). Id structure: `id = key + heroine_offset` with offsets `0xa0000000 / 0x58000000 / 0x08000000`; the directory holds 4,979 keys x 3 heroines = 14,937 ids plus 1,395 other-class ids (e.g. 586 with top byte 0x80).
4. **Outcome**: `EventLogic_RunEventStateMachine @0x20a40b8` runs the script, then applies outcome by kind: `Stats_ApplyGrowth_Ofs0x2b4..0x2b7` (four stats), `EventMeter_AddClamped_Ofs0x303/0x304`, save-log entries, flags, fades. Event numbers 1..1028 index `mail/condition.bin` (1,028 x 16B records; u16 fields at +8/+10/+12) and `mail/sendinfo.bin` (490 x 3B; two byte deltas feed the event meters).

## 10. Voice audio (verified)
- Voice id (script cmd 0x5c a0 / 0xfb) = clip index into `Siren14Voice.vtb` (40,555 x (offset,size) u32 LE, both x16); clip = `Siren14Voice.spp[off/16 : (off+size)/16]`. All real sizes are multiples of 40 bytes.
- Codec: the game's own **G.722.1 / G.722.1C decoder** (`G7221_*` @0x2000c64-0x20020d0 in the static binary, tables incl. expected_bits_table @0x2113c10). Configuration used by the voice bank: **16 kbps (40 B per 20 ms frame), bandwidth 14000 (28 regions, 640 samples/frame), 32 kHz mono, frame bytes read as-is (not byte-swapped)**. API: `G7221_InitDecoder(state[0xfb0], cbtab{ctx,..,fn@+0x10}, bitrate, bandwidth)`; `G7221_DecodeFrame(state, out_s16[640], 0, 0)` pulls the frame pointer from the callback.
- `tools/siren_emu.py` runs that decoder in Unicorn as a bit-exact oracle (~9 s per 1 s of audio; fine for validation, not for playback). Parameters confirmed by listening (correct pitch/speed/text, high quality); spectrogram of clip 0 shows clean harmonic speech; durations match script text (e.g. voice 23099 "うん、ちょっと。" 2.16 s).
- **Clean C port: `tools/siren14dec.c`** (about 250 lines; `clang -O2 -o siren14dec tools/siren14dec.c`; usage `siren14dec ARM9_IMAGE VTB SPP CLIP out.wav [bitrate bandwidth]`). **Bit-exact against the game's decoder (0 mismatches over 160k+ samples on 6 clips)**, 0 error frames over a 300-clip random sample, fast (300 clips in <1 s). It reads the lookup tables (expected-bits, Huffman/region-power trees, centroids, DCT twiddles/signs, 10x10 matrix, windows) from the user's own decompressed ARM9 image at the game's addresses (0x2113c10, 0x2115110, 0x2115b60, 0x2113c20, 0x2113d74, 0x2112e60.., 0x2113008, 0x2115be0/0x2115e60, ...), so no proprietary tables live in this repo; a port ships a one-time table dump from the user's ROM. Decoder state must be reset per clip (the game does); carrying state across clips changes the output.
