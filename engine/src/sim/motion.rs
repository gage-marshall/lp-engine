//! Heroine motion controller (reference `sandbox/gx/motion_sim.py`; 1,173 / 1,177 updates of the recorded scene exact).
//!
//! Ports: `MapCtl_UpdateTouchSelection` 0x204f664 (per-update driver: start a requested transition once its hold (+0x90) has
//! run out and the previous blend is over; pending mood (+0x4, delay +0x94); when idle and the pose is ready: the queued
//! action (+0x74 = cmd 0x5d) or the default key (+0x9c)), `MapCtl_BeginTransitionByKey` 0x204fd50 (cmd f),
//! `MapCtl_QueuePendingAction74` 0x2050034 (cmd 0x5d), `Gfx3D_UpdateActor/ProcessActor/UpdateModel_2/InitializeModel`,
//! `Gfx3D_DrawActor` 0x207d764 (blend weights `Motion_UpdateBlendWeights` 0x1ffa56c or the group state machine
//! `Motion_UpdateStateMachine` 0x1ffa03c), `Mod2045ee8_Process` 0x2046670 (per-layer frame advance).
//! Not modelled: async loading (ready at once), touch input, random transitions (0x4d).

use crate::heroine::bundle_entries;
use crate::nns3d::{parse_bca, parse_bva, visible, VisAnim};
use crate::rom::Rom;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct MotionRec {
    pub bundle: u32,
    pub stem: String,
    /// group info record (`Motion_GetInfoRecord`, 0x2126028 + 6 group): type, mood
    pub typ: i32,
    pub mood: i32,
}

/// content the controller reads: the motion table, the key table, clip lengths and visibility tracks
pub struct MotionData {
    pub recs: Vec<MotionRec>,
    /// tm_model_block_base.arc/0101.bin: 911 keys x 3 classes x 3 heroines x {s8 mood, s8 eye, s16 index}
    keys: Vec<u8>,
    /// `Motion_GetTableValueOrOne` segment counts per group type (0x2102e08)
    pub segments: [i32; 9],
    /// eye request codes of the eye track's visible node (`Gfx3D_AdvanceActorLoop`, 0x2102e64)
    pub eye_code: Vec<i32>,
    arc: PathBuf,
    nframes: Mutex<HashMap<usize, i32>>,
    bva: Mutex<HashMap<usize, Arc<Vec<(String, VisAnim)>>>>,
}

