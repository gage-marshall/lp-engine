//! The top of the game's flow: Main_ScopedFadeCheck 0x203fb20 (called every frame by GameHeap_UpdateGameState) and the
//! top state machine Mod204af08_RunGameStateMachine 0x204b0c0 (depth 2), with its boot overlay sequence 0x204afdc.
//!
//! States (docs/GAME_FLOW.md): boot 0..3, title 10/11, title menu + file select 0x1e/0x1f/0x20, the room hub 0x28,
//! scenes 0x5a/0x5b, the per-activity states the room hands off to, and the day / date / overnight sequences.
use super::addr::*;
use super::{c, Arg, M};
use crate::rom::Rom;

pub(crate) struct Consts {
    base: i32,         // 0x10e: the date / overnight block of states
    ov_title: u32,     // 13
    ov_time: u32,      // 43: game-time reconciliation
    date_entry: i32,   // 0x122
    ov_date: u32,      // 2: the date scene
    ov_minigame: u32,  // 37
    overnight: i32,    // 0x136
    ov_web: u32,       // 36
    scoped_bit: u32,   // 0xe9a
    ov_menu38: u32,    // 38
    ov_clock: u32,     // 20
    date_bit: i32,     // 0xeb3
    ov_overnight: u32, // 41
    boot_counter: u32, // 0x21415b0
    ov_boot: u32,      // 0
}

impl Consts {
    pub fn new(rom: &Rom) -> Consts {
        let w = |a| rom.u32(a);
        Consts {
            base: w(0x204c078) as i32,
            ov_title: w(0x204c07c),
            ov_time: w(0x204c080),
            date_entry: w(0x204c084) as i32,
            ov_date: w(0x204c088),
            ov_minigame: w(0x204c08c),
            overnight: w(0x204c090) as i32,
            ov_web: w(0x204c094),
            scoped_bit: w(0x204c098),
            ov_menu38: w(0x204c09c),
            ov_clock: w(0x204c0a0),
            date_bit: w(0x204c0a4) as i32,
            ov_overnight: w(0x204c0a8),
            boot_counter: w(0x204b0b8),
            ov_boot: w(0x204b0bc),
        }
    }
}

/// 0x203fb20: runs the top machine while the frame loop's own state is 0; 2 when the game shuts down
pub(crate) fn scoped_fade_check(m: &mut M) -> i32 {
    c!(m, Main_IncStateDepth);
    let mut r = 0;
    if c!(m, Main_GetStateAtDepth) == 0 && c!(m, Mod204af08_RunGameStateMachine) < 0 {
        r = 2;
    }
    c!(m, Main_DecStateDepth);
    r
}

/// 0x204afdc: load overlay 0 and run its three passes (6 states); 1 when done
pub(crate) fn boot_overlays(m: &mut M) -> i32 {
    let (counter, ov) = (m.f.top.boot_counter, m.f.top.ov_boot);
    let mut r = 0;
    c!(m, Main_IncStateDepth);
    let s = c!(m, Main_GetStateAtDepth);
    let mut adv = false;
    match s {
        0 => {
            m.st(counter, 0);
            adv = true;
        }
        1 => {
            c!(m, Overlay_Load, ov);
            c!(m, 0x2161674u32);
            adv = true;
        }
        2 => adv = c!(m, 0x2161528u32) != 0,
        3 => {
            let n = m.ld(counter);
            c!(m, 0x216168cu32, n);
            adv = true;
        }
        4 => {
            if c!(m, 0x2161528u32) != 0 {
                let n = m.ld(counter);
                if n < 2 {
                    m.st_n(counter, n + 1, 4);
                    c!(m, Main_SetState, 3);
                } else {
                    adv = true;
                }
            }
        }
        5 => {
            c!(m, 0x21616a8u32);
            adv = true;
        }
        6 => {
            if c!(m, 0x2161528u32) != 0 {
                c!(m, Overlay_Unload, ov);
                c!(m, SaveData_InvokeCallbackWithField1ec);
                r = 1;
            }
        }
        _ => {}
    }
    if adv {
        c!(m, Main_AdvanceStateSequence);
    }
    c!(m, Main_DecStateDepth);
    r
}

