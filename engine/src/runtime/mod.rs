//! The game running on the engine (docs/RUNTIME.md): the ported flow machines (`flow`) and the game's own code
//! (`interp`) over the game's memory (`ram`). Every function call goes through a registry (`ext`) that says what
//! answers it:
//! * `Native`: game logic or SDK hardware edge ported to the engine (`natives`, `sdk`), on RAM at the game's addresses;
//! * `Subsystem`: a part of the game an engine subsystem may provide (a declined call runs as game code);
//! * `Pending`: not ported: the game's own code runs in the interpreter, counted in `Machine::pending`.
//! A function pointer the game calls through (a hook in its data) is read from RAM and dispatched through the same
//! registry, so indirect calls land on the game's own targets.
//!
//! The frame driver follows Main_UpdateFrame 0x203d9d4: input, then (outside 3D capture phases) the game state
//! update GameHeap_UpdateGameState 0x203fb50 (state 0: GameHeap_StartGameLoad, 5: the flow), the graphics, then
//! ScreenFade_Process; the ARM7's sound driver (`snd7`) catches up with the game's clock at each frame start.

pub mod check;
pub mod ext;
pub mod gpu2d;
pub mod gpu3d;
pub mod gx;
pub mod interp;
pub mod natives;
pub mod ppu;
pub mod ram;
pub mod raster3d;
pub mod sdk;
pub mod present;
pub mod profile;
pub mod snd7;
pub mod texcache;
pub mod vram;

use crate::flow::{Arg, Cx, Flow};
use crate::rom::Rom;
use ram::Ram;
use std::collections::BTreeMap;

/// what the player does (touch in DS bottom-screen pixels, keys as the DS pad bits)
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub touch: Option<(u16, u16)>,
    pub keys: u16,
}

