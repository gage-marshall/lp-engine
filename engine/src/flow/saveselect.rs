//! SaveSelect_ProcessScreen 0x2074600 (top state 0x1f, depth 3): the title menu and everything it starts, and the
//! SceneLoop A / B helpers it runs the file select and file delete screens with.
//!
//! State 0x28 runs the title menu (screen 0) and branches on its result (ov21 maps the button to it: table 0x217041c).
//! Each single-player branch is ported; the wireless ones (communication talk, card exchange, すれちがい, save data send /
//! receive / transfer, ご当地カプセル) report `not_ported` with the state. A branch ends by requesting the title menu
//! again with the page to open (its entry argument) and restarting at 0, or by leaving SaveSelect:
//!     returns -1   back to the title screen (the menu timed out)
//!     returns  1   a game is to start (new: flag 0x20000000 set by the profile; continue otherwise)
use super::addr::*;
use super::{c, Arg, M};
use crate::rom::Rom;

pub(crate) struct Consts {
    disp_or: u32,   // 0x200010
    disp_mask: u32, // 0xffcfffef
    illus: i32,     // 0x119
    dispcnt_b: u32, // 0x4001000
    ov_menu38: u32,
    ov_illus: u32,
    ov_gallery_xfer: u32,
    ov_own_card: u32,
    ov_friends: u32,
    ov_staff: u32,
    ov_gotochi_chara: u32,
    ov_pair_event: u32,
    trips: i32, // 0x209
    no_save_palette: u32,
    no_save_text: u32,
    scroll_a: u32, // 0x400001c: BG3 offsets of both engines, cleared
    scroll_b: u32,
}

impl Consts {
    pub fn new(rom: &Rom) -> Consts {
        let w = |a| rom.u32(a);
        Consts {
            disp_or: w(0x20755c4),
            disp_mask: w(0x20755c8),
            illus: w(0x20755cc) as i32,
            dispcnt_b: w(0x20755e0),
            ov_menu38: w(0x2076670),
            ov_illus: w(0x2076674),
            ov_gallery_xfer: w(0x2076664),
            ov_own_card: w(0x2076c60),
            ov_friends: w(0x2076c64),
            ov_staff: w(0x2076c6c),
            ov_gotochi_chara: w(0x2076c54),
            ov_pair_event: w(0x2076c5c),
            trips: w(0x20755dc) as i32,
            no_save_palette: w(0x2076c70),
            no_save_text: w(0x2076c74),
            scroll_a: w(0x209f4fc),
            scroll_b: w(0x209f648),
        }
    }
}

const DISPCNT_A: u32 = 0x4000000;

/// title menu result -> SaveSelect state (the switch in state 0x28)
fn branch(r: i32) -> Option<i32> {
    Some(match r {
        0 => 0x3c,
        1 => 0x50,
        2 => 100,
        3 => 0x78,
        4 => 0x8c,
        5 => 0xa0,
        6 => 0xb4,
        7 => 200,
        8 => 0xdc,
        10 => 0x118,
        11 => 300,
        12 => 0x1b8,
        13 => 0x1cc,
        14 => 0x1e0,
        15 => 0x17c,
        16 => 400,
        0x11 => 100,
        0x12 => 500,
        0x13 => 0x230,
        0x14 => 0x208,
        0x15 => 0x1a4,
        0x16 => 0x21c,
        _ => return None,
    })
}
/// results whose branch starts without a fade
const NO_FADE: [i32; 2] = [6, 7];
/// the wireless features: state ranges not ported
const WIRELESS: [(i32, i32); 8] = [(0x51, 0x88), (100, 0x6a), (0x78, 0x88), (0x17c, 0x1b8), (400, 0x1a4), (0x1a4, 0x1b8), (500, 0x208), (0x185, 0x1b8)];

enum Act {
    Stay,
    Set(i32),
    Adv,
    /// back to the title menu on a page
    Menu(i32),
    Ret(i32, i32),
}
use Act::*;

fn disp(m: &mut M, addr: u32) {
    let (mask, or) = (m.f.save_select.disp_mask, m.f.save_select.disp_or);
    let v = m.ld(addr);
    m.st_n(addr, (v & mask) | or, 4);
}