/// what a state does to the machine after its work
pub(crate) enum Act {
    Stay,
    Set(i32),
    Adv,
    /// return a value (and set a state)
    Ret(i32, Option<i32>),
    Quit,
}
use Act::*;

/// the persistent screen block's first word (the room's entry argument)
fn saved(m: &mut M) -> u32 {
    c!(m, Screen_GetSaved) as u32
}

/// 0x204b0c0: one frame of the top machine; -1 when the game shuts down
pub(crate) fn run(m: &mut M) -> i32 {
    let mut ret = 0;
    c!(m, Main_IncStateDepth);
    let s = c!(m, Main_GetStateAtDepth);
    match state(m, s) {
        Set(n) => {
            c!(m, Main_SetState, n);
        }
        Ret(v, n) => {
            ret = v;
            if let Some(n) = n {
                c!(m, Main_SetState, n);
            }
        }
        Quit => ret = -1,
        Adv => {
            c!(m, Main_AdvanceStateSequence);
        }
        Stay => {}
    }
    // every frame
    c!(m, GameClock_LoadState);
    c!(m, SceneRoute_ProcessClockTransition);
    c!(m, Transform_UpdateFrame);
    c!(m, GameFlags_AdvanceCalendarEvents);
    c!(m, MapCtl_UpdateTouchSelection);
    c!(m, SprFx_SelectDataMu_d2c38);
    c!(m, Main_DecStateDepth);
    ret
}

/// request the room (screen 0x11) with an entry argument and go to the hub
fn room(m: &mut M, arg: i32) -> Act {
    c!(m, Screen_Request, 0, 0x11, arg);
    Set(0x28)
}

fn room_saved(m: &mut M) {
    let p = saved(m);
    let arg = m.ld(p);
    c!(m, Screen_Request, 0, 0x11, arg);
}

fn clock_hook0(m: &mut M) {
    if c!(m, GameState_GetField0x78) != 0 {
        c!(m, GameClock_CallHookWith0);
    }
}