/// an engine-side part of the game: answers the calls the registry routes to it
pub trait Subsystems {
    /// a registry entry of kind `Subsystem(name)` was called; None: not handled (counted as pending)
    fn call(&mut self, g: &mut Machine, target: u32, name: &'static str, args: &mut [Arg]) -> Option<i32>;
}

/// no engine subsystems: everything they would answer is reported as pending
pub struct NoSubsystems;
impl Subsystems for NoSubsystems {
    fn call(&mut self, _: &mut Machine, _: u32, _: &'static str, _: &mut [Arg]) -> Option<i32> {
        None
    }
}

/// the game's state: its memory and the engine-side bookkeeping natives need
pub struct Machine {
    pub rom: Rom,
    pub ram: Ram,
    /// the card ROM the SDK's file system reads (content/fs), and the reads made (offset, length)
    pub vrom_reads: Vec<(u32, u32)>,
    pub input: Input,
    /// the touch panel's last pressed position: a pen-up sample still carries it (measured in the emulator)
    pub tp_last: (u16, u16),
    /// host time as the DS real-time clock reports it: (year 0-99, month, day, weekday, hour, minute, second)
    pub rtc: [u32; 7],
    /// when set, the clock is this local time (seconds since 1970) at tick 0 and runs with the game's clock instead
    /// of being set from outside (repeatable runs)
    pub rtc_base: Option<i64>,
    pub frame: u64,
    /// calls answered by a Pending entry or an unhandled subsystem: name -> count
    pub pending: BTreeMap<&'static str, u64>,
    /// non-ported targets reached through function pointers: address -> count
    pub unknown: BTreeMap<u32, u64>,
    pub not_ported: Vec<String>,
    /// functions run as the game's code (interp): address -> calls
    pub interpreted: BTreeMap<u32, u64>,
    /// profiling (`--profile`): instructions run per halfword of the game's code (`arm::profile_index`), and the
    /// counts set aside each time the overlays change, with the overlays loaded then (id, base, end)
    pub profile: Option<Vec<u32>>,
    pub profile_epochs: Vec<(Vec<(u32, u32, u32)>, Vec<(u32, u32)>)>,
    pub overlays_loaded: Vec<(u32, u32, u32)>,
    /// the voice decoder's state per (game decoder state, channel), and its tables (content/engine/g7221.pack)
    pub g7221: std::collections::HashMap<(u32, u32), crate::siren::Decoder<'static>>,
    pub g7221_tables: Option<&'static crate::siren::Tables>,
    /// frames the voice decoder has decoded
    pub g7221_frames: u64,
    /// instructions the interpreter ran
    pub steps: u64,
    /// errors the interpreter stopped at
    pub faults: Vec<String>,
    /// the stack top for the next interpreted call (below the frames of interpreted callers)
    pub sp_top: u32,
    pub trap_depth: u32,
    /// the return address (lr) of the game's call the engine is answering now (natives that tell callers apart)
    pub trap_lr: u32,
    /// a native's high return word (r1) for a 64-bit result
    pub ret_hi: Option<u32>,
    /// r1 when the last interpreted call returned (for natives that return 64 bits or two values, checked against
    /// the game's own code)
    pub last_r1: u32,
    /// LP_CHECK_NATIVE=<address>: every call to that native also runs the game's function from the same state and
    /// compares memory and the geometry engine (counts: compared, equal, and differences by place)
    pub check_native: Option<u32>,
    pub check_counts: (u64, u64, std::collections::BTreeMap<String, u64>),
    /// a script id handed to the next scene start through the game's debug override (`CharId_GetGlobalValue`, the
    /// word at 0x214695c that MapArc_GetActiveCharIdAndMode reads first; nothing in the game writes it): served until
    /// the script start (0x205e7c4) receives it, then the game's own selection resumes. Starts served: (frame, id)
    pub event_override: Option<u32>,
    /// proportional text (the English patch's `patch/text.json` {"proportional": true}): per font (LC12, LC10) and
    /// ASCII character, where its ink starts and ends from the pen position (natives::text_get_glyph_width)
    pub text_ink: Option<Box<[[(i8, i8); 95]; 2]>>,
    /// the English patch's speaker names (`patch/text.json` "speakers"): each English name and the original names it
    /// replaces, so the game still tells who speaks (natives::char_lookup_id_by_name)
    pub speakers: std::collections::HashMap<Vec<u8>, Vec<Vec<u8>>>,
    pub events_served: Vec<(u64, u32)>,
    /// instructions one interpreted call may run before it is reported as stuck (generous: saving LZ-compresses the
    /// save data in one call, MI_CompressLZ, well over 20 million instructions)
    pub budget: u64,
    /// the chain of calls in progress (engine and interpreted), for fault reports
    pub chain: Vec<u32>,
    /// SDK thread switches performed
    pub switches: u64,
    /// PXI receive callbacks the SDK registered (tag -> function) and words sent per tag
    pub pxi_callbacks: BTreeMap<u32, u32>,
    pub pxi_sent: BTreeMap<u32, u64>,
    /// the words of a microphone command still being sent (PXI tag 9, consecutive packets)
    pub mic_packet: Vec<u32>,
    /// the microphone: the host's input, auto sampling, recorded samples (sdk::Mic)
    pub mic: sdk::Mic,
    /// the game asked to sleep (PM_GoSleepMode, the lid closed): the host holds the passes until the lid opens
    pub sleeping: bool,
    /// the display capture the last game pass made (its DISPCAPCNT), for presentation (`smooth`)
    pub last_capture: Option<u32>,
    /// the ARM7's sound driver
    pub snd: snd7::Snd7,
    /// the clock when the game started the OS tick (timer 0, OS_InitTick): OS_GetTick counts from here, as the
    /// hardware timer does
    pub tick_base: Option<u64>,
    /// diagnostics (LP_SHADOW): every pure native getter is also run as the game's own code and the results
    /// compared, per function: (agree, disagree, first disagreements as (frame, native, game))
    pub shadow: Option<BTreeMap<&'static str, (u64, u64, Vec<(u64, i32, i32)>)>>,
    /// the battery save (the backup chip's contents), whether the game changed it, and the requests made
    pub backup: Vec<u8>,
    pub backup_dirty: bool,
    pub card_requests: BTreeMap<u32, u64>,
    /// the card's chip ID (from the header's capacity)
    pub card_id: u32,
    /// the interpreter's recent calls (diagnostics; None: off)
    pub trace: Option<std::collections::VecDeque<(u32, u32)>>,
    /// addresses where interpretation stops with a report (diagnostics)
    pub breakpoints: Vec<u32>,
    /// interpretation returns when execution reaches this address (power-on: the end of the game's crt0)
    pub stop_at: Option<u32>,
    /// (frame, function, directory): snapshot memory when that call starts and log its instructions (diagnostics)
    pub snapshot_at: Option<(u64, u32, String)>,
    pub pclog: Option<Vec<[u32; 17]>>,
    pub pclog_owner: Option<u32>,
    /// LP_PCLOG=start,end,path: every instruction between pc `start` and pc `end` logged (pc and registers after it)
    pub pclog_window: Option<(u32, u32, String)>,
    pub pclog_done: bool,
    /// the SDK's tick (OS_GetTick: the CPU clock / 64), advanced by frames and by idle waits
    pub tick: u64,
    /// running an interrupt handler (the interpreter reports IRQ mode)
    pub in_irq: u32,
    /// the tick of the next VBlank, and instructions run toward the next tick
    pub vblank_tick: u64,
    pub instr_carry: u64,
    /// interrupts delivered, and halts (all threads waiting) resolved by passing time
    pub irqs: u64,
    pub irq_counts: BTreeMap<u32, u64>,
    pub idle_waits: u64,
}

impl Machine {
    pub fn new(rom: Rom) -> Machine {
        let text_ink = natives::proportional_text_ink(&rom);
        let speakers = natives::patch_speakers(&rom);
        let mut ram = Ram::new(&rom);
        ram.vrom = sdk::VirtualRom::open(rom.content_dir());
        let mut m = Machine {
            rom,
            ram,
            vrom_reads: Vec::new(),
            rtc_base: None,
            input: Input::default(),
            tp_last: (0, 0),
            rtc: [0, 1, 1, 0, 0, 0, 0],
            frame: 0,
            pending: BTreeMap::new(),
            unknown: BTreeMap::new(),
            not_ported: Vec::new(),
            interpreted: BTreeMap::new(),
            profile: None,
            profile_epochs: Vec::new(),
            overlays_loaded: Vec::new(),
            g7221: Default::default(),
            g7221_tables: None,
            g7221_frames: 0,
            steps: 0,
            faults: Vec::new(),
            sp_top: interp::STACK_TOP,
            trap_depth: 0,
            trap_lr: 0,
            ret_hi: None,
            last_r1: 0,
            check_native: std::env::var("LP_CHECK_NATIVE").ok().and_then(|v| u32::from_str_radix(v.trim_start_matches("0x"), 16).ok()),
            check_counts: Default::default(),
            event_override: None,
            text_ink,
            speakers,
            events_served: Vec::new(),
            budget: 4_000_000_000,
            chain: Vec::new(),
            switches: 0,
            pxi_callbacks: BTreeMap::new(),
            pxi_sent: BTreeMap::new(),
            mic_packet: Vec::new(),
            mic: Default::default(),
            sleeping: false,
            last_capture: None,
            snd: Default::default(),
            tick_base: None,
            shadow: std::env::var("LP_SHADOW").ok().map(|_| BTreeMap::new()),
            backup: vec![0xff; 0x80000],
            backup_dirty: false,
            card_requests: BTreeMap::new(),
            card_id: 0,
            trace: None,
            breakpoints: Vec::new(),
            stop_at: None,
            snapshot_at: None,
            pclog: None,
            pclog_owner: None,
            pclog_window: std::env::var("LP_PCLOG").ok().and_then(|v| {
                let p: Vec<&str> = v.split(',').collect();
                let h = |x: &str| u32::from_str_radix(x.trim_start_matches("0x"), 16).ok();
                Some((h(p.first()?)?, h(p.get(1)?)?, p.get(2)?.to_string()))
            }),
            pclog_done: false,
            tick: 0,
            in_irq: 0,
            vblank_tick: TICKS_PER_FRAME,
            instr_carry: 0,
            irqs: 0,
            irq_counts: BTreeMap::new(),
            idle_waits: 0,
        };
        interp::install_traps(&mut m);
        m
    }

