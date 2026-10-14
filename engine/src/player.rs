//! Scene player: drives the scene simulation (`sim::scene::SceneSim`) one game frame at a time and turns its state into what
//! is drawn (3D heroine, backdrop, dialogue window) and heard (`audio::AudioEvent`s).
//!
//! Time: the simulation runs on the game's own frame clock (59.8261 Hz, logic updates on the 4-frame Capture3D cycle). A front
//! end calls `tick()` once per game frame (a fixed-step accumulator against wall time) and may draw more often: with
//! `interpolate`, a drawn frame blends the continuous quantities (animation layers, fade) between the last two updates;
//! discrete state (eye / mouth textures, hand models, text, box) is shown as of the last update.
//! Windowing and audio output live in `bin/scene_player.rs`.

use crate::audio::AudioEvent;
use crate::backdrop::{backdrop_for, BgChoice};
use crate::camera::{self, GameCamera, Preset};
use crate::font::Font;
use crate::game::Rng;
use crate::heroine::{FaceTables, Pose, Wardrobe};
use crate::host::{Effect, Scripted};
use crate::machine::ScriptStore;
use crate::nns2d::{bg_window, rotate_ccw, Image};
use crate::render::*;
use crate::rom::Rom;
use crate::save::{Save, SaveFacts};
use crate::scene::{draw_dialogue, CameraTrack};
use crate::scene_inputs as si;
use crate::sim::clock::{Nominal, Recorded, FRAME_HZ};
use crate::sim::scene::{SceneFrame, SceneSetup, SceneSim, SimEvent};
use crate::sim::sched::ScriptEvent;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub struct PlayerOptions {
    pub look: String,
    /// answers for choice screens, in order
    pub choices: Vec<u32>,
    /// the game's save (backup image): sound levels, text speed, calendar state, heroine records. `None`: levels 15, text
    /// speed 1, records 0
    pub save: Option<PathBuf>,
    /// game clock (month, schedule hour); the defaults are GameState's
    pub month: Option<u32>,
    pub hour: Option<u32>,
    /// the player's name ({v0}, {v1}; {v26} = name + 君)
    pub player: Option<String>,
    /// draw between logic updates (continuous quantities interpolated)
    pub interpolate: bool,
    /// close each line this many game frames after it is fully typed (demos, stills); `None` = wait for `tap`
    pub auto_advance: Option<u32>,
    /// RNG state at the scene start (needed only by blinks under mood 0x1a)
    pub rng: Option<u64>,
    /// replay a recorded session instead of playing live: its frame clock, start and tap releases
    pub replay: Option<Replay>,
}

/// a recorded session's inputs (engine/tests/data/<scene>/inputs.json: `sandbox/gx/export_fixtures.py`)
#[derive(Clone, Debug)]
pub struct Replay {
    pub first_frame: i64,
    /// Capture3D phase digit per frame from `first_frame`
    pub phases: String,
    pub start_update: u64,
    /// tap release frames
    pub releases: Vec<i64>,
    pub prev_preset: Option<i32>,
}

impl Replay {
    pub fn from_json(v: &serde_json::Value) -> Option<Replay> {
        Some(Replay {
            first_frame: v["frames"][0].as_i64()?,
            phases: v["phases"].as_str()?.to_string(),
            start_update: v["script_start_update"].as_u64()?,
            releases: v["releases"].as_array()?.iter().filter_map(|x| x.as_i64()).collect(),
            prev_preset: v["prev_preset"].as_i64().map(|x| x as i32),
        })
    }
}

impl Default for PlayerOptions {
    fn default() -> Self {
        PlayerOptions { look: "soft-s".into(), choices: Vec::new(), save: None, month: None, hour: None, player: None, interpolate: true, auto_advance: Some(90), rng: None, replay: None }
    }
}

pub struct Frame {
    pub data: FrameData,
    pub view: View,
    /// darkening toward black, 0..1 (master brightness)
    pub fade: f32,
    /// the UI overlay when it changed since the last frame (None = keep the previous one)
    pub ui: Option<Image>,
    pub clear_ui: bool,
    pub heroine: String,
}

