//! Game audio: turns the scene simulation's sound events into driver calls, plays them in real time or renders them to a buffer.
//!
//! Sound requests of the script commands (decoded from the game's GameSnd_* code):
//!   bgm id [fade]            sequence `id` of the sound archive on the BGM slot (1000/1001 = per-heroine aliases, see bgm_tables.json); replays of the
//!                            running id are ignored; `fade` = fade-in in game frames
//!   se arc idx               sub-sequence `idx` of sequence archive `arc` (one-shot; the SE player allows several at once)
//!   ambient arc idx fade     looping sub-sequence on the ambient slot (a new one replaces the old)
//!   stop_seq / stop_ambient  stop the BGM / ambient slot with a fade
//!   voice clip               G.722.1C clip from Siren14Voice, started by the simulation 4 updates after its request
//!
//! Loudness (docs/AUDIO.md, reference `sandbox/audio_mix`):
//!   * `GameSnd_PlaySeq` 0x2070cc0: the player starts at volume 0 -> 127 in one frame, then `NNS_SndPlayerMoveVolume(127, fade)`;
//!     a stop moves it to 0 over `fade` frames and then ends the sequence. The NitroSystem fader moves the volume v linearly once
//!     per game frame; its gain is (v / 127)^2 (`SND_CalcDecibel`).
//!   * slot volume (extVolume) `GameSnd_ApplyChannelVolume` 0x20724f8: 105 level / 15 from the save's music / sound levels.
//!   * voice: two hardware channels with the same mono PCM, pan 0x40 each: per side 2 (127/128) (63/127 | 64/127), scaled by the
//!     voice level's attenuation (`scene_inputs::voice_cb`): amplitude 10^(cB / 200), silent at or below -723 cB.

use crate::host::Effect;
use crate::scene_inputs::{slot_volume, voice_cb};
use crate::sdat::Sdat;
use crate::siren::{decode_clip, Tables};
use crate::sim::clock::FRAME_HZ;
use crate::snd::{Driver, SeqHandle};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct BgmTables {
    pub by_slot: Vec<u8>,
    pub by_mode: Vec<u8>,
    pub stage: Vec<u8>,
}

/// NitroSystem player fader (TwlSystem snd fader.c): linear in volume, one step per game frame
#[derive(Clone, Copy, Debug)]
struct Fader {
    origin: f64,
    target: f64,
    frames: u32,
    counter: u32,
}

impl Fader {
    fn get(&self) -> f64 {
        if self.counter >= self.frames { self.target } else { self.origin + (self.target - self.origin) * self.counter as f64 / self.frames as f64 }
    }
    fn set(&mut self, target: f64, frames: u32) {
        self.origin = self.get();
        self.target = target;
        self.frames = frames;
        self.counter = 0;
    }
    fn update(&mut self) {
        if self.counter < self.frames {
            self.counter += 1;
        }
    }
    fn done(&self) -> bool {
        self.counter >= self.frames
    }
}

struct SlotPlay {
    id: u32,
    h: SeqHandle,
    fader: Fader,
    stopping: bool,
}

pub struct AudioDirector {
    drv: Driver,
    sdat: Arc<Sdat>,
    tables: Tables,
    vtb: Vec<u8>,
    spp: Vec<u8>,
    bgm_tables: BgmTables,
    /// slot 0 = BGM, 1 = ambient
    slots: [Option<SlotPlay>; 2],
    voice: Option<u64>,
    voice_cache: HashMap<u32, Arc<Vec<i16>>>,
    /// the save's sound levels 0..15: music, sound, voice
    pub levels: [u8; 3],
    out_rate: u32,
    /// output samples until the next game frame
    frame_acc: f64,
}

/// NitroSystem squared volume law
fn sq(v: f64) -> f64 {
    (v / 127.0).powi(2)
}

