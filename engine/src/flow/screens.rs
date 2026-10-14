//! The screen manager (static 0x2089d8c..0x208ad90; the auto-labels call it Sprite_*): menu-type screens are requested,
//! run until they finish, and can hand over to another screen.
//!
//! Request block REQ (0x2152ea4):
//!     +0x00 run state  0 idle, 1 start, 2 build, 3 first draw, 5 running, 6 switched (restart at 1), 7 finished (result),
//!           8 finished (alternative result)
//!     +0x04 kind (0: a screen with an overlay; 1, 2: special set-ups)   +0x0c screen id   +0x10 next screen
//!     +0x14 result   +0x18 alternative result   +0x1c entry argument   +0x20 the screen object (0x29c bytes)
//!     +0x24 switch pending
//! A screen tells the manager what to do by the value its frame function returns (Screen_RunCurrent 0x20882e0):
//! 3 switch to the screen it set with 0x20882c8, 4 finish with the result it set with 0x2088624, 5 finish with the
//! alternative result (0x208a200).
use super::addr::*;
use super::code::case_literals;
use super::{c, M};
use crate::rom::Rom;

pub(crate) struct Consts {
    pub req: u32,
    /// screen id -> overlay (None: the screen is static code)
    overlay_of_screen: Vec<Option<u32>>,
    unload_of_screen: Vec<Option<u32>>,
    bg_mode_mask: u32,
    winout: u32,
    res_a: u32,
    res_b: u32,
}

impl Consts {
    pub fn new(rom: &Rom) -> Consts {
        Consts {
            req: rom.u32(0x2089e00),
            overlay_of_screen: case_literals(rom, 0x208a994),
            unload_of_screen: case_literals(rom, 0x208abd4),
            bg_mode_mask: rom.u32(0x208a1d4), // 0xffcfffef: clears the OBJ mapping bits
            winout: rom.u32(0x208a1d0),       // sub WINOUT 0x400104a
            res_a: rom.u32(0x208a1d8),
            res_b: rom.u32(0x208a1dc),
        }
    }

    /// the overlay a screen loads (None: static code)
    pub fn overlay_of(&self, screen: i32) -> Option<u32> {
        if (0..=20).contains(&screen) { self.overlay_of_screen.get(screen as usize).copied().flatten() } else { None }
    }
}

const DISPCNT_A: u32 = 0x4000000;
const DISPCNT_B: u32 = 0x4001000;

/// 0x2089d8c Screen_Request(kind, screen, arg)
pub(crate) fn request(m: &mut M, kind: i32, screen: i32, arg: Option<i32>) {
    let req = m.f.screens.req;
    m.st(req + 4, kind);
    m.st(req, 1);
    if kind == 0 {
        c!(m, Screen_LoadOverlay, screen);
        m.st(req + 0xc, screen);
        m.st(req + 0x10, -1);
        m.st_opt(req + 0x1c, arg);
    } else if kind == 1 || kind == 2 {
        m.st(req + 0xc, -1);
        m.st(req + 0x10, -1);
        m.st_opt(req + 0x1c, arg);
    }
}

/// 0x208a994: Overlay_Load + GameHeap_LoadOverlayInfoAlt of the screen's overlay
pub(crate) fn load_overlay(m: &mut M, screen: i32) {
    if let Some(ov) = m.f.screens.overlay_of(screen) {
        c!(m, Overlay_Load, ov);
        c!(m, GameHeap_LoadOverlayInfoAlt, ov);
    }
}

/// 0x208abd4: Overlay_Unload(all) + GameHeap_LoadOverlayInfo(the screen's overlay)
pub(crate) fn unload_overlay(m: &mut M, screen: i32) {
    let ov = if (0..=20).contains(&screen) { m.f.screens.unload_of_screen.get(screen as usize).copied().flatten() } else { None };
    if let Some(ov) = ov {
        c!(m, Overlay_Unload, -1);
        c!(m, GameHeap_LoadOverlayInfo, ov);
    }
}

/// 0x208a2b4: what the running screen asked for
pub(crate) fn transition(m: &mut M, mode: i32) {
    let req = m.f.screens.req;
    match mode {
        3 => {
            // switch to the next screen
            let cur = m.ld(req + 0xc);
            c!(m, Screen_UnloadOverlay, cur);
            m.st(req, 6);
            let nxt = c!(m, Screen_GetNext);
            m.st(req + 0xc, nxt);
            c!(m, Screen_LoadOverlay, nxt);
        }
        4 => {
            // finished with a result
            let cur = m.ld(req + 0xc);
            c!(m, Screen_UnloadOverlay, cur);
            m.st(req, 7);
            m.st(req + 0xc, -1);
            let r = c!(m, Screen_GetResult);
            m.st(req + 0x14, r);
        }
        5 => {
            // finished with the alternative result
            let r = c!(m, Screen_GetAltResult);
            m.st(req + 0x18, r);
            m.st(req, 8);
        }
        _ => {}
    }
}

fn rmw(m: &mut M, addr: u32, clear: u32, set: u32, size: u32) {
    let v = m.ld_n(addr, size);
    m.st_n(addr, (v & !clear) | set, size);
}