fn state(m: &mut M, mut s: i32) -> Act {
    let k = &m.f.top;
    let (base, date_entry, overnight) = (k.base, k.date_entry, k.overnight);
    let (ov_title, ov_time, ov_date, ov_minigame, ov_web) = (k.ov_title, k.ov_time, k.ov_date, k.ov_minigame, k.ov_web);
    let (ov_menu38, ov_clock, ov_overnight) = (k.ov_menu38, k.ov_clock, k.ov_overnight);
    // ---- boot and title ----
    if s == 0 {
        c!(m, Gfx_ApplyDefaultBanksAndInitVramMgrs);
        c!(m, Main_AdvanceStateSequence);
        s = 1;
    }
    match s {
        1 => {
            c!(m, Mod204af08_InitGameSystems);
            return Adv;
        }
        2 => return if c!(m, Mod204af08_StepOverlayLoadSequence) != 0 { Adv } else { Stay },
        3 => return if c!(m, Transform_GetInputContextStatus) != 0 { Set(10) } else { Stay },
        4..=9 => return Stay,
        10 => {
            c!(m, Overlay_Load, ov_title);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_title);
            return Adv;
        }
        11 => {
            let r = c!(m, 0x216a9a8u32); // the title screen (ov13); its value is the machine's
            if r < 1 {
                return Ret(r, None);
            }
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_title);
            return Ret(r, Some(0x1e));
        }
        0x1e => {
            // back to the title menu
            c!(m, Display_ResetBgAndVram);
            c!(m, Sprite_ClearAnimation);
            c!(m, Screen_Request, 0, 0, -1);
            return Set(0x1f);
        }
        0x1f => {
            // title menu, file select, onboarding (SaveSelect)
            let r = c!(m, SaveSelect_ProcessScreen);
            if r == -1 {
                return Set(10);
            }
            if r != 1 {
                return Stay;
            }
            c!(m, FlowState_InitSingleton);
            if c!(m, GameFlags_Test, 0x20000000, 0) != 0 {
                // a new game: straight to the opening
                c!(m, GameFlags_Assign, 0x20000000, 0, 0);
                return Set(0x118);
            }
            c!(m, UiChoice_ResetPrimary);
            return Adv;
        }
        0x20 => {
            // continue: reconcile the game time with the clock
            if c!(m, UiChoice_DispatchInput) == 0 {
                return Stay;
            }
            c!(m, UiChoice_ResetSecondary);
            c!(m, Transform_UpdateInputContext);
            c!(m, Overlay_Load, ov_time);
            let mut nxt = date_entry;
            if c!(m, 0x216ac5cu32) == 0 {
                // ov43
                nxt = if c!(m, GameState_GetField0x78) != 0 { base } else { 0x118 };
            }
            c!(m, Main_SetState, nxt);
            c!(m, Overlay_Unload, ov_time);
            return Stay;
        }
        0x21..=0x27 => return Stay,
        // ---- the room hub ----
        0x28 => {
            let r = c!(m, Screen_Run);
            return hub(m, r);
        }
        0x32 => {
            let r = c!(m, UiScreen_RunStateMachine);
            return if r == 1 || r == 2 { room(m, 10) } else { Stay };
        }
        0x3c => {
            let r = c!(m, DebugMenu_Main);
            return match r {
                0 => Set(0x28),
                1 => Set(0x32),
                _ => Stay,
            };
        }
        0x50 => {
            c!(m, Overlay_Load, ov_web);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_web);
            c!(m, 0x216f07cu32);
            return Adv;
        }
        0x51 => {
            if c!(m, 0x216f110u32) != 0 {
                return Stay;
            }
            c!(m, 0x216f0e4u32);
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_web);
            return room(m, 10);
        }
        0x5a => {
            let code = c!(m, Mod2049a04_RunMainStateMachine);
            return scene_end(m, code);
        }
        0x5b => return Set(0x5a),
        100 => {
            if c!(m, 0x216f858u32) == 0 {
                return Stay;
            }
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_minigame);
            room_saved(m);
            c!(m, Transform_SetContextEnabled, 0);
            clock_hook0(m);
            return Set(0x28);
        }
        0x6e => return event_end(m),
        0x82 | 0x8c => {
            let r = if s == 0x82 { c!(m, Menu_RunOptionScreenState) } else { c!(m, Settings_RunScreenState) };
            return if r != 0 { room(m, -1) } else { Stay };
        }
        0x96 => {
            c!(m, GameFlags_Assign, 0x10000000, 0, 1);
            c!(m, TextRes_FreeBuffer, 1);
            c!(m, BgLoad_CallHookPtr);
            c!(m, GfxRes_CallHookPtr);
            c!(m, TextRes_ProcessInputText, 2);
            c!(m, Overlay_Load, ov_menu38);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_menu38);
            c!(m, 0x21708acu32, 0);
            return Adv;
        }
        0x97 => {
            let r = c!(m, 0x21709f8u32);
            if r == 0 {
                return Stay;
            }
            c!(m, 0x21709a8u32, r); // its result, still in r0
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_menu38);
            c!(m, Transform_UpdateInputContext);
            c!(m, TextRes_FreeBuffer, 2);
            c!(m, BgLoad_CallHookPtr);
            c!(m, GfxRes_CallHookPtr);
            c!(m, TextRes_ProcessInputText, 1);
            c!(m, GameFlags_Assign, 0x10000000, 0, 0);
            c!(m, GameFlags_ClearSequenceState);
            c!(m, Screen_Request, 0, 0x11, 100);
            c!(m, Transform_SetContextEnabled, 0);
            if r == 1 {
                return Set(0x28);
            }
            if r != 2 {
                return Stay;
            }
            c!(m, ScreenFade_ToClear, 1, 1, 0);
            c!(m, ScreenFade_ToClear, 2, 1, 0);
            c!(m, Schedule_AllocateStorage);
            let slot = c!(m, GameState_GetField0x98);
            c!(m, GameState_GetField0x98);
            let hook = c!(m, SaveSlot_CallStateHook);
            c!(m, Schedule_LoadStorage, 1, slot, hook);
            return Set(300);
        }
        0xa0 => {
            let r = c!(m, UiScreen_RunStateMachine);
            return match r {
                1 => room(m, 0x28),
                2 => room(m, 0x32),
                _ => Stay,
            };
        }
        0xbe => {
            c!(m, Overlay_Load, ov_clock);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_clock);
            return Adv;
        }
        0xbf => {
            if c!(m, 0x216a920u32) == 0 {
                // ov20
                return Stay;
            }
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_clock);
            c!(m, SaveData_InitializeClockFields);
            return Set(0x1e);
        }
        0xd2 => return date_start(m),
        0xd3 => {
            if c!(m, ScreenFade_IsActive, 1) != 0 || c!(m, ScreenFade_IsActive, 2) != 0 {
                return Stay;
            }
            let p = saved(m);
            m.st(p, -1);
            c!(m, 0x216e790u32);
            c!(m, Sprite_ProcessAnimationFrame);
            c!(m, GameClock_CallHookWith1);
            return Set(0x5a);
        }
        0xdc => {
            if c!(m, Overlay_UpdateInitState) == 0 {
                return Stay;
            }
            c!(m, Overlay_UnloadCleanup);
            c!(m, GameFlags_Assign, 0x1000000, 0, 1);
            c!(m, SaveData_InitializeClockFields);
            return Set(0x1f);
        }
        0xe6 => {
            let r = c!(m, UiScreen_RunStateMachine);
            if r != 1 && r != 2 {
                return Stay;
            }
            c!(m, Script_WriteVariable, 0x26, if r == 1 { 0 } else { 1 });
            return Set(0x5a);
        }
        0xf0 => return new_day(m),
        0xfa => return time_mode_change(m),
        0x104 => {
            c!(m, Mod204af08_ShutdownGameSystems);
            return Quit;
        }
        _ => {}
    }
    if s == base {
        // 0x10e
        c!(m, Screen_Request, 0, 0xc, -1);
        return Adv;
    }
    if s == base + 1 {
        return if c!(m, Screen_Run) != -1 { Set(0x118) } else { Stay };
    }
    if s == 0x118 {
        if c!(m, GameState_GetField0x7a) != 0 {
            return Adv;
        }
        c!(m, UiInput_Reset);
        c!(m, Main_SetState, 0x5b);
        let p = saved(m);
        m.st(p, -1);
        return Stay;
    }
    if s == base + 0xb {
        // 0x119: clock synchronisation
        c!(m, GameFlags_Assign, 0x40000000, 0, 1);
        let n = if c!(m, GameFlags_RunClockSynchronization) == 0 {
            c!(m, Screen_Request, 0, 0x11, -1);
            0x28
        } else {
            c!(m, GameClock_CallHookWith1);
            0x5b
        };
        c!(m, Main_SetState, n);
        c!(m, GameFlags_Assign, 0x40000000, 0, 0);
        if c!(m, GameFlags_CallHookWithConst) != 0 {
            let p = saved(m);
            m.st(p, 0x82);
        }
        return Stay;
    }
    if s == date_entry {
        // 0x122: a date
        c!(m, Overlay_Load, ov_date);
        c!(m, Mod2049a04_CallThreeExtFuncs);
        if c!(m, DateScene_Initialize) == 0 {
            c!(m, Main_SetState, base);
            c!(m, Overlay_Unload, ov_date);
            return Stay;
        }
        return Set(base + 0x15);
    }
    if s == base + 0x15 {
        if c!(m, DateScene_Update) == 0 {
            return Stay;
        }
        c!(m, Overlay_Unload, ov_date);
        c!(m, Overlay_Load, ov_time);
        c!(m, 0x216a920u32); // ov43
        return Set(0x124);
    }
    if s == 0x124 {
        if c!(m, 0x216a938u32) == 0 {
            return Stay;
        }
        c!(m, Overlay_Unload, ov_time);
        return Set(base);
    }
    if s == 300 {
        if c!(m, Schedule_ProcessStorage) == 0 {
            c!(m, Main_SetState, 0x96);
            c!(m, Schedule_UpdateStorage);
            c!(m, Schedule_FreeStorage);
            let p = c!(m, GameState_GetPtrForSlotOrDefault, -1);
            c!(m, GameState_UpdateProgressUnlocks, p.wrapping_add(0x3a20));
        }
        return Stay;
    }
    if s == overnight {
        // 0x136
        c!(m, Overlay_Load, ov_overnight);
        c!(m, GameHeap_LoadOverlayInfoAlt, ov_overnight);
        let who = c!(m, MapArc_GetCurrentCharId);
        c!(m, 0x216aa98u32, who, 0);
        return Set(overnight + 1);
    }
    if s == overnight + 1 {
        let r = c!(m, 0x216acbcu32);
        if r == 0 {
            return Stay;
        }
        c!(m, 0x216ace0u32, r);
        return Set(overnight + 2);
    }
    if s == overnight + 2 {
        c!(m, Overlay_Unload, -1);
        c!(m, GameHeap_LoadOverlayInfo, ov_overnight);
        room_saved(m);
        c!(m, Transform_SetContextEnabled, 0);
        return Set(0x28);
    }
    Stay
}