impl MotionData {
    pub fn load(content: &Path, rom: &Rom) -> Result<MotionData, String> {
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(content.join("engine/motion_table.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let recs = v["records"]
            .as_array()
            .ok_or("motion_table records")?
            .iter()
            .map(|r| MotionRec {
                bundle: r["bundle"].as_u64().unwrap_or(0) as u32,
                stem: r["stem"].as_str().unwrap_or("").to_string(),
                typ: r["info"][0].as_i64().unwrap_or(0) as i32,
                mood: r["info"][1].as_i64().unwrap_or(-1) as i32,
            })
            .collect();
        let arc = content.join("assets/arc");
        let keys = std::fs::read(arc.join("tm_model_block_base.arc/0101.bin")).map_err(|e| format!("motion key table: {e}"))?;
        let s = rom.i32s(0x2102e08, 9);
        Ok(MotionData {
            recs,
            keys,
            segments: [s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7], s[8]],
            eye_code: rom.i32s(0x2102e64, 22),
            arc,
            nframes: Mutex::new(HashMap::new()),
            bva: Mutex::new(HashMap::new()),
        })
    }

    /// (mood, eye, motion index) of a script key for a heroine and model class
    pub fn key_entry(&self, key: i32, heroine: u32, cls: u32) -> (i32, i32, i32) {
        let o = 4 + key as usize * 36 + cls as usize * 12 + heroine as usize * 4;
        match self.keys.get(o..o + 4) {
            Some(b) => (b[0] as i8 as i32, b[1] as i8 as i32, i16::from_le_bytes([b[2], b[3]]) as i32),
            None => (-1, -1, -1),
        }
    }

    pub fn group(&self, idx: i32) -> i64 {
        self.recs.get(idx as usize).map_or(-1, |r| r.bundle as i64)
    }

    fn info_type(&self, idx: i32) -> i32 {
        self.recs.get(idx as usize).map_or(0, |r| r.typ)
    }

    /// `Motion_FindGroupStartIndex`
    pub fn group_start(&self, idx: i32) -> i32 {
        let g = self.group(idx);
        let from = if idx < 4 { 0 } else { idx - 4 };
        (from..idx).find(|&j| self.group(j) == g).unwrap_or(idx)
    }

    /// `Motion_IsFrameOffsetValidForType`
    fn offset_valid(&self, idx: i32) -> bool {
        let (t, s) = (self.info_type(idx), idx - self.group_start(idx));
        ((t == 2 || t == 3) && s == 1) || (t == 4 && (s == 1 || s == 3)) || ((t == 6 || t == 7) && s == 2)
    }

    /// `Motion_IsIdInSpecialRangeSet` 0x1ffaa74: idle-type groups (their key becomes the default key, MapCtl +0x9c)
    fn special(&self, idx: i32) -> bool {
        let s = self.group_start(idx);
        [200, 346, 363, 364, 365, 719, 733, 735, 941, 942, 943, 944].contains(&s) || (236..=247).contains(&s) || self.info_type(idx) == 4
    }

    /// clip length in frames (the `<stem>_body` track of `<stem>.nsbca` in async3d bundle `group`)
    pub fn nframe(&self, idx: i32) -> i32 {
        if let Some(&n) = self.nframes.lock().unwrap().get(&(idx as usize)) {
            return n;
        }
        let Some(r) = self.recs.get(idx as usize) else { return 1 };
        let n = std::fs::read(self.arc.join(format!("async3d.arc/{:04}.bin", r.bundle)))
            .ok()
            .and_then(|b| bundle_entries(&b))
            .and_then(|e| e.into_iter().find(|(n, _)| *n == format!("{}.nsbca", r.stem)))
            .and_then(|(_, d)| parse_bca(&Arc::new(d)).ok())
            .and_then(|anims| anims.into_iter().find(|a| a.name == format!("{}_body", r.stem)).map(|a| a.frames as i32))
            .unwrap_or(1);
        self.nframes.lock().unwrap().insert(idx as usize, n);
        n
    }

    /// the motion's visibility tracks by part name (`eye00`, `lhand`, `rhand`, ...), in file order: async3d bundle group + 549
    pub fn bva(&self, idx: i32) -> Arc<Vec<(String, VisAnim)>> {
        if let Some(v) = self.bva.lock().unwrap().get(&(idx as usize)) {
            return v.clone();
        }
        let Some(r) = self.recs.get(idx as usize) else { return Arc::new(Vec::new()) };
        let pre = format!("{}_", r.stem);
        let m: Vec<(String, VisAnim)> = std::fs::read(self.arc.join(format!("async3d.arc/{:04}.bin", r.bundle + 549)))
            .ok()
            .and_then(|b| bundle_entries(&b))
            .and_then(|e| e.into_iter().find(|(n, _)| *n == format!("{}.nsbva", r.stem)))
            .and_then(|(_, d)| parse_bva(&d).ok())
            .map(|v| v.into_iter().map(|a| (a.name.strip_prefix(&pre).unwrap_or(&a.name).to_string(), a)).collect())
            .unwrap_or_default();
        let m = Arc::new(m);
        self.bva.lock().unwrap().insert(idx as usize, m.clone());
        m
    }
}

/// `FX_Div`: (numer << 32) / denom, truncating, then rounded to 20.12
pub fn fx_div(numer: i32, denom: i32) -> i32 {
    let q = ((numer as i64) << 32) / denom as i64;
    ((q + (1 << 19)) >> 20) as i32
}

/// AnmObj ratios (layer 0 = new, 1 = previous, 2 = third) `ticks` updates into a blend of length `blend`
/// (`Gfx3D_InitializeModel` 0x207d218 + `Motion_UpdateBlendWeights` 0x1ffa56c; single-precision steps as in the code)
pub fn blend_ratios(blend: i32, ticks: i32) -> Option<[i32; 3]> {
    let inc0 = (4096.0f32 / (blend as f32 * 0.7f32)) as i32;
    let (mut remaining, mut w) = (blend, [0i32, 4096, 0]);
    let mut n = None;
    for _ in 0..ticks {
        if remaining <= 0 {
            return None;
        }
        let t = fx_div(blend - remaining, blend) as f32;
        if t >= 1228.8f32 {
            w[0] = (w[0] + inc0).min(4096);
        }
        w[1] = (w[1] - inc0).max(0);
        remaining -= 1;
        let s = w[0] + w[1] + w[2];
        let (n0, n1) = (fx_div(w[0], s), fx_div(w[1], s));
        n = Some([n0, n1, 4096 - n0 - n1]);
    }
    n
}

#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub idx: i32,
    /// frame in 20.12
    pub frame: i32,
}