impl AudioDirector {
    pub fn open(content: &Path, out_rate: u32) -> Result<AudioDirector, String> {
        let rd = |p: &str| std::fs::read(content.join(p)).map_err(|e| format!("{p}: {e}"));
        let sdat = Arc::new(Sdat::parse(rd("assets/sys/sound_data.sdat")?)?);
        let tables = Tables::from_pack(&rd("engine/g7221.pack")?)?;
        let bt: serde_json::Value = serde_json::from_slice(&rd("engine/bgm_tables.json")?).map_err(|e| e.to_string())?;
        let arr = |k: &str| bt[k].as_array().map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect()).unwrap_or_default();
        Ok(AudioDirector {
            drv: Driver::new(sdat.clone(), out_rate),
            sdat,
            tables,
            vtb: rd("assets/sys/Siren14Voice.vtb")?,
            spp: rd("assets/sys/Siren14Voice.spp")?,
            bgm_tables: BgmTables { by_slot: arr("by_slot"), by_mode: arr("by_mode"), stage: arr("stage") },
            slots: [None, None],
            voice: None,
            voice_cache: HashMap::new(),
            levels: [15, 15, 15],
            out_rate,
            frame_acc: 0.0,
        })
    }

    pub fn driver(&mut self) -> &mut Driver {
        &mut self.drv
    }

    /// resolve the alias ids of `GameSnd_ResolveBgmAlias`
    pub fn resolve_bgm(&self, id: u32, slot: u32, mode: bool) -> u32 {
        let r = match id {
            1000 => self.bgm_tables.by_slot.get(slot as usize).copied(),
            1001 => self.bgm_tables.by_mode.get(slot as usize * 2 + mode as usize).copied(),
            _ => None,
        };
        r.map_or(id, |v| v as u32)
    }

    fn ext(&self, slot: usize) -> f64 {
        sq(slot_volume(if slot == 0 { self.levels[0] } else { self.levels[1] }) as f64)
    }

    fn start_slot(&mut self, slot: usize, id: u32, sd: Option<crate::sdat::SeqData>, fade: u32) {
        if let Some(p) = self.slots[slot].take() {
            self.drv.stop(p.h, 0);
        }
        let Some(sd) = sd else { return };
        if let Some(h) = self.drv.start(&sd, sd.info.player as usize, 0) {
            let mut f = Fader { origin: 0.0, target: 0.0, frames: 0, counter: 0 };
            f.set(127.0, 1); // player init: 0 -> 127 in one frame
            if fade > 0 {
                f.set(127.0, fade); // GameSnd_PlaySeq: NNS_SndPlayerMoveVolume(127, fade)
            }
            self.drv.fade_to(h, 0.0, 0);
            self.slots[slot] = Some(SlotPlay { id, h, fader: f, stopping: false });
        }
    }

    fn stop_slot(&mut self, slot: usize, fade: u32) {
        if let Some(mut p) = self.slots[slot].take() {
            if fade == 0 {
                self.drv.stop(p.h, 0);
            } else {
                p.fader.set(0.0, fade);
                p.stopping = true;
                self.slots[slot] = Some(p);
            }
        }
    }

    pub fn play_bgm(&mut self, id: u32, fade: Option<u32>, slot: u32, mode: bool) {
        let id = self.resolve_bgm(id, slot, mode);
        if let Some(p) = &self.slots[0] {
            if p.id == id && !p.stopping && self.drv.is_running(p.h) {
                return; // same sequence already playing: ignored by the game
            }
        }
        let sd = self.sdat.sequence(id as usize);
        self.start_slot(0, id, sd, fade.unwrap_or(0));
    }

    pub fn stop_bgm(&mut self, fade_frames: u32) {
        self.stop_slot(0, fade_frames);
    }

    pub fn play_ambient(&mut self, arc: u32, idx: u32, fade: u32) {
        let sd = self.sdat.archive_sequence(arc as usize, idx as usize);
        self.start_slot(1, u32::MAX, sd, fade);
    }

    pub fn stop_ambient(&mut self, fade_frames: u32) {
        self.stop_slot(1, fade_frames);
    }

    pub fn play_se(&mut self, arc: u32, idx: u32) {
        if let Some(sd) = self.sdat.archive_sequence(arc as usize, idx as usize) {
            if let Some(h) = self.drv.start(&sd, sd.info.player as usize, 0) {
                self.drv.fade_to(h, self.ext(1) as f32, 0);
            }
        }
    }

    pub fn voice_clip(&mut self, clip: u32) -> Option<Arc<Vec<i16>>> {
        if let Some(c) = self.voice_cache.get(&clip) {
            return Some(c.clone());
        }
        let o = clip as usize * 8;
        let e = self.vtb.get(o..o + 8)?;
        let (off, size) = (u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize / 16, u32::from_le_bytes([e[4], e[5], e[6], e[7]]) as usize / 16);
        let (pcm, _) = decode_clip(&self.tables, self.spp.get(off..off + size)?, 16000, 14000);
        let a = Arc::new(pcm);
        self.voice_cache.insert(clip, a.clone());
        Some(a)
    }

    /// per-side voice gains at the save's voice level
    pub fn voice_gains(&self) -> (f32, f32) {
        let cb = voice_cb(self.levels[2]);
        let amp = if cb > -723 { 10f64.powf(cb as f64 / 200.0) } else { 0.0 };
        let k = 2.0 * 127.0 / 128.0;
        ((k * 63.0 / 127.0 * amp) as f32, (k * 64.0 / 127.0 * amp) as f32)
    }

    pub fn play_voice(&mut self, clip: u32) {
        if let Some(id) = self.voice.take() {
            self.drv.stop_clip(id);
        }
        if let Some(pcm) = self.voice_clip(clip) {
            let (l, r) = self.voice_gains();
            self.voice = Some(self.drv.play_clip(pcm, 32000, l, r));
        }
    }

    pub fn voice_playing(&self) -> bool {
        self.voice.map_or(false, |id| self.drv.clip_playing(id))
    }

    /// apply a sound command (`slot` = running heroine slot, `mode` = the game-flow mode flag used by the 1001 alias)
    pub fn apply(&mut self, e: &Effect, slot: u32, mode: bool) {
        match e {
            Effect::Bgm { id, fade } => self.play_bgm(*id, *fade, slot, mode),
            Effect::StopSeq { frames } => self.stop_bgm(*frames),
            Effect::Ambient { player, seq, volume } => self.play_ambient(*player, *seq, *volume),
            Effect::StopAmbient { frames } => self.stop_ambient(*frames),
            Effect::Se { player, id, .. } => self.play_se(*player, *id),
            _ => {}
        }
    }

    /// one game frame of the slot faders
    pub fn tick_frame(&mut self) {
        for s in 0..2 {
            let ext = self.ext(s);
            let mut end = false;
            if let Some(p) = self.slots[s].as_mut() {
                p.fader.update();
                self.drv.fade_to(p.h, (sq(p.fader.get()) * ext) as f32, 0);
                if p.stopping && p.fader.done() {
                    self.drv.stop(p.h, 0);
                    end = true;
                }
            }
            if end {
                self.slots[s] = None;
            }
        }
    }

    /// render interleaved stereo; the slot faders step at every game frame boundary inside the buffer
    pub fn render(&mut self, out: &mut [f32]) {
        let spf = self.out_rate as f64 / FRAME_HZ;
        let mut pos = 0usize;
        let n = out.len() / 2;
        while pos < n {
            if self.frame_acc <= 0.0 {
                self.tick_frame();
                self.frame_acc += spf;
            }
            let take = (self.frame_acc.ceil() as usize).max(1).min(n - pos);
            self.drv.render(&mut out[pos * 2..(pos + take) * 2]);
            self.frame_acc -= take as f64;
            pos += take;
        }
    }
}

