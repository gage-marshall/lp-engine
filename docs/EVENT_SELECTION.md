# Event selection: verified findings (work in progress)

Method: user played a week with `tools/play_monitor.py` (exec hooks -> `tools/emu_scripts/states/logs/*.jsonl`, git-ignored), cross-read with Ghidra decompiles. Function names in `fid/labels_all.csv` are agent guesses; several modules are mislabelled (see "Misleading names").

## How a script id reaches the VM
`ScriptMgr_Start(id)` (0x205e7c4) is called from `Mod2049a04_RunMainStateMachine` (0x204a0c4, state 0x16) with the id from `MapArc_GetActiveCharIdAndMode(0, &bgm, &mode)` (0x20703d0). That function returns, in priority order:
1. a pending "global char id" (`CharId_GetGlobalValue`), else by `GameFlags`:
2. flag `1` set: **text route**: `SceneRoute_SelectTextRoute` (below), mode 1;
3. flag `0x100000`: `StateLogic_UpdateData_dd26c`, mode 0x10;
4. flag `0x100` (replay/gallery): `ItemDb_GetSelectedQueueValue`, mode 0x10;
5. otherwise the id stored by `MapArc_SetCurrentCharId` (struct +0x1c, mode +0x20; set via `MapArc_ApplyIdByKind` 0x20700dc, which resolves by kind: 2 `MapEvent_InvokeCallback/ApplyOutcome`, 3 `MapSelector_RunTextStateMachine`, 4 `Schedule_ResolveMapMessage`, 5 `MapEvent_GetByteField`, 6 `TextEvent_*`).

## Script id structure
`id = heroine_offset + key`; heroine offset = top byte pattern 0xa0 (slot 0, Manaka), 0x58 (slot 1, Rinko), 0x08 (slot 2, Nene); `CharId_DecodeSlotFromHighBits` recovers the slot. Class-0x80 ids are heroine-independent. Within a key, bits 7-8 are a variant (0..3), the low 7 bits an entry index, bits 8+ step by 5/50 (stage/threshold-like groups). Log: Manaka's first meeting started `0xa0006600`, Rinko's `0x58006600`.

## Text routes (decoded, `tools/event_routes.py`)
`SceneRoute_SelectTextRoute` (0x209f020): `kind = GameFlow field +4` (1..0x10) selects one of 15 tables; row = current heroine slot (0x10 bytes), column = stage from `TextRes_UpdateSlotState` (0x209ef54): slot 0: save value 0->2, 2->1, else 0; slots 1,2: 0->2, 1->1, else 0; column 3 when not in slot mode (`GameClock_GetCurrentSlot(1)==0`). Tables hold the same per-heroine key with offset, e.g. kind 1 = group 0x0708 variants 0x80/0x100/0x180, kind 2 = 0x0708 +0x500, kind 4 = 0x070c (4 columns), kind 0x10 = 0x0730. Unmapped kinds (0, 0xa, 0x11, 0x12) return the default id. Still unknown: what writes GameFlow +4 and what each kind means (likely greeting / date / holiday style scenes).

## Afternoon encounters (from the play log)
`EventPick_RunNormal(kind 3)` -> `EventPick_GetTimeVariantEventId(code)` (switch on `GameFlags field +4`, season tables) -> `FlowState_SetEventPair(number)` -> `Scene_SelectRandomTransition`/`RandEvent_RunSequenceData` -> id. Numbers are per heroine slot (slot 0: 0x4d, 0x6a, 0x5a; slot 1: 0xa0; slot 2: 0xf2, 0xfc, 0x10c); default per-slot numbers in `EventPick_ApplyDefaultEvent` (slot 0: 0x4d/0x4e, slot 2: 0xf2/0xf3). Common class-0x80 scenes advance by 0x3200 per visit (0x80040500 -> 0x80043700 -> 0x80046900). The number -> key tables (heroine progression) are still to decode; candidate tables by code reference: 0x2107488 (TextEvent), 0x21075e8 (MapEvent text), 0x2109e10-0x2112530 (Schedule_*), 0x210a2c8-0x210a454 (MapEvent outcome/variant).