/// one animation layer as presentation needs it
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRef {
    pub bundle: u32,
    pub stem: String,
    /// whole frames
    pub frame: i32,
    /// AnmObj ratio, 4096 = 1
    pub ratio: i32,
}

/// `MapCtl_StartRandomTransition` 0x204fedc: idle key 0x28e for these camera presets, else 0x217
pub fn idle_key(preset: i32) -> i32 {
    if [1008, 1009, 1010, 2008, 2009, 2010, 3008, 3009, 3010].contains(&preset) { 0x28e } else { 0x217 }
}

#[derive(Clone)]
pub struct MotionSim {
    h: u32,
    cls: u32,
    pub idx: i32,
    typ: i32,
    seg: i32,
    rem: i32,
    total: i32,
    pub n: [i32; 3],
    s30: i32,
    s34: i32,
    s38: i32,
    pub mood: i32,
    pub layers: [Option<Layer>; 3],
    loop0: bool,
    loop1: bool,
    busy: bool,
    target: i32,
    by_key: bool,
    key: i32,
    blend: i32,
    m6c: i32,
    hold: i32,
    m88: i32,
    m8c: i32,
    q74: bool,
    q80: i32,
    q84: i32,
    mood_pending: i32,
    mood_after: i32,
    mood_delay: i32,
    default_key: i32,
    /// `Model_SetMotionIdAndInitFlags` calls since the last `take_mood_events` (each starts a type-2 eye reload)
    mood_events: u32,
    /// motion model class of the outfit (`MotionEntry_GetShort6`; set by script 12e 4 before the first motion)
    pub cls_set: bool,
}

impl MotionSim {
    pub fn new(heroine: u32, cls: u32) -> MotionSim {
        MotionSim {
            h: heroine,
            cls,
            idx: -1,
            typ: 0,
            seg: 0,
            rem: 0,
            total: 0,
            n: [4096, 0, 0],
            s30: 0,
            s34: 0,
            s38: 0,
            mood: 0,
            layers: [None, None, None],
            loop0: false,
            loop1: false,
            busy: false,
            target: -1,
            by_key: false,
            key: -1,
            blend: 0xf,
            m6c: 0,
            hold: 0,
            m88: 0,
            m8c: 0,
            q74: false,
            q80: -1,
            q84: 0xf,
            mood_pending: -1,
            mood_after: -1,
            mood_delay: 0,
            default_key: -1,
            mood_events: 0,
            cls_set: false,
        }
    }

    pub fn set_class(&mut self, cls: u32) {
        self.cls = cls;
        self.cls_set = true;
    }

    pub fn take_mood_events(&mut self) -> u32 {
        std::mem::take(&mut self.mood_events)
    }

    // ---------------------------------------------------------------- script commands
    /// cmd f key [blend [hold]] (`MapCtl_BeginTransitionByKeyWithMode8` 0x204fe9c: +0x88 = 8, +0x90 = hold)
    pub fn cmd_f(&mut self, d: &MotionData, key: i32, blend: i32, hold: i32) {
        self.begin_by_key(d, key, blend, 0);
        self.m88 = 8;
        self.hold = hold;
    }

