//! `SceneSim`: one heroine scene, frame by frame (reference: the per-update loop of `sandbox/gx/export_fixtures.py`, which is
//! `scene_video.sim_timeline`). Order inside an update, as traced in the game (`Main_UpdateFrame`, `Model_UpdateCharaScene`):
//!
//! 1. ScreenFade passes of the cycle (3), then the task list and the script (`sched`); the update's fade commands start fades
//! 2. lip-sync (`VoiceQueue_UpdateMode`) with the voice stream that started 4 updates after its request
//! 3. eye step (`UiAsset_UpdateSceneVariants`): it sees the mood from before this update's motion start
//! 4. the motion commands, then `MapCtl_UpdateTouchSelection` (motion start, blend, state machine)
//! 5. `Gfx3D_UpdateActorState`: eye request and hand poses from the visibility tracks at the current frame
//! 6. frame advance (`Mod2045ee8_ProcessessGraphics_10`)
//!
//! Then, per frame: the typewriter, then the player's tap releases.

use crate::game::{GameState, Rng};
use crate::host::{DefaultPolicy, Effect, Host, Policy};
use crate::machine::{MEvent, Machine, ScriptStore};
use crate::rom::Rom;
use crate::save::SaveFacts;
use crate::scene_inputs as si;
use crate::sim::clock::{Clock, PhaseSource};
use crate::sim::eyes::{EyeSim, EyeTables};
use crate::sim::fade::ScreenFade;
use crate::sim::lipsync::{block_peaks, LipKeys, LipSync};
use crate::sim::motion::{LayerRef, MotionData, MotionSim};
use crate::sim::sched::{ScriptEvent, Scheduler, VOICE_START};
use crate::siren::{decode_clip, Tables};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// what the scene starts from (everything else comes from game data)
pub struct SceneSetup {
    pub content: PathBuf,
    pub event_id: u32,
    pub save: SaveFacts,
    /// game clock (month 1-12, schedule hour)
    pub month: u32,
    pub hour: u32,
    /// the Capture3D phase of each frame (`Nominal` live; the recorded phases in a replay)
    pub phases: Box<dyn PhaseSource + Send>,
    pub first_frame: i64,
    /// update in which the script starts (after the event load)
    pub start_update: u64,
    /// camera preset in force when the event starts (`None`: the previous event's last cmd 6)
    pub prev_preset: Option<i32>,
    /// RNG state at the scene start (runtime state; needed only by blinks under mood 0x1a)
    pub rng: Option<Rng>,
    pub policy: Box<dyn Policy + Send>,
    /// string variables {vN} (player name etc.); unset ones stay as "{vN}"
    pub vars: HashMap<u16, String>,
    /// script-visible state loaded from the save (`Save::game_state`: flags, counters, heroine records, name); `None`: a
    /// fresh state
    pub game: Option<GameState>,
}

/// the state presentation needs after an update
#[derive(Clone, Debug, PartialEq)]
pub struct SceneFrame {
    pub update: u64,
    /// last frame of the update's phase-0 run
    pub frame: i64,
    pub heroine: u32,
    /// wardrobe numbers (model names `<h>_01_<outfit>_*`, `<h>_00_<hair>_hair`) and the shading time mode
    pub outfit: Option<u32>,
    pub hair: u32,
    pub time_mode: u32,
    /// animation layers, head first
    pub chain: Vec<LayerRef>,
    pub mood: i32,
    pub eye_tex: i32,
    pub eye_mode: i32,
    pub eye_trans: i32,
    /// hand pose per side (left, right), the last value the tracks gave
    pub hands: [Option<i32>; 2],
    /// lip-sync shape (5 = rest: the mood's default mouth)
    pub mouth: i32,
    /// ScreenFade 16.16 after this update, and as seen at the end of the cycle (two more passes)
    pub fade_cur: i32,
    pub seen_cur: i32,
    pub preset: i32,
    /// accessory key id * 100 + variant (script 12e 10: `SceneState_UpdateFlaggedField` -> `Model_SetPartSlot1FromId`)
    pub accessory: Option<u32>,
}

