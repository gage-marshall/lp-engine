//! The scene machine Mod2049a04_RunMainStateMachine 0x204a0c4 (top state 0x5a, depth 3): enters a scene (the map's
//! active character and mode decide which script runs), plays it through the scene overlay (ov02 0x2158000..: its
//! polls decide where the scene goes next), and, when it ends, does the end-of-scene bookkeeping and returns an exit
//! code to the top machine:
//!     1 the flow is in mode 0xc (a UI choice follows)      2 a normal end (back to the room)
//!     3 flag 0x80000   4 flag 0x100000   8 flag (0, 0x100)   5 the map mode is 0xb   6 the time hook asks for ov38
//! States: 0..2 enter (0 also saves the clock), 10 re-enter, 0x14..0x16 start, 0x1e playing, 0x1f, 0x28 clock widget,
//! 0x29, 0x32 / 0x33 / 0x3c the end, 0x46 / 0x50 / 0x5a / 100 the scene's sub-screens.
use super::addr::*;
use super::{c, Arg, M};
use crate::rom::Rom;

pub(crate) struct Consts {
    /// 0x213fcb4: the scene machine's block (+8 heroine, +0xc map kind, +0x10, +0x14, +0x1c, +0x20)
    pub p: u32,
    none16: i32,     // 0xffff
    ov_sub: u32,     // 0x13
    last_state: u32, // 0x212420c: the machine's state, kept for others
    white: i32,      // 0x7fff: the sub screen's backdrop colour
}

impl Consts {
    pub fn new(rom: &Rom) -> Consts {
        Consts { p: rom.u32(0x204aef4), none16: rom.u32(0x204aef8) as i32, ov_sub: rom.u32(0x204af00), last_state: rom.u32(0x204af04), white: rom.u32(0x204aefc) as i32 & 0xffff }
    }
}

enum Act {
    Stay,
    Set(i32),
}
use Act::*;

pub(crate) fn run(m: &mut M) -> i32 {
    let (p, ov_sub, last_state) = (m.f.scene.p, m.f.scene.ov_sub, m.f.scene.last_state);
    let mut code = 0;
    c!(m, Main_IncStateDepth);
    let s = c!(m, Main_GetStateAtDepth);
    let mut act = Stay;
    if s > 0x28 {
        match s {
            0x33 => {
                if c!(m, ScreenFade_IsActive, 1) == 0 && c!(m, ScreenFade_IsActive, 2) == 0 {
                    code = end(m);
                }
            }
            0x29 => {
                let v = m.lds(p + 0x1c);
                if c!(m, GameClock_ClearManualTime, v) == 0 {
                    m.st(p + 0x1c, 0);
                    act = Set(0x1e);
                }
            }
            0x32 => act = closing(m),
            0x3c => {
                let busy = c!(m, ScreenFade_IsActive, 1) != 0 || (c!(m, MapArc_GetCurrentMode) == 0xb && c!(m, ScreenFade_IsActive, 2) != 0);
                if !busy {
                    c!(m, Mod2049a04_TeardownSceneIfMapState0xb_0x14);
                    act = Set(10);
                }
            }
            0x46 => {
                if c!(m, 0x215847cu32) == 5 {
                    c!(m, 0x2158298u32);
                    act = Set(0x1e);
                }
            }
            0x50 => {
                let v = m.lds(p);
                if c!(m, 0x2159c84u32, v) == 0x1e {
                    c!(m, 0x21598d4u32, 0x1e); // the poll's result, still in r0
                    if c!(m, Bg_ComputeTimedValuePrimary) != 0 && c!(m, Camera_GetAreaForCurrentPreset) == 0 {
                        c!(m, Bg_ResetAuxiliary, 1);
                    }
                    c!(m, UiLists_ClearBoth);
                    c!(m, GameFlags_Assign, 0x800000, 0, 0);
                    c!(m, GameFlags_SetField0x110, 0);
                    act = Set(0x1e);
                }
            }
            0x5a => {
                let v = m.lds(p);
                if c!(m, 0x215a148u32, v) == 0x1e {
                    c!(m, 0x21598d4u32, 0x1e);
                    if c!(m, Overlay_IsLoaded, ov_sub) != 0 {
                        c!(m, Overlay_Unload, ov_sub);
                        c!(m, GameHeap_LoadOverlayInfo, ov_sub);
                    }
                    act = Set(0x1e);
                }
            }
            100 => {
                if c!(m, 0x2158584u32) == 5 {
                    c!(m, 0x2158298u32);
                    act = Set(0x1e);
                }
            }
            _ => {}
        }
    } else {
        match s {
            0x28 => {
                let r = c!(m, ClockWidget_RunStateMachine);
                if (2..=5).contains(&r) {
                    c!(m, GameFlow_InvokeCallbackMode8, 0);
                    if r == 5 {
                        act = Set(0x1e);
                    }
                }
            }
            0x16 => {
                if c!(m, ScreenFade_IsActive, 1) == 0 {
                    act = start(m);
                }
            }
            0x1e => act = playing(m),
            0x1f => act = Set(0x32),
            0..=2 | 10 => {
                if s == 0 {
                    c!(m, Mod2049a04_EnterSceneSaveClock);
                    c!(m, Main_AdvanceStateSequence);
                }
                if s == 0 || s == 1 {
                    c!(m, Main_AdvanceStateSequence);
                }
                if s == 10 {
                    reenter(m);
                } else {
                    enter(m);
                }
                act = Set(0x14);
            }
            0x14 => {
                c!(m, Main_AdvanceStateSequence);
                c!(m, Main_AdvanceStateSequence);
            }
            0x15 => {
                c!(m, Main_AdvanceStateSequence);
            }
            _ => {}
        }
    }
    if let Set(n) = act {
        c!(m, Main_SetState, n);
    }
    // every frame
    c!(m, GameFlags_ConfigureField0x90);
    let st = c!(m, Main_GetStateAtDepth);
    m.st(last_state, st);
    if c!(m, MapArc_GetCurrentMode) != 0xb {
        c!(m, DateUi_SelectNightStyle);
    }
    c!(m, Bg_DispatchState);
    c!(m, Main_DecStateDepth);
    code
}