/// what the room asks for (its screen result) -> the next state
fn hub(m: &mut M, r: i32) -> Act {
    let (ov_minigame, overnight) = (m.f.top.ov_minigame, m.f.top.overnight);
    match r {
        0 => {
            c!(m, UiScreen_InitializeDateFields, 0, -1);
            Set(0x32)
        }
        1 => Set(0x5a),
        2 => Set(0x3c),
        3 => Set(0x50),
        4 => {
            // back to the title menu
            c!(m, Display_ResetBgAndVram);
            c!(m, SaveData_InitializeClockFields);
            c!(m, Screen_Request, 0, 0, -1);
            Set(0x1f)
        }
        5 => {
            c!(m, Overlay_Load, ov_minigame);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_minigame);
            c!(m, Transform_SetContextEnabled, 0);
            let who = c!(m, MapArc_GetCurrentCharId);
            c!(m, 0x216f5e0u32, who, 0);
            Set(100)
        }
        6 => {
            c!(m, EventLogic_SelectEvent);
            Set(0x6e)
        }
        7 => Set(0x82),
        8 => Set(0x8c),
        9 => Set(0x96),
        0xd => Set(0xf0),
        0xe => Set(0xfa),
        10 => {
            let p = c!(m, Screen_GetSaved) as u32;
            let v = m.lds(p + 0x127c);
            c!(m, UiScreen_InitializeDateFields, 1, v.wrapping_add(1));
            Set(0xa0)
        }
        11 => Set(0xbe),
        12 => {
            // a date with the heroine
            c!(m, GfxRes_CallHookPtr);
            c!(m, TaskList_InitGlobalWithFnPtr);
            let who = if c!(m, MapArc_GetCurrentMode) == 0x14 { c!(m, GameState_GetSlotTableValue_0210e1ac) } else { c!(m, MapArc_GetCurrentCharId) };
            c!(m, GameClock_SaveState, who, 0);
            c!(m, GameClock_ClearUpdateCallback);
            Set(0xd2)
        }
        0xf => Set(overnight),
        _ => Stay,
    }
}

