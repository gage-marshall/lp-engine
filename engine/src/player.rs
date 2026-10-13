//! Scene player: runs a script event through the host, paces it, and produces frames (3D heroine + backdrop + dialogue UI) at any scene time.
//! Windowing and audio output live in `bin/scene_player.rs`; `frame` is also used headlessly (`--shot`) to render stills.

use crate::audio::{pace_with, Timed};
use crate::camera;
use crate::font::Font;
use crate::game::GameState;
use crate::heroine::{FaceTables, Wardrobe};
use crate::host::{Effect, Host, Policy, Scripted};
use crate::machine::ScriptStore;
use crate::nns2d::Image;
use crate::render::*;
use crate::scene::*;
use crate::tessellate;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

pub struct PlayerOptions {
    /// outfit id; `None` = what she wears in that scene (see `default_outfit`)
    pub outfit: Option<u32>,
    /// hair id; `None` = the heroine's own default (Manaka 0, Rinko 200 as seen in the game, Nene 0 until verified)
    pub hair: Option<u32>,
    pub look: String,
    pub choices: Vec<u32>,
    pub fov: f64,
    pub seed: u64,
    /// game clock month / hour the scene runs at (season and time period feed script branches and music); `None` keeps the default
    pub month: Option<u32>,
    pub hour: Option<u32>,
    /// the player's name from the save (v0/v1; v26 is the name with 君, how the heroines address the player)
    pub player: Option<String>,
    /// time-of-day shading table (0 day, 1 evening, 2 night); `None` = derived from the hour
    pub tod: Option<u32>,
}

impl Default for PlayerOptions {
    fn default() -> Self {
        PlayerOptions { outfit: None, hair: None, look: "soft-s".into(), choices: Vec::new(), fov: 30.0, seed: 1, month: None, hour: None, player: None, tod: None }
    }
}

pub struct Frame {
    pub data: FrameData,
    pub view: View,
    pub fade: f32,
    /// the UI overlay when it changed since the last frame (None = keep the previous one); `Some(None)`-style clearing is `clear_ui`
    pub ui: Option<Image>,
    pub clear_ui: bool,
    pub heroine: String,
}

pub struct ScenePlayer {
    pub timeline: Vec<Timed>,
    pub duration: f64,
    pub slot: u32,
    content: std::path::PathBuf,
    state: SceneState,
    cursor: usize,
    presets: BTreeMap<i32, camera::Preset>,
    motions: MotionTable,
    tables: FaceTables,
    lip: Option<LipSync>,
    voice_ticks: Vec<f64>,
    wardrobes: std::collections::HashMap<String, Wardrobe>,
    arc: std::path::PathBuf,
    font: Font,
    cache: ImageCache,
    opts: PlayerOptions,
    smooth: u32,
    pub render_opts: RenderOpts,
    view0: Option<View>,
    ui_key: Option<(String, usize, usize, usize)>,
    ui_shown: bool,
    /// the game's own backdrop picture for this event (168x256), if it has one
    pub backdrop: Option<(crate::backdrop::BgChoice, Image)>,
    tod0: u32,
}

fn voice_lengths(vtb: &[u8]) -> Vec<f64> {
    vtb.chunks_exact(8).map(|c| (u32::from_le_bytes([c[4], c[5], c[6], c[7]]) as f64 / 16.0) / 40.0 * 0.020).collect()
}