    /// a word of the game image's literal pools (the addresses and constants the code uses)
    /// the game's crt0 from `entry` until it calls _fp_init (0x2034a08): the program as the card holds it unpacked,
    /// ITCM / DTCM filled, the bss cleared
    pub fn run_crt0(&mut self, subs: &mut dyn Subsystems, entry: u32) {
        const FP_INIT: u32 = 0x2034a08;
        self.stop_at = Some(FP_INIT);
        self.ram.set_trap(FP_INIT);
        interp::interpret(self, subs, entry, &mut []);
        self.stop_at = None;
        if ext::entry(FP_INIT).map_or(true, |e| matches!(e.kind, ext::Kind::Pending)) {
            self.ram.unset_trap(FP_INIT);
        }
        self.ram.boot_entry = None;
    }

    pub fn lit(&self, addr: u32) -> u32 {
        self.rom.u32(addr)
    }
}

/// SDK ticks per frame: the ARM9 bus clock 33,513,982 Hz / 64, at 59.8261 frames per second
pub const TICKS_PER_FRAME: u64 = 8753;
/// OS ticks per second (the ARM9 bus clock / 64)
pub const TICK_HZ: f64 = 33_513_982.0 / 64.0;

impl Machine {
    /// emulated CPU time a native port takes in place of the game code it replaces: `instructions` at the
    /// interpreter's rate (`interp::INSTR_PER_TICK`), as running that code would have advanced the clock
    pub fn spend(&mut self, instructions: u64) {
        self.instr_carry += instructions;
        self.tick += self.instr_carry / interp::INSTR_PER_TICK;
        self.instr_carry %= interp::INSTR_PER_TICK;
        if self.in_irq == 0 && self.tick >= self.vblank_tick {
            self.vblank_tick += TICKS_PER_FRAME;
        }
    }

