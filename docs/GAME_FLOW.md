# Game flow: state machines, screens, overlays

How the game moves between its parts (title, menus, onboarding, the room, scenes, dates). Decoded from the static
code (Ghidra decompiles `fid/decomp/gsm*.c`, `msm.c`, and the disassembly where the decompiler was unsure) and
implemented in Python (`sandbox/flow/lpflow`, see the last section), which is checked call by call against the game
running in DeSmuME: 63 recordings, 311,719 frames, 5.9 million calls, no divergence.
Markers: **[S]** read from code, **[O]** observed in a trace.

Overlay names are the auto-labels' module prefixes; the ones marked `?` have not been confirmed on screen yet.

## Mechanisms

### The state stack [S][O]

Every frame `GameHeap_UpdateGameState → Main_ScopedFadeCheck 0x203fb20 → Mod204af08_RunGameStateMachine 0x204b0c0`.
Nested state machines share one stack: depth counter `0x2139cdc`, states `0x2139ce0` (15 words), per-level buffers
at `+0x40`.

| function | real meaning |
|---|---|
| `Main_IncStateDepth 0x203dba4` / `Main_DecStateDepth 0x203dbbc` | enter / leave a machine (each machine brackets its frame with these) |
| `Main_GetStateAtDepth 0x203dbd4` | this machine's state |
| `Main_FreeStateBuffer 0x203dbf0` (s) | **set this machine's state to s**; every deeper level is reset to 0 and its buffer freed |
| `Main_AdvanceStateSequence 0x203dc58` | state + 1 |

Depths in practice: 1 `Main_ScopedFadeCheck`, 2 the top machine, 3 the machine the top state runs (scene machine,
SaveSelect, ...), 4 below that (SceneLoop A/B, ...).

### The screen manager [S][O]

The menu-type screens run through one manager. The auto-labels call it `Sprite_*`; the names are wrong.
Two blocks: the manager's request block `0x2152ea4` (+0 run state 0 idle / 1 start / 2 build / 3 first frame /
5 running / 6 switching / 7 finished / 8 finished with the alternative result, +4 kind, +0xc screen id, +0x10 next,
+0x14 result, +0x18 alternative result, +0x1c entry argument, +0x20 the screen object, +0x24 switch pending), and the
running screen's own block `0x214fde0` (what the screen asks for: +4 next screen, +0xc the mode its frame returns,
+0x14 result, +0x1c alternative result, +0x24 / +0x28 switch / finish flags).

| function | real meaning |
|---|---|
| `0x2089d8c` `Sprite_UpdateAnimationFrame(kind, screen, arg)` | **request screen** `screen` (kind 0); `arg` is kept at +0x1c and read by the screen (the room uses it as its entry reason) |
| `0x2089e04` `Sprite_DispatchFrameState()` | **run the requested screen**; returns -1 while it runs, its result when it finishes |
| `0x208a994` / `0x208abd4` | load / unload the screen's overlay |
| `0x20882c8` (id) | a screen asks to **switch to screen id** (the manager unloads it and loads the next; mode 3) — the caller does not see a switch: the room opens the schedule, settings, ... this way |
| `0x2088624` (n) | a screen posts its **result** n (mode 4: the manager finishes and returns n) |
| `0x2088644` (n) | a screen finishes with the **alternative result** n (mode 5) |
| `0x208a1f0` / `0x208a200` (n) | read / set the **entry argument** (request +0x1c); the room resets it to -1 before switching away |