fn s8(v: i32) -> i32 {
    v as i8 as i32
}
fn s16(v: i32) -> i32 {
    v as i16 as i32
}

/// Schedule_DispatchEventLoad and the map selection made from it -> (selection, flag)
fn selection(m: &mut M, mode: i32, sel: i32, kind: Option<i32>, second_lookup_ok: bool) -> ((i32, i32, i32), i32) {
    let none16 = m.f.scene.none16;
    let v = c!(m, Schedule_DispatchEventLoad, mode, sel);
    let (lo, b2, b3) = (s16(v), s8(v >> 16), s8(v >> 24));
    if mode == 0xb {
        return ((300, b2, b3), 1);
    }
    let mut ident = lo;
    if kind == Some(0x10) {
        if c!(m, StateLogic_CalculateData_dd2e8, 0x10, b2) != none16 {
            ident = c!(m, StateLogic_CalculateData_dd2e8);
        }
    } else if second_lookup_ok && kind == Some(0x11) {
        return ((0xd5, b2, b3), 0);
    } else if second_lookup_ok && mode == 0x14 {
        ident = 0x8e;
    }
    ((ident, b2, b3), 0)
}

/// the selection struct {u16 ident, s8 b2, s8 b3} is passed by address (a stack buffer the callee reads and writes
/// back; the replay does not compare it)
fn apply_selection(m: &mut M, selection: (i32, i32, i32), flag: i32) {
    let (ident, b2, b3) = selection;
    let mut buf = [Some((ident & 0xffff) | (b2 & 0xff) << 16 | (b3 & 0xff) << 24)];
    c!(m, Mod2049a04_ApplyMapSelection, Arg::Out(&mut buf), flag);
}