/// something for the audio side or a front end, stamped with its frame
#[derive(Clone, Debug, PartialEq)]
pub enum SimEvent {
    Script(ScriptEvent),
    /// a voice stream starts (`VoiceQueue_PlayEntry`)
    Voice { frame: i64, clip: u32 },
    /// a sound command (bgm, se, ambient, stops) issued in the update ending at `frame`
    Sound { frame: i64, effect: Effect },
}

struct Voices {
    tables: Tables,
    vtb: Vec<u8>,
    spp: Vec<u8>,
    peaks: HashMap<u32, Arc<Vec<i32>>>,
}

impl Voices {
    fn pcm(&self, clip: u32) -> Option<Vec<i16>> {
        let o = clip as usize * 8;
        let e = self.vtb.get(o..o + 8)?;
        let (off, size) = (u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize / 16, u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize / 16);
        Some(decode_clip(&self.tables, self.spp.get(off..off + size)?, 16000, 14000).0)
    }

    fn peaks(&mut self, clip: u32) -> Arc<Vec<i32>> {
        if let Some(p) = self.peaks.get(&clip) {
            return p.clone();
        }
        let p = Arc::new(self.pcm(clip).map(|x| block_peaks(&x)).unwrap_or_default());
        self.peaks.insert(clip, p.clone());
        p
    }
}

/// camera preset of the previous event of a chain (id - 1): its last cmd 6
pub fn prev_event_preset(store: &Arc<ScriptStore>, event_id: u32, gs: GameState) -> Option<i32> {
    let mut m = Machine::start(store.clone(), event_id.wrapping_sub(1)).ok()?;
    let mut host = Host::new(store.clone(), gs);
    let mut pol = DefaultPolicy;
    let (mut reply, mut last) = (None, None);
    for _ in 0..20_000 {
        match m.step(reply.take()) {
            MEvent::Cmd { id, args, strs } => {
                if id == 6 {
                    last = args.first().map(|&a| a as i32);
                }
                reply = host.command(id, &args, &strs, &mut pol).0;
            }
            MEvent::Text { mode, .. } => reply = mode.filter(|m| (2..=5).contains(m)).map(|_| 1),
            MEvent::End(_) | MEvent::UnresolvedCall(_) | MEvent::Limit(_) | MEvent::Fault(_) | MEvent::BadOp { .. } => break,
            _ => {}
        }
    }
    last
}

pub struct SceneSim {
    pub clock: Clock,
    first_frame: i64,
    start_update: u64,
    pub sched: Scheduler,
    pub fade: ScreenFade,
    motion: MotionSim,
    mdata: MotionData,
    eye: EyeSim,
    eyet: EyeTables,
    lip: LipSync,
    lipkeys: LipKeys,
    voices: Voices,
    voice_at: BTreeMap<u64, u32>,
    mouth: i32,
    mouth_hold: i32,
    prev_end: Option<i64>,
    hands: [Option<i32>; 2],
    preset: i32,
    heroine: u32,
    arc: PathBuf,
    rom: Arc<Rom>,
    outfit: Option<u32>,
    accessory: Option<u32>,
    hair: u32,
    time_mode: u32,
    pub last: Option<SceneFrame>,
    pub events: Vec<SimEvent>,
}