Screen id → overlay (`0x208a994`'s jump table and literals; `lpflow/gamecode.py` reads it from the game):

| id | overlay | | id | overlay |
|---|---|---|---|---|
| 0 | ov21 TitleMenu [O] | | 11 | ov06 Profile (handwriting) [O] |
| 1 | ov27 FileSelect [O] | | 12 | ov22 SelectMode ? |
| 2 | ov28 FileDelete | | 13 | ov07 GalleryOption ? |
| 3, 17 | ov35 MainScene (the room) [O] | | 14 | ov08 Calendar |
| 4 | ov34 Schedule | | 18, 19 | ov39 / ov40 MenuAim ? |
| 5 | ov33 Gallery ? | | 20 | ov42 TimeSetting [O] |
| 6 | ov29 Gallery ? | | 10, 15, 16 | none (static code) |
| 7, 8, 9 | ov31, ov30, ov32 Gallery ? | | | |

Every call site of the switch / result setters, with its immediate argument, is listed by
`sandbox/flow/scan_requests.py` (static + all overlays).

## The top machine `Mod204af08_RunGameStateMachine` (depth 2)

| state | does | exits |
|---|---|---|
| 0, 1 | VRAM banks, `Mod204af08_InitGameSystems` | → 2 |
| 2 | `Mod204af08_StepOverlayLoadSequence` (loads an overlay, 3 passes) | → 3 |
| 3 | waits for the input context | → 10 |
| 10 | load ov13 Title | → 11 |
| 11 | title screen (`0x216a9a8`) until it returns > 0; unload | → 0x1e |
| 0x1e | reset display; **request screen 0** (title menu) | → 0x1f [O] |
| 0x1f | `SaveSelect_ProcessScreen` (below): -1 → 10 (title) [O]; 1 → new game (flag 0x20000000) → **0x118** [O], else 0x20 | |
| 0x20 | continue: `UiChoice_DispatchInput`; load ov43, `0x216ac5c` (reconcile game time); unload | ≠0 → 0x122; else heroine set → 0x10e; else 0x118 |
| **0x28** | **the room hub**: runs the requested screen (normally 17, the room) and dispatches its result (next table) | |
| 0x32 | `UiScreen_RunStateMachine` until 1/2 | → room (arg 10) |
| 0x3c | `DebugMenu_Main` | 1 → 0x32 |
| 0x50, 0x51 | ov36 Web: init `0x216f07c`, run `0x216f110` until done; unload | → room (arg 10) |
| **0x5a** | **the scene machine** `Mod2049a04_RunMainStateMachine` (depth 3; plays story scenes, ov02) — exit codes below | |
| 0x5b | | → 0x5a [O] |
| 0x64 | ov37: run `0x216f858` until done; unload | → room (arg = saved entry) |
| 0x6e | `EventLogic_RunEventStateMachine`, then by event result: back into a scene (0x5a, map mode 0xe), a date (0xd2, map mode 0x14), or the room | |
| 0x82 | `Menu_RunOptionScreenState` (unused in normal play) | → room (-1) |
| 0x8c | `Settings_RunScreenState` | → room (-1) |
| 0x96, 0x97 | text bank swap; ov38 `0x21708ac` / `0x21709f8`: 1 → room (arg 100); 2 → schedule storage → 300 | |
| 0xa0 | `UiScreen_RunStateMachine`: 1 → room (arg 0x28), 2 → room (arg 0x32) | |
| 0xbe, 0xbf | ov20: run `0x216a920` until done; unload; clock fields reset | → 0x1e (title menu) |
| 0xd2 | after `GameClock_ClearRtcCache`: map mode 0x14 (date) → slot transition, fade to white → 0xd3; else room (flags) | |
| 0xd3 | fades done | → 0x5a |
| 0xdc | `Overlay_UpdateInitState` done; flag 0x1000000 (SaveSelect then starts at file select) | → 0x1f |
| 0xe6 | `UiScreen_RunStateMachine` (kind 4): script variable 0x26 = 0 / 1 | → 0x5a |
| 0xf0 | day change: schedule / calendar / random-event updates, flag (0, 0x1000) | → 0x28 |
| 0xfa | time-mode change: same family, schedule rho / upsilon by date | → 0x28 |
| 0x104 | `Mod204af08_ShutdownGameSystems`; the machine returns -1 | |
| 0x10e, 0x10f | request screen 0xc (ov22) and run it | → 0x118 |
| 0x118 | GameState +0x7a set → 0x119; else `UiInput_Reset` → **0x5b** (a scene) [O] | |
| 0x119 | clock synchronisation: → room (screen 17, -1) or 0x5b; `GameFlags_CallHookWithConst` set → room entry 0x82 | |
| 0x122, 0x123 | ov02 `DateScene_Initialize` / `DateScene_Update`; then ov43 `0x216a920` | → 0x124 |
| 0x124 | ov43 `0x216a938` done; unload | → 0x10e |
| 300 | `Schedule_ProcessStorage` done; progress unlocks | → 0x96 |
| 0x136..0x138 | ov41 Overnight: init (heroine), run `0x216acbc`, `0x216ace0`; unload | → room (saved entry) |

Every state finishes its frame with the same tail (clock, transform, calendar events, map touch, effects).

### The room hub (state 0x28): screen result → next state [S]

| result | → | | result | → |
|---|---|---|---|---|
| 0 | UiScreen(0) → 0x32 | | 8 | 0x8c settings |
| 1 | 0x5a scene [O] | | 9 | 0x96 ov38 |
| 2 | 0x3c debug menu | | 10 | UiScreen(1, period + 1) → 0xa0 |
| 3 | 0x50 ov36 | | 11 | 0xbe ov20 |
| 4 | reset, clock fields, request screen 0 → 0x1f (title menu) [O] | | 12 | save clock → 0xd2 (a date) |
| 5 | load ov37 → 0x64 | | 13 | 0xf0 |
| 6 | `EventLogic_SelectEvent` → 0x6e | | 14 | 0xfa |
| 7 | 0x82 option screen | | 15 | 0x136 ov41 |

The room (ov35) posts results 0, 1, 3, 4, 5, 10, 11, 13, 14, 15 and switches to screens 4, 5, 16, 17 itself
(call sites: `scan_requests.py`). Much of what the player does in the room never reaches the hub [O]:
- display / sound settings and the schedule are **screen switches** inside the screen manager (room → screen 0x10 /
  0xf / 4 → back to screen 17); the hub keeps running state 0x28 throughout;
- sleeping (寝る) and the next morning, and the room's scripted events, run **inside the room overlay** (two play
  sessions of 66k and 70k frames with events never left 0x28);