pub(crate) fn run(m: &mut M) -> i32 {
    let mut ret = 0;
    c!(m, Main_IncStateDepth);
    let s = c!(m, Main_GetStateAtDepth);
    match state(m, s) {
        Set(n) => {
            c!(m, Main_SetState, n);
        }
        Adv => {
            c!(m, Main_AdvanceStateSequence);
        }
        Menu(page) => {
            c!(m, Screen_Request, 0, 0, page);
            c!(m, Main_SetState, 0);
        }
        Ret(v, n) => {
            ret = v;
            c!(m, Main_SetState, n);
        }
        Stay => {}
    }
    c!(m, Main_DecStateDepth);
    ret
}

fn fades_done(m: &mut M) -> bool {
    c!(m, ScreenFade_IsActive, 1) == 0 && c!(m, ScreenFade_IsActive, 2) == 0
}

fn menu_after_reset(m: &mut M, page: i32) -> Act {
    c!(m, Display_ResetBgAndVram);
    Menu(page)
}

/// display / sound settings, gallery settings: a static-code screen requested with the sub LCD set up
fn screen_branch(m: &mut M, screen: i32) -> Act {
    let b = m.f.save_select.dispcnt_b;
    c!(m, SaveSelect_Shutdown);
    c!(m, Display_SetLcdScreenSwap, 2);
    disp(m, b);
    c!(m, Screen_Request, 0, screen, -1);
    Adv
}

fn overlay_branch(m: &mut M) {
    let b = m.f.save_select.dispcnt_b;
    c!(m, SaveSelect_Shutdown);
    c!(m, Display_SetLcdScreenSwap, 2);
    disp(m, DISPCNT_A);
    disp(m, b);
}

