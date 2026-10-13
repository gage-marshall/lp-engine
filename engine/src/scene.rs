//! Scene player core: turns the paced effect timeline into what is on screen at a given time — heroine pose and face, camera, fade, dialogue box.
//! Everything here is data and logic (no GPU, no window); `bin/scene_player.rs` and `bin/scene_shot.rs` draw it.

use crate::camera::Preset;
use crate::font::Font;
use crate::heroine::{ComposeOpts, FaceTables};
use crate::host::Effect;
use crate::nns2d::Image;
use crate::render::Camera;
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct MotionRec {
    pub heroine: char,
    pub stem: String,
    /// default mood of the motion (-1 = none)
    pub mood: i32,
}

/// motion index (script command argument) -> motion stem
pub struct MotionTable {
    pub recs: Vec<MotionRec>,
}

impl MotionTable {
    pub fn from_json(v: &serde_json::Value) -> Result<MotionTable, String> {
        let r = v["records"].as_array().ok_or("records")?;
        Ok(MotionTable {
            recs: r
                .iter()
                .map(|x| MotionRec { heroine: x["heroine"].as_str().and_then(|s| s.chars().next()).unwrap_or('m'), stem: x["stem"].as_str().unwrap_or("").to_string(), mood: x["mood"].as_i64().unwrap_or(-1) as i32 })
                .collect(),
        })
    }
    pub fn get(&self, index: u32) -> Option<&MotionRec> {
        self.recs.get(index as usize)
    }
}

/// per-clip mouth shapes: `lipsync_add.dat` = (offset, length) per clip into `lipsync.dat`; bytes 0 a, 1 i, 2 u, 3 e, 4 o, 5 rest
pub struct LipSync {
    add: Vec<(usize, usize)>,
    keys: Vec<u8>,
}

impl LipSync {
    pub fn new(add: &[u8], keys: Vec<u8>) -> LipSync {
        LipSync { add: add.chunks_exact(8).map(|c| (u32::from_le_bytes([c[0], c[1], c[2], c[3]]) as usize, u32::from_le_bytes([c[4], c[5], c[6], c[7]]) as usize)).collect(), keys }
    }
    /// viseme (0..5) at time `t` of a clip of `duration` seconds
    pub fn viseme(&self, clip: u32, t: f64, duration: f64) -> usize {
        let (off, len) = match self.add.get(clip as usize) {
            Some(&e) => e,
            None => return 5,
        };
        let k = match self.keys.get(off..off + len) {
            Some(k) => k,
            None => return 5,
        };
        let k: Vec<u8> = k.iter().copied().take_while(|&b| b != 0xfe).collect();
        if t < 0.0 || t >= duration || k.is_empty() {
            return 5;
        }
        (k[((t / duration * k.len() as f64) as usize).min(k.len() - 1)] as usize).min(5)
    }
}

#[derive(Clone, Copy)]
struct CamAnim {
    from: Camera,
    to: Camera,
    t0: f64,
    dur: f64,
}

#[derive(Clone)]
pub struct Dialogue {
    pub name: String,
    pub text: String,
    pub t0: f64,
}

pub struct SceneState {
    pub prefix: String,
    pub outfit: u32,
    pub hair: u32,
    motion: Option<String>,
    motion_t0: f64,
    pub mood: usize,
    voice: Option<(u32, f64, f64)>, // clip, start, duration
    cam: CamAnim,
    pub lights: [(i32, i32); 3],
    fade: (f32, f32, f64, f64), // from, to, t0, duration
    pub dialogue: Option<Dialogue>,
    pub state: u32, // time of day for shading
    pub fov: f64,
    /// idle pose used until a script sets a motion
    pub idle: Option<String>,
}

pub const DEFAULT_PRESET: i32 = 1000;

impl SceneState {
    pub fn new(prefix: &str, outfit: u32, hair: u32, presets: &BTreeMap<i32, Preset>) -> SceneState {
        let p = presets.get(&DEFAULT_PRESET);
        let cam = p.map_or(Camera { eye: [0.0, 144.0, 150.0], target: [0.0, 124.0, 0.0], fov_deg: 30.0 }, |p| Camera { eye: p.eye, target: p.target, fov_deg: 30.0 });
        SceneState {
            prefix: prefix.to_string(),
            outfit,
            hair,
            motion: None,
            motion_t0: 0.0,
            mood: 0,
            voice: None,
            cam: CamAnim { from: cam, to: cam, t0: 0.0, dur: 0.0 },
            lights: p.map_or([(-48, -18), (-180, 122), (-180, 180)], |p| p.lights),
            fade: (0.0, 0.0, 0.0, 0.0),
            dialogue: None,
            state: 0,
            fov: 30.0,
            idle: None,
        }
    }