/// the scene machine's exit code -> the next state
fn scene_end(m: &mut M, code: i32) -> Act {
    match code {
        1 => {
            c!(m, Transform_SetContextEnabled, 0);
            c!(m, UiScreen_InitializeDateFields, 4, -1);
            Set(0xe6)
        }
        2 => {
            c!(m, Transform_SetContextEnabled, 0);
            room_saved(m);
            c!(m, Main_SetState, 0x28);
            clock_hook0(m);
            Stay
        }
        5 => {
            c!(m, Transform_SetContextEnabled, 0);
            c!(m, Overlay_LoadInit);
            Set(0xdc)
        }
        6 => {
            c!(m, Main_SetState, 0x96);
            clock_hook0(m);
            if c!(m, GameFlags_CallHookWithConst) != 0 {
                c!(m, GameFlags_Assign, 0, 0x20000, 1);
            }
            Stay
        }
        7 => {
            let n = m.f.top.overnight;
            c!(m, Main_SetState, n);
            clock_hook0(m);
            Stay
        }
        3 | 4 | 8 => {
            c!(m, Transform_SetContextEnabled, 0);
            if c!(m, GameFlags_Test, 0, 0x10) == 0 {
                room_saved(m);
                clock_hook0(m);
            } else {
                c!(m, GameFlags_SetByte0x104, 0);
                c!(m, GameFlags_SetByte0x105, 0);
                c!(m, GameFlags_Assign, 0, 0x10, 0);
                c!(m, Screen_Request, 0, 4, 2); // the schedule
            }
            Set(0x28)
        }
        _ => Stay,
    }
}