impl ScenePlayer {
    pub fn new(content: &Path, event_id: u32, opts: PlayerOptions) -> Result<ScenePlayer, String> {
        let rd = |p: &str| std::fs::read(content.join(p)).map_err(|e| format!("{p}: {e}"));
        let store = Arc::new(ScriptStore::open(content)?);
        let mut gs = GameState::new(opts.seed);
        if let Some(m) = opts.month {
            gs.clock.month = m;
        }
        if let Some(h) = opts.hour {
            gs.clock.hour = h;
        }
        let mut host = Host::new(store, gs);
        // string variables normally come from the save (names, how the heroine addresses the player); placeholders until the save is modelled
        let pre_slot = GameState::slot_of_event_id(event_id).unwrap_or(0) as usize;
        let (family, given) = [("高嶺", "愛花"), ("小早川", "凛子"), ("姉ヶ崎", "寧々")][pre_slot % 3];
        let pname = opts.player.clone().unwrap_or_else(|| "あなた".to_string());
        let called = format!("{pname}君");
        for (k, v) in [(0u16, pname.as_str()), (1, pname.as_str()), (26, called.as_str()), (3, family), (8, given)] {
            host.vars.insert(k, v.to_string());
        }
        let vtb = rd("assets/sys/Siren14Voice.vtb")?;
        host.load_voice_lengths(&vtb);
        let mut pol: Box<dyn Policy> = Box::new(Scripted::new(opts.choices.clone()));
        let tr = host.run_event(event_id, pol.as_mut())?;
        let voice_ticks = voice_lengths(&vtb);
        let timeline = pace_with(&tr.steps, &mut |c| voice_ticks.get(c as usize).copied().unwrap_or(0.0));
        let duration = timeline.last().map_or(0.0, |t| t.sec) + 2.0;
        let slot = host.state.current_slot;
        let backdrop = crate::backdrop::backdrop_for(content, event_id, host.state.clock.month, host.state.time_period());
        let backdrop_archive = backdrop.as_ref().map(|b| b.0.archive.clone());
        let arc = content.join("assets/arc");
        let presets = camera::load_presets(&arc);
        let prefix = ["m", "r", "n"][slot as usize % 3];
        let (render_opts, smooth) = look(&opts.look).ok_or_else(|| format!("unknown look {}", opts.look))?;
        let tables = FaceTables::from_json(&serde_json::from_slice(&rd("engine/face_tables.json")?).map_err(|e| e.to_string())?)?;
        let motions = MotionTable::from_json(&serde_json::from_slice(&rd("engine/motion_table.json")?).map_err(|e| e.to_string())?)?;
        let lip = match (rd("assets/lipsync/lipsync_add.dat"), rd("assets/lipsync/lipsync.dat")) {
            (Ok(a), Ok(k)) => Some(LipSync::new(&a, k)),
            _ => None,
        };
        let font = Font::parse(&rd("assets/test_font/LC12_UTF8_R90.NFTR")?)?;
        let mut state = SceneState::new(prefix, opts.outfit.unwrap_or(default_outfit(slot, backdrop_archive.as_deref())), opts.hair.unwrap_or(default_hair(slot)), &presets);
        state.fov = opts.fov;
        let derived = crate::backdrop::BackdropSelect::load(content).map(|b| b.tod_of_period(host.state.time_period())).unwrap_or(0);
        state.state = opts.tod.unwrap_or(derived);
        state.idle = motions.recs.iter().find(|r| r.heroine.to_string() == prefix).map(|r| r.stem.clone());
        let mut p = ScenePlayer {
            timeline,
            duration,
            slot,
            content: content.to_path_buf(),
            state,
            cursor: 0,
            presets,
            motions,
            tables,
            lip,
            voice_ticks,
            wardrobes: Default::default(),
            arc,
            font,
            cache: ImageCache::default(),
            opts,
            smooth,
            render_opts,
            view0: None,
            ui_key: None,
            ui_shown: false,
            backdrop,
            tod0: derived,
        };
        p.ensure_wardrobe();
        Ok(p)
    }

    fn ensure_wardrobe(&mut self) {
        let prefix = self.state.prefix.clone();
        if !self.wardrobes.contains_key(&prefix) {
            self.wardrobes.insert(prefix.clone(), Wardrobe::load(&self.arc, &prefix));
        }
    }

    pub fn content(&self) -> &Path {
        &self.content
    }