- saving (the save button, confirm, セーブが終了しました。タイトル画面へ戻ります。) ends with result 4 → title menu.

The room's frame (`Ov35MainScene_UpdateMainSceneClock 0x216f5e0`, the time-route mode) dispatches the selected action
(`+0x78`, roommenu.py) [S]: 1 / 2 / 3 / 4 → the room's own sub-modes 1 / 2 / 3 / 8; 5 → save checks, result 1; 0xb →
switch to the schedule (screen 4) or, on special days (flag field 0xe0), the calendar (screen 14); 0xc → result 3;
0xd → result 0; 0xe → switch to screen 5; 0x11 → result 9; 0x12 / 0x14 (go out) and 0x13 → finish with the
alternative result 0xc; 0xf, 0x10, 0x15, 0x1b, 0x1c → room sub-modes. Pinning every action → result down is part of
porting the room screen itself.

### Scene machine exit codes (state 0x5a) [S]

`Mod2049a04_RunMainStateMachine` (depth 3) returns, when its scene is done:

| code | when | top machine → |
|---|---|---|
| 1 | `GameFlow_GetStateField4() == 0xc` | UiScreen(4) → 0xe6 (a choice written to script variable 0x26, then back to 0x5a) |
| 2 | default end of a scene [O] | room (screen 17, arg = saved entry) |
| 3, 4, 8 | flags 0x80000 / 0x100000 / (0, 0x100) | room, or with flag (0, 0x10) the schedule (screen 4, arg 2) |
| 5 | map mode 0xb | `Overlay_LoadInit` → 0xdc → file select |
| 6 | time hook set, period not 0 / 5 | 0x96 (ov38) |
| 7 | no path in the scene machine returns 7 (dead case) | 0x136 (overnight) |

## SaveSelect (top state 0x1f, depth 3) and the title menu [S][O]

`SaveSelect_ProcessScreen 0x2074600` starts at 0 (or, with flag 0x1000000, directly at the file select 0x3d), runs
the title menu (screen 0) in its state 0x28 and branches on the result. The title menu's result is the button's
code mapped through ov21 table 0x217041c (`0x216b92c`); the button code is its description index (text bank 1
group 0 item 1).