pub struct ScenePlayer {
    pub sim: SceneSim,
    rom: Arc<Rom>,
    pub slot: u32,
    presets: BTreeMap<i32, Preset>,
    camera: CameraTrack,
    wardrobe: Wardrobe,
    tables: FaceTables,
    font: Font,
    cache: ImageCache,
    smooth: u32,
    pub render_opts: RenderOpts,
    opts: PlayerOptions,
    prev: Option<SceneFrame>,
    cur: Option<SceneFrame>,
    /// game frames since the last update
    since_update: u32,
    taps: usize,
    /// sound and voice events for the audio side: (game frame, event)
    audio_out: Vec<(i64, AudioEvent)>,
    /// every script event so far
    pub script: Vec<ScriptEvent>,
    height: f64,
    /// the event's BG1 panorama (384 x 256), if it has one
    pub backdrop: Option<(BgChoice, Image)>,
    bg_key: Option<(i32, i32)>,
    view0: Option<View>,
    ui_key: Option<(String, usize, usize, usize)>,
    ui_shown: bool,
    pub first_frame: i64,
    levels: [u8; 3],
    /// the scene's accessory (key, model, offset in the Head joint's frame)
    accessory: Option<(u32, Arc<crate::nns3d::Model>, [f64; 3])>,
    arc: PathBuf,
}

impl ScenePlayer {
    pub fn new(content: &Path, event_id: u32, opts: PlayerOptions) -> Result<ScenePlayer, String> {
        let rd = |p: &str| std::fs::read(content.join(p)).map_err(|e| format!("{p}: {e}"));
        let rom = Arc::new(Rom::open(content)?);
        let store = Arc::new(ScriptStore::open(content)?);
        let heroine = si::heroine_of_event(event_id).ok_or_else(|| format!("event {event_id:08x} names no heroine"))?;
        let (save, game, image) = match &opts.save {
            Some(p) => {
                let s = Save::open(p)?;
                let image = s.image(crate::save::Kind::Game, 0).map(|g| g.to_vec());
                (SaveFacts::read(&s)?, Some(s.game_state(1, heroine)?), image)
            }
            None => (SaveFacts { levels: [15, 15, 15], text_speed: 1, calendar_78: 0, record_0d: [0; 3], record_107: [0; 3] }, None, None),
        };
        let clock = crate::game::Clock::default();
        let (month, hour) = (opts.month.unwrap_or(clock.month), opts.hour.unwrap_or(clock.hour));
        // text variables: the game's name vars from the save (`si::name_vars`); --player overrides the player's name, a
        // run without a save uses a placeholder name
        let mut vars = si::name_vars(content, &rom, image.as_deref(), heroine)?;
        let pname = opts.player.clone().or_else(|| if image.is_none() { Some("あなた".to_string()) } else { None });
        if let Some(n) = pname {
            for k in [0u16, 0x1b, 0x1c] {
                vars.insert(k, n.clone());
            }
            vars.insert(0x1a, format!("{n}{}", if heroine == 1 { "" } else { "くん" }));
        }
        let (phases, first_frame, start_update, prev_preset): (Box<dyn crate::sim::clock::PhaseSource + Send>, i64, u64, Option<i32>) = match &opts.replay {
            Some(r) => (Box::new(Recorded::from_digits(r.first_frame, &r.phases)), r.first_frame, r.start_update, r.prev_preset),
            None => (Box::new(Nominal), 0, 0, None),
        };
        let setup = SceneSetup {
            content: content.to_path_buf(),
            event_id,
            save: save.clone(),
            month,
            hour,
            phases,
            first_frame,
            start_update,
            prev_preset,
            rng: opts.rng.map(Rng::new),
            policy: Box::new(Scripted::new(opts.choices.clone())),
            vars,
            game,
        };
        let sim = SceneSim::new(rom.clone(), store, setup)?;
        let arc = content.join("assets/arc");
        let presets = camera::load_presets(&arc);
        let tod = si::backdrop_tod(&rom, month, hour, save.calendar_78, false, false, false);
        let backdrop = backdrop_for(content, event_id, month, tod);
        let (render_opts, smooth) = look(&opts.look).ok_or_else(|| format!("unknown look {}", opts.look))?;
        let tables = FaceTables::from_json(&serde_json::from_slice(&rd("engine/face_tables.json")?).map_err(|e| e.to_string())?)?;
        let font = Font::parse(&rd("assets/test_font/LC12_UTF8_R90.NFTR")?)?;
        let camera = CameraTrack::new(None);
        Ok(ScenePlayer {
            height: camera::heroine_height(&rom, heroine),
            sim,
            rom,
            slot: heroine,
            camera,
            presets,
            wardrobe: Wardrobe::load(&arc, si::PREFIX[heroine as usize]),
            tables,
            font,
            cache: ImageCache::default(),
            smooth,
            render_opts,
            opts,
            prev: None,
            cur: None,
            since_update: 0,
            taps: 0,
            audio_out: Vec::new(),
            script: Vec::new(),
            backdrop,
            bg_key: None,
            view0: None,
            ui_key: None,
            ui_shown: false,
            first_frame,
            levels: save.levels,
            accessory: None,
            arc: arc.clone(),
        })
    }

