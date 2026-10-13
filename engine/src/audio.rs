//! Game audio: turns host effects (BGM, SE, ambient, voice, stops) into driver calls, plays them in real time or renders them to a buffer.
//!
//! Sound requests of the script commands (decoded from the game's GameSnd_* code):
//!   bgm id [fade]            sequence `id` of the sound archive on the BGM slot (1000/1001 = per-heroine aliases, see bgm_tables.json); replays of the
//!                            running id are ignored; `fade` = fade-in in 1/60 s
//!   se arc idx               sub-sequence `idx` of sequence archive `arc` (one-shot; the SE player allows several at once)
//!   ambient arc idx fade     looping sub-sequence on the ambient slot (a new one replaces the old)
//!   stop_seq / stop_ambient  stop the BGM / ambient slot with a fade
//!   voice clip               G.722.1C clip from Siren14Voice (decoded on demand)

use crate::host::{Effect, Step};
use crate::sdat::Sdat;
use crate::siren::{decode_clip, Tables};
use crate::snd::{Driver, SeqHandle, FRAME_SECONDS};
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct BgmTables {
    pub by_slot: Vec<u8>,
    pub by_mode: Vec<u8>,
    pub stage: Vec<u8>,
}

pub struct AudioDirector {
    drv: Driver,
    sdat: Arc<Sdat>,
    tables: Tables,
    vtb: Vec<u8>,
    spp: Vec<u8>,
    bgm_tables: BgmTables,
    bgm: Option<(u32, SeqHandle)>,
    ambient: Option<SeqHandle>,
    voice: Option<u64>,
    voice_cache: HashMap<u32, Arc<Vec<i16>>>,
    /// loudness of voice clips relative to music (the game mixes them on different paths)
    pub voice_gain: f32,
}