    /// set the profile's counts aside with the overlays loaded now (before the overlays change, and at the end)
    pub fn profile_flush(&mut self) {
        let Some(p) = self.profile.as_mut() else { return };
        let counts: Vec<(u32, u32)> = p.iter().enumerate().filter(|(_, &n)| n > 0).map(|(i, &n)| (crate::arm::profile_pc(i), n)).collect();
        p.iter_mut().for_each(|n| *n = 0);
        if !counts.is_empty() {
            self.profile_epochs.push((self.overlays_loaded.clone(), counts));
        }
    }

    /// the last swapped 3D frame drawn as engine A's 3D layer
    pub fn layer3d(&self) -> Vec<u16> {
        let mut t = raster3d::Target::new(1);
        t.draw(&self.ram, &self.ram.gx.frame);
        t.layer()
    }

    /// both screens as RGBA8 (top, bottom), 256 x 192 each
    pub fn screens(&self, layer3d: Option<&[u16]>) -> (Vec<u8>, Vec<u8>) {
        let a = ppu::render(&self.ram, 0, layer3d);
        let b = ppu::render(&self.ram, 1, None);
        if ppu::a_on_top(&self.ram) { (a, b) } else { (b, a) }
    }
}

/// the game: the machine, the flow ported from its code, and the engine's subsystems
pub struct Game<S: Subsystems> {
    pub m: Machine,
    pub flow: Flow,
    pub subs: S,
}

impl<S: Subsystems> Game<S> {
    pub fn new(rom: Rom, subs: S) -> Game<S> {
        let flow = Flow::new(&rom);
        Game { m: Machine::new(rom), flow, subs }
    }