/// 0x2089e04 Screen_Run: one frame of the manager; -1 while the screen runs, its result when it finishes
pub(crate) fn run(m: &mut M) -> i32 {
    let k = &m.f.screens;
    let (req, mask, winout) = (k.req, k.bg_mode_mask, k.winout);
    let (res_a, res_b) = (k.res_a, k.res_b);
    let mut ret = -1;
    match m.ld(req) {
        1 => {
            // start: display set-up, the screen object
            c!(m, 0x2047d40u32); // GfxRes_CallHookPtr
            rmw(m, DISPCNT_A, 0xe000, 0, 4);
            rmw(m, DISPCNT_B, 0xe000, 0, 4);
            rmw(m, 0x400004a, 0x3f, 0, 2);
            rmw(m, winout, 0x3f, 0, 2);
            rmw(m, 0x4000048, 0x3f00, 0, 2);
            rmw(m, winout - 2, 0x3f00, 0, 2);
            c!(m, 0x201a950u32, 0x4000050u32, 0, 0, 0); // G2x_SetBlendAlpha_ (main)
            c!(m, 0x201a950u32, winout + 6, 0, 0, 0); // (sub)
            let kind = m.ld(req + 4);
            if kind == 0 {
                if m.ld(req + 0xc) != 0 {
                    c!(m, 0x201a73cu32); // GX_DispOn
                    rmw(m, DISPCNT_B, 0, 0x10000, 4);
                    c!(m, 0x20410e0u32);
                    c!(m, 0x2041e1cu32); // 2D-only VRAM banks
                    rmw(m, DISPCNT_A, 0x1f00, 0x1f00, 4);
                    rmw(m, DISPCNT_B, 0x1f00, 0x1f00, 4);
                    c!(m, 0x2084ed4u32); // Bg_ClearAll
                }
                let screen = m.ld(req + 0xc);
                let (a, b) = match screen {
                    5 => (0x200010, 0x10),
                    3 | 2 | 1 | 6 | 8 | 7 | 9 | 0xf | 0x10 | 0x11 | 4 | 0x12 | 0xb | 0xc | 0xd | 0xe | 0x13 | 0x14 => (0x200010, 0x200010),
                    _ => (0x10, 0x10),
                };
                let v = m.ld(DISPCNT_A);
                m.st_n(DISPCNT_A, (v & mask) | a, 4);
                let v = m.ld(DISPCNT_B);
                m.st_n(DISPCNT_B, (v & mask) | b, 4);
                let obj = c!(m, 0x203fda8u32, 0x29c); // GameHeap_AllocZeroed
                m.st(req + 0x20, obj);
                c!(m, 0x208757cu32, obj); // the object, still in r0
            } else if kind == 1 {
                c!(m, 0x201a73cu32);
                rmw(m, DISPCNT_B, 0, 0x10000, 4);
                c!(m, 0x2041de0u32); // default VRAM banks
                rmw(m, DISPCNT_A, 0x1f00, 0x1800, 4);
            }
            m.st(req, 2);
        }
        2 => {
            // build the screen
            if m.ld(req + 4) == 0 {
                c!(m, ResTable_Open, 1, res_a);
                let screen = m.ld(req + 0xc);
                let proc_ = c!(m, 0x2087538u32, screen); // the screen's procedure table
                let obj = m.ld(req + 0x20);
                c!(m, 0x20875fcu32, obj, proc_);
                let obj = m.ld(req + 0x20);
                let res = m.ld(obj + 0x11c);
                c!(m, ResTable_Open, 1, if res != 0 { res } else { res_b });
                c!(m, 0x208a344u32);
                c!(m, 0x208a488u32);
                c!(m, ResTable_Open, 1, res_b);
            }
            m.st(req, 3);
        }
        3 => {
            // first frame
            let kind = m.ld(req + 4);
            if kind == 0 {
                let screen = m.ld(req + 0xc);
                c!(m, 0x2088298u32, screen);
                let obj = m.ld(req + 0x20);
                if m.ld(obj + 0x298) != 0 {
                    c!(m, 0x2085360u32, 2, 1); // Bg_SelectState
                }
                let obj = m.ld(req + 0x20);
                let res = m.ld(obj + 0x11c);
                if res != 0 {
                    c!(m, ResTable_Open, 1, res);
                }
                c!(m, 0x208a5e4u32);
                c!(m, ResTable_Open, 1, res_b);
            } else if kind == 2 {
                c!(m, 0x2085360u32, 2, 0);
                c!(m, 0x2043084u32, 1, 0, 0); // Gfx_SetBgVisibleFlag
            }
            m.st(req, 5);
        }
        4 => m.st(req, 5),
        5 => {
            // running
            if m.ld(req + 4) == 0 {
                let mode = c!(m, Screen_RunCurrent);
                c!(m, Screen_Transition, mode);
            }
            c!(m, 0x2085568u32); // Bg_DispatchState
        }
        6 => {
            // switched: restart with the next screen
            c!(m, 0x2088288u32);
            c!(m, 0x2085018u32);
            m.st(req + 0x24, 0);
            c!(m, 0x2047d4cu32);
            m.st(req, 1);
        }
        7 => {
            // finished: hand back the result
            ret = m.lds(req + 0x14);
            c!(m, 0x2088288u32);
            c!(m, 0x2085018u32);
            m.st(req + 0x24, 0);
            c!(m, 0x2047d4cu32);
            m.st(req, 0);
            let v = m.ld(DISPCNT_A);
            m.st_n(DISPCNT_A, (v & mask) | 0x200010, 4);
        }
        8 => {
            ret = m.lds(req + 0x18);
            m.st(req, 5);
        }
        _ => {}
    }
    ret
}