    /// set_actor (cmd 2) -> `MapCtl_SetupAndRefresh` -> `MapCtl_StartRandomTransition(0)`: the preset's idle motion
    pub fn cmd_set_actor(&mut self, d: &MotionData, preset: i32) {
        self.begin_by_key(d, idle_key(preset), 0, 0);
    }

    /// cmd 0x5d key [timeout [blend]]
    pub fn cmd_5d(&mut self, key: i32, timeout: i32, blend: i32) {
        self.q74 = true;
        self.q80 = key;
        self.q84 = blend;
        self.m88 = timeout;
        self.m8c = 0;
    }

    /// cmd 0x34 mood [delay]: at once when no transition runs and delay <= 0, else pending
    pub fn cmd_34(&mut self, mood: i32, delay: i32) {
        if !self.busy && delay <= 0 {
            self.set_mood(mood);
        } else {
            self.mood_pending = mood;
            self.mood_delay = delay;
        }
    }

    /// cmd 0x136 mood: the mood for after the current motion
    pub fn cmd_136(&mut self, mood: i32) {
        self.mood_after = mood;
    }

    fn begin_by_key(&mut self, d: &MotionData, key: i32, blend: i32, m6c: i32) {
        let idx = d.key_entry(key, self.h, self.cls).2;
        if idx < 0 || idx as usize >= d.recs.len() {
            return; // no motion for this key, heroine and class
        }
        if self.idx >= 0 && idx == d.group_start(self.idx) {
            return;
        }
        self.busy = true;
        self.target = idx;
        self.by_key = true;
        self.key = key;
        self.blend = blend;
        self.m6c = m6c;
        self.q74 = false;
        self.hold = 0;
        if d.special(idx) {
            self.default_key = key;
        }
    }

    // ---------------------------------------------------------------- per update
    fn all_finished(&self, d: &MotionData) -> bool {
        match &self.layers[0] {
            Some(l) => !self.loop0 && l.frame == d.nframe(l.idx) << 12,
            None => false,
        }
    }

    fn pose_ready(&self, d: &MotionData) -> bool {
        if self.idx == -1 {
            return true;
        }
        let t = d.info_type(self.idx);
        let cnt = if t >= 0 { d.segments[t as usize] } else { 1 };
        if self.idx - (cnt - 1) < 0 || d.group(self.idx - (cnt - 1)) != d.group(self.idx) {
            return false;
        }
        self.all_finished(d)
    }

    /// `MapCtl_UpdateTouchSelection` (without touch); `Model_UpdateCharaScene` -> `Gfx3D_DrawActor` first
    pub fn update(&mut self, d: &MotionData) {
        self.draw_actor(d);
        if !self.busy {
            if self.mood_delay > 0 {
                self.mood_delay -= 1;
            }
            if self.mood_delay < 1 && self.mood_pending != -1 {
                let m = self.mood_pending;
                self.set_mood(m);
                self.mood_pending = -1;
            }
        } else {
            if self.hold > 0 {
                self.hold -= 1;
            }
            if self.hold < 1 && self.rem <= 0 {
                let key = if self.by_key { Some(self.key) } else { None };
                self.start(d, self.target, self.blend, self.m6c, key);
                self.busy = false;
                self.by_key = false;
            }
        }
        if self.idx == 0 || self.busy {
            return;
        }
        if !self.pose_ready(d) {
            if d.offset_valid(self.idx) {
                self.m8c += 1;
                if self.m8c >= self.m88 {
                    self.s34 = 1;
                }
            }
            return;
        }
        if self.q74 {
            self.begin_by_key(d, self.q80, self.q84, 0);
        } else {
            self.begin_by_key(d, self.default_key, 0xf, 0);
        }
        self.mood_pending = self.mood_after;
        self.mood_after = -1;
        self.m8c = 0;
        self.m88 = 0;
    }