/// states 0..2: set the scene up from the map's active character
fn enter(m: &mut M) {
    c!(m, Gfx_SetBgVisibleFlag, 1, 0, 1);
    c!(m, Gfx_SetBgVisibleFlag, 1, 0, 0);
    let (mut kind_o, mut ptr_o) = ([Some(0)], [Some(-1)]);
    let cid = c!(m, MapArc_GetActiveCharIdAndMode, 0, Arg::Out(&mut kind_o), Arg::Out(&mut ptr_o));
    let kind = kind_o[0];
    let ptr = match ptr_o[0] {
        Some(-1) => Some(0),
        p => p,
    };
    c!(m, GameState_SetPointer, ptr);
    if c!(m, GameFlow_GetStateField4) == 9 {
        c!(m, GameFlags_GetField0x0);
        c!(m, TextRes_UpdateRandomSelection);
    }
    if cid == 0 {
        c!(m, ScreenFade_ToClear, 1, 10, 0);
        c!(m, ScreenFade_ToClear, 2, 10, 0);
    } else {
        let mut mode = c!(m, MapArc_GetCurrentMode);
        let mut sel = cid;
        if c!(m, GameFlags_Test, 1, 0) != 0 {
            mode = 1;
            if c!(m, GameFlow_GetStateField4) == 9 {
                sel = c!(m, TextRes_GetRandomSelection);
            }
        }
        let (selection, flag) = selection(m, mode, sel, kind, true);
        apply_selection(m, selection, flag);
        let g = c!(m, GameFlags_GetField0x0);
        c!(m, GameFlags_PushField0x4, g); // the value just read, still in r0
        c!(m, GameFlags_SetField0x0, -1);
        c!(m, DayEvent_GetResultIdForSlotEvent, kind, g);
        c!(m, GameSnd_PlaySceneAudio, kind, 0, 0x69);
        if c!(m, GameFlags_Test, 1, 0) == 0 || c!(m, GameFlow_GetStateField4) != 9 {
            if kind == Some(0xe) {
                c!(m, Schedule_IncrementPrimaryNormalCounters, g);
            }
        } else {
            c!(m, Schedule_IncrementPrimaryCounters, g);
        }
        if kind == Some(0x10) && c!(m, StateLogic_ClearDataPi_dd750, cid, ptr) != 0 {
            c!(m, GameSnd_SetSceneAudioLock, 3);
            c!(m, GameSnd_PlaySceneAudio, -1, 0, 0x69);
        }
        c!(m, ColorMath_ComputeRampWrapper);
        if kind == Some(0xe) {
            c!(m, FlowState_SetFlag0x20AndNotify);
        }
        c!(m, UiMain_UpdateMode, kind, 0);
        if c!(m, GameFlow_GetStateField4) != 0xc {
            let hour = c!(m, GameClock_GetHour);
            let per = c!(m, TimePeriod_FromHour, hour);
            let y = c!(m, GameClock_GetYear);
            let mo = c!(m, GameClock_GetMonth);
            let d = c!(m, GameClock_GetDay);
            c!(m, StateLogic_UpdateDataTau_dc9f8, y, mo, d, per, kind, cid);
        }
        if matches!(kind, Some(k) if (0..2).contains(&(k - 0x17))) {
            c!(m, ItemDb_StoreQueuedValue, cid, 0);
        }
        if matches!(kind, Some(1) | Some(0xe)) {
            c!(m, EventLogic_SetRecordFlagPrimary, 0x400, 1);
            c!(m, SaveSlot_ClearWord_Ofs0x50);
        }
    }
    if c!(m, Mod2049a04_IsIdInTables, cid) != 0 {
        c!(m, Transform_SetContextEnabled, 1);
    }
}