    /// what the DS does from reset to the first frame: crt0's memory set-up (done by `Ram::new`), its interrupt vector
    /// (OS_IrqHandler 0x1ff80d0 at 0x27e3ffc), _fp_init, NitroStartUp, the static constructors, then NitroMain's
    /// Main_InitializeRuntime and Main_Startup (0x2000c38). Returns the faults the interpreter stopped at.
    pub fn power_on(&mut self) -> Vec<String> {
        // what the firmware leaves at the top of main RAM for a card boot: the cartridge header at 0x27ffe00 and the
        // boot type (1: ROM) at 0x27ffc40
        if let Ok(h) = std::fs::read(self.m.rom.content_dir().join("fs/header.bin")) {
            self.m.ram.put(0x27ffe00, &h[..0x170.min(h.len())]);
            self.m.card_id = sdk::card_id_from_header(&h);
            self.m.ram.card.card_id = self.m.card_id;
            self.m.ram.card.capacity = h.get(0x14).map_or(0, |&n| 0x20000u32.checked_shl(n as u32).unwrap_or(0));
        }
        self.m.ram.set_u16(0x27ffc40, 1);
        self.m.ram.set_u32(0x27ff800, self.m.card_id);
        self.m.ram.set_u32(0x27ffc00, self.m.card_id);
        self.m.ram.set_u32(0x2ffff9c, 0);
        sdk::write_user_info(&mut self.m); // the user settings and Wi-Fi identity the ARM7 copies from the firmware
        self.m.ram.set_u32(0x27e3ffc, 0x1ff80d0);
        if let Some(entry) = self.m.ram.boot_entry {
            // the game's own start-up code (crt0) on the program as loaded from the card: it unpacks the program,
            // copies the ITCM / DTCM blocks, clears memory and the bss, then calls _fp_init, where it hands over
            self.run_crt0(entry);
            if !self.m.faults.is_empty() {
                return std::mem::take(&mut self.m.faults);
            }
        }
        for f in [0x2034a08u32, 0x2000b60, 0x2031ba0, 0x203d8b0, 0x203d8f4] {
            // _fp_init, NitroStartUp, __call_static_initializers, Main_InitializeRuntime, Main_Startup
            interp::interpret(&mut self.m, &mut self.subs, f, &mut []);
            if !self.m.faults.is_empty() {
                break;
            }
        }
        std::mem::take(&mut self.m.faults)
    }

    /// crt0 from `entry` until it calls _fp_init (0x2034a08)
    pub fn run_crt0(&mut self, entry: u32) {
        self.m.run_crt0(&mut self.subs, entry);
    }