fn state(m: &mut M, s: i32) -> Act {
    let k = &m.f.save_select;
    let (illus, trips, dispcnt_b) = (k.illus, k.trips, k.dispcnt_b);
    let (ov_menu38, ov_illus, ov_gallery_xfer, ov_own_card) = (k.ov_menu38, k.ov_illus, k.ov_gallery_xfer, k.ov_own_card);
    let (ov_friends, ov_staff, ov_gotochi_chara, ov_pair_event) = (k.ov_friends, k.ov_staff, k.ov_gotochi_chara, k.ov_pair_event);
    let (no_save_palette, no_save_text) = (k.no_save_palette, k.no_save_text);
    match s {
        0 => {
            if c!(m, GameFlags_Test, 0x1000000, 0) != 0 {
                return Set(0x3d); // straight to the file select (after a soft reset)
            }
            if c!(m, SaveSelect_Init) != 0 {
                c!(m, Main_SetState, 0x28);
                c!(m, SaveSelect_RefreshNewestSlotDisplay);
                return Stay;
            }
            return Set(0x154); // the save data cannot be used
        }
        0x14 => return Set(0x28),
        0x28 => {
            // the title menu
            let r = c!(m, Screen_Run);
            if r == 0 {
                c!(m, ScreenFade_ToWhite, 1, 0x1e);
                c!(m, ScreenFade_ToWhite, 2, 0x1e);
                return Set(0x3c);
            }
            if r == 9 {
                // timed out: back to the title screen
                c!(m, ScreenFade_ToWhite, 1, 0x1e);
                c!(m, ScreenFade_ToWhite, 2, 0x1e);
                c!(m, Main_SetState, 0x104);
                c!(m, UiFlow_ThunkTertiary, 0);
                c!(m, SaveSelect_TickWaitCounter);
                return Stay;
            }
            if let Some(n) = branch(r) {
                if !NO_FADE.contains(&r) {
                    c!(m, ScreenFade_ToWhite, 1, 0x1e);
                    c!(m, ScreenFade_ToWhite, 2, 0x1e);
                }
                c!(m, Main_SetState, n);
                c!(m, UiFlow_ThunkTertiary, 0);
            }
            return Stay;
        }
        // ---- start: file select -> time setting -> profile (SceneLoop A) ----
        0x3c | 0x8c | 0xa0 | 0xdc => {
            // fade out, then the branch's screen
            if !fades_done(m) {
                return Stay;
            }
            c!(m, SaveSelect_Shutdown);
            return Adv;
        }
        0x3d => {
            return match c!(m, SceneLoop_RunSpriteSystemA) {
                1 => menu_after_reset(m, 0), // backed out of the file select
                2 => Set(0x140),             // a game was chosen / set up
                _ => Stay,
            };
        }
        0x140 => return Ret(1, 0),
        0xf0 => return if fades_done(m) { Set(0x140) } else { Stay },
        0x104 => {
            // back to the title screen
            if !fades_done(m) {
                return Stay;
            }
            c!(m, SaveSelect_Shutdown);
            return Ret(-1, 0);
        }
        // ---- settings page ----
        0xb4 => return screen_branch(m, 0x10), // display settings
        200 => return screen_branch(m, 0xf),   // sound settings
        0xb5 | 0xc9 => return if c!(m, Screen_Run) == -1 { Stay } else { menu_after_reset(m, 1) },
        0xdd => return if c!(m, SceneLoop_RunSpriteSystemB) == 1 { menu_after_reset(m, 1) } else { Stay }, // delete data
        0x1b8 => {
            // staff roll (ov23)
            c!(m, SaveSelect_Shutdown);
            c!(m, Display_SetLcdScreenSwap, 2);
            disp(m, dispcnt_b);
            c!(m, GfxRes_CallHookPtr);
            c!(m, Overlay_Load, ov_staff);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_staff);
            c!(m, 0x216cfc4u32);
            return Adv;
        }
        0x1b9 => {
            let r = c!(m, 0x216cfdcu32);
            if r == 0 {
                return Stay;
            }
            c!(m, 0x216cff0u32, r);
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_staff);
            c!(m, BgLoad_CallHookPtr);
            return Adv;
        }
        0x1ba => return menu_after_reset(m, 1),
        // ---- gallery page ----
        0x8d => return if c!(m, SelectScreen_Update) == 1 { menu_after_reset(m, 3) } else { Stay }, // confession events
        0xa1 => return if c!(m, MainScene_RunStateMachine) == 1 { menu_after_reset(m, 3) } else { Stay }, // dream events
        0x118 => {
            // illustrations (ov29)
            overlay_branch(m);
            c!(m, Main_AdvanceStateSequence);
            c!(m, Overlay_Load, ov_illus);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_illus);
            return Stay;
        }
        _ => {}
    }
    if s == illus {
        if c!(m, 0x216b3f8u32) == 0 {
            return Stay;
        }
        c!(m, Overlay_Unload, -1);
        c!(m, GameHeap_LoadOverlayInfo, ov_illus);
        return menu_after_reset(m, 3);
    }
    if s == 300 {
        return screen_branch(m, 0xd); // gallery settings
    }
    if s == illus + 0x14 {
        return if c!(m, Screen_Run) == -1 { Stay } else { menu_after_reset(m, 3) };
    }
    if s == 0x208 {
        // trip events
        if !fades_done(m) {
            return Stay;
        }
        c!(m, SaveSelect_Shutdown);
        return Set(trips);
    }
    if s == trips {
        if c!(m, 0x20f444cu32) != 1 {
            return Stay;
        }
        return menu_after_reset(m, 3);
    }
    match s {
        // ---- communication page: own card, friends' cards (no wireless needed) ----
        0x1cc => {
            // own card; a pending event pair plays its event first
            overlay_branch(m);
            if c!(m, GameState_HasAsymmetricEventPair) == 0 {
                c!(m, Main_AdvanceStateSequence);
                c!(m, GfxRes_CallHookPtr);
                c!(m, Schedule_AllocateAuxState);
                c!(m, Schedule_AllocateEventState);
                c!(m, TextRes_ProcessInputText, 6);
                c!(m, Overlay_Load, ov_own_card);
                c!(m, GameHeap_LoadOverlayInfoAlt, ov_own_card);
                return Stay;
            }
            c!(m, GfxRes_CallHookPtr);
            c!(m, GameState_SetSecondEventBits);
            c!(m, Overlay_Load, ov_pair_event);
            c!(m, 0x2161624u32);
            let s = c!(m, Main_GetStateAtDepth);
            Set(s + 3)
        }
        0x1cd => {
            if c!(m, 0x218c048u32) == 0 {
                return Stay;
            }
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_own_card);
            c!(m, TextRes_FreeBuffer, 6);
            Adv
        }
        0x1ce | 0x1e2 => {
            c!(m, Schedule_FreeEventState);
            c!(m, Schedule_FreeAuxState);
            c!(m, BgLoad_CallHookPtr);
            menu_after_reset(m, 2)
        }
        0x1cf => {
            if c!(m, 0x21613e8u32) == 0 {
                return Stay;
            }
            c!(m, Overlay_Unload, ov_pair_event);
            c!(m, Bg_Initialize, 0xd5, -1, -1, 0);
            c!(m, Bg_SelectState, 1, 1);
            c!(m, ScreenFade_ToClear, 1, 0, 0);
            c!(m, RandEvent_UpdateData_d9ce8, 10, 2, 0, -1);
            Adv
        }
        0x1d0 => {
            if c!(m, RandEvent_SelectDataUpsilon_d9c7c) != 0 {
                c!(m, RandEvent_UpdateData_d9d94, 10, 2);
                c!(m, ScreenFade_ToWhite, 1, 0);
                let s = c!(m, Main_GetStateAtDepth);
                c!(m, Main_SetState, s - 3);
                c!(m, Schedule_AllocateAuxState);
                c!(m, Schedule_AllocateEventState);
                c!(m, TextRes_ProcessInputText, 6);
                c!(m, Overlay_Load, ov_own_card);
                c!(m, GameHeap_LoadOverlayInfoAlt, ov_own_card);
                c!(m, Bg_ClearState);
            }
            Stay
        }
        0x1e0 => {
            // friends' cards (ov16)
            overlay_branch(m);
            c!(m, Main_AdvanceStateSequence);
            c!(m, GfxRes_CallHookPtr);
            c!(m, Schedule_AllocateAuxState);
            c!(m, Schedule_AllocateEventState);
            c!(m, TextRes_ProcessInputText, 6);
            c!(m, Overlay_Load, ov_friends);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_friends);
            Stay
        }
        0x1e1 => {
            if c!(m, 0x2173844u32) == 0 {
                return Stay;
            }
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_friends);
            c!(m, TextRes_FreeBuffer, 6);
            Adv
        }
        // ---- ご当地 page: characters ----
        0x21c => {
            overlay_branch(m);
            c!(m, Main_AdvanceStateSequence);
            c!(m, GfxRes_CallHookPtr);
            c!(m, TextRes_ProcessInputText, 6);
            c!(m, Schedule_AllocateAuxState);
            c!(m, Schedule_AllocateEventState);
            c!(m, Overlay_Load, ov_gotochi_chara);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_gotochi_chara);
            Stay
        }
        0x21d => {
            if c!(m, 0x2178804u32) == 0 {
                return Stay;
            }
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_gotochi_chara);
            c!(m, TextRes_FreeBuffer, 6);
            Adv
        }
        0x21e => {
            c!(m, Schedule_FreeEventState);
            c!(m, Schedule_FreeAuxState);
            c!(m, BgLoad_CallHookPtr);
            c!(m, Display_ResetBgAndVram);
            Menu(4)
        }
        // ---- gallery data transfer (ov10) ----
        0x230 => {
            overlay_branch(m);
            c!(m, Overlay_Load, ov_gallery_xfer);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_gallery_xfer);
            c!(m, 0x2170890u32);
            Adv
        }
        0x231 => {
            if c!(m, 0x2171910u32) != 0 {
                return Stay;
            }
            c!(m, 0x21709d4u32);
            c!(m, Overlay_Unload, -1);
            c!(m, GameHeap_LoadOverlayInfo, ov_gallery_xfer);
            c!(m, Display_ResetBgAndVram);
            Menu(6)
        }
        // ---- the save data cannot be used ----
        0x154 => {
            c!(m, SaveSelect_Shutdown);
            c!(m, Gfx_Apply2dOnlyBanksAndInitVramMgrs);
            // the message's palette, copied to the stack
            let mut pal = [None; 8];
            for (i, w) in pal.iter_mut().enumerate() {
                let lo = m.ld_n(no_save_palette + 4 * i as u32, 2);
                let hi = m.ld_n(no_save_palette + 4 * i as u32 + 2, 2);
                *w = Some(((lo & 0xffff) | hi << 16) as i32);
            }
            let mut pal_b = pal;
            c!(m, GX_LoadBGPltt, Arg::Out(&mut pal), 0, 2);
            c!(m, GXS_LoadBGPltt, Arg::Out(&mut pal_b), 0, 2);
            let v = m.ld(DISPCNT_A);
            m.st_n(DISPCNT_A, (v & !0x1f00) | 0x1400, 4);
            let v = m.ld(dispcnt_b);
            m.st_n(dispcnt_b, (v & !0x1f00) | 0x1000, 4);
            c!(m, MsgBox_Open, no_save_text, 2, 0, 0xf);
            c!(m, MsgBox_DrawAdvanceMarker, 0);
            for (i, row) in [7, 6, 5, 4, 3].into_iter().enumerate() {
                c!(m, MsgBox_PrintLine, i as i32, row, 1);
            }
            c!(m, Main_SetState, 0x168);
            c!(m, ScreenFade_ToClear, 1, 0x14, 0);
            c!(m, ScreenFade_ToClear, 2, 0x14, 0);
            Stay
        }
        0x50 => {
            // (result 1) ov38
            c!(m, SaveSelect_Shutdown);
            c!(m, Overlay_Load, ov_menu38);
            c!(m, GameHeap_LoadOverlayInfoAlt, ov_menu38);
            c!(m, 0x21708acu32, 0);
            Adv
        }
        _ => {
            if WIRELESS.iter().any(|&(lo, hi)| (lo..hi).contains(&s)) {
                m.cx.not_ported(format!("SaveSelect state {s:#x} (a wireless feature)"));
            }
            Stay
        }
    }
}