/// state 10: the next scene of a chain
fn reenter(m: &mut M) {
    c!(m, Mod2049a04_ResetSceneSystems);
    let (mut kind_o, mut ptr_o) = ([Some(0)], [Some(-1)]);
    let mut cid = c!(m, MapArc_GetActiveCharIdAndMode, 0, Arg::Out(&mut kind_o), Arg::Out(&mut ptr_o));
    let kind = kind_o[0];
    c!(m, GameState_SetPointer, ptr_o[0]);
    if c!(m, GameFlow_GetStateField4) == 9 {
        c!(m, GameFlags_GetField0x0);
        c!(m, TextRes_UpdateRandomSelection);
    }
    if cid == 0 {
        c!(m, ScreenFade_ToClear, 1, 10, 0);
        c!(m, ScreenFade_ToClear, 2, 10, 0);
        return;
    }
    let mut mode = c!(m, MapArc_GetCurrentMode);
    if c!(m, GameFlags_Test, 1, 0) != 0 {
        mode = 1;
        if c!(m, GameFlow_GetStateField4) == 9 {
            cid = c!(m, TextRes_GetRandomSelection);
        }
    }
    let (selection, flag) = selection(m, mode, cid, kind, false);
    apply_selection(m, selection, flag);
    let g = c!(m, GameFlags_GetField0x0);
    if g != -1 {
        c!(m, GameFlags_PushField0x4, g);
        c!(m, DayEvent_GetResultIdForSlotEvent, mode, g);
        if c!(m, GameFlags_Test, 1, 0) == 0 || c!(m, GameFlow_GetStateField4) != 9 {
            if kind == Some(0xe) {
                c!(m, Schedule_IncrementPrimaryNormalCounters, g);
            }
        } else {
            c!(m, Schedule_IncrementPrimaryCounters, g);
        }
    }
    c!(m, GameFlags_SetField0x0, -1);
    c!(m, GameSnd_PlaySceneAudio, kind, 0, 0x69);
    c!(m, ColorMath_ComputeRampWrapper);
    c!(m, UiMain_UpdateMode, kind, 0);
}

/// 0x16: the scene's character and music; no character -> straight to the end
fn start(m: &mut M) -> Act {
    let p = m.f.scene.p;
    let (mut kind_o, mut ptr_o) = ([None], [None]);
    let cid = c!(m, MapArc_GetActiveCharIdAndMode, 0, Arg::Out(&mut kind_o), Arg::Out(&mut ptr_o));
    c!(m, GameState_SetPointer, ptr_o[0]);
    c!(m, GameFlags_SetField0x98, -1);
    c!(m, MapArc_ClearCurrentCharId);
    c!(m, MapArc_SetField34, cid);
    c!(m, MapArc_SetBgmId, kind_o[0]);
    if cid == 0 {
        return if c!(m, FadeState_InvokeCallbackA) != 0 { Set(0x32) } else { Stay };
    }
    m.st(p + 8, cid);
    m.st_opt(p + 0xc, kind_o[0]);
    c!(m, GameClock_SaveState, cid, 0, kind_o[0]); // r2 holds the kind (the store above)
    m.st(p + 0x14, 0);
    Set(0x1e)
}

/// 0x1e: the scene runs; the scene overlay's polls open its sub-screens (0x46 / 0x50 / 0x5a / 100) or the clock
/// widget (0x28)
fn playing(m: &mut M) -> Act {
    let p = m.f.scene.p;
    let held = |m: &mut M, off: u32| {
        let v = m.lds(p + off);
        c!(m, GameClock_ClearManualTime, v) != 0
    };
    let over = !held(m, 8) && !held(m, 0x10) && !held(m, 0x20);
    if over {
        m.st(p + 8, 0);
        m.st(p + 0x10, 0);
        c!(m, Main_AdvanceStateSequence); // the scene is over: 0x1f
    } else if c!(m, GameFlags_Test, 0x80000000u32, 0) != 0 {
        c!(m, Mod2049a04_ResetClockScriptSoundFade);
        c!(m, Main_SetState, 0x32);
    }
    if c!(m, ClockWidget_UpdateStateB) != 0 {
        return Set(0x28);
    }
    if c!(m, 0x21582f4u32) != 0 {
        c!(m, 0x21583e4u32, 1);
        return Set(0x46);
    }
    if c!(m, 0x215836cu32) != 0 {
        sub_backdrop(m);
        c!(m, 0x21583e4u32, 0);
        return Set(100);
    }
    if c!(m, 0x2159a6cu32) != 0 {
        c!(m, GameFlags_Assign, 0x80000000u32, 0, 0);
        c!(m, UiSelection_SetValue, 0);
        c!(m, 0x2159888u32, -1);
        c!(m, 0x2159a98u32, 1);
        return Set(0x50);
    }
    if c!(m, 0x2159a88u32) != 0 {
        sub_backdrop(m);
        c!(m, 0x2159a98u32, 0);
        return Set(0x5a);
    }
    Stay
}