| button (description) | code | result | SaveSelect state |
|---|---|---|---|
| ゲームを開始します (start) | 0 | 0 | 0x3c → 0x3d: SceneLoop A = **file select → time setting → profile** [O] |
| 表示設定 (display settings) | 5 | 6 | 0xb4: screen 16 (static) |
| サウンド設定 (sound settings) | 6 | 7 | 200: screen 15 (static) |
| データを削除 (delete data) | 7 | 8 | 0xdc → 0xdd: SceneLoop B (screen 2, ov28) |
| スタッフロール (staff roll) | 8 | 12 | 0x1b8 |
| 通信会話 (comm talk) | 10 | 3 | 0x78 |
| 自分の名刺 (own card) | 11 | 13 | 0x1cc |
| 友達の名刺 (friends' cards) | 12 | 14 | 0x1e0 |
| 名刺を直接交換 (card exchange) | 25 | 15 | 0x17c |
| すれちがい通信 | 26 | 16 | 400 |
| セーブデータ送受信 | 28 | 17 | 100 |
| セーブデータ引き継ぎ | 29 | 18 | 500 |
| ギャラリーデータ引き継ぎ | 30 | 19 | 0x230 |
| 夢イベント (dream events) | 16 | 5 | 0xa0 → 0xa1: `MainScene_RunStateMachine` |
| 告白までのイベント (confession events) | 17 | 4 | 0x8c → 0x8d: `SelectScreen_Update` |
| 旅行イベント (trip events) | 18 | 20 | 0x208 |
| イラスト (illustrations) | 19 | 10 | 0x118 |
| ギャラリー設定 (gallery settings) | 20 | 11 | 300: screen 13 |
| ご当地カプセル | 22 | 21 | 0x1a4 |
| ご当地キャラクター | 23 | 22 | 0x21c |
| (title menu timeout) | | 9 | 0x104: back to the title [O] |

### How a new game reaches the story [O]

1. Title menu result 0 → SaveSelect 0x3c/0x3d → SceneLoop A (depth 4) requests screen 1 (ov27 file select).
2. The file select switches to screen 20 (ov42 time setting), which switches to screen 11 (ov06 profile).
3. The profile posts result 2 (`Ov06ProfileDlt_RunProfileScreenStateMachine`) → SceneLoop A returns 2 → SaveSelect
   0x140 returns 1 with the new-game flag 0x20000000 set.
4. Top machine: 0x1f → **0x118** (GameState +0x7a clear) → 0x5b → **0x5a**: the scene machine loads ov02 and plays
   the opening scenes, then exits with code 2.
5. → room: screen 17 (ov35) requested, state 0x28. Room result 1 → the next scene (0x5a), result 4 → title menu.

Every single-player branch ends by requesting the title menu again with the page to open as its entry argument
(0 main, 1 settings, 2 communication, 3 gallery, 4 ご当地, 6 save transfer) and restarting at state 0 [S][O]. The own
card branch first plays a pending event when the save holds an event pair (`GameState_HasAsymmetricEventPair`: ov00,
0x1cf / 0x1d0). The wireless branches (comm talk 0x78.., card exchange 0x17c.., すれちがい 400.., save data 100.. /
500.., ご当地カプセル 0x1a4..) are not ported.

Input notes for driving it [O]: the title menu highlights a button on the first touch and selects it on a touch of
the highlighted button, but ignores touches for ~150 frames while the highlight animates; the file select needs
three taps (select the disc, arm the centre target, confirm).

## The Python implementation (`sandbox/flow/lpflow`)

| module | ports |
|---|---|
| `top.py` | `Main_ScopedFadeCheck` 0x203fb20, the top machine 0x204b0c0, the boot overlay sequence 0x204afdc |
| `saveselect.py` | SaveSelect 0x2074600 (title menu branches; wireless ones raise `NotPorted`), SceneLoop A / B 0x209f4cc..0x209f700 |
| `scene.py` | the scene machine 0x204a0c4 |
| `screens.py` | the screen manager 0x2089d8c / 0x2089e04 / 0x208a2b4 / 0x208a994 / 0x208abd4 |
| `names.py`, `gamecode.py` | game function addresses by label; literal-pool words and switch tables read from `content/engine` |
| `ctx.py` | `LogCtx`: replays a recording and checks the port against it |
| `sim.py` | `SimCtx`: runs the port without the game against a `World` (the state stack native, memory a dict) |

The ports are plain Python with all I/O through a context: `cx.call(address, args)` for every game function,
`cx.load` / `cx.store` for global memory, `cx.out(n)` for the stack buffers callees fill. Constants that live in the
code's literal pools are read from the game image; immediates are written as in the code (each one is checked by the
recordings, since they reach calls as arguments or memory as stores).

**Verification.** `calllog.py` hooks every BL / BLX site and every non-stack load / store inside the ported
functions in DeSmuME and records calls (target, r0..r3), returns, the stack buffers callees filled, and the memory
accesses. `check.py` / `verify_all.py` replay a recording through the port: the call targets must come in the same
order, the arguments the port knows must match, the loads and stores between two calls must match as a multiset (the
compiler orders them freely), and every ported function must return what the game's did. `explore_calls.py` records
while exploring: from a session frame or from power-on with a battery save, scripted taps, and *forced* results
(`force_result`: the next screen result; `force_ret`: a call's return value — recorded as `F site original forced`,
the port must compute the original). Planting a wrong argument or store in the port makes the check fail at the
exact frame and call site.