    /// the player taps the screen (released at the next tick)
    pub fn tap(&mut self) {
        self.taps += 1;
    }

    /// current game frame
    pub fn frame_no(&self) -> i64 {
        self.sim.clock.frame
    }

    pub fn seconds(&self) -> f64 {
        (self.sim.clock.frame - self.first_frame) as f64 / FRAME_HZ
    }

    pub fn finished(&self) -> bool {
        self.sim.sched.done()
    }

    /// the save's sound levels (music, sound, voice) for the audio director
    pub fn levels(&self) -> [u8; 3] {
        self.levels
    }

    /// one game frame
    pub fn tick(&mut self) {
        let f = self.sim.clock.frame;
        if let (Some(hold), Some(tb), None) = (self.opts.auto_advance, &self.sim.sched.text, &self.opts.replay) {
            if tb.resume_update.is_none() && tb.typed_frame.map_or(false, |t| f >= t + hold as i64) {
                self.taps += 1;
            }
        }
        if let Some(r) = &self.opts.replay {
            self.taps += r.releases.iter().filter(|&&x| x == f).count();
        }
        let n = std::mem::take(&mut self.taps);
        let sf = self.sim.step_frame(n);
        for e in std::mem::take(&mut self.sim.events) {
            match e {
                SimEvent::Script(se) => {
                    if let ScriptEvent::Cmd { effects, update, .. } = &se {
                        let at = self.cur.as_ref().map_or(f, |c| if c.update == *update { c.frame } else { f });
                        for ef in effects {
                            if matches!(ef, Effect::Camera { .. }) {
                                self.camera.apply(at, ef, &self.presets);
                            }
                        }
                    }
                    self.script.push(se);
                }
                SimEvent::Sound { frame, effect } => self.audio_out.push((frame, AudioEvent::Sound(effect))),
                SimEvent::Voice { frame, clip } => self.audio_out.push((frame, AudioEvent::Voice(clip))),
            }
        }
        if let Some(key) = sf.as_ref().and_then(|s| s.accessory) {
            if self.accessory.as_ref().map(|a| a.0) != Some(key) {
                self.load_accessory(key);
            }
        }
        match sf {
            Some(sf) => {
                self.prev = self.cur.take();
                self.cur = Some(sf);
                self.since_update = 0;
            }
            None => self.since_update += 1,
        }
    }

    fn load_accessory(&mut self, key: u32) {
        let rd = |kind: i32| si::accessory_entry(&self.rom, key, kind).and_then(|e| std::fs::read(self.arc.join(format!("block3d.arc/{e:04}.bin"))).ok());
        let model = rd(0).and_then(|b| crate::nns3d::parse_file(Arc::new(b)).ok()).and_then(|f| f.models.into_iter().next());
        let ofs = si::accessory_offset(&self.arc, key, self.slot);
        if let (Some(m), Some(o)) = (model, ofs) {
            if let Some(t) = rd(1) {
                self.wardrobe.add_textures(t);
            }
            self.accessory = Some((key, Arc::new(m), o));
        }
    }

    /// run game frames until scene time `t` (seconds)
    pub fn run_to(&mut self, t: f64) {
        while self.seconds() < t && !(self.finished() && self.seconds() > 0.0) {
            self.tick();
        }
    }

    /// characters of the current line on screen
    pub fn revealed(&self) -> Option<usize> {
        self.sim.sched.text.as_ref().map(|t| t.drawn)
    }

    /// sound events since the last call: (game frame, event)
    pub fn take_audio(&mut self) -> Vec<(i64, AudioEvent)> {
        std::mem::take(&mut self.audio_out)
    }