fn sub_backdrop(m: &mut M) {
    c!(m, UiLists_ClearSub);
    c!(m, TransitionBg_ClearVramBanks);
    let mut colour = [Some(m.f.scene.white)]; // a u16 on the stack
    c!(m, GXS_LoadBGPltt, Arg::Out(&mut colour), 0, 2);
    c!(m, Gfx_GetBgScreenSize, 2, 1);
}

/// 0x32: fade out; a scene that leads on (0x33) or one that re-enters (0x3c -> 10)
fn closing(m: &mut M) -> Act {
    c!(m, Projection_SetCallback, 0);
    if c!(m, GameFlags_Test, 1, 0) == 0 {
        if c!(m, GameFlags_SetBitRange) != 0 {
            if c!(m, StateLogic_UpdateData_dd4dc) == 0 {
                return fade_both_and_advance(m);
            }
            c!(m, ScreenFade_ToBlack, 1, 10);
            return Set(0x3c);
        }
        if c!(m, MapArc_GetCurrentCharId) == 0 {
            return fade_both_and_advance(m);
        }
        c!(m, ScreenFade_ToBlack, 1, 10);
        if c!(m, MapArc_GetCurrentMode) == 0xb {
            c!(m, ScreenFade_ToBlack, 2, 10);
        }
        return Set(0x3c);
    }
    if c!(m, GameFlow_GetStateField4) == 0x12 || c!(m, GameFlow_GetStateField4) == 0xc {
        return fade_both_and_advance(m);
    }
    c!(m, ScreenFade_ToBlack, 1, 10);
    Set(0x3c)
}

fn fade_both_and_advance(m: &mut M) -> Act {
    c!(m, ScreenFade_ToBlack, 1, 10);
    c!(m, ScreenFade_ToBlack, 2, 10);
    c!(m, Main_AdvanceStateSequence);
    Stay
}