impl SceneSim {
    pub fn new(rom: Arc<Rom>, store: Arc<ScriptStore>, setup: SceneSetup) -> Result<SceneSim, String> {
        let c = &setup.content;
        let heroine = si::heroine_of_event(setup.event_id).ok_or("event id names no heroine")?;
        let mut gs = setup.game.clone().unwrap_or_else(|| GameState::new(1));
        gs.clock.month = setup.month;
        gs.clock.hour = setup.hour;
        gs.current_slot = heroine;
        let preset = match setup.prev_preset {
            Some(p) => p,
            None => prev_event_preset(&store, setup.event_id, gs.clone()).unwrap_or(-1),
        };
        let mut host = Host::new(store.clone(), gs);
        host.slot_tables = Some(crate::game::SlotTables::from_rom(&rom));
        for (k, v) in setup.vars {
            host.vars.insert(k, v);
        }
        let machine = Machine::start(store, setup.event_id)?;
        let sched = Scheduler::new(machine, host, setup.policy, si::char_intervals(&rom), setup.save.text_speed);
        let state = si::random_event_state(setup.save.calendar_78, setup.save.record_107[heroine as usize]);
        let hair_label = si::hair_label(c, heroine, setup.save.record_0d[heroine as usize], state)?;
        let arc = c.join("assets/arc");
        let hair = si::label_number(&arc, heroine, hair_label.max(0) as u32, true)?;
        let rd = |p: &str| std::fs::read(c.join(p)).map_err(|e| format!("{p}: {e}"));
        let voices = Voices { tables: Tables::from_pack(&rd("engine/g7221.pack")?)?, vtb: rd("assets/sys/Siren14Voice.vtb")?, spp: rd("assets/sys/Siren14Voice.spp")?, peaks: HashMap::new() };
        let eyet = EyeTables::from_rom(&rom);
        Ok(SceneSim {
            clock: Clock::new(setup.phases, setup.first_frame),
            first_frame: setup.first_frame,
            start_update: setup.start_update,
            sched,
            fade: ScreenFade::black(),
            motion: MotionSim::new(heroine, 0),
            mdata: MotionData::load(c, &rom)?,
            eye: EyeSim::new(&eyet, setup.rng),
            eyet,
            lip: LipSync::default(),
            lipkeys: LipKeys::load(c, &rom)?,
            voices,
            voice_at: BTreeMap::new(),
            mouth: 5,
            mouth_hold: 0,
            prev_end: None,
            hands: [None, None],
            preset,
            heroine,
            time_mode: si::time_mode(&rom, setup.month, setup.hour, 3),
            arc,
            rom,
            outfit: None,
            accessory: None,
            hair,
            last: None,
            events: Vec::new(),
        })
    }

    pub fn rom(&self) -> &Rom {
        &self.rom
    }

    /// advance one display frame; `releases` = tap releases in this frame. Returns the SceneFrame when an update ran.
    pub fn step_frame(&mut self, releases: usize) -> Option<SceneFrame> {
        let f = self.clock.frame;
        if self.clock.cycle_starts(f, self.first_frame) {
            self.clock.cycle += 1;
        }
        let mut out = None;
        if self.clock.update_ends(f) {
            let u = self.clock.updates;
            self.clock.updates += 1;
            let sf = self.update(u, f);
            self.last = Some(sf.clone());
            out = Some(sf);
        }
        self.sched.frame(f, &self.clock);
        for _ in 0..releases {
            self.sched.release(f, self.clock.cycle);
        }
        self.clock.frame += 1;
        out
    }