/// 0x6e: the event the room picked has run (EventLogic); where it leads
fn event_end(m: &mut M) -> Act {
    let scoped_bit = m.f.top.scoped_bit;
    if c!(m, EventLogic_RunEventStateMachine) == 0 {
        return Stay;
    }
    c!(m, Transform_UpdateInputContext);
    let k = c!(m, EventLogic_GetContextField);
    let into_scene = (k == 0 || (k == 2 && c!(m, GameFlags_Test, 0x2000000, 0) == 0)) && c!(m, GameFlags_Test, 0x100, 0) != 0;
    if into_scene {
        c!(m, 0x216e790u32);
        c!(m, Sprite_ProcessAnimationFrame);
        // the character is what the call before returns (r0 carried into MapArc_SetCurrentCharId, 0x204b9a4)
        let who = c!(m, StateSegment_UpdateStateDataPhi_dbf4c);
        c!(m, MapArc_SetCurrentCharId, who);
        c!(m, MapArc_SetCurrentMode, 0xe);
        let v = c!(m, SaveSlot_GetField1c);
        c!(m, GameFlags_SetField0xc, v); // r0 carried (0x204b9bc)
        c!(m, GameFlags_SetField0x0, v);
        c!(m, SaveSlot_SetField1c, -1);
        return Set(0x5a);
    }
    let into_date = (k == 1 || k == 3)
        && c!(m, GameFlags_Test, 0x100, 0) != 0
        && c!(m, SaveSlot_GetCombinedFlag_Ofs0x39ff_3a00_3a02) != 0
        && c!(m, GameFlags_TestScopedBit, scoped_bit) == 0;
    // the date's character: the slot table's value, the condition's last test (r0 carried, 0x204ba20)
    let date_who = if into_date { c!(m, GameState_GetSlotTableValue_0210e1a0) } else { 0 };
    if date_who != 0 {
        c!(m, MapArc_SetCurrentCharId, date_who);
        c!(m, MapArc_SetCurrentMode, 0x14);
        c!(m, GameFlags_SetField0x102, 1);
        c!(m, GfxRes_CallHookPtr);
        c!(m, TaskList_InitGlobalWithFnPtr);
        let who = c!(m, GameState_GetSlotTableValue_0210e1a0);
        c!(m, GameClock_SaveState, who, 0);
        c!(m, GameClock_ClearUpdateCallback);
        return Set(0xd2);
    }
    if k == 5 && c!(m, GameFlags_Test, 0x100, 0) != 0 {
        let n = c!(m, SaveSlot_GetLowNibble_Ofs0x67);
        c!(m, SaveSlot_SetLowNibble_Ofs0x67, (n + 1) & 0xff);
    }
    if k == 6 && c!(m, GameFlags_Test, 0x100, 0) != 0 {
        let n = c!(m, SaveSlot_GetHighNibble_Ofs0x67);
        c!(m, SaveSlot_SetHighNibble_Ofs0x67, (n + 1) & 0xff);
    }
    c!(m, Sprite_SetField8, 4);
    c!(m, Sprite_SetFieldc, 2);
    clock_hook0(m);
    Set(0x28)
}