/// Render a scene's sound offline: `events` = (game frame, event) from the scene simulation, frames counted from `first`
pub fn render_events(audio: &mut AudioDirector, events: &[(i64, AudioEvent)], first: i64, frames: i64, slot: u32) -> Vec<f32> {
    let spf = audio.out_rate as f64 / FRAME_HZ;
    let mut out = vec![0f32; ((frames as f64 * spf) as usize + 2) * 2];
    let mut pos = 0usize;
    let mut ei = 0;
    for k in 0..frames {
        while ei < events.len() && events[ei].0 <= first + k {
            match &events[ei].1 {
                AudioEvent::Sound(e) => audio.apply(e, slot, false),
                AudioEvent::Voice(c) => audio.play_voice(*c),
            }
            ei += 1;
        }
        let end = ((k + 1) as f64 * spf) as usize;
        if end > pos {
            audio.render(&mut out[pos * 2..end * 2]);
            pos = end;
        }
    }
    out.truncate(pos * 2);
    out
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    Sound(Effect),
    Voice(u32),
}

// ------------------------------------------------------------------ real-time output
pub struct AudioOutput {
    _stream: cpal::Stream,
    pub director: Arc<Mutex<AudioDirector>>,
    pub sample_rate: u32,
}

impl AudioOutput {
    /// open the default output device and start playing `director`'s mix
    pub fn start(content: &Path) -> Result<AudioOutput, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let dev = host.default_output_device().ok_or("no audio output device")?;
        let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
        let rate = cfg.sample_rate().0;
        let channels = cfg.channels() as usize;
        let director = Arc::new(Mutex::new(AudioDirector::open(content, rate)?));
        let d2 = director.clone();
        let mut scratch: Vec<f32> = Vec::new();
        let stream = dev
            .build_output_stream(
                &cfg.into(),
                move |data: &mut [f32], _| {
                    let frames = data.len() / channels;
                    scratch.clear();
                    scratch.resize(frames * 2, 0.0);
                    if let Ok(mut d) = d2.try_lock() {
                        d.render(&mut scratch);
                    }
                    for (i, f) in data.chunks_mut(channels).enumerate() {
                        f[0] = scratch[2 * i].clamp(-1.0, 1.0);
                        if channels > 1 {
                            f[1] = scratch[2 * i + 1].clamp(-1.0, 1.0);
                        }
                        for x in f.iter_mut().skip(2) {
                            *x = 0.0;
                        }
                    }
                },
                |e| eprintln!("audio stream error: {e}"),
                None,
            )
            .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(AudioOutput { _stream: stream, director, sample_rate: rate })
    }
}