    fn update(&mut self, u: u64, f: i64) -> SceneFrame {
        // 1. fade passes, then tasks and the script
        for _ in 0..3 {
            self.fade.process();
        }
        let evs = if u >= self.start_update { self.sched.update(u, f, &self.fade) } else { Vec::new() };
        let mut cmds: Vec<(u32, Vec<u32>, Vec<Effect>)> = Vec::new();
        for e in evs {
            if let ScriptEvent::Cmd { id, args, effects, .. } = &e {
                cmds.push((*id, args.clone(), effects.clone()));
            }
            self.events.push(SimEvent::Script(e));
        }
        for (id, a, effects) in &cmds {
            let arg = |i: usize, d: i32| a.get(i).map_or(d, |&v| v as i32);
            match id {
                0xd => self.fade.to_black(arg(0, 10)),
                0xe => self.fade.to_clear(arg(0, 10), 6),
                0x1b3 => {
                    if a.len() > 1 {
                        self.lip.set_param(a[1], a[0] as i32)
                    } else {
                        self.lip.set_param(arg(0, 0) as u32, 0)
                    }
                }
                0x5c if arg(0, -1) >= 0 => {
                    self.voice_at.insert(u + VOICE_START, a[0]);
                }
                0x12e if arg(0, 0) == 4 => {
                    // outfit label (flow_dispatch -> FlowState -> MapCtl_SetupContext): wardrobe number and motion class
                    let label = arg(1, 0) as u32;
                    self.outfit = si::label_number(&self.arc, self.heroine, label, false).ok();
                    if let Ok(cls) = si::label_class(&self.arc, label) {
                        self.motion.set_class(cls);
                    }
                }
                0x12e if arg(0, 0) == 10 => self.accessory = Some(arg(1, 0) as u32),
                _ => {}
            }
            for e in effects {
                if matches!(e, Effect::Bgm { .. } | Effect::Se { .. } | Effect::Ambient { .. } | Effect::StopSeq { .. } | Effect::StopAmbient { .. }) {
                    self.events.push(SimEvent::Sound { frame: f, effect: e.clone() });
                }
            }
        }
        // 2. lip-sync
        if let Some(clip) = self.voice_at.remove(&u) {
            let peaks = self.voices.peaks(clip);
            self.lip.play(self.lipkeys.keys(clip), peaks.to_vec(), f - 1);
            self.events.push(SimEvent::Voice { frame: f, clip });
        }
        if let Some(p) = self.prev_end {
            self.lip.frames(p, f);
        }
        self.prev_end = Some(f);
        let calls = self.lip.update(f, &self.lipkeys.pattern);
        if let Some(&v) = calls.last() {
            self.mouth = v;
            self.mouth_hold = 20;
        } else if self.mouth_hold > 0 {
            self.mouth_hold -= 1;
        } else {
            self.mouth = 5;
        }
        // 3. eye step
        self.eye.update(&self.eyet, self.motion.mood);
        // 4. motion commands, controller
        for (id, a, _) in &cmds {
            let arg = |i: usize, d: i32| a.get(i).map_or(d, |&v| v as i32);
            let d = &self.mdata;
            match id {
                0x6 => self.preset = arg(0, self.preset),
                0x2 => self.motion.cmd_set_actor(d, self.preset),
                0xf => self.motion.cmd_f(d, arg(0, 0), arg(1, 0xf), arg(2, 0)),
                0x5d => self.motion.cmd_5d(arg(0, 0), arg(1, 8), arg(2, 0xf)),
                0x34 => self.motion.cmd_34(arg(0, 0), arg(1, 0)),
                0x136 => self.motion.cmd_136(arg(0, 0)),
                _ => {}
            }
        }
        self.motion.update(&self.mdata);
        if self.motion.take_mood_events() > 0 {
            self.eye.mood_set();
        }
        // 5. actor state: eye request + hands at the current frame
        if self.motion.layers[0].is_some() {
            match self.motion.track_value(&self.mdata, "eye00", 0, true) {
                Some(v) if v >= 0 => self.eye.request(self.mdata.eye_code.get(v as usize).copied().unwrap_or(9)),
                _ => self.eye.request(9),
            }
            for (k, side) in ["lhand", "rhand"].iter().enumerate() {
                if let Some(v) = self.motion.track_value(&self.mdata, side, 3, false) {
                    self.hands[k] = Some(v);
                }
            }
        }
        // 6. frame advance
        self.motion.advance(&self.mdata);
        let mut seen = self.fade;
        seen.process();
        seen.process();
        SceneFrame {
            update: u,
            frame: f,
            heroine: self.heroine,
            outfit: self.outfit,
            hair: self.hair,
            time_mode: self.time_mode,
            chain: self.motion.chain(&self.mdata),
            mood: self.motion.mood,
            eye_tex: self.eye.st.eye,
            eye_mode: self.eye.st.mode,
            eye_trans: self.eye.trans,
            hands: self.hands,
            mouth: self.mouth,
            fade_cur: self.fade.cur,
            seen_cur: seen.cur,
            preset: self.preset,
            accessory: self.accessory,
        }
    }
}

#[allow(dead_code)]
fn _assert_send(_: &Path) {}
