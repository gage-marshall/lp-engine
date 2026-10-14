//! Lip-sync driver (reference `sandbox/gx/lipsync_sim.py`): voice PCM + lip-sync keys -> mouth shape calls per update.
//!
//! `VoiceQueue_UpdateMode` 0x209ab74 (per update while a voice plays): peak = max |sample| over every 5th sample of the
//! stream blocks decoded since the last update (`GameSnd_StreamAccumulatePeak` 0x2078088, 640-sample blocks); peak < 1:
//! nothing; peak < threshold (+0xb12, 1000): shape 5, speaking flag off; else flag on and shape = keys[elapsed n / duration]
//! via `VoiceQueue_PickRandomPattern` 0x209ad64 (shape 5 replaced by the rotating rest pattern 0x21046f4 after +0xb14 updates);
//! the caller holds the shape 20 updates (`Model_SetMotionModeFromTable`). elapsed (+0xafc) counts VBlanks while the flag
//! is on; duration = the clip's 20 ms frames. keys = lipsync/lipsync.dat at lipsync_add.dat[clip] (size - 2 bytes).

use crate::rom::Rom;
use crate::sim::clock::FRAME_HZ;
use std::path::Path;

pub const THRESHOLD: i32 = 0x3e8; // VoiceQueue_Init 0x209a630: +0xb12; cmd 0x1b3 (value, 0) changes it
pub const REST_HOLD: i32 = 0x3c; // +0xb14; cmd 0x1b3 (value, 1)
const PREFILL: i64 = 3; // blocks decoded right after PlayEntry (stream ring prefill)

/// lip-sync keys of every clip
pub struct LipKeys {
    add: Vec<u8>,
    keys: Vec<u8>,
    pub pattern: Vec<i32>,
}

impl LipKeys {
    pub fn load(content: &Path, rom: &Rom) -> Result<LipKeys, String> {
        let rd = |p: &str| std::fs::read(content.join(p)).map_err(|e| format!("{p}: {e}"));
        Ok(LipKeys { add: rd("assets/lipsync/lipsync_add.dat")?, keys: rd("assets/lipsync/lipsync.dat")?, pattern: rom.i32s(0x21046f4, 16) })
    }

    pub fn keys(&self, clip: u32) -> Vec<u8> {
        let o = clip as usize * 8;
        let Some(e) = self.add.get(o..o + 8) else { return Vec::new() };
        let (off, n) = (u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize, u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize);
        self.keys.get(off..off + n.saturating_sub(2)).map(|k| k.to_vec()).unwrap_or_default()
    }
}

/// peak of each 640-sample block (every 5th sample, at most 256 of them)
pub fn block_peaks(pcm: &[i16]) -> Vec<i32> {
    pcm.chunks_exact(640).map(|b| b.iter().step_by(5).take(128).map(|&s| (s as i32).abs()).max().unwrap_or(0)).collect()
}

#[derive(Clone)]
pub struct LipSync {
    pub active: bool,
    thr: i32,
    b14: i32,
    keys: Vec<u8>,
    peaks: Vec<i32>,
    dur: i64,
    start: i64,
    el: i64,
    flag: bool,
    b04: i32,
    b08: i32,
    done_block: i64,
}

impl Default for LipSync {
    fn default() -> Self {
        LipSync { active: false, thr: THRESHOLD, b14: REST_HOLD, keys: Vec::new(), peaks: Vec::new(), dur: 0, start: 0, el: 0, flag: false, b04: 0, b08: 0, done_block: -1 }
    }
}

impl LipSync {
    /// the stream starts (`VoiceQueue_PlayEntry`) at frame `frame`
    pub fn play(&mut self, keys: Vec<u8>, peaks: Vec<i32>, frame: i64) {
        self.dur = peaks.len() as i64;
        self.keys = keys;
        self.peaks = peaks;
        self.start = frame;
        self.el = 0;
        self.flag = false;
        self.b04 = 0;
        self.b08 = 0;
        self.active = true;
        self.done_block = -1;
    }

    /// `VoiceQueue_SetParam` 0x209af4c (cmd 0x1b3)
    pub fn set_param(&mut self, which: u32, value: i32) {
        match which {
            0 => self.thr = value,
            1 => self.b14 = value,
            _ => {}
        }
    }

    /// VBlanks f0 < f <= f1 pass: elapsed += 1 per frame while speaking
    pub fn frames(&mut self, f0: i64, f1: i64) {
        if self.active && self.flag {
            self.el += f1 - f0;
        }
    }

    fn decoded(&self, frame: i64) -> i64 {
        if frame < self.start + 1 {
            return 0;
        }
        let d = frame - self.start;
        (d.min(PREFILL) + (((d - PREFILL) as f64) * (50.0 / FRAME_HZ)).max(0.0) as i64).min(self.dur)
    }

    /// one `VoiceQueue_UpdateMode` at frame `frame`: the shapes dispatched
    pub fn update(&mut self, frame: i64, pattern: &[i32]) -> Vec<i32> {
        if !self.active {
            return Vec::new();
        }
        let t = (frame - self.start) as f64 * (1000.0 / FRAME_HZ);
        let last = self.decoded(frame) - 1;
        let lo = (self.done_block + 1).max(0) as usize;
        let hi = (last + 1).max(0) as usize;
        let peak = if hi > lo { self.peaks[lo..hi.min(self.peaks.len())].iter().copied().max().unwrap_or(0) } else { 0 };
        self.done_block = self.done_block.max(last);
        self.flag = false;
        if peak < 1 {
            self.b04 = 0;
            self.b08 = 0;
            if last >= self.dur - 1 && t >= self.dur as f64 * 20.0 {
                self.active = false;
                return vec![5];
            }
            return Vec::new();
        }
        let v = if peak < self.thr {
            5
        } else {
            self.flag = true;
            let dur = self.dur.max(1);
            if dur < self.el {
                self.el = dur;
            }
            let k = if self.keys.is_empty() { 5 } else { self.keys[((self.el as usize * self.keys.len()) / dur as usize).min(self.keys.len() - 1)] as i32 };
            self.pick(peak, k, pattern)
        };
        let mut out = vec![v];
        if self.dur <= self.el {
            out.push(5);
            self.active = false;
        }
        out
    }

    fn pick(&mut self, peak: i32, v: i32, pattern: &[i32]) -> i32 {
        if v == 5 {
            if self.b04 < self.b14 {
                self.b04 += 1;
                v
            } else {
                self.b04 = 0;
                let r = pattern[(((peak > 0x5d) as usize) * 8 + ((self.b08 / 2) % 7) as usize).min(pattern.len() - 1)];
                self.b08 += 1;
                if self.b08 > 999 {
                    self.b08 = 0;
                }
                r
            }
        } else {
            self.b04 = 0;
            self.b08 = 0;
            v
        }
    }
}