// ---- SceneLoop A (file select chain, screen 1) / B (file delete, screen 2) ----------------------------------------

pub(crate) fn loop_a_init(m: &mut M) {
    let a = m.f.save_select.scroll_a;
    c!(m, GfxRes_CallHookPtr);
    c!(m, Screen_Request, 0, 1, -1);
    m.st(a, 0);
    m.st(a + 0x1000, 0);
}

pub(crate) fn loop_b_init(m: &mut M) {
    let b = m.f.save_select.scroll_b;
    c!(m, GfxRes_CallHookPtr);
    c!(m, Screen_Request, 0, 2, -1);
    m.st(b, 0);
    m.st(b + 0x1000, 0);
}

/// 1: backed out, 2: done (a game to start)
pub(crate) fn loop_a_run(m: &mut M) -> i32 {
    let mut ret = 0;
    let mut nxt = None;
    c!(m, Main_IncStateDepth);
    let s = c!(m, Main_GetStateAtDepth);
    match s {
        0 => {
            c!(m, SceneLoop_InitializeSpriteSystemA);
            nxt = Some(0x14);
        }
        0x14 => {
            nxt = match c!(m, Screen_Run) {
                1 => Some(0x1e),
                2 => Some(0x28),
                _ => None,
            }
        }
        0x1e | 0x28 => {
            if fades_done(m) {
                nxt = Some(if s == 0x1e { 0x32 } else { 0x3c });
            }
        }
        0x32 | 0x3c => {
            c!(m, SceneLoop_NopCallbackA);
            nxt = Some(0);
            ret = if s == 0x32 { 1 } else { 2 };
        }
        _ => {}
    }
    if let Some(n) = nxt {
        c!(m, Main_SetState, n);
    }
    c!(m, Main_DecStateDepth);
    ret
}

pub(crate) fn loop_b_run(m: &mut M) -> i32 {
    let mut ret = 0;
    let mut nxt = None;
    c!(m, Main_IncStateDepth);
    let s = c!(m, Main_GetStateAtDepth);
    match s {
        0 => {
            c!(m, SceneLoop_InitializeSpriteSystemB);
            nxt = Some(0x14);
        }
        0x14 => {
            if c!(m, Screen_Run) == 1 {
                nxt = Some(0x1e);
            }
        }
        0x1e => {
            if fades_done(m) {
                nxt = Some(0x32);
            }
        }
        0x32 => {
            c!(m, SceneLoop_NopCallbackB);
            nxt = Some(0);
            ret = 1;
        }
        _ => {}
    }
    if let Some(n) = nxt {
        c!(m, Main_SetState, n);
    }
    c!(m, Main_DecStateDepth);
    ret
}