    /// one pass of Main_UpdateFrame's loop (0x203d9d4), ported: input, touch, power, then (outside the 3D capture's
    /// odd phases) sound and the game state update (the flow), then the frame's graphics, VRAM work and the fade.
    /// The VBlank wait happens inside the game's own code (its OS_WaitVBlankIntr idles until the interrupt).
    /// Returns 1 when the power manager asks for a reset (the loop's exit).
    pub fn frame(&mut self) -> i32 {
        let c = |g: &mut Self, a: u32, args: &[i32]| {
            let mut v: Vec<Arg> = args.iter().map(|&x| Arg::I(x)).collect();
            ext::call(&mut g.m, &mut g.subs, a, &mut v)
        };
        sdk::write_keys(&mut self.m);
        sdk::mic_advance(&mut self.m, &mut self.subs);
        if let Some(b) = self.m.rtc_base {
            self.m.rtc = sdk::rtc_from_local_seconds(b + (self.m.tick as f64 / TICK_HZ) as i64);
        }
        // the ARM7's sound driver catches up with the time the last frame took (its alarms interrupt the ARM9)
        snd7::Snd7::advance(&mut self.m, &mut self.subs);
        let (flag, counter) = (self.m.lit(0x203daf4), self.m.lit(0x203daf8));
        let off_phase = |g: &mut Self, arg: i32| c(g, 0x2043030, &[arg]) == 0 || c(g, 0x2043064, &[]) == 0;
        let n = self.m.ram.i32(counter);
        let poll = off_phase(self, n);
        if poll {
            c(self, 0x204065c, &[]); // Input_Poll
        }
        c(self, 0x2040b58, &[poll as i32]); // Input_SwitchMode
        c(self, 0x204dc5c, &[0]); // Touch_Update
        let p = c(self, 0x2045a68, &[]); // PowerMgr_Process
        if p == 1 {
            return 1;
        }
        if p == 3 {
            self.m.ram.set_u32(flag, 1);
        }
        if off_phase(self, 0) {
            c(self, 0x2070b50, &[]); // GameSnd_NoOp
            c(self, 0x2072a78, &[]); // GameSnd_TickFromMain
            self.update_game_state();
            if c(self, 0x2045a2c, &[]) != 0 {
                c(self, 0x2045a3c, &[0]); // PowerMgr_Calculate_4
            }
        }
        // the game's task pool runs the tasks of the 3D capture's current phase (its r0 is the phase just read;
        // the kiss's 2D effect is a phase-1 task)
        let phase = c(self, 0x2043064, &[]); // Capture3D_GetCurrentPhase
        c(self, 0x204032c, &[phase]); // Input_ResetSlotsById: the task pool's tick
        c(self, 0x2042d24, &[]); // Gfx_RenderCapturedFrame
        c(self, 0x20468dc, &[]); // Mod2045ee8_ProcessessGraphics_10
        c(self, 0x20490e0, &[]); // Sprite_DrawAll
        if off_phase(self, 0) {
            c(self, 0x204deec, &[0]); // Touch_ResetForMode
            self.m.ram.set_u32(flag, 0);
        }
        c(self, 0x2044ee8, &[]); // VramBank_Process
        natives::screen_fade_process(&mut self.m);
        // the frame is displayed: a requested display capture takes engine A's picture into VRAM
        self.m.last_capture = None;
        if self.m.ram.u32(0x4000064) & 0x8000_0000 != 0 {
            self.m.last_capture = Some(self.m.ram.u32(0x4000064));
            let l3 = self.m.layer3d();
            ppu::capture(&mut self.m.ram, Some(&l3));
        }
        let n = self.m.ram.u32(counter).wrapping_add(1);
        self.m.ram.set_u32(counter, n);
        self.m.frame += 1;
        0
    }

    /// GameHeap_UpdateGameState 0x203fb50: the depth-1 state runs the boot load (0), the flow (5) or nothing (7)
    pub fn update_game_state(&mut self) -> i32 {
        let s = natives::main_get_state(&self.m);
        let next = match s {
            0 => natives::game_heap_start_game_load(&mut self.m, &mut self.subs),
            5 => {
                let mut cx = GameCx { m: &mut self.m, subs: &mut self.subs };
                self.flow.frame(&mut cx)
            }
            _ => 0,
        };
        if next != 0 {
            natives::main_set_state(&mut self.m, &mut self.subs, next);
        }
        next
    }
}

/// the flow machines' context over the machine
pub struct GameCx<'a> {
    pub m: &'a mut Machine,
    pub subs: &'a mut dyn Subsystems,
}

impl Cx for GameCx<'_> {
    fn external(&mut self, target: u32, args: &mut [Arg]) -> i32 {
        ext::call(self.m, self.subs, target, args)
    }
    fn enter(&mut self, _: u32, _: &'static str, _: &mut [Arg]) {}
    fn leave(&mut self, _: u32, ret: Option<i32>, _: &mut [Arg]) -> i32 {
        ret.unwrap_or(0)
    }
    fn load(&mut self, addr: u32, size: u32) -> u32 {
        self.m.ram.read(addr, size)
    }
    fn store(&mut self, addr: u32, value: Option<u32>, size: u32) {
        if let Some(v) = value {
            self.m.ram.write(addr, v, size)
        }
    }
    fn pick(&mut self, cond: Option<bool>, _: &[(bool, u32)]) -> bool {
        cond.unwrap_or(false)
    }
    fn not_ported(&mut self, what: String) {
        self.m.not_ported.push(what);
    }
}