    /// apply an effect that happens at scene time `t` (seconds); `voice_len(clip)` = clip length in seconds
    pub fn apply(&mut self, t: f64, e: &Effect, presets: &BTreeMap<i32, Preset>, motions: &MotionTable, voice_len: &dyn Fn(u32) -> f64) {
        match e {
            Effect::Motion { motion, .. } => {
                if let Some(r) = motions.get(*motion) {
                    // indices 0-738 (stems m_*) are the common set every heroine plays; 739+ belong to Rinko / Nene only
                    if r.heroine == 'm' || r.heroine.to_string() == self.prefix {
                        self.motion = Some(r.stem.clone());
                        self.motion_t0 = t;
                        if r.mood >= 0 {
                            self.mood = r.mood as usize;
                        }
                    }
                }
            }
            Effect::Face { mood, .. } | Effect::FaceAfter { mood } => self.mood = (*mood as usize).min(44),
            Effect::Camera { preset, frames, snap } => {
                if let Some(p) = presets.get(&(*preset as i32)) {
                    let to = Camera { eye: p.eye, target: p.target, fov_deg: self.fov };
                    let now = self.camera_at(t);
                    let dur = if *snap { 0.0 } else { frames.map_or(0.0, |f| f as f64 / 60.0) };
                    self.cam = CamAnim { from: now, to, t0: t, dur };
                    self.lights = p.lights;
                }
            }
            Effect::Fade { out, frames, .. } => {
                let (to, dur) = (if *out { 1.0 } else { 0.0 }, *frames as f64 / 60.0);
                self.fade = (self.fade_at(t), to, t, dur);
            }
            Effect::Dialogue { name, text, .. } => self.dialogue = Some(Dialogue { name: name.clone(), text: text.clone(), t0: t }),
            Effect::Voice { clip, .. } if *clip >= 0 => self.voice = Some((*clip as u32, t, voice_len(*clip as u32))),
            _ => {}
        }
    }

    pub fn camera_at(&self, t: f64) -> Camera {
        let c = &self.cam;
        let k = if c.dur <= 0.0 { 1.0 } else { ((t - c.t0) / c.dur).clamp(0.0, 1.0) };
        let k = k * k * (3.0 - 2.0 * k); // smoothstep
        let l = |a: f64, b: f64| a + (b - a) * k;
        Camera {
            eye: [l(c.from.eye[0], c.to.eye[0]), l(c.from.eye[1], c.to.eye[1]), l(c.from.eye[2], c.to.eye[2])],
            target: [l(c.from.target[0], c.to.target[0]), l(c.from.target[1], c.to.target[1]), l(c.from.target[2], c.to.target[2])],
            fov_deg: self.fov,
        }
    }

    pub fn fade_at(&self, t: f64) -> f32 {
        let (a, b, t0, d) = self.fade;
        if d <= 0.0 {
            return b;
        }
        let k = ((t - t0) / d).clamp(0.0, 1.0) as f32;
        a + (b - a) * k
    }

    /// heroine compose options at scene time `t`
    pub fn compose_opts(&self, t: f64, tables: &FaceTables, lip: Option<&LipSync>) -> ComposeOpts {
        let frame = ((t - self.motion_t0).max(0.0) * 30.0) as usize;
        let viseme = match (&self.voice, lip) {
            (Some((clip, t0, dur)), Some(l)) => l.viseme(*clip, t - t0, *dur),
            _ => 5,
        };
        ComposeOpts {
            outfit: self.outfit,
            hair: self.hair,
            motion: self.motion.clone().or_else(|| self.idle.clone()),
            frame,
            mood: self.mood,
            viseme,
            eye_column: 0,
            blink: tables.blink_closed((t * 30.0) as u64),
            state: self.state,
            ..Default::default()
        }
    }

    /// characters of the current dialogue revealed at time `t` (typewriter, 14 characters per second)
    pub fn revealed(&self, t: f64) -> usize {
        match &self.dialogue {
            Some(d) => ((t - d.t0).max(0.0) * 14.0) as usize,
            None => 0,
        }
    }

