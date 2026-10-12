# The 36 event resolver stages (all ported and verified)

`tools/stage_eval.py` is the executable specification of the 36 stage handlers of the event pipeline (table `0x210b8f0`,
see `EVENT_SELECTION.md`). It was checked against the real ARM handlers with `tools/stage_check.py`: the handler's own machine code runs in
Unicorn, every call that leaves the handler is answered by a deterministic mock game state (`tools/cond_mock.py`), and the Python port
reads the same leaves by name. Result: **0 mismatches over 38,304 mocked states per stage (36 stages), bit-exact (return value and result id)**, with 1-22
distinct leaf-call paths per stage (the rest of the paths are reached only by values the mock never produces: see "What the leaves hide").
`tools/pipeline_check.py` checks the two layers around the stages (`MicSession_UpdateTimed`, the session-mask builder, and `MicSession_Clear`, the
dispatcher) the same way. Stage tables the handlers index are dumped by `tools/stage_tables.py` (`content/engine/stage_tables.json`).

Conventions: `act` = activity code `session[5]` (from the player's schedule), `partner` = `session[4]`, `period` = time period 0..5,
`slot` = heroine slot (0 Manaka, 1 Rinko, 2 Nene), `stage` = relationship column 0..2 from `TextRes_ProcessSlotState` (`SAVE_RECORD.md`).
"guard3" = `Schedule_HasActivePrimaryState`, `Schedule_IsSecondaryState2`, `Schedule_IsSpecialDayActive` all clear. A stage returns 0 only to ask for a retry on the
next update (async loading); otherwise 1, with `*result` = script id (0 = nothing found, the pipeline continues).

| # | handler | kind | accepted `act` | conditions | result |
|---|---|---|---|---|---|
| 0 | Schedule_ResetIfIdle | 0x14 | any | - | never an id (touches the schedule state only) |
| 1 | ActionInput_StoreCheckResult | 9 | any | - | `UiInput_CheckAction`: a fixed id when counter +0xa0 > 98 or all three heroines have per-slot flag 0x400 |
| 2 | ActionInput_PollIfNotScheduled | 9 | any | no active primary/secondary schedule | `UiInput_Process(period, session)`: touch-driven pick over the three `Heroine_CheckMapNameList_Group4` statuses (tables 0x20b873c-0x20b8748, random start of 3) |
| 3 | Schedule_SelectEventResult | 0x1b | 0x71, 0x66 | main game, heroine = field 0x98 | primary state 2 and act 0x71: `stage3_q[slot][stage][counter != 1]`; act 0x66 in period 1: if the primary/secondary schedule date is today (or states 4/5) `stage3_f_a/b[slot][stage]` |
| 4 | (0x20f890c) | 0x1c | 0x72 | main game, secondary state 2, period 1 or 3 | probabilistic (threshold table on the heroine's value, +10 when `sub_20f8200`, `Rand_NextLcg32(100) < p`): `stage4_final[slot][stage][sub_20f81b4 == 0]` |
| 5 | Schedule_FilterDateEntries | 0x1a | 0x6b (0x66 with low-five flags) | main game, no active schedule | TERM merged list `schedule_filter_date[slot]`: first unused record, else random |
| 6 | MapEvent_FinalizeWorkResult | 8 | 0x66, 0x0d, 0x0e, 0x6e, 0x65 | (main game: guard + no low-five flags) | part-time-work key `MapEvent_CheckWorkState(3)` |
| 7 | MapEvent_ProcessWorkState | 8 | same | same | work key `CheckWorkState(0)`; side effect `StateLogic_GetDataMu(5)` for job keys `0x?ae6c880..980` (3 heroines x 4 stages) and `(0)` for `0x?ae60080..180` |
| 8 | Schedule_ProcessState | 0x19 | 0x66 | main game, scoped flag (0x20f1a78) or +0xb set, no schedule | TERM merged list `schedule_process_state[slot]`: first unused |
| 9 | date schedule (flag 0) | 0x17 | any | main game, no schedule | per-heroine dated entries (below), any time of day |
| 10 | Schedule_TestEventType16Eligibility | 0xd | 0x10 | period 2 or 3, guard3 | `stage10_slot_stage[slot][stage]` |
| 11 | MapEvent_SelectWorkResult | 8 | 0x6c | as stage 6 | work key `CheckWorkState(2)` unless it is one of `0x?cd86480/500/580` (then 0) |
| 12 | MapEvent_SelectFromCurrentRule | 2 | 5..15, 0x65, 0x66 | today has a calendar rule (`CalendarRule_RunDailyEventLogic` -> char, code 0..0x2d; 0x23/0x24 are stored as pending special) and the rule char is the current one | main game: variant value (`MapEvent_GetVariantValue(code, char, period)`), result = `code`, with bit 31 if the save logic applies, 0x40000000 if low-five flags else 0x20000000 if a primary schedule; otherwise the variant value itself |
| 13 | MapEvent_TryConsumePendingSpecialValue | 0x16 | any | secondary state clear | `MapEvent_ConsumePendingSpecialValue`: constant id for pending rule 0x23/0x24 (then cleared) |
| 14 | MapEvent_TryGetTodaySlotValue | 0x16 | any | secondary state clear | table value for the current slot when the day flags allow |
| 15 | (0x20b447c) | 0x15 | any | secondary state clear | `sub_20b43ac(0)` |
| 16 | StateLogic_SelectDataNu | 9 | 0, 10, 11, 12 | not main game | first heroine h (0..2) with `StateLogic_SelectDataMu(h, period, act) != 0x19`: id `stage16_by_choice[choice][0]` + bookkeeping |
| 17 | SlotCmd_PollFromInputByte | 9 | 10, 11, 12 | no active primary, secondary clear | `SlotCmd_ProcessSlotCommand(heroine = act-10, period)` |
| 18 | action queue (0x20b8b34) | 9 | any | no active schedule | TERM queue of `stage18_action_queue[slot]` (afternoon meeting with heroine): random unused record; returns 0 (retry) while the queue is loading |
| 19 | (0x20f6eec) | 0xd | any | period 3, guard3, scoped flag (0x20f6f98) | `stage19_slot_stage[slot][stage]` and marks the slot selected |
| 20 | Schedule_TestEventEligibility | 0xd | 5..14 | period 2 (not Saturday) or 3 (weekday/date rule), guard3, no special date code, date event available | `stage20_slot_stage[slot][stage]` |
| 21 | (0x20f6fa0) | 0xd | 0x1e..0x5d | period 1..3, guard3, stage <= 2, no special date code, date event available | `stage21_slot_stage[slot][stage]` |
| 22 | MapEvent_TestEventEligibility | 5 | any (rule code known) | `EventLogic_ClearData_cdf80(act) != -1` | constant id (0x20ae48c) plus callbacks |
| 23 | (0x20b4a60) | 8 | != 0xff | main game: guard3 | `MapEvent_CheckWorkState(1)` (result left unchanged otherwise) |
| 24 | MapSelector_RunLogic | 3 | 5..9, 0x6e (not period 2) | guard3, scoped flag (0x20af8b4) in period 1 | map selector queue: `MapSelector_RunTextStateMachine(random entry)` |
| 25 | Schedule_CheckPromptAvailability | 0x1f | 10, 11, 12 | main game, state record >= 2, `EventChoice_GetResult` 1/2/3 matches act 10/11/12 | `stage25_slot_stage[slot][stage]` |
| 26 | TextEvent_RunPrimaryLogic | 6 | 5..15, 0x6d, 0x6e, 0x6f, 0x70 | see below | TextEvent queue result (index id) |
| 27 | TextEvent_RunEventStateMachine | 6 | 10..12 (not main) / 5..15, 0x6d, 0x6e (main) | period 3, weekday rule; not-main: best-level heroine; main: calendar/date map | `stage27_final[slot][mode][stage]` or TERM queue (`text_event[slot]`) |
| 28 | (0x20e5478) | 4 | 5..9 | main game, not weekend, guard3, state value > 1 | `stage28_slot_stage[slot][stage]` |
| 29 | StateLogic_SelectDataChi | 0xf | all except 1-4, 0xd-0x10, 0xff | heroine order from `StateLogic_UpdateDataXi_d70e0`, `d7e28 == 0`, `d73a0 != 0` | TERM list via `StateLogic_ClearDataOmicron_d7164(h)` (4 files per heroine: first unused else random) |
| 30 | MapArcQueue_Update | 7 | any | - | map-arc queue random unused entry (TERM list `map_arc_queue[slot]`) |
| 31 | Schedule_ResolveRuleMessage | 4 | rule id of `act` known | partner <= 1 when not main game | `stage31_slot_stage[slot][stage]` (4 columns) + applies the rule |
| 32 | date schedule (flag 1) | 0x17 | any | as stage 9 | as stage 9, but only inside the 60-minute window starting at the entry's start minute |
| 33 | TextEvent_RunSecondaryLogic | 6 | any | period 1, main game, not weekend, no special day, clock events clear, no 0x17/0x1e calendar event | TextEvent queue (`text_event[slot]`) |
| 34 | TextEvent_RunTertiaryLogic | 6 | 10, 11, 12 | period 3, main game, weekday rule, availability | `stage34_final[slot][love > 127][stage]` or the low-five table |
| 35 | MapArcQueue_TestSlotFlagB | 0x11 | any | scoped flag (0x20aec80) clear, state record >= 3 | `stage35_slot[slot]` |

Stages 9 and 32 share the body at `0x20f1750` (`r3` = 0/1): per-heroine pointer table `0x20f1918` -> per-date-entry pointers -> arrays of 0x14-byte records
`{s8 kind (0xff ends), u32 period, u32 id (+8), u16 start minute (+0xc), s8 switch (+0xe), s8 state (+0x10)}`; the candidate id must be in the list produced by
`Schedule_UpdateState(buf, 0x60)`, `Schedule_SwitchWrapper(switch, upcoming entry)` and `Schedule_GetState(state, entry flag)` must be non-zero and the record period must equal the current period.

## What the leaves hide
Everything the handlers call outside themselves is a black box to this verification: the stage logic above is exact, but the values those leaves return are not
re-derived (clock/calendar rules, schedule state, save-record getters, the queue loaders and the id-producing leaves listed below). They fall into three groups:
1. **Id producers** (documented, tables in `stage_tables.json`): `MapEvent_CheckWorkState` (first non-zero `MapEvent_ClearWorkState` over three work-state ids), `MapEvent_GetVariantValue` (rule code -> id: code 6 random of two constants, 0x25/0x28 per-char tables, 0x2a/0x2b constants, else a table by code), `MapEvent_ConsumePendingSpecialValue`, `MapEvent_GetTodaySlotValue`, `UiInput_CheckAction/Process`, `StateLogic_ClearDataOmicron_d7164` (TERM list), the TERM queue loaders (`MapArc_LoadMergedEntryList` + `CondList_*`, verified in `CONDITIONS.md`).
2. **Clock/calendar** (`Calendar_IsEventActive` ...): a rule table, listed per calendar-event index in `CONDITIONS.md` ops 55-58/153/154.
3. **Schedule/save getters**: `SAVE_RECORD.md`.

### Schedule leaf glossary (decompiled; the values the stage guards read)
- `Schedule_HasActivePrimaryState`: the global primary schedule state (`**0x20f7794`) is 1..4 (an outing/date sequence is running). `Schedule_GetPrimaryState` returns it (0 when none); `Schedule_IsSecondaryState2`: the secondary state (`*(0x20f7fac+4)`) == 2.
- `Schedule_HasLowFiveFlags`: `(*(u16*)ptr & 0x1f) != 0`, the five low "special day in progress" flags (`ptr` = `**0x20f6378`). `Schedule_IsSpecialDayActive` = main game and (low-five flags or `Schedule_TestAvailability`).
- `Schedule_GetUpcomingEntry(out, entry)`: reads the saved upcoming-date entry (`GameState_GetPtrForSlotOrDefault(-1) + 0x3a50`: kind, y/m/d, weekday); an entry kind outside 0x7e..0x81 is reset to 0xa7 (none). `Schedule_GetDateEntry(today)` = days between today and that entry's date when its kind is 0x7e..0x81, else -1. `Schedule_GetEntryFlag` = byte +0x24 of it.
- `Schedule_SwitchWrapper(mask, entry)`: `mask & (1 << entry-bit)`; `Schedule_GetState(mask, flag)`: `mask & (flag ? 2 : 1)`.
- `Schedule_TestTimePeriodAvailable`: 0 in the main-game weekend mode (`GameFlags +0xe0`); otherwise 1 only when there is no dated entry today (`GetDateEntry == 0`) and the period is >= 1 (period 5 only from its start hour).
- `Schedule_TestSpecialDateCode`: main-game weekend mode only: the saved schedule record of today (`StateLogic_InvokeCallbackData_cfc34(y, m, d)`, 0x20 bytes per day) has code 0x0d at +0x28.
- `Schedule_TestDateEventAvailability(mode, slot, session)`: the heroine's state value >= 4, stat 0x2b5 (sense) <= 0x60, `EventTimer_SetPrimaryValue(slot)` available, today in the monthly special dates (`Calendar_TestMonthlySpecialDates`) and the per-slot "used" byte (+0x36) clear; for mode 0 also `Schedule_ClearState` and period 1..2.

## RNG
`Rand_Next(n)`: 64-bit LCG `s = s * 0x5D588B656C078965 + 0x269EC3`, returns `(hi32(s) * n) >> 32` (hi32 when n == 0). `Rand_NextLcg32` is the 32-bit variant. The state is seeded once at boot
(function `0x2040204`) from `GameClock_GetRtcTimestamp()` (real-time clock), so event picks are not reproducible across runs unless the RTC is fixed. Where the pipeline draws random numbers
(stage 4 percent roll, stage 5 / queue picks, TERM op 78, `MapEvent_GetVariantValue(6)`) the draw order is that of the code above; `tools/cond_eval.py` evaluates every condition
(no short-circuit) precisely so that this order matches.