    fn start(&mut self, d: &MotionData, idx: i32, blend: i32, m6c: i32, key: Option<i32>) {
        let t = d.info_type(idx);
        let mut lp = m6c;
        self.s30 = m6c;
        match t {
            1 => lp = 1,
            8 => {
                lp = 0;
                self.s30 = 1;
            }
            0 => {}
            _ => lp = 0,
        }
        self.typ = t;
        self.seg = idx - d.group_start(idx);
        self.init_model(d, idx, if self.idx != -1 { blend } else { 0 }, lp != 0);
        let mut mood = self.mood_pending;
        if mood == -1 {
            if let Some(k) = key {
                mood = d.key_entry(k, self.h, self.cls).0;
            }
        }
        if mood == -1 {
            mood = d.recs.get(idx as usize).map_or(-1, |r| r.mood);
        }
        self.set_mood(if mood == -1 { 0 } else { mood });
    }

    /// `Model_SetMotionIdAndInitFlags` 0x2079c74: mood (+0x454); always starts a type-2 eye reload
    fn set_mood(&mut self, mood: i32) {
        self.mood = mood;
        self.mood_events += 1;
    }

    fn init_model(&mut self, d: &MotionData, idx: i32, blend: i32, lp: bool) {
        let prev = self.layers[0].clone();
        let prev_idx = self.idx;
        match prev {
            Some(p) if blend >= 1 => {
                self.rem = blend;
                self.total = blend;
                self.n = [0, 4096, 0];
                self.layers = [Some(Layer { idx, frame: 0 }), Some(Layer { idx: prev_idx, frame: p.frame }), None];
                self.loop1 = d.offset_valid(prev_idx) || d.info_type(prev_idx) == 1;
            }
            _ => {
                self.rem = 0;
                self.total = 0;
                self.layers = [Some(Layer { idx, frame: 0 }), None, None];
            }
        }
        self.loop0 = lp;
        self.idx = idx;
    }

    fn draw_actor(&mut self, d: &MotionData) {
        if self.rem < 1 {
            self.state_machine(d);
        } else {
            let n = blend_ratios(self.total, self.total - self.rem + 1);
            self.rem -= 1;
            if self.rem < 1 {
                self.n = [4096, 0, 0];
                self.layers[1] = None;
                self.layers[2] = None;
            } else if let Some(n) = n {
                self.n = n;
            }
        }
    }

    fn state_machine(&mut self, d: &MotionData) {
        if self.idx == -1 {
            return;
        }
        let t = self.typ;
        let s = self.idx - d.group_start(self.idx);
        let fin = self.all_finished(d);
        let pm = |me: &mut MotionSim, step: i32, lp: bool| {
            me.idx += step;
            let segs = d.segments[t as usize].max(1);
            me.seg = (me.seg + 1) % segs;
            me.init_model(d, me.idx, 0, lp);
        };
        let clear = |me: &mut MotionSim| {
            if me.s34 == 1 {
                me.s34 = 0;
                me.loop0 = false;
            }
        };
        let s30 = self.s30 == 1;
        match t {
            1 => {
                if self.s38 == 1 && self.s34 == 1 {
                    self.s34 = 0;
                    self.loop0 = false;
                }
            }
            2 => {
                if s == 0 && fin {
                    pm(self, 1, true)
                } else if s == 1 {
                    clear(self);
                    if s30 && self.all_finished(d) {
                        pm(self, -1, false)
                    }
                }
            }
            3 => {
                if s == 0 && fin {
                    pm(self, 1, true)
                } else if s == 1 {
                    clear(self);
                    if self.all_finished(d) {
                        pm(self, 1, false)
                    }
                } else if s == 2 && s30 && fin {
                    pm(self, -2, false)
                }
            }
            4 => {
                if s == 0 && fin {
                    pm(self, 1, true)
                } else if s == 1 {
                    clear(self);
                    if self.all_finished(d) {
                        pm(self, 1, false)
                    }
                } else if s == 2 && fin {
                    pm(self, 1, true)
                } else if s == 3 {
                    clear(self);
                    if s30 && self.all_finished(d) {
                        pm(self, -3, false)
                    }
                }
            }
            5 => {
                if s == 0 && fin {
                    pm(self, 1, false)
                } else if s == 1 && s30 && fin {
                    pm(self, -1, false)
                }
            }
            6 => {
                if s == 0 && fin {
                    pm(self, 1, false)
                } else if s == 1 && fin {
                    pm(self, 1, true)
                } else if s == 2 {
                    clear(self);
                    if s30 && self.all_finished(d) {
                        pm(self, -3, false)
                    }
                }
            }
            7 => {
                if s == 0 && fin {
                    pm(self, 1, false)
                } else if s == 1 && fin {
                    pm(self, 1, true)
                } else if s == 2 {
                    clear(self);
                    if self.all_finished(d) {
                        pm(self, 1, false)
                    }
                } else if s == 3 && s30 && fin {
                    pm(self, -3, false)
                }
            }
            8 => {
                if s == 0 && fin {
                    pm(self, 1, false)
                } else if s == 1 && fin {
                    pm(self, -1, false)
                }
            }
            _ => {}
        }
    }