/// 0x33: the scene is over; bookkeeping and the exit code
fn end(m: &mut M) -> i32 {
    if c!(m, GameFlags_Test, 0x80000000u32, 0) != 0 {
        if c!(m, GameFlags_Test, 2, 0) != 0 {
            c!(m, EventAssets_CheckLoaded);
            c!(m, EventAssets_SetPrimary);
            c!(m, BgLoad_CallHookPtr);
        }
        c!(m, Transform_UpdateInputContext);
    }
    if c!(m, MapArc_GetCurrentMode) == 0x1c {
        let mut d = [None; 4];
        c!(m, GameClock_GetDate, Arg::Out(&mut d));
        let hour = c!(m, GameClock_GetHour);
        let per = c!(m, TimePeriod_FromHour, hour);
        let mut date = d; // the date, copied to the stack
        if c!(m, Schedule_TestSecondaryDateAndFlag, Arg::Out(&mut date), per) != 0 {
            c!(m, Schedule_IncrementSecondaryCounters);
            c!(m, Schedule_SubtractSecondaryField0x04By16);
            c!(m, EventLogic_DispatchTrigger, 0x5d, -1);
            c!(m, SaveSlot_UpdateRecordState, 0xd, -1);
        }
    }
    c!(m, Mod2049a04_TeardownSceneSystems);
    let mut code;
    if c!(m, GameFlow_GetStateField4) == 0xc {
        code = 1;
    } else {
        c!(m, GameFlags_UpdateField0xa4);
        code = 2;
        if c!(m, GameFlags_Test, 4, 0) != 0 {
            c!(m, SaveCheck_UpdateStateTimerExtended);
            if c!(m, GameState_GetField0x78) != 0 && c!(m, GameFlags_GetField0xe0) == 0 {
                c!(m, RandEvent_UpdateData_d9ce8, 0x31, 1, 0, -1);
            }
        }
        if c!(m, MapArc_GetCurrentMode) == 0xe {
            c!(m, SaveCheck_ResetCounter);
            c!(m, FlowState_ClearFlag0x20);
        }
        if c!(m, GameFlags_Test, 0x100000, 0) == 0 {
            if c!(m, GameFlags_Test, 0, 0x100) != 0 {
                c!(m, Callback_InvokeWith0100);
                code = 8;
            }
        } else {
            c!(m, StateLogic_CalculateDataMu_dd5d8);
            code = 4;
        }
        if c!(m, GameFlags_Test, 0x80000, 0) != 0 {
            c!(m, GameFlags_Assign, 0x80000, 0, 0);
            code = 3;
        }
        if c!(m, MapArc_GetCurrentMode) == 0xb {
            code = 5;
        }
        if c!(m, MapArc_GetCurrentMode) == 6 && c!(m, GameClock_GetTimePeriod) == 3 {
            c!(m, Schedule_GetDataPsi_d85cc, 0, -1, 3); // (from a stack struct: 0, -1, period 3)
        }
        if c!(m, MapArc_GetCurrentMode) == 0xd {
            let low = (c!(m, Stats_GetBase_Ofs0x2b5) as u32) < 0x81;
            c!(m, EventLogic_SetRecordFlagPrimary, 0x20, if low { 0 } else { 1 });
            c!(m, EventLogic_SetRecordFlagPrimary, 0x40, if low { 1 } else { 0 });
        }
        if c!(m, MapArc_GetCurrentMode) == 0x1b {
            c!(m, SaveSlot_UpdateRecordState, 0xc, -1);
            c!(m, GameState_SetBitFlag, 0x37, 1);
        }
        c!(m, MapArc_GetCurrentMode);
        if code != 4 && code != 8 && code != 3 && c!(m, GameFlags_CallHookWithConst) != 0 {
            if c!(m, GameClock_GetTimePeriod) == 5 || c!(m, GameClock_GetTimePeriod) == 0 {
                code = 2;
            } else if c!(m, GameFlags_Test, 0, 1) == 0 {
                code = 6;
            } else {
                code = 2;
                c!(m, GameFlags_Assign, 0, 1, 0);
            }
        }
        if matches!(c!(m, MapArc_GetCurrentMode), 8 | 0x17 | 0x18) {
            c!(m, EventQueue_BuildOutput);
        }
        c!(m, Transform_SetContextEnabled, 0);
        if code != 4 && code != 8 && c!(m, GameFlow_GetStateField4) != 0xc {
            c!(m, GameState_ResetToTitleAudio);
            c!(m, StateLogic_UpdateData_dd028);
        }
        let bgm = c!(m, MapArc_GetBgmId);
        if (0..2).contains(&(bgm.wrapping_sub(0x17))) {
            // a stack block {year, month, day, weekday, hour, minute, second}: the time part goes to
            // Schedule_GetDateEntry, the whole block to Schedule_GetFilteredCount
            let weekday = c!(m, GameClock_CallHookGetWeekday);
            let hour = c!(m, GameClock_CallHookGetHour);
            let minute = c!(m, GameClock_CallHookGetMinute);
            let second = c!(m, GameClock_CallHookGetSecond);
            let year = c!(m, GameClock_CallHookGetYear);
            let month = c!(m, GameClock_CallHookGetMonth);
            let day = c!(m, GameClock_CallHookGetDay);
            let mut time = [Some(weekday), Some(hour), Some(minute), Some(second)];
            let e = c!(m, Schedule_GetDateEntry, Arg::Out(&mut time), 0);
            let mut block = [Some(year), Some(month), Some(day), Some(weekday), Some(hour), Some(minute), Some(second)];
            c!(m, Schedule_GetFilteredCount, e, Arg::Out(&mut block), 0);
            c!(m, ItemDb_IncrementQueueIndex);
            c!(m, SaveSlot_UpdateRecordState, 0xe, -1);
            if bgm == 0x18 {
                c!(m, GameFlags_Assign, 0, 0x80, 0);
            }
        }
        c!(m, SaveSlot_CloseStorage);
        if bgm == 1 || bgm == 0xe {
            let x = c!(m, SaveSlot_ProcessStateRecord, -1);
            c!(m, StateLogic_GetDataChi_cc3bc, x); // its result, still in r0
        }
        if c!(m, SaveData_IsRecord0Used) == 0 {
            for i in 0..3 {
                c!(m, SaveData_EncodeRecord, i);
            }
        }
    }
    c!(m, MapArc_SetCurrentMode, 0);
    c!(m, GameFlags_Assign, 0x80000000u32, 0, 0);
    c!(m, UiSelection_SetValue, 1);
    code
}