impl AudioDirector {
    pub fn open(content: &Path, out_rate: u32) -> Result<AudioDirector, String> {
        let rd = |p: &str| std::fs::read(content.join(p)).map_err(|e| format!("{p}: {e}"));
        let sdat = Arc::new(Sdat::parse(rd("assets/sys/sound_data.sdat")?)?);
        let tables = Tables::from_pack(&rd("engine/g7221.pack")?)?;
        let bt: serde_json::Value = serde_json::from_slice(&rd("engine/bgm_tables.json")?).map_err(|e| e.to_string())?;
        let arr = |k: &str| bt[k].as_array().map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as u8).collect()).unwrap_or_default();
        let mut drv = Driver::new(sdat.clone(), out_rate);
        drv.master = 2.0;
        Ok(AudioDirector {
            drv,
            sdat,
            tables,
            vtb: rd("assets/sys/Siren14Voice.vtb")?,
            spp: rd("assets/sys/Siren14Voice.spp")?,
            bgm_tables: BgmTables { by_slot: arr("by_slot"), by_mode: arr("by_mode"), stage: arr("stage") },
            bgm: None,
            ambient: None,
            voice: None,
            voice_cache: HashMap::new(),
            voice_gain: 1.0,
        })
    }

    pub fn driver(&mut self) -> &mut Driver {
        &mut self.drv
    }

    fn game_frames_to_driver(frames: u32) -> u32 {
        (frames as f64 / 60.0 / FRAME_SECONDS) as u32
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

    pub fn play_bgm(&mut self, id: u32, fade: Option<u32>, slot: u32, mode: bool) {
        let id = self.resolve_bgm(id, slot, mode);
        if let Some((cur, h)) = self.bgm {
            if cur == id && self.drv.is_running(h) {
                return; // same sequence already playing: ignored by the game
            }
            self.drv.stop(h, 0);
        }
        self.bgm = None;
        if let Some(sd) = self.sdat.sequence(id as usize) {
            let fade_in = fade.map_or(0, Self::game_frames_to_driver);
            if let Some(h) = self.drv.start(&sd, sd.info.player as usize, fade_in) {
                self.bgm = Some((id, h));
            }
        }
    }

    pub fn stop_bgm(&mut self, fade_frames: u32) {
        if let Some((_, h)) = self.bgm.take() {
            self.drv.stop(h, fade_frames);
        }
    }

    pub fn play_ambient(&mut self, arc: u32, idx: u32, fade: u32) {
        if let Some(h) = self.ambient.take() {
            self.drv.stop(h, 0);
        }
        if let Some(sd) = self.sdat.archive_sequence(arc as usize, idx as usize) {
            self.ambient = self.drv.start(&sd, sd.info.player as usize, Self::game_frames_to_driver(fade));
        }
    }

    pub fn stop_ambient(&mut self, fade_frames: u32) {
        if let Some(h) = self.ambient.take() {
            self.drv.stop(h, fade_frames);
        }
    }

    pub fn play_se(&mut self, arc: u32, idx: u32) {
        if let Some(sd) = self.sdat.archive_sequence(arc as usize, idx as usize) {
            self.drv.start(&sd, sd.info.player as usize, 0);
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

    pub fn play_voice(&mut self, clip: u32) {
        if let Some(id) = self.voice.take() {
            self.drv.stop_clip(id);
        }
        if let Some(pcm) = self.voice_clip(clip) {
            self.voice = Some(self.drv.play_clip(pcm, 32000, self.voice_gain));
        }
    }

    pub fn voice_playing(&self) -> bool {
        self.voice.map_or(false, |id| self.drv.clip_playing(id))
    }

    /// apply one host effect (`slot` = running heroine slot, `mode` = the game-flow mode flag used by the 1001 alias)
    pub fn apply(&mut self, e: &Effect, slot: u32, mode: bool) {
        match e {
            Effect::Bgm { id, fade } => self.play_bgm(*id, *fade, slot, mode),
            Effect::StopSeq { frames } => self.stop_bgm(*frames),
            Effect::Ambient { player, seq, volume } => self.play_ambient(*player, *seq, *volume),
            Effect::StopAmbient { frames } => self.stop_ambient(*frames),
            Effect::Se { player, id, .. } => self.play_se(*player, *id),
            Effect::Voice { clip, .. } if *clip >= 0 => self.play_voice(*clip as u32),
            _ => {}
        }
    }

    pub fn render(&mut self, out: &mut [f32]) {
        self.drv.render(out);
    }
}

/// one script tick in seconds (3.8 display frames at 60 Hz)
pub const TICK_SECONDS: f64 = 3.8 / 60.0;

/// an effect at a wall-clock time (seconds from the start of the scene)
#[derive(Clone, Debug)]
pub struct Timed {
    pub sec: f64,
    pub effect: Effect,
}

/// Turn the host's script-time effects into a playable timeline.  The script itself does not say how long a line is on screen (the player taps
/// through), so dialogue is paced here: a line starts when the previous one is done, lasts as long as its voice clip (plus a short beat) or an
/// estimated reading time, and its voice starts with it.  Every other effect keeps its script time, shifted by the accumulated pacing delay.
pub fn pace(audio: &mut AudioDirector, steps: &[Step]) -> Vec<Timed> {
    pace_with(steps, &mut |clip| audio.voice_clip(clip).map_or(0.0, |p| p.len() as f64 / 32000.0))
}

/// `pace` with an explicit voice-length source (seconds per clip), for callers without an audio device
pub fn pace_with(steps: &[Step], voice_len: &mut dyn FnMut(u32) -> f64) -> Vec<Timed> {
    let mut out = Vec::new();
    let (mut shift, mut dialogue_end) = (0.0f64, 0.0f64);
    let mut pending_voice: Option<(Effect, f64)> = None;
    for s in steps {
        let base = s.t * TICK_SECONDS + shift;
        match &s.effect {
            Effect::Voice { clip, .. } if *clip >= 0 => {
                let len = voice_len(*clip as u32);
                pending_voice = Some((s.effect.clone(), len));
            }
            Effect::Dialogue { text, .. } => {
                let start = base.max(dialogue_end);
                shift += start - base;
                let (read, vlen) = (0.5 + 0.07 * text.chars().count() as f64, pending_voice.as_ref().map_or(0.0, |v| v.1));
                if let Some((v, _)) = pending_voice.take() {
                    out.push(Timed { sec: start, effect: v });
                }
                out.push(Timed { sec: start, effect: s.effect.clone() });
                dialogue_end = start + if vlen > 0.0 { vlen + 0.4 } else { read };
            }
            e => out.push(Timed { sec: base, effect: e.clone() }),
        }
    }
    if let Some((v, _)) = pending_voice {
        out.push(Timed { sec: dialogue_end, effect: v });
    }
    out.sort_by(|a, b| a.sec.partial_cmp(&b.sec).unwrap());
    out
}

/// Render a paced timeline offline (interleaved stereo f32).
pub fn render_timeline(audio: &mut AudioDirector, timeline: &[Timed], slot: u32, out_rate: u32, tail_seconds: f64) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let mut now = 0usize;
    for t in timeline {
        let target = (t.sec * out_rate as f64) as usize;
        if target > now {
            let mut chunk = vec![0f32; (target - now) * 2];
            audio.render(&mut chunk);
            out.extend(chunk);
            now = target;
        }
        audio.apply(&t.effect, slot, false);
    }
    let mut tail = vec![0f32; (tail_seconds * out_rate as f64) as usize * 2];
    audio.render(&mut tail);
    out.extend(tail);
    out
}

/// Render a transcript's audio offline: effects are applied at their script time; returns interleaved stereo f32 and the duration in seconds.
pub fn render_transcript(audio: &mut AudioDirector, steps: &[Step], slot: u32, out_rate: u32, tail_seconds: f64) -> Vec<f32> {
    let mut out: Vec<f32> = Vec::new();
    let mut now_samples = 0usize;
    for s in steps {
        let target = (s.t * TICK_SECONDS * out_rate as f64) as usize;
        if target > now_samples {
            let mut chunk = vec![0f32; (target - now_samples) * 2];
            audio.render(&mut chunk);
            out.extend(chunk);
            now_samples = target;
        }
        audio.apply(&s.effect, slot, false);
    }
    let mut tail = vec![0f32; (tail_seconds * out_rate as f64) as usize * 2];
    audio.render(&mut tail);
    out.extend(tail);
    out
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
