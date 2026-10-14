//! The game's control flow: the top state machine, the screen manager, the scene machine and the title menu's
//! SaveSelect machine, ported from the ARM code (docs/GAME_FLOW.md; the Python original is `sandbox/flow/lpflow`).
//!
//! A machine is plain code that does all its I/O through a context (`Cx`): calls to other game functions by address,
//! and loads / stores of globals. Two contexts drive it:
//! * `log::LogCx` replays a call log recorded from the game and checks the port call for call (targets, arguments,
//!   the loads and stores between two calls, every ported function's return value);
//! * the game's own context answers the calls with the engine's subsystems.
//! A call whose target is itself ported runs the port (the context sees it begin and end).
//!
//! Larger constants live in the code's literal pools and are read from the game image (`Rom`), as is the screen ->
//! overlay table (a switch in the code, `code::case_literals`).

pub mod addr;
pub mod code;
pub mod log;
mod saveselect;
mod scene;
mod screens;
mod top;

use crate::rom::Rom;

/// an argument of a call the game makes
pub enum Arg<'a> {
    I(i32),
    /// a value the port cannot know (a stack address): not compared
    Any,
    /// a stack buffer the callee fills (read its words after the call; None until filled)
    Out(&'a mut [Option<i32>]),
}

impl From<i32> for Arg<'_> {
    fn from(v: i32) -> Self {
        Arg::I(v)
    }
}
impl From<u32> for Arg<'_> {
    fn from(v: u32) -> Self {
        Arg::I(v as i32)
    }
}
impl From<Option<i32>> for Arg<'_> {
    fn from(v: Option<i32>) -> Self {
        v.map_or(Arg::Any, Arg::I)
    }
}
impl<'a> From<&'a mut [Option<i32>]> for Arg<'a> {
    fn from(v: &'a mut [Option<i32>]) -> Self {
        Arg::Out(v)
    }
}

impl Arg<'_> {
    pub fn int(&self) -> Option<i32> {
        match self {
            Arg::I(v) => Some(*v),
            _ => None,
        }
    }
}

/// what a machine runs against
pub trait Cx {
    /// a call to a game function that is not ported; its r0
    fn external(&mut self, target: u32, args: &mut [Arg]) -> i32;
    /// a ported function begins (called with `args`) ...
    fn enter(&mut self, target: u32, name: &'static str, args: &mut [Arg]);
    /// ... and returns `ret` (None: a void function); what the caller sees
    fn leave(&mut self, target: u32, ret: Option<i32>, args: &mut [Arg]) -> i32;
    fn load(&mut self, addr: u32, size: u32) -> u32;
    /// value None: one the port cannot know (from a stack buffer)
    fn store(&mut self, addr: u32, value: Option<u32>, size: u32);
    /// a branch on data only the callee's stack buffers hold: `cond` when known; a replay resolves it from the next
    /// call target
    fn pick(&mut self, cond: Option<bool>, targets: &[(bool, u32)]) -> bool;
    /// the machine reached code that is not ported (the wireless features)
    fn not_ported(&mut self, what: String);
}

type Port = fn(&mut M, &mut [Arg]) -> Option<i32>;

/// the constants the machines take from the game image
pub struct Flow {
    pub(crate) top: top::Consts,
    pub(crate) screens: screens::Consts,
    pub(crate) scene: scene::Consts,
    pub(crate) save_select: saveselect::Consts,
}

impl Flow {
    pub fn new(rom: &Rom) -> Flow {
        Flow { top: top::Consts::new(rom), screens: screens::Consts::new(rom), scene: scene::Consts::new(rom), save_select: saveselect::Consts::new(rom) }
    }

    /// the ported functions by address
    pub(crate) fn port(target: u32) -> Option<(&'static str, Port)> {
        Some(match target {
            0x203fb20 => ("Main_ScopedFadeCheck", |m, _| Some(top::scoped_fade_check(m))),
            0x204b0c0 => ("Top_Run", |m, _| Some(top::run(m))),
            0x204afdc => ("Top_BootOverlays", |m, _| Some(top::boot_overlays(m))),
            0x2089d8c => ("Screen_Request", |m, a| {
                screens::request(m, a[0].int().unwrap_or(0), a[1].int().unwrap_or(0), a[2].int());
                None
            }),
            0x2089e04 => ("Screen_Run", |m, _| Some(screens::run(m))),
            0x208a2b4 => ("Screen_Transition", |m, a| {
                screens::transition(m, a[0].int().unwrap_or(0));
                None
            }),
            0x208a994 => ("Screen_LoadOverlay", |m, a| {
                screens::load_overlay(m, a[0].int().unwrap_or(-1));
                None
            }),
            0x208abd4 => ("Screen_UnloadOverlay", |m, a| {
                screens::unload_overlay(m, a[0].int().unwrap_or(-1));
                None
            }),
            0x204a0c4 => ("Scene_Run", |m, _| Some(scene::run(m))),
            0x2074600 => ("SaveSelect_Run", |m, _| Some(saveselect::run(m))),
            0x209f4cc => ("SceneLoopA_Init", |m, _| {
                saveselect::loop_a_init(m);
                None
            }),
            0x209f50c => ("SceneLoopA_Run", |m, _| Some(saveselect::loop_a_run(m))),
            0x209f618 => ("SceneLoopB_Init", |m, _| {
                saveselect::loop_b_init(m);
                None
            }),
            0x209f658 => ("SceneLoopB_Run", |m, _| Some(saveselect::loop_b_run(m))),
            _ => return None,
        })
    }

    /// one frame of the game's flow (GameHeap_UpdateGameState -> Main_ScopedFadeCheck); 2 when the game shuts down.
    /// The frame's entry is the hook itself: no call record exists for it, so it is run directly.
    pub fn frame(&self, cx: &mut dyn Cx) -> i32 {
        top::scoped_fade_check(&mut M { f: self, cx })
    }
}

/// a machine's view: the constants and the context
pub(crate) struct M<'a> {
    pub f: &'a Flow,
    pub cx: &'a mut dyn Cx,
}

impl M<'_> {
    pub fn call(&mut self, target: u32, args: &mut [Arg]) -> i32 {
        match Flow::port(target) {
            Some((name, port)) => {
                self.cx.enter(target, name, args);
                let r = port(&mut M { f: self.f, cx: &mut *self.cx }, args);
                self.cx.leave(target, r, args)
            }
            None => self.cx.external(target, args),
        }
    }
    pub fn ld(&mut self, addr: u32) -> u32 {
        self.cx.load(addr, 4)
    }
    pub fn lds(&mut self, addr: u32) -> i32 {
        self.cx.load(addr, 4) as i32
    }
    pub fn ld_n(&mut self, addr: u32, size: u32) -> u32 {
        self.cx.load(addr, size)
    }
    pub fn st(&mut self, addr: u32, v: i32) {
        self.cx.store(addr, Some(v as u32), 4)
    }
    pub fn st_n(&mut self, addr: u32, v: u32, size: u32) {
        self.cx.store(addr, Some(v), size)
    }
    pub fn st_opt(&mut self, addr: u32, v: Option<i32>) {
        self.cx.store(addr, v.map(|v| v as u32), 4)
    }
}

/// `c!(m, TARGET, a, b, ...)`: a call with arguments converted to `Arg`
macro_rules! c {
    ($m:expr, $t:expr $(, $a:expr)* $(,)?) => {
        $m.call($t, &mut [$($crate::flow::Arg::from($a)),*])
    };
}
pub(crate) use c;