    pub fn state(&self) -> Option<&SceneFrame> {
        self.cur.as_ref()
    }

    /// blend weight of the current update over the previous one for the frame being drawn
    fn alpha(&self, sub: f64) -> f64 {
        if !self.opts.interpolate || self.prev.is_none() {
            return 1.0;
        }
        ((self.since_update as f64 + sub) / 4.0).clamp(0.0, 1.0)
    }

    fn pose(&self, a: f64) -> Option<Pose> {
        let cur = self.cur.as_ref()?;
        let mut layers: Vec<(String, usize, f64)> = cur.chain.iter().map(|l| (l.stem.clone(), l.frame.max(0) as usize, l.ratio as f64 / 4096.0 * a)).collect();
        if a < 1.0 {
            if let Some(p) = &self.prev {
                layers.extend(p.chain.iter().map(|l| (l.stem.clone(), l.frame.max(0) as usize, l.ratio as f64 / 4096.0 * (1.0 - a))));
            }
        }
        layers.retain(|l| l.2 > 0.0 && self.wardrobe.motions.contains_key(&l.0));
        Some(Pose {
            outfit: cur.outfit.unwrap_or(0),
            hair: cur.hair,
            time_mode: cur.time_mode,
            layers,
            mood: cur.mood.max(0) as usize,
            eye_tex: cur.eye_tex,
            mouth: cur.mouth.clamp(0, 5) as usize,
            hands: [cur.hands[0].unwrap_or(0).max(0) as usize, cur.hands[1].unwrap_or(0).max(0) as usize],
            accessory: self.accessory.as_ref().map(|(_, m, o)| (m.clone(), *o)),
        })
    }

    fn game_camera(&self, sub: f64) -> Option<(Preset, GameCamera)> {
        let id = self.cur.as_ref().map_or(-1, |c| c.preset);
        let p = if self.camera.id() == -1 { self.presets.get(&id).cloned()? } else { self.camera.at(self.sim.clock.frame as f64 - 1.0 + sub) };
        let gc = GameCamera::new(&p, &self.rom, self.height);
        Some((p, gc))
    }

    /// the drawn frame for an output of `w` x `h` (portrait, 3:4 is the console's picture); `sub` = fraction of the
    /// current game frame elapsed (0 when drawing once per game frame)
    pub fn frame(&mut self, w: u32, h: u32, sub: f64) -> Result<Frame, String> {
        let a = self.alpha(sub);
        let pose = self.pose(a).ok_or("no update simulated yet")?;
        let mut c = self.wardrobe.compose_pose(&pose, &self.tables)?;
        if self.view0.is_none() {
            self.view0 = Some(fit_view(&c.tris, std::f64::consts::PI, 0.0, [(0, 0); 3]));
        }
        // the heroine exists from the script's actor setup on (cmd 2 -> `MapCtl_SetupAndRefresh` starts her first motion);
        // before that the game draws no model (otherwise this would be the bind pose)
        if self.cur.as_ref().map_or(true, |f| f.chain.is_empty()) {
            c.tris.clear();
        }
        let mut view = self.view0.clone().unwrap();
        view.game = self.game_camera(sub).map(|(_, g)| g);
        view.bg = [0.0, 0.0, 0.0];
        let sm = if self.smooth > 1 { Some((self.smooth, 1.0, 50.0)) } else { None };
        let data = build_frame_smooth(&c.tris, &c.mats, &c.toon, &c.edge, &self.wardrobe.tex, &mut self.cache, sm);
        // master brightness: the fade's 16.16 level as seen at the end of the cycle (whole levels without interpolation)
        let cur = self.cur.as_ref().unwrap();
        let fade = if self.opts.interpolate {
            let p = self.prev.as_ref().map_or(cur.seen_cur, |p| p.seen_cur) as f64;
            -(p + (cur.seen_cur as f64 - p) * a) / 65536.0
        } else {
            -((cur.seen_cur / 0x1000) as f64) / 16.0
        };
        // dialogue window: redraw when the text, the revealed count or the size changed
        let (key, line) = match &self.sim.sched.text {
            Some(t) => ((t.name.clone() + "\u{1}" + &t.text, t.drawn, w as usize, h as usize), Some((t.name.clone(), t.text.clone(), t.drawn))),
            None => ((String::new(), 0, w as usize, h as usize), None),
        };
        let (mut ui, mut clear_ui) = (None, false);
        if self.ui_key.as_ref() != Some(&key) {
            let mut img = Image::new(w as usize, h as usize);
            let shown = line.map_or(false, |(n, t, r)| draw_dialogue(&mut img, &n, &t, r, &self.font));
            if shown {
                ui = Some(img);
            } else if self.ui_shown {
                clear_ui = true;
            }
            self.ui_shown = shown;
            self.ui_key = Some(key);
        }
        Ok(Frame { data, view, fade: fade.clamp(0.0, 1.0) as f32, ui, clear_ui, heroine: si::PREFIX[self.slot as usize].to_string() })
    }