    fn reset(&mut self) {
        let prefix = ["m", "r", "n"][self.slot as usize % 3];
        self.state = SceneState::new(prefix, self.opts.outfit.unwrap_or(default_outfit(self.slot, self.backdrop.as_ref().map(|b| b.0.archive.as_str()))), self.opts.hair.unwrap_or(default_hair(self.slot)), &self.presets);
        self.state.fov = self.opts.fov;
        self.state.state = self.opts.tod.unwrap_or(self.tod0);
        self.state.idle = self.motions.recs.iter().find(|r| r.heroine.to_string() == prefix).map(|r| r.stem.clone());
        self.cursor = 0;
    }

    /// apply every effect up to scene time `t` (seeking backwards replays from the start)
    pub fn advance_to(&mut self, t: f64) {
        if self.cursor > 0 && self.timeline.get(self.cursor - 1).map_or(false, |e| e.sec > t) {
            self.reset();
        }
        let vt = &self.voice_ticks;
        let vl = |c: u32| vt.get(c as usize).copied().unwrap_or(0.0);
        while self.cursor < self.timeline.len() && self.timeline[self.cursor].sec <= t {
            let e = self.timeline[self.cursor].clone();
            self.state.apply(e.sec, &e.effect, &self.presets, &self.motions, &vl);
            self.cursor += 1;
        }
    }

    /// the effects that fired in (from, to] — for the audio side
    pub fn effects_between(&self, from: f64, to: f64) -> Vec<&Effect> {
        self.timeline.iter().filter(|e| e.sec > from && e.sec <= to).map(|e| &e.effect).collect()
    }

    pub fn state(&self) -> &SceneState {
        &self.state
    }

    /// the frame at scene time `t` for an output of `w` x `h`
    pub fn frame(&mut self, t: f64, w: u32, h: u32) -> Result<Frame, String> {
        self.advance_to(t);
        self.ensure_wardrobe();
        let wd = &self.wardrobes[&self.state.prefix];
        self.state.advance(t, &|stem| wd.motion_frames(stem));
        let mut compose = self.state.compose_opts(t, &self.tables, self.lip.as_ref());
        compose.look_yaw_deg = self.look_yaw(t);
        let c = wd.compose(&compose, Some(&self.tables))?;
        if self.view0.is_none() {
            let bind = wd.compose(&crate::heroine::ComposeOpts { motion: None, frame: 0, ..compose.clone() }, None)?;
            self.view0 = Some(fit_view(&bind.tris, std::f64::consts::PI, 0.0, self.state.lights));
        }
        let mut view = self.view0.clone().unwrap();
        let mut cam = self.state.camera_at(t);
        // The real camera (read from the emulator) has the same eye/target as the preset, and a heroine at x = 0 would sit half off-screen at presets with x != 0 (Nene's, eye x -18, target x -13),
        // while the game shows her centred: she stands at the preset's target x (stage position).  Verified against a real frame by eye only; LP_NO_STAGE_X=1 turns it off for comparison.
        if std::env::var("LP_STAGE_X").is_ok() {
            let sx = cam.target[0];
            cam.eye[0] -= sx;
            cam.target[0] -= sx;
        }
        // the game's projection (Camera_Reset / MTX_PerspectiveW): half-angle table value tan = 706/4035 across the short side of its raw 256x192 screen, which is the portrait
        // width here (the whole camera is rolled 90 degrees for the sideways console); the vertical angle follows the output aspect
        let tan_half = std::env::var("LP_TAN").ok().and_then(|v| v.parse().ok()).unwrap_or(GAME_TAN_HALF_WIDTH); // debug override
        cam.fov_deg = 2.0 * (tan_half * h as f64 / w as f64).atan().to_degrees();
        view.camera = Some(cam);
        view.lights = self.state.lights;
        view.bg = [0.18, 0.2, 0.3];
        let sm = if self.smooth > 1 { Some((self.smooth, 1.0, 50.0)) } else { None };
        let data = build_frame_smooth(&c.tris, &c.mats, &c.toon, &c.edge, &wd.tex, &mut self.cache, sm);
        // UI overlay: redraw only when the text, reveal count or size changed
        let (name, revealed, text_len) = match &self.state.dialogue {
            Some(d) => (d.name.clone() + "\u{1}" + &d.text, self.state.revealed(t), d.text.chars().count()),
            None => (String::new(), 0, 0),
        };
        let key = (name, revealed.min(text_len), w as usize, h as usize);
        let (mut ui, mut clear_ui) = (None, false);
        if self.ui_key.as_ref() != Some(&key) {
            let mut img = Image::new(w as usize, h as usize);
            let shown = self.state.draw_ui(&mut img, t, &self.font);
            if shown {
                ui = Some(img);
            } else if self.ui_shown {
                clear_ui = true;
            }
            self.ui_shown = shown;
            self.ui_key = Some(key);
        }
        Ok(Frame { data, view, fade: self.state.fade_at(t), ui, clear_ui, heroine: self.state.prefix.clone() })
    }
}