/// 0xd2: after the room picked a date (or an event led to one)
fn date_start(m: &mut M) -> Act {
    let date_bit = m.f.top.date_bit;
    if c!(m, GameClock_ClearRtcCache) != 0 {
        return Stay;
    }
    c!(m, TaskList_ClearGlobalPtr);
    c!(m, Transform_UpdateInputContext);
    if c!(m, MapArc_GetCurrentMode) == 0x14 {
        if c!(m, Transform_GetContextStatePlusOne) == 1 {
            // the character is what the request returns (r0 carried into MapArc_SetCurrentCharId, 0x204bd4c)
            let who = if c!(m, GameFlags_GetField0x102) == 0 {
                c!(m, GameState_RequestFlaggedSlotTransition)
            } else {
                c!(m, GameState_RequestSlotTransitionIfCond)
            };
            c!(m, MapArc_SetCurrentCharId, who);
            c!(m, MapArc_SetCurrentMode, 0x14);
            c!(m, ScreenFade_ToWhite, 1, 10);
            c!(m, ScreenFade_ToWhite, 2, 10);
            c!(m, UiFlow_ThunkTertiary, 0x1e);
            c!(m, Main_AdvanceStateSequence);
        } else {
            c!(m, Sprite_SetField8, 4);
            c!(m, Sprite_SetFieldc, 2);
            c!(m, 0x216e97cu32);
            c!(m, GameFlags_SetScopedBit, date_bit, 0);
            c!(m, MapArc_SetCurrentMode, 0);
            let d = if c!(m, GameFlags_GetField0x102) == 0 { -0x1a } else { -0x19 };
            c!(m, GameFlags_SetScopedBit, date_bit + d, 1);
            c!(m, Main_SetState, 0x28);
        }
        c!(m, Gfx_GetBgScreenSize, 1, 3);
        c!(m, GameFlags_SetField0x102, 0);
    } else {
        c!(m, Sprite_SetField8, 4);
        c!(m, Sprite_SetFieldc, 2);
        c!(m, 0x216e97cu32);
        c!(m, Main_SetState, 0x28);
    }
    clock_hook0(m);
    c!(m, BgLoad_CallHookPtr);
    c!(m, Transform_SetContextEnabled, 0);
    Stay
}

fn clear_saved_and_request_room(m: &mut M) {
    let p = c!(m, Screen_GetSaved);
    let n = c!(m, Screen_GetSavedSize);
    c!(m, memset, p, 0, n);
    c!(m, Screen_Request, 0, 0x11, -1);
}

/// 0xf0: a new day
fn new_day(m: &mut M) -> Act {
    c!(m, Schedule_UpdateDataPhi_d84c8);
    c!(m, GameFlags_HandleTimeModeChange, 1, 0);
    c!(m, Schedule_CacheCurrentSlot);
    clear_saved_and_request_room(m);
    let mut d = [None; 4];
    c!(m, GameClock_GetDate, Arg::Out(&mut d));
    c!(m, GameFlags_ComputeField0x104, d[0], d[1], d[2], d[3], 1);
    c!(m, Schedule_UpdateTimeDataRho_d7f70, 0);
    c!(m, RandEvent_InvokeCallbackData_d9fc8);
    c!(m, Sprite_ClearContext);
    c!(m, Sprite_SetField20, 0x1e);
    c!(m, RandEvent_UpdateData_d9fd4, 0);
    c!(m, RandEvent_UpdateData_d9ffc, 0);
    c!(m, RandEvent_UpdateData_da024, 0);
    c!(m, GameFlags_Assign, 0, 0x1000, 1);
    Set(0x28)
}

/// 0xfa: the clock moved (time mode change); the day's data is rebuilt, the same day keeps its schedule
fn time_mode_change(m: &mut M) -> Act {
    c!(m, GameFlags_HandleTimeModeChange, 0, 0);
    clear_saved_and_request_room(m);
    let mut a = [None; 4];
    c!(m, GameFlags_CopyVec4_Ofs0xb8_0xc8, Arg::Out(&mut a), 0);
    let mut b = [None; 4];
    c!(m, GameClock_GetDate, Arg::Out(&mut b));
    let same_day = match (a[0], b[0]) {
        (Some(_), Some(_)) => Some(a[..3] == b[..3]),
        _ => None,
    };
    if m.cx.pick(same_day, &[(true, Schedule_UpdateDataUpsilon_d83e0), (false, Schedule_UpdateTimeDataRho_d7f70)]) {
        c!(m, Schedule_UpdateDataUpsilon_d83e0, 0);
    } else {
        c!(m, Schedule_UpdateTimeDataRho_d7f70, 0);
    }
    c!(m, GameFlags_ComputeField0x104, b[0], b[1], b[2], b[3], 0);
    c!(m, RandEvent_InvokeCallbackData_d9fc8);
    c!(m, Sprite_ClearContext);
    c!(m, Sprite_SetField20, 0x1e);
    Set(0x28)
}