    /// the backdrop for an output of `w` x `h`: BG1's window at the camera's scroll (`GameCamera::bg_scroll`), turned to
    /// portrait and scaled to cover; `None` when the scroll has not changed since the last call
    pub fn backdrop_image(&mut self, w: usize, h: usize) -> Option<Image> {
        let (p, gc) = self.game_camera(0.0)?;
        let scroll = gc.bg_scroll(&p);
        if self.bg_key == Some(scroll) {
            return None;
        }
        self.bg_key = Some(scroll);
        Some(match &self.backdrop {
            Some((_, pano)) => cover(&rotate_ccw(&bg_window(pano, scroll.0, scroll.1)), w, h),
            None => gradient_backdrop(w, h),
        })
    }

    /// force the next `backdrop_image` call to redraw (after a resize)
    pub fn invalidate_backdrop(&mut self) {
        self.bg_key = None;
    }
}

/// The game's picture is the console's portrait screen (192 x 256, 3:4). For an output `w` x `h` this returns the largest 3:4
/// rectangle that fits and its offset: `(width, height, x, y)`; the rest of a wider window stays black (pillarbox).
pub fn ds_fit(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (sw, sh) = if w * 4 >= h * 3 { (h * 3 / 4, h) } else { (w, w * 4 / 3) };
    let (sw, sh) = (sw.max(1), sh.max(1));
    (sw, sh, (w.saturating_sub(sw)) / 2, (h.saturating_sub(sh)) / 2)
}

/// bilinear "cover" scaling of `src` to `w` x `h`, cropping the overflow evenly
pub fn cover(src: &Image, w: usize, h: usize) -> Image {
    let sc = (w as f64 / src.w as f64).max(h as f64 / src.h as f64);
    let (ox, oy) = ((src.w as f64 * sc - w as f64) / 2.0, (src.h as f64 * sc - h as f64) / 2.0);
    let mut out = Image::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (((x as f64 + 0.5 + ox) / sc - 0.5).clamp(0.0, src.w as f64 - 1.0), ((y as f64 + 0.5 + oy) / sc - 0.5).clamp(0.0, src.h as f64 - 1.0));
            let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(src.w - 1), (y0 + 1).min(src.h - 1));
            let (tx, ty) = (fx - x0 as f64, fy - y0 as f64);
            let d = (y * w + x) * 4;
            for c in 0..3 {
                let g = |xx: usize, yy: usize| src.buf[(yy * src.w + xx) * 4 + c] as f64;
                let v = g(x0, y0) * (1.0 - tx) * (1.0 - ty) + g(x1, y0) * tx * (1.0 - ty) + g(x0, y1) * (1.0 - tx) * ty + g(x1, y1) * tx * ty;
                out.buf[d + c] = v.round() as u8;
            }
            out.buf[d + 3] = 255;
        }
    }
    out
}

/// a soft gradient for events without a backdrop record
pub fn gradient_backdrop(w: usize, h: usize) -> Image {
    let mut img = Image::new(w, h);
    for y in 0..h {
        let k = y as f32 / h as f32;
        let top = [86.0, 118.0, 190.0];
        let mid = [170.0, 160.0, 190.0];
        let bot = [214.0, 170.0, 150.0];
        let c: Vec<f32> = (0..3).map(|i| if k < 0.6 { top[i] + (mid[i] - top[i]) * k / 0.6 } else { mid[i] + (bot[i] - mid[i]) * (k - 0.6) / 0.4 }).collect();
        for x in 0..w {
            let o = (y * w + x) * 4;
            img.buf[o..o + 4].copy_from_slice(&[c[0] as u8, c[1] as u8, c[2] as u8, 255]);
        }
    }
    img
}