/// The game's picture is the console's portrait screen (192 x 256, 3:4).  For an output `w` x `h` this returns the largest 3:4 rectangle that fits and its offset:
/// `(width, height, x, y)`; the rest of a wider window stays black (pillarbox).
pub fn ds_fit(w: u32, h: u32) -> (u32, u32, u32, u32) {
    let (sw, sh) = if w * 4 >= h * 3 { (h * 3 / 4, h) } else { (w, w * 4 / 3) };
    let (sw, sh) = (sw.max(1), sh.max(1));
    (sw, sh, (w.saturating_sub(sw)) / 2, (h.saturating_sub(sh)) / 2)
}

/// tan of the camera's half field of view across the portrait width: sin/cos 706/4035 for the angle 1820 (10 degrees) in `Camera_Reset`
pub const GAME_TAN_HALF_WIDTH: f64 = 706.0 / 4035.0;

/// how far the head turns toward the camera relative to the body (degrees); estimated from the recorded Famires scene (body about 87 degrees away, head close to facing the camera)
pub const LOOK_LIMIT_DEG: f64 = 60.0;

/// the hair each heroine wears in the recorded play (hair id of `<p>_00_<id>_hair`; Manaka 0 unverified, Nene 200 matches the bob in the Famires frames)
pub fn default_hair(slot: u32) -> u32 {
    [0, 200, 200][slot as usize % 3]
}

/// what she wears in a scene, from the recorded play: the school uniform (outfit 0) everywhere except Nene at the Famires, where she works in the waitress uniform (outfit 10, checked against the frames)
pub fn default_outfit(slot: u32, backdrop_archive: Option<&str>) -> u32 {
    if slot == 2 && backdrop_archive == Some("bg_center") {
        10
    } else {
        0
    }
}

impl ScenePlayer {
    /// head turn toward the camera: the angle from the body's forward direction to the camera, clamped (the game turns the heroine's head to the player)
    fn look_yaw(&self, t: f64) -> f64 {
        let cam = self.state.camera_at(t);
        let head = [0.0, 142.0, 0.0];
        let v = [cam.eye[0] - head[0], cam.eye[1] - head[1], cam.eye[2] - head[2]];
        let (s, c) = cam.yaw_deg.to_radians().sin_cos();
        let (xb, zb) = (v[0] * c - v[2] * s, v[0] * s + v[2] * c);
        -xb.atan2(zb).to_degrees().clamp(-LOOK_LIMIT_DEG, LOOK_LIMIT_DEG)
    }

    /// the backdrop image for a `w` x `h` output: the game's picture scaled to cover (centred), or the gradient placeholder when the event has none
    pub fn backdrop_image(&self, w: usize, h: usize) -> Image {
        match &self.backdrop {
            Some((_, pic)) => cover(pic, w, h),
            None => gradient_backdrop(w, h),
        }
    }
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

/// a soft dusk-sky gradient backdrop (placeholder until the game's own backgrounds are mapped to scenes)
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