## The event engine (second play log, 2026-10-12)
All 21 script starts of a played week (7 days, 3 heroines) map to the same chain: `EventState_Initialize` (0x2056248, `MapArc_SetCurrentCharId` caller, return 0x2056284) asks the "session" for the current time period's queued event: `MicSession_ForwardProcess(timePeriod)` then `MicSession_GetField14` = **script id**, `GetField18` = mode/kind; continuations of a multi-part event come from `GameFlow_RunMainLoop` (return 0x205b730, ids +1: `08143401`, `08143402`...). `EventPick_*`/`FlowState_SetEventPair` are downstream bookkeeping (the number they log, e.g. 0x4d/0xa0/0xf2, is the heroine's default event number), not the producer. Heroine identity is the id's top byte (a0/a4 Manaka, 58/5c/5f Rinko, 08/0f Nene), confirmed against the player's markers. Observed pattern per day: 8h nothing or a shared opener, 12h `x44c1b00`/`x44c1600`-style scene, 15h `x0143400`..`x0143404` entries; class-0x80 ids (`80280000`, `802a3200`, `80286400`) are heroine-independent school scenes.

The `Mic*`/`MicSession`/`MicSequence`/`EventLogic_QueueEvent` names (0x20c0430-0x20c3300) are wrong (not microphone): this is the **event condition engine**.
- `condition.bin` row n (16 bytes, n = 1..1028) is the event definition; no script ids inside (checked: 0 of 1,028 rows contain a directory id).
- `MicSequence_Process(n, heroine, checkMask, kindFilter)` (0x20c237c) is the eligibility test: mask bit 8 = event already done (`Inventory_GetItemOwnedFlag(n)`: done-flags live in the item-owned bit array), bit 1 = context mask match (`MicSequence_DispatchMode`, 0x20c21ac, builds the context word below; it must contain the row's required bits), bit 2 = slot/period gate, bit 4 = timed/cooldown (`MicSequence_UpdateTimed`), bit 0x10 = another row field must be zero, `kindFilter` compared with row byte +0xe; rows flagged bit 28 need hour >= 7.
- Context word (`MicSequence_DispatchMode`): mode 0x200000/0x400000/0x800000 (field0x78/0xe0 states); relationship stage bits 1,2,4,8,0x10 (from `MicSequence_Initialize(heroine)`); mood byte of the heroine save record (0 -> 0x100, 1 -> 0x80, 2 -> 0x20); time period 0..5 -> 0x200..0x4000; special day 0x10000 else 0x8000; season 0x20000/0x40000/0x80000/0x100000.
- `MicSession_UpdateTimed` (0x20c1dd0) builds the candidate pool for a period (`WorkBuf_InitPool`, `GameFlags_ComputeSlotAvailability`, `GameFlags_ComputeSlotRanking`); the `EventLogic_Trigger*` functions queue event numbers into it (`EventLogic_QueueEvent` = `MicSequence_Process` + `CheckEventAllowed`), so they are story-relevant after all (the earlier "mail only" reading was too narrow: mail is one consumer).
- Still to decode: the `condition.bin` row fields (+0 u32 context mask, +4.., +8/+10/+12 u16, +0xe kind), the ranking/availability functions, and the event number -> script id mapping (the id appears in the session record at +0x14).

## Misleading names (do not trust)
- `EventLogic_Trigger*`, `EventLogic_QueueEvent/DispatchTrigger/Update` (0x20c7378-0x20c9954) = **mail/notification triggers** (event numbers 1..1028 = `mail/condition.bin` rows), not story selection. A separate `EventLogic_SelectEvent/RunEventStateMachine` (0x20a3dd0/0x20a40b8) does run scripts (kinds 0..10, tables at 0x21067f0..).
- `Overlay_*`/`OverlayState_*` (0x20ed790-0x20ef700) = **fashion parameter tables** (`fashion\fashion_{hair,cloth,coat,color,item,present}_param*.bin`, 6 tables x 3 heroines, ref-counted loads). `EventPick_BuildCandidateList/PickRandomVariant` pick outfits, not scenes.
- `Slot*/SaveSlot_GetSlotPointer*` are heroine-save-record field accessors.

## Next
Decode GameFlow +4 writers, the number -> key tables behind `MapEvent_*`/`Schedule_*`, the `GameFlags field +4` code (schedule entry), and the heroine save record (stage value `SaveSlot_GetSlotPointerA`). Validate against new play logs (hooks for `MapArc_*`, `MapEvent_*`, `DayEvent_*` are installed).