    /// draw the dialogue box into `img` (straight-alpha RGBA, transparent elsewhere); returns true when something is shown
    pub fn draw_ui(&self, img: &mut Image, t: f64, font: &Font) -> bool {
        let d = match &self.dialogue {
            Some(d) if !d.text.trim().is_empty() || !d.name.trim().is_empty() => d,
            _ => return false,
        };
        let (w, h) = (img.w as i32, img.h as i32);
        let box_h = (h as f32 * 0.24) as i32;
        let (bx, by, bw) = ((w as f32 * 0.04) as i32, h - box_h - (h as f32 * 0.035) as i32, (w as f32 * 0.92) as i32);
        let scale = ((box_h as f32 * 0.78) / (3.0 * 12.0 + 6.0)).floor().max(1.0) as i32;
        let fill = |img: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]| {
            for y in y0.max(0)..y1.min(h) {
                for x in x0.max(0)..x1.min(w) {
                    let o = (y as usize * img.w + x as usize) * 4;
                    img.buf[o..o + 4].copy_from_slice(&c);
                }
            }
        };
        // window: dark translucent body with a cyan frame, like the game's message window
        let b = (scale / 2).max(1);
        fill(img, bx, by, bx + bw, by + box_h, [0, 190, 215, 230]);
        fill(img, bx + b, by + b, bx + bw - b, by + box_h - b, [8, 18, 40, 215]);
        // name tag
        let name = d.name.trim();
        if !name.is_empty() {
            let (tw, th) = (font.line_width(name) * scale + 6 * scale, 14 * scale);
            fill(img, bx + 2 * scale, by - th + b, bx + 2 * scale + tw, by + b, [250, 250, 250, 245]);
            font.draw(img, name, bx + 5 * scale, by - th + 2 * scale, scale, [20, 30, 50], usize::MAX);
        }
        let n = self.revealed(t);
        font.draw(img, &d.text, bx + 5 * scale, by + 4 * scale, scale, [255, 255, 255], n);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn presets() -> BTreeMap<i32, Preset> {
        let mut m = BTreeMap::new();
        m.insert(1000, Preset { id: 1000, eye: [0.0, 144.0, 150.0], target: [0.0, 124.0, 0.0], lights: [(-48, -18), (-180, 122), (-180, 180)] });
        m.insert(2000, Preset { id: 2000, eye: [50.0, 100.0, 300.0], target: [0.0, 90.0, 0.0], lights: [(0, 0); 3] });
        m
    }

    fn motions() -> MotionTable {
        MotionTable { recs: vec![MotionRec { heroine: 'm', stem: "m_0000_40".into(), mood: -1 }, MotionRec { heroine: 'r', stem: "r_0001_30".into(), mood: 3 }, MotionRec { heroine: 'm', stem: "m_0002_30".into(), mood: 7 }] }
    }

    fn tables() -> FaceTables {
        FaceTables::from_json(&serde_json::json!({
            "moods": [{"brow": 0, "mouth_row": 0, "cheek": 0}], "mouth_rows": [[0, 1, 2, 3]], "eye_table": [[0, 1, 2]],
            "viseme": {"col": [0, 1, 2, 3, 2, 3], "next_mode": [1, 2, 3, 4, 5, 0]}, "blink": {"closed_column": 2, "interval": [60, 90], "hold": [3, 4]}
        }))
        .unwrap()
    }

    #[test]
    fn effects_change_pose_face_and_motion_time() {
        let (p, m) = (presets(), motions());
        let mut s = SceneState::new("m", 0, 0, &p);
        let none = |_: u32| 0.0;
        s.apply(1.0, &Effect::Motion { motion: 0, blend: None, hold: None }, &p, &m, &none);
        assert_eq!(s.compose_opts(1.5, &tables(), None).motion.as_deref(), Some("m_0000_40"));
        assert_eq!(s.compose_opts(1.5, &tables(), None).frame, 15, "30 updates per second since the motion started");
        // a motion for another heroine is ignored
        // (but the common m_ set plays on any heroine)
        let mut rin = SceneState::new("r", 0, 0, &p);
        rin.apply(1.0, &Effect::Motion { motion: 0, blend: None, hold: None }, &p, &m, &none);
        assert_eq!(rin.compose_opts(1.5, &tables(), None).motion.as_deref(), Some("m_0000_40"));
        s.apply(2.0, &Effect::Motion { motion: 1, blend: None, hold: None }, &p, &m, &none);
        assert_eq!(s.compose_opts(2.5, &tables(), None).motion.as_deref(), Some("m_0000_40"));
        // a motion with a default mood sets it; Face overrides it
        s.apply(3.0, &Effect::Motion { motion: 2, blend: None, hold: None }, &p, &m, &none);
        assert_eq!(s.mood, 7);
        s.apply(3.1, &Effect::Face { mood: 99, delay: 0 }, &p, &m, &none);
        assert_eq!(s.mood, 44, "moods are clamped to the 45 defined ones");
    }