    /// `Mod2045ee8_Process` for layers 0..2: += 1 frame, wrap if the layer loops, else clamp
    pub fn advance(&mut self, d: &MotionData) {
        let (l0, l1) = (self.loop0, self.loop1);
        for (k, l) in self.layers.iter_mut().enumerate() {
            if let Some(l) = l {
                let n = d.nframe(l.idx) << 12;
                l.frame += 0x1000;
                if l.frame >= n {
                    let loops = if k == 1 { l1 } else { l0 };
                    l.frame = if loops { 0 } else { n };
                }
            }
        }
    }

    /// the AnmObj chain presentation draws (head first)
    pub fn chain(&self, d: &MotionData) -> Vec<LayerRef> {
        self.layers
            .iter()
            .enumerate()
            .filter_map(|(k, l)| {
                let l = l.as_ref()?;
                let r = &d.recs[l.idx as usize];
                let ratio = if self.rem > 0 { self.n[k] } else if k == 0 { 4096 } else { 0 };
                Some(LayerRef { bundle: r.bundle, stem: r.stem.clone(), frame: l.frame >> 12, ratio })
            })
            .collect()
    }

    /// value of a visibility track at layer 0's current frame: first visible node at or above `base` (minus `base`); the
    /// eye track skips nodes 0, 2, 3, 4 (`Gfx3D_UpdateActorState` 0x207db00)
    pub fn track_value(&self, d: &MotionData, name: &str, base: usize, eye: bool) -> Option<i32> {
        let l = self.layers[0].as_ref()?;
        let tracks = d.bva(l.idx);
        let any = &tracks.first()?.1;
        let mut fr = (l.frame >> 12) as usize;
        if any.frames > 0 && any.frames <= fr {
            fr = any.frames - 1;
        }
        let a = &tracks.iter().find(|(n, _)| n == name)?.1;
        (base..a.nodes).find(|&n| (!eye || ![0, 2, 3, 4].contains(&n)) && visible(a, fr, n)).map(|n| (n - base) as i32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blend_ratios_follow_the_game_steps() {
        assert_eq!(blend_ratios(15, 0), None);
        let n1 = blend_ratios(15, 1).unwrap();
        assert_eq!(n1.iter().sum::<i32>(), 4096);
        assert_eq!(n1[0], 0, "the new motion fades in only after 30 % of the blend");
        let n8 = blend_ratios(15, 8).unwrap();
        assert!(n8[0] > 0 && n8[1] < 4096);
        assert_eq!(blend_ratios(15, 16), None, "over after `blend` updates");
        assert_eq!(fx_div(1, 2), 2048);
    }

    #[test]
    fn idle_keys() {
        assert_eq!(idle_key(3047), 535);
        assert_eq!(idle_key(3009), 0x28e);
    }
}