Recordings: the onboarding session and 7 more play sessions, the room's 13 menu slots, settings and save from the
room, power-on → continue, the 13 single-player title menu branches, all 15 room hub results, scene exit codes
1 / 3 / 4 / 5 / 6 / 7 / 8, the continue path with time to reconcile. States run (at least one frame each):

    Top_Run          0 2 3 a b 1e 1f 20 28 32 3c 50 51 5a 5b 64 6e 82 8c 96 97 a0 be bf d2 dc e6 f0 fa 118 119 122 123 136 137 138
    SaveSelect_Run   0 28 3c 3d 8c 8d a0 a1 b4 b5 c8 c9 dc dd 104 118 119 12c 12d 140 1b8 1b9 1ba 1cc 1cd 1ce 1e0 1e1 1e2 208 209 21c 21d 21e
    Scene_Run        0 a 14 16 1e 1f 32 33 3c        SceneLoopA_Run 0 14 28 3c      SceneLoopB_Run 0 14 1e 32
    Top_BootOverlays 0..6

Not yet run: top 0xd3, 0x104, 0x10e / 0x10f, 0x124, 300; the scene machine's sub-screens (0x28 clock widget, 0x29,
0x46, 0x50, 0x5a, 100); SaveSelect 0x154 (unusable save), 0xf0, 0x1cf / 0x1d0, 0x230.

**Running it without the game.** `simulate.py` plays a new game from power-on with a scripted world (screens finish
with chosen results, scenes last 10 frames, fades end at once): boot → title → title menu → file select → time
setting → profile → two opening scenes → room → back to the title menu, all decided by the ported machines; it also
lists the game functions the flow calls that the world left unanswered — the flow's interface to the rest of the game.

    .venv/bin/python sandbox/flow/simulate.py [--calls]
    .venv/bin/python sandbox/flow/calllog.py SESSION.jsonl OUT.txt          # record (about 3 min per 50k frames)
    .venv/bin/python sandbox/flow/explore_calls.py SESSION.jsonl FRAME SCRIPT.json OUT_DIR
    .venv/bin/python sandbox/flow/check.py OUT.txt                          # replay through the port
    .venv/bin/python sandbox/flow/verify_all.py DIR...

## The engine port (`engine/src/flow`)

The same machines in Rust, structured like the Python (one function per game function, every call through a context):
`top.rs`, `screens.rs`, `scene.rs`, `saveselect.rs`; `addr.rs` names the called functions; `code.rs` reads the
screen → overlay switch from the game image; constants come from the literal pools via `Rom`. The context trait `Cx`
(`external`, `enter` / `leave` for ported callees, `load`, `store`, `pick`, `not_ported`) is implemented by
`log::LogCx` (replay and check) and by the runtime's `GameCx`, which runs the game ([RUNTIME.md](RUNTIME.md)).

`tests/flow_logs.rs` replays every log in `sandbox/flow/logs/` (git-ignored; the recordings listed above): 63 logs,
0 diverged, 311,719 frames, 5,875,633 calls, 521,972 loads, 35,462 stores, the same totals as the Python check, in
about 0.6 s. Where the Python passed an untracked stack buffer (`GameClock_GetDate` in states 0xf0 / 0xfa), the Rust
passes it as an `Out` buffer, so the date words the game passes on are compared too.

    cd engine && cargo test --release --test flow_logs -- --nocapture

**Coverage gap found by mutation.** Changing scene exit code 6 to 2 is *not* caught: the exit codes reached by forcing a
return value (`force_ret`) prove what the top machine does with each code, but the scene machine branches that
compute each code are covered only where play reached that code naturally (code 6 never was). A changed hub
transition (result 0xd → state 0xf1) is caught at the exact frame and call site.

## Open

- The room screen's own machine (ov35): every action → result / switch, and the game state each reads. Known so
  far: `Ov35MainScene_RunMainSceneState` 0x216b65c switches on its state (+0x74); in state 2 taps go through
  `Ov35MainScene_UpdateMainSceneTouchInput` to the tap handler 0x216c800 (labelled `NopStub_4`, which it is not).
- What each overlay marked `?` actually shows; ov20 vs ov42 (both "TimeSetting"); ov22, ov37, ov38, ov41, ov43.
- What each menu writes (its outputs): watches on GameState / flags / save during each menu.