    #[test]
    fn camera_presets_snap_or_ease() {
        let (p, m) = (presets(), motions());
        let mut s = SceneState::new("m", 0, 0, &p);
        let none = |_: u32| 0.0;
        assert_eq!(s.camera_at(0.0).eye, [0.0, 144.0, 150.0]);
        s.apply(1.0, &Effect::Camera { preset: 2000, frames: Some(60), snap: false }, &p, &m, &none);
        assert_eq!(s.camera_at(1.0).eye[2], 150.0);
        let mid = s.camera_at(1.5).eye[2];
        assert!(mid > 150.0 && mid < 300.0);
        assert_eq!(s.camera_at(3.0).eye, [50.0, 100.0, 300.0]);
        assert_eq!(s.lights[0], (0, 0));
        s.apply(5.0, &Effect::Camera { preset: 1000, frames: None, snap: true }, &p, &m, &none);
        assert_eq!(s.camera_at(5.0).eye, [0.0, 144.0, 150.0]);
        s.apply(6.0, &Effect::Camera { preset: 12345, frames: None, snap: true }, &p, &m, &none);
        assert_eq!(s.camera_at(6.0).eye, [0.0, 144.0, 150.0], "unknown presets are ignored");
    }

    #[test]
    fn fades_ramp_between_levels() {
        let (p, m) = (presets(), motions());
        let mut s = SceneState::new("m", 0, 0, &p);
        let none = |_: u32| 0.0;
        s.apply(0.0, &Effect::Fade { out: true, frames: 60, screen: 0 }, &p, &m, &none);
        assert!((s.fade_at(0.5) - 0.5).abs() < 1e-6 && s.fade_at(2.0) == 1.0);
        s.apply(3.0, &Effect::Fade { out: false, frames: 30, screen: 0 }, &p, &m, &none);
        assert!((s.fade_at(3.25) - 0.5).abs() < 1e-6 && s.fade_at(4.0) == 0.0);
    }

    #[test]
    fn dialogue_reveals_over_time_and_lip_sync_follows_the_voice() {
        let (p, m) = (presets(), motions());
        let mut s = SceneState::new("m", 0, 0, &p);
        s.apply(2.0, &Effect::Dialogue { name: "高嶺".into(), sub: "".into(), text: "こんにちは".into(), mode: None, choice: None }, &p, &m, &|_| 0.0);
        assert_eq!(s.revealed(2.0), 0);
        assert_eq!(s.revealed(2.5), 7);
        assert_eq!(s.dialogue.as_ref().unwrap().name, "高嶺");
        // lip sync: keys 0,1,2,3 over a 4 s clip, rest outside it
        let mut add = Vec::new();
        add.extend_from_slice(&0u32.to_le_bytes());
        add.extend_from_slice(&4u32.to_le_bytes());
        let lip = LipSync::new(&add, vec![0, 1, 2, 3]);
        assert_eq!([lip.viseme(0, 0.1, 4.0), lip.viseme(0, 1.5, 4.0), lip.viseme(0, 3.9, 4.0), lip.viseme(0, 4.5, 4.0), lip.viseme(9, 1.0, 4.0)], [0, 1, 3, 5, 5]);
        s.apply(1.0, &Effect::Voice { clip: 0, level: None, frames: None }, &p, &m, &|_| 4.0);
        assert_eq!(s.compose_opts(2.5, &tables(), Some(&lip)).viseme, 1);
        assert_eq!(s.compose_opts(2.5, &tables(), None).viseme, 5);
    }

    #[test]
    fn blinks_follow_the_tables() {
        let t = tables();
        assert!(!t.blink_closed(0) && !t.blink_closed(59));
        assert!(t.blink_closed(60) && t.blink_closed(62) && !t.blink_closed(63));
        assert!(t.blink_closed(60 + 3 + 90) && !t.blink_closed(60 + 3 + 94));
        assert_eq!(t.blink_closed(60 + 3 + 94), t.blink_closed(60 + 3 + 94 + 157), "cycle repeats");
    }

    #[test]
    fn motion_table_json() {
        let m = MotionTable::from_json(&serde_json::json!({"records": [{"heroine": "r", "stem": "r_1", "mood": 4}, {"heroine": "n", "stem": "n_2"}]})).unwrap();
        assert_eq!(m.get(0).unwrap().heroine, 'r');
        assert_eq!(m.get(1).unwrap().mood, -1);
        assert!(m.get(5).is_none());
    }
}
