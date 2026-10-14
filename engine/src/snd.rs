//! Sound driver: sequence player, tracks, channels (envelope, LFO, portamento) and mixer, following the behaviour of the game's own
//! NITRO-Composer sound driver (the ARM7 code: SND_SeqMain / PlayerSeqMain / SND_ExChannelMain, decompiled for reference).
//!
//! One driver "frame" is 5.2095 ms (191.96 Hz).  Per frame: every running sequence advances `tempo/240` ticks (commands execute on ticks),
//! then every channel updates its envelope / LFO / portamento and derives volume, pan and pitch.  Between frames the 16 channels mix
//! their waves (PCM / PSG pulse / noise) at the device sample rate.  Levels are in centibels with the squared volume law.

use crate::sdat::{Bank, PlayerInfo, Region, SeqData, Wave};
use std::sync::{Arc, OnceLock};

/// seconds per driver frame: 2728 ticks of the 523 656 Hz system timer
pub const FRAME_SECONDS: f64 = 2728.0 * 64.0 / 33_513_982.0;
const HW_CLOCK: f64 = 16_756_991.0;
const CHAN_ORDER: [usize; 16] = [4, 5, 6, 7, 2, 0, 3, 1, 8, 9, 10, 11, 14, 12, 15, 13];
const ATTACK_FAST: [u8; 19] = [0x00, 0x01, 0x05, 0x0e, 0x1a, 0x26, 0x33, 0x3f, 0x49, 0x54, 0x5c, 0x64, 0x6d, 0x74, 0x7b, 0x7f, 0x84, 0x89, 0x8f];
const MIN_AMP: i32 = -92544; // -723 << 7

fn sq_db_table() -> &'static [i32; 128] {
    static T: OnceLock<[i32; 128]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0i32; 128];
        t[0] = -32768;
        for v in 1..128 {
            t[v] = ((400.0 * (v as f64 / 127.0).log10()).round() as i32).max(-722);
        }
        t
    })
}

/// squared-scale value (0..127) -> centibels
pub fn sq_db(v: u8) -> i32 {
    sq_db_table()[(v & 0x7f) as usize]
}

fn sin_idx(i: usize) -> i32 {
    // 128-step sine, amplitude 127 (quarter-wave table mirrored)
    let q = |k: usize| (127.0 * (k as f64 * std::f64::consts::PI / 64.0).sin()).round() as i32;
    if i < 32 {
        q(i)
    } else if i < 64 {
        q(64 - i)
    } else if i < 96 {
        -q(i - 64)
    } else {
        -q(128 - i)
    }
}

fn calc_release(v: i32) -> u16 {
    if v == 0x7f {
        0xffff
    } else if v == 0x7e {
        0x3c00
    } else if v < 0x32 {
        (v * 2 + 1) as u16
    } else {
        (0x1e00 / (0x7e - v)) as u16
    }
}

fn attack_rate(v: i32) -> u8 {
    if v < 0x6d { (255 - v) as u8 } else { ATTACK_FAST[(0x7f - v) as usize] }
}

#[derive(Clone, Copy, Default)]
struct Lfo {
    kind: u8,
    speed: u8,
    depth: u8,
    range: u8,
    delay: u16,
    counter: u16,
    phase: u16,
}

impl Lfo {
    fn init() -> Lfo {
        Lfo { kind: 0, speed: 0x10, depth: 0, range: 1, delay: 0, counter: 0, phase: 0 }
    }
    fn value(&self) -> i32 {
        if self.depth == 0 || self.counter < self.delay {
            0
        } else {
            self.range as i32 * self.depth as i32 * sin_idx((self.phase >> 8) as usize & 127)
        }
    }
    fn update(&mut self) {
        if self.delay <= self.counter {
            let mut i = (self.phase as u32 + self.speed as u32 * 0x40) >> 8;
            while i > 0x7f {
                i -= 0x80;
            }
            let frac = ((self.phase as u32 + ((self.speed as u32) << 6)) & 0xff) as u16;
            self.phase = frac | ((i as u16) << 8);
        } else {
            self.counter += 1;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Pcm,
    Psg,
    Noise,
}

#[derive(Clone)]
struct Chan {
    active: bool,
    kind: Kind,
    env: u8, // 0 attack, 1 decay, 2 sustain, 3 release
    sweep_by_frame: bool,
    prio: u8,
    owner: Option<(usize, usize)>, // (sequence, track)
    key: u8,
    orig_key: u8,
    vel: u8,
    pan_inst: i32,
    pan_track: i32,
    pan_range: u8,
    pitch_ex: i32,
    vol_sum: i32,
    vol_adj: i32,
    amp: i32,
    sweep_pos: i32,
    sweep_len: i32,
    sweep_pitch: i32,
    attack: u8,
    decay: u16,
    sustain: u8,
    release: u16,
    lfo: Lfo,
    note_len: i32,
    wave: Option<Arc<Wave>>,
    duty: u8,
    base_timer: f64,
    // outputs of the last frame
    gain: f32,
    pan: u8,
    step: f64, // wave samples (or PSG steps) per second
    // playback
    pos: f64,
    lfsr: u16,
    /// gain applied by the owning sequence's fade
    seq_gain: f32,
}

impl Chan {
    fn free() -> Chan {
        Chan {
            active: false,
            kind: Kind::Pcm,
            env: 0,
            sweep_by_frame: true,
            prio: 0,
            owner: None,
            key: 60,
            orig_key: 60,
            vel: 127,
            pan_inst: 0,
            pan_track: 0,
            pan_range: 127,
            pitch_ex: 0,
            vol_sum: 0,
            vol_adj: 0,
            amp: MIN_AMP,
            sweep_pos: 0,
            sweep_len: 0,
            sweep_pitch: 0,
            attack: 0,
            decay: 0,
            sustain: 0x7f,
            release: 0,
            lfo: Lfo::init(),
            note_len: 0,
            wave: None,
            duty: 0,
            base_timer: 0.0,
            gain: 0.0,
            pan: 64,
            step: 0.0,
            pos: 0.0,
            lfsr: 0x7fff,
            seq_gain: 1.0,
        }
    }
}

#[derive(Clone)]
struct Track {
    pc: Option<usize>,
    notewait: bool,
    tie: bool,
    porta: bool,
    cond: bool,
    mute: bool,
    wait_release: bool,
    prg: u16,
    vol: u8,
    expr: u8,
    bend: i8,
    bend_range: u8,
    pan: i8,
    pan_range: u8,
    pan_adj: i8,
    prio: u8,
    transpose: i8,
    last_key: u8,
    porta_time: u8,
    sweep: i16,
    lfo: Lfo,
    env: [u8; 4],
    wait: i32,
    stack: [usize; 3],
    counts: [u8; 3],
    depth: usize,
    chans: Vec<usize>, // most recent first
    /// set by the ARM9 (TRACK_PARAM / ALLOCATABLE_CHANNEL): channels this track may take, volume and pitch offsets
    ch_mask: u16,
    ext_fader: i16,
    ext_pitch: i16,
}

impl Track {
    fn new() -> Track {
        Track {
            pc: None,
            notewait: true,
            tie: false,
            porta: false,
            cond: true,
            mute: false,
            wait_release: false,
            prg: 0,
            vol: 0x7f,
            expr: 0x7f,
            bend: 0,
            bend_range: 2,
            pan: 0,
            pan_range: 0x7f,
            pan_adj: 0,
            prio: 0x40,
            transpose: 0,
            last_key: 0x3c,
            porta_time: 0,
            sweep: 0,
            lfo: Lfo::init(),
            env: [0xff; 4],
            wait: 0,
            stack: [0; 3],
            counts: [0; 3],
            depth: 0,
            chans: Vec::new(),
            ch_mask: 0xffff,
            ext_fader: 0,
            ext_pitch: 0,
        }
    }
}

struct Seq {
    ticks: u64,
    data: Arc<Vec<u8>>,
    bank: Arc<Bank>,
    sdat_arcs: [Option<usize>; 4],
    tracks: Vec<Option<Track>>,
    vars: [i16; 16],
    tempo: u16,
    ratio: u16,
    counter: u16,
    volume: u8,   // sequence master volume (command 0xc2)
    prio: u8,     // player priority
    nns_vol: i32, // centibel offset from the sequence's configured volume
    allowed: u16,
    paused: bool,
    /// prepared (PREPARE_SEQ) and not started yet
    prepared: bool,
    nns_player: usize,
    serial: u64,
    // fade (game-level): gain ramps from `gain` to `target` over `frames_left` driver frames; `stop_at_end` finishes the sequence
    gain: f32,
    target: f32,
    step: f32,
    stop_at_end: bool,
}

/// where banks, waves and player settings come from (an SDAT, or a mock in tests)
pub trait Source: Send + Sync {
    fn bank(&self, id: usize) -> Option<Arc<Bank>>;
    /// wave archive ids of a bank's four slots
    fn bank_wavearcs(&self, id: usize) -> [Option<usize>; 4];
    fn wave(&self, arc: usize, n: usize) -> Option<Arc<Wave>>;
    fn player(&self, n: usize) -> Option<PlayerInfo>;
}

impl Source for crate::sdat::Sdat {
    fn bank(&self, id: usize) -> Option<Arc<Bank>> {
        crate::sdat::Sdat::bank(self, id)
    }
    fn bank_wavearcs(&self, id: usize) -> [Option<usize>; 4] {
        let mut o = [None; 4];
        for (k, slot) in o.iter_mut().enumerate() {
            *slot = self.bank_wavearc(id, k);
        }
        o
    }
    fn wave(&self, arc: usize, n: usize) -> Option<Arc<Wave>> {
        crate::sdat::Sdat::wave(self, arc, n)
    }
    fn player(&self, n: usize) -> Option<PlayerInfo> {
        self.players.get(n).and_then(|p| *p)
    }
}

/// a one-shot voice clip (decoded PCM)
struct Clip {
    pcm: Arc<Vec<i16>>,
    rate: f64,
    pos: f64,
    /// left / right gains
    gain: f32,
    gain_r: f32,
    id: u64,
}

/// handles to running sequences (index + serial so stale handles are harmless)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SeqHandle {
    slot: usize,
    serial: u64,
}

/// one executed note command (for tests and tools that compare against the reference interpreter)
#[derive(Clone, Debug, PartialEq)]
pub struct NoteLog {
    pub tick: u64,
    pub track: usize,
    pub key: u8,
    pub vel: u8,
    pub prg: u16,
    pub len: i32,
}

pub struct Driver {
    src: Arc<dyn Source>,
    pub note_log: Option<Vec<NoteLog>>,
    chans: Vec<Chan>,
    seqs: Vec<Option<Seq>>,
    gvars: [i16; 16],
    rand: u32,
    serial: u64,
    acc: f64, // seconds accumulated towards the next driver frame
    out_rate: f64,
    clips: Vec<Clip>,
    next_clip: u64,
    pub master: f32,
    /// channels locked by the ARM9 (LOCK_CHANNEL): never allocated to sequences
    pub locked: u16,
}

fn read_be_u24(d: &[u8], p: usize) -> usize {
    (d[p] as usize) | (d[p + 1] as usize) << 8 | (d[p + 2] as usize) << 16
}

impl Driver {
    pub fn new(src: Arc<dyn Source>, out_rate: u32) -> Driver {
        Driver { src, note_log: None, chans: vec![Chan::free(); 16], seqs: (0..16).map(|_| None).collect(), gvars: [-1; 16], rand: 0x1234_5678, serial: 0, acc: 0.0, out_rate: out_rate as f64, clips: Vec::new(), next_clip: 1, master: 1.0, locked: 0 }
    }

    fn calc_random(&mut self) -> u16 {
        self.rand = self.rand.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.rand >> 16) as u16
    }

    // ------------------------------------------------------------------ starting / stopping
    /// Start a sequence on `nns_player` (an SDAT player: max simultaneous sequences and channel mask apply).  `fade_in` in driver frames.
    pub fn start(&mut self, sd: &SeqData, nns_player: usize, fade_in: u32) -> Option<SeqHandle> {
        let bank = self.src.bank(sd.info.bank as usize)?;
        let sdat_arcs = self.src.bank_wavearcs(sd.info.bank as usize);
        let (mask, max_seqs) = match self.src.player(nns_player) {
            Some(p) => (p.channel_mask, p.max_seqs.max(1) as usize),
            None => (0xffff, 1),
        };
        // enforce the player's sequence limit (oldest goes first)
        loop {
            let running: Vec<usize> = (0..16).filter(|&i| self.seqs[i].as_ref().map_or(false, |s| s.nns_player == nns_player)).collect();
            if running.len() < max_seqs {
                break;
            }
            let oldest = *running.iter().min_by_key(|&&i| self.seqs[i].as_ref().unwrap().serial).unwrap();
            self.finish_seq(oldest, true);
        }
        let slot = (0..16).find(|&i| self.seqs[i].is_none())?;
        let data = sd.data.clone();
        let mut tracks: Vec<Option<Track>> = (0..16).map(|_| None).collect();
        let mut t0 = Track::new();
        let mut pc = sd.start;
        if data.get(pc) == Some(&0xfe) {
            let mask = (data[pc + 1] as u32 | (data[pc + 2] as u32) << 8) & 0x1ffff;
            pc += 3;
            for i in 1..16 {
                if mask >> i & 1 == 1 {
                    tracks[i] = Some(Track::new());
                }
            }
        }
        t0.pc = Some(pc);
        tracks[0] = Some(t0);
        self.serial += 1;
        let gain0 = if fade_in > 0 { 0.0 } else { 1.0 };
        self.seqs[slot] = Some(Seq {
            ticks: 0,
            data,
            bank,
            sdat_arcs,
            tracks,
            vars: [-1; 16],
            tempo: 120,
            ratio: 0x100,
            counter: 0xf0,
            volume: 0x7f,
            prio: sd.info.player_priority,
            nns_vol: sq_db(((sd.info.volume as u32 * 106) >> 7).min(127) as u8),
            allowed: mask,
            paused: false,
            prepared: false,
            nns_player,
            serial: self.serial,
            gain: gain0,
            target: 1.0,
            step: if fade_in > 0 { 1.0 / fade_in as f32 } else { 0.0 },
            stop_at_end: false,
        });
        Some(SeqHandle { slot, serial: self.serial })
    }

    fn valid(&self, h: SeqHandle) -> bool {
        self.seqs.get(h.slot).and_then(|s| s.as_ref()).map_or(false, |s| s.serial == h.serial)
    }

    pub fn is_running(&self, h: SeqHandle) -> bool {
        self.valid(h)
    }

    /// Stop a sequence; with `fade_frames > 0` its output fades out first (frames are 1/60 s game updates).
    pub fn stop(&mut self, h: SeqHandle, fade_frames: u32) {
        if !self.valid(h) {
            return;
        }
        if fade_frames == 0 {
            self.finish_seq(h.slot, true);
        } else {
            let frames = (fade_frames as f64 / 60.0 / FRAME_SECONDS).max(1.0) as f32;
            let s = self.seqs[h.slot].as_mut().unwrap();
            s.target = 0.0;
            s.step = s.gain.max(0.001) / frames;
            s.stop_at_end = true;
        }
    }

    pub fn fade_to(&mut self, h: SeqHandle, target: f32, game_frames: u32) {
        if let Some(Some(s)) = self.seqs.get_mut(h.slot) {
            if s.serial == h.serial {
                let frames = (game_frames as f64 / 60.0 / FRAME_SECONDS).max(1.0) as f32;
                s.target = target;
                s.step = (target - s.gain).abs() / frames;
            }
        }
    }

    pub fn stop_all(&mut self, fade_frames: u32) {
        for i in 0..16 {
            if let Some(s) = &self.seqs[i] {
                let h = SeqHandle { slot: i, serial: s.serial };
                self.stop(h, fade_frames);
            }
        }
    }

    /// ticks executed by a running sequence
    pub fn seq_ticks(&self, h: SeqHandle) -> Option<u64> {
        if self.valid(h) { self.seqs[h.slot].as_ref().map(|s| s.ticks) } else { None }
    }

    pub fn running(&self) -> usize {
        self.seqs.iter().filter(|s| s.is_some()).count()
    }

    pub fn active_channels(&self) -> usize {
        self.chans.iter().filter(|c| c.active).count()
    }

    /// end a sequence: its channels go into release (so notes ring out) unless `hard` also silences them
    fn finish_seq(&mut self, si: usize, hard: bool) {
        if let Some(seq) = self.seqs[si].take() {
            for t in seq.tracks.into_iter().flatten() {
                for c in t.chans {
                    if hard {
                        self.chans[c].env = 3;
                        self.chans[c].prio = 1;
                    }
                    self.chans[c].owner = None;
                }
            }
        }
    }

    /// play a decoded voice clip (mono PCM); returns an id for `stop_clip`
    /// `gain_l` / `gain_r`: per-side gains (the game's voice stream: 2 channels, `AudioDirector::voice_gains`)
    pub fn play_clip(&mut self, pcm: Arc<Vec<i16>>, rate: u32, gain_l: f32, gain_r: f32) -> u64 {
        let id = self.next_clip;
        self.next_clip += 1;
        self.clips.push(Clip { pcm, rate: rate as f64, pos: 0.0, gain: gain_l, gain_r, id });
        id
    }

    pub fn stop_clip(&mut self, id: u64) {
        self.clips.retain(|c| c.id != id);
    }

    pub fn clip_playing(&self, id: u64) -> bool {
        self.clips.iter().any(|c| c.id == id)
    }

    // ------------------------------------------------------------------ the ARM7 driver's player interface
    // (the SDK sound commands address sequence players 0..15 directly; NNS-level policy is the ARM9's)

    /// PREPARE_SEQ / START_SEQ on player `p`: the sequence data (offsets relative to `data`), starting at `start`,
    /// with the bank `bank` (a `Source` id). A prepared sequence waits for `start_prepared`.
    pub fn start_player(&mut self, p: usize, data: Arc<Vec<u8>>, start: usize, bank: usize, prepared: bool) -> bool {
        if p >= 16 {
            return false;
        }
        self.finish_seq(p, true);
        let Some(bank_data) = self.src.bank(bank) else { return false };
        let sdat_arcs = self.src.bank_wavearcs(bank);
        let mut tracks: Vec<Option<Track>> = (0..16).map(|_| None).collect();
        let mut pc = start;
        if data.get(pc) == Some(&0xfe) {
            let mask = (data[pc + 1] as u32 | (data[pc + 2] as u32) << 8) & 0x1ffff;
            pc += 3;
            for (i, t) in tracks.iter_mut().enumerate().skip(1) {
                if mask >> i & 1 == 1 {
                    *t = Some(Track::new());
                }
            }
        }
        let mut t0 = Track::new();
        t0.pc = Some(pc);
        tracks[0] = Some(t0);
        self.serial += 1;
        self.seqs[p] = Some(Seq {
            ticks: 0,
            data,
            bank: bank_data,
            sdat_arcs,
            tracks,
            vars: [-1; 16],
            tempo: 120,
            ratio: 0x100,
            counter: 0xf0,
            volume: 0x7f,
            prio: 0x40,
            nns_vol: 0,
            allowed: 0xffff,
            paused: false,
            prepared,
            nns_player: p,
            serial: self.serial,
            gain: 1.0,
            target: 1.0,
            step: 0.0,
            stop_at_end: false,
        });
        true
    }
    pub fn start_prepared(&mut self, p: usize) {
        if let Some(Some(s)) = self.seqs.get_mut(p) {
            s.prepared = false;
        }
    }
    /// STOP_SEQ: the player ends, its notes are released
    pub fn stop_player(&mut self, p: usize) {
        if p < 16 {
            self.finish_seq(p, true);
        }
    }
    pub fn pause_player(&mut self, p: usize, flag: bool) {
        if let Some(Some(s)) = self.seqs.get_mut(p) {
            s.paused = flag;
        }
    }
    pub fn player_active(&self, p: usize) -> bool {
        self.seqs.get(p).map_or(false, |s| s.is_some())
    }
    pub fn player_ticks(&self, p: usize) -> u64 {
        self.seqs.get(p).and_then(|s| s.as_ref()).map_or(0, |s| s.ticks)
    }
    pub fn player_vars(&self, p: usize) -> Option<[i16; 16]> {
        self.seqs.get(p).and_then(|s| s.as_ref()).map(|s| s.vars)
    }
    pub fn set_player_var(&mut self, p: usize, n: usize, v: i16) {
        if let Some(Some(s)) = self.seqs.get_mut(p) {
            if n < 16 {
                s.vars[n] = v;
            }
        }
    }
    pub fn global_vars(&self) -> [i16; 16] {
        self.gvars
    }
    pub fn set_global_var(&mut self, n: usize, v: i16) {
        if n < 16 {
            self.gvars[n] = v;
        }
    }
    /// the bank a player uses (its `Source` id is not kept; the bank's data is) and its sequence data
    pub fn player_data(&self, p: usize) -> Option<Arc<Vec<u8>>> {
        self.seqs.get(p).and_then(|s| s.as_ref()).map(|s| s.data.clone())
    }
    /// PLAYER_PARAM: a field of the SDK's SNDPlayer (offset, size in bytes) set by the ARM9
    pub fn set_player_param(&mut self, p: usize, offset: u32, data: u32, size: u32) {
        let Some(Some(s)) = self.seqs.get_mut(p) else { return };
        match (offset, size) {
            (4, _) => s.prio = data as u8,
            (5, _) => s.volume = data as u8,
            (6, _) => s.nns_vol = data as u16 as i16 as i32,
            (0x18, _) => s.tempo = data as u16,
            (0x1a, _) => s.ratio = data as u16,
            _ => {}
        }
    }
    /// TRACK_PARAM: a field of the SDK's SNDTrack for the tracks in `mask`
    pub fn set_track_param(&mut self, p: usize, mask: u32, offset: u32, data: u32, size: u32) {
        let Some(Some(s)) = self.seqs.get_mut(p) else { return };
        for (i, t) in s.tracks.iter_mut().enumerate() {
            let Some(t) = t else { continue };
            if mask >> i & 1 == 0 {
                continue;
            }
            match (offset, size) {
                (4, _) => t.vol = data as u8,
                (5, _) => t.expr = data as u8,
                (6, _) => t.bend = data as i8,
                (7, _) => t.bend_range = data as u8,
                (8, _) => t.pan = data as i8,
                (9, _) => t.pan_adj = data as i8,
                (0xa, _) => t.ext_fader = data as u16 as i16,
                (0xc, _) => t.ext_pitch = data as u16 as i16,
                _ => {}
            }
        }
    }
    /// ALLOCATABLE_CHANNEL: the channels the tracks in `mask` may take
    pub fn set_allocatable(&mut self, p: usize, mask: u32, chans: u16) {
        let Some(Some(s)) = self.seqs.get_mut(p) else { return };
        for (i, t) in s.tracks.iter_mut().enumerate() {
            if let Some(t) = t {
                if mask >> i & 1 == 1 {
                    t.ch_mask = chans;
                }
            }
        }
    }
    /// MUTE_TRACK
    pub fn mute_tracks(&mut self, p: usize, mask: u32, mute: u32) {
        let Some(Some(s)) = self.seqs.get_mut(p) else { return };
        for (i, t) in s.tracks.iter_mut().enumerate() {
            if let Some(t) = t {
                if mask >> i & 1 == 1 {
                    t.mute = mute != 0;
                }
            }
        }
    }
    /// the channels sounding (the shared work's channelStatus)
    pub fn channel_mask(&self) -> u16 {
        self.chans.iter().enumerate().filter(|(_, c)| c.active).fold(0, |m, (i, _)| m | 1 << i)
    }
    /// per player: (track slots in use as in SNDPlayer.tracks, per track (prg, vol, expr, bend, bend range, pan,
    /// transpose, channels)) - for the driver info the ARM9 reads
    pub fn track_info(&self, p: usize, t: usize) -> Option<(u16, u8, u8, i8, u8, i8, i8, Vec<usize>)> {
        let tr = self.seqs.get(p)?.as_ref()?.tracks.get(t)?.as_ref()?;
        Some((tr.prg, tr.vol, tr.expr, tr.bend, tr.bend_range, tr.pan, tr.transpose, tr.chans.clone()))
    }

    // ------------------------------------------------------------------ rendering
    /// Render `frames` stereo frames into `out` (interleaved L R, added to what is there).
    pub fn render(&mut self, out: &mut [f32]) {
        let n = out.len() / 2;
        let dt = 1.0 / self.out_rate;
        for i in 0..n {
            if self.acc <= 0.0 {
                self.driver_frame();
                self.acc += FRAME_SECONDS;
            }
            self.acc -= dt;
            let (mut l, mut r) = (0.0f32, 0.0f32);
            for c in self.chans.iter_mut() {
                if !c.active || c.gain <= 0.0 {
                    continue;
                }
                let s = match c.kind {
                    Kind::Pcm => {
                        let w = match &c.wave {
                            Some(w) => w,
                            None => continue,
                        };
                        let len = w.pcm.len();
                        if len == 0 {
                            c.active = false;
                            continue;
                        }
                        let i0 = c.pos as usize;
                        if i0 >= len {
                            c.active = false;
                            continue;
                        }
                        let frac = (c.pos - i0 as f64) as f32;
                        let a = w.pcm[i0] as f32;
                        let next = if i0 + 1 < len {
                            w.pcm[i0 + 1]
                        } else if let Some((ls, _)) = w.looped {
                            w.pcm[ls.min(len - 1)]
                        } else {
                            w.pcm[i0]
                        } as f32;
                        c.pos += c.step * dt;
                        if let Some((ls, le)) = w.looped {
                            let le = le.min(len);
                            if c.pos >= le as f64 && le > ls {
                                c.pos -= (le - ls) as f64;
                            }
                        }
                        (a + (next - a) * frac) / 32768.0
                    }
                    Kind::Psg => {
                        let ph = c.pos % 8.0;
                        c.pos = ph + c.step * dt;
                        // duty n: high for (n + 1) of 8 steps; 7 is silent
                        if c.duty >= 7 { 0.0 } else if (ph as u8) <= c.duty { 0.5 } else { -0.5 }
                    }
                    Kind::Noise => {
                        c.pos += c.step * dt;
                        while c.pos >= 1.0 {
                            c.pos -= 1.0;
                            let bit = (c.lfsr ^ (c.lfsr >> 1)) & 1;
                            c.lfsr = (c.lfsr >> 1) | (bit << 14);
                        }
                        if c.lfsr & 1 == 1 { -0.5 } else { 0.5 }
                    }
                };
                let g = c.gain * c.seq_gain;
                let (lg, rg) = ((127 - c.pan as i32) as f32 / 127.0, c.pan as f32 / 127.0);
                l += s * g * lg;
                r += s * g * rg;
            }
            for c in self.clips.iter_mut() {
                let i0 = c.pos as usize;
                if i0 >= c.pcm.len() {
                    continue;
                }
                let frac = (c.pos - i0 as f64) as f32;
                let a = c.pcm[i0] as f32;
                let b = c.pcm.get(i0 + 1).copied().unwrap_or(c.pcm[i0]) as f32;
                let s = (a + (b - a) * frac) / 32768.0;
                c.pos += c.rate * dt;
                l += s * c.gain;
                r += s * c.gain_r;
            }
            out[2 * i] += l * self.master;
            out[2 * i + 1] += r * self.master;
        }
        self.clips.retain(|c| (c.pos as usize) < c.pcm.len());
    }

    /// one 5.2 ms driver frame
    fn driver_frame(&mut self) {
        // sequences: ticks + track channel update
        for si in 0..16 {
            if self.seqs[si].is_none() {
                continue;
            }
            // game-level fade
            {
                let s = self.seqs[si].as_mut().unwrap();
                if (s.gain - s.target).abs() > 1e-6 {
                    if s.gain < s.target {
                        s.gain = (s.gain + s.step).min(s.target);
                    } else {
                        s.gain = (s.gain - s.step).max(s.target);
                    }
                }
                if s.stop_at_end && s.gain <= 1e-5 {
                    self.finish_seq(si, true);
                    continue;
                }
            }
            let (paused, mut ticks) = {
                let s = self.seqs[si].as_mut().unwrap();
                let mut t = 0;
                if !s.paused && !s.prepared {
                    while s.counter >= 0xf0 {
                        t += 1;
                        s.counter -= 0xf0;
                    }
                }
                (s.paused || s.prepared, t)
            };
            if !paused {
                let mut finished = false;
                while ticks > 0 {
                    ticks -= 1;
                    if self.seq_tick(si) {
                        finished = true;
                        break;
                    }
                }
                if finished {
                    // all tracks ended: release remaining notes
                    self.finish_seq(si, false);
                    for c in self.chans.iter_mut().filter(|c| c.owner.map_or(false, |o| o.0 == si)) {
                        c.owner = None;
                    }
                    continue;
                }
                let s = self.seqs[si].as_mut().unwrap();
                s.counter = s.counter.wrapping_add(((s.tempo as u32 * s.ratio as u32 * 0x100) >> 16) as u16);
            }
            for ti in 0..16 {
                self.update_track_channels(si, ti);
            }
        }
        // channels
        for ci in 0..16 {
            self.exchannel_frame(ci);
        }
        self.calc_random();
    }

    // ------------------------------------------------------------------ track channel update (UpdateTrackChannel)
    fn update_track_channels(&mut self, si: usize, ti: usize) {
        let (vol_sum, vol_adj, pitch_ex, pan_v, chans, lfo, pan_range, seq_gain) = {
            let s = match self.seqs[si].as_ref() { Some(s) => s, None => return };
            let t = match s.tracks[ti].as_ref() { Some(t) => t, None => return };
            if t.chans.is_empty() {
                return;
            }
            let mut pan = t.pan as i32;
            if t.pan_range != 0x7f {
                pan = (pan * t.pan_range as i32 + 0x40) >> 7;
            }
            let vol_sum = (sq_db(s.volume) + sq_db(t.vol) + sq_db(t.expr) + t.ext_fader as i32).max(-0x8000);
            let pan_v = (pan + t.pan_adj as i32).clamp(-0x80, 0x7f);
            let pitch_ex = ((t.bend as i32 * t.bend_range as i32 * 0x40) >> 7) as i32 + t.ext_pitch as i32;
            (vol_sum, s.nns_vol, pitch_ex, pan_v, t.chans.clone(), t.lfo, t.pan_range, s.gain)
        };
        for ci in chans {
            let ch = &mut self.chans[ci];
            ch.vol_adj = vol_adj;
            if ch.env != 3 {
                ch.vol_sum = vol_sum;
                ch.pitch_ex = pitch_ex;
                ch.pan_track = pan_v;
                ch.pan_range = pan_range;
                ch.lfo.kind = lfo.kind;
                ch.lfo.speed = lfo.speed;
                ch.lfo.depth = lfo.depth;
                ch.lfo.range = lfo.range;
                ch.lfo.delay = lfo.delay;
                if ch.note_len == 0 {
                    ch.prio = 1;
                    ch.env = 3;
                }
            }
            ch.seq_gain = seq_gain;
        }
    }

    // ------------------------------------------------------------------ channels (SND_ExChannelMain)
    fn exchannel_frame(&mut self, ci: usize) {
        let ch = &mut self.chans[ci];
        if !ch.active {
            return;
        }
        // envelope
        match ch.env {
            0 => {
                ch.amp = -(((-(ch.amp as i64) * ch.attack as i64) >> 8) as i32);
                if ch.amp == 0 {
                    ch.env = 1;
                }
            }
            1 => {
                let target = sq_db(ch.sustain) * 0x80;
                ch.amp -= ch.decay as i32;
                if ch.amp <= target {
                    ch.amp = target;
                    ch.env = 2;
                }
            }
            3 => ch.amp -= ch.release as i32,
            _ => {}
        }
        let env_cb = ch.amp >> 7;
        // portamento
        let mut sweep = 0i32;
        if ch.sweep_pitch != 0 && ch.sweep_pos < ch.sweep_len {
            sweep = ((ch.sweep_pitch as i64 * (ch.sweep_len - ch.sweep_pos) as i64) / ch.sweep_len as i64) as i32;
            if ch.sweep_by_frame {
                ch.sweep_pos += 1;
            }
        }
        let mut vol = sq_db(ch.vel) + env_cb + ch.vol_sum + ch.vol_adj;
        let mut pitch = (ch.key as i32 - ch.orig_key as i32) * 0x40 + sweep + ch.pitch_ex;
        // LFO
        let lv = ch.lfo.value() as i64;
        let mut lfo_pan = 0i32;
        if lv != 0 {
            let scaled = match ch.lfo.kind {
                1 => lv * 60,
                _ => lv * 64,
            } >> 14;
            match ch.lfo.kind {
                0 => pitch += scaled as i32,
                1 => {
                    if vol > -0x8000 {
                        vol += scaled as i32;
                    }
                }
                _ => lfo_pan = scaled as i32,
            }
        }
        ch.lfo.update();
        let mut pan = lfo_pan + ch.pan_inst;
        if ch.pan_range != 0x7f {
            pan = (pan * ch.pan_range as i32 + 0x40) >> 7;
        }
        let pan = (pan + ch.pan_track + 0x40).clamp(0, 0x7f);
        if ch.env == 3 && vol <= -723 {
            ch.active = false;
            ch.prio = 0;
            return;
        }
        let vol = vol.clamp(-723, 0);
        ch.gain = (10f64.powf(vol as f64 / 200.0).min(1.0) * 127.0 / 128.0) as f32;
        ch.pan = pan as u8;
        let pitch_mul = 2f64.powf(pitch as f64 / 768.0);
        ch.step = match ch.kind {
            Kind::Pcm => ch.wave.as_ref().map_or(0.0, |w| w.rate as f64) * pitch_mul,
            _ => HW_CLOCK / ch.base_timer * pitch_mul,
        };
    }

    // ------------------------------------------------------------------ allocation (SND_AllocExChannel)
    fn alloc_channel(&mut self, type_mask: u16, allowed: u16, prio: i32) -> Option<usize> {
        let mask = type_mask & allowed & !self.locked;
        let mut best: Option<usize> = None;
        for &ci in CHAN_ORDER.iter() {
            if mask >> ci & 1 == 0 {
                continue;
            }
            best = Some(match best {
                None => ci,
                Some(b) => {
                    let (cb, cc) = (&self.chans[b], &self.chans[ci]);
                    let amp = |c: &Chan| (c.gain * 4096.0) as i32;
                    if cc.prio < cb.prio || (cc.prio == cb.prio && amp(cb) > amp(cc)) {
                        ci
                    } else {
                        b
                    }
                }
            });
        }
        let v = best?;
        if prio < self.chans[v].prio as i32 {
            return None;
        }
        if let Some((si, ti)) = self.chans[v].owner {
            if let Some(Some(t)) = self.seqs.get_mut(si).map(|s| s.as_mut().and_then(|s| s.tracks[ti].as_mut())) {
                t.chans.retain(|&c| c != v);
            }
        }
        let mut c = Chan::free();
        c.active = true;
        c.prio = prio.clamp(0, 255) as u8;
        c.decay = 0xffff;
        c.release = 0xffff;
        self.chans[v] = c;
        Some(v)
    }

    fn region_setup(&self, ch: &mut Chan, bank_arcs: &[Option<usize>; 4], r: &Region, key: u8, vel: u8, len: i32) -> bool {
        let (rel, len) = if r.release == 0xff { (0u8, -1) } else { (r.release, len) };
        match r.kind {
            1 | 4 => {
                let arc = match bank_arcs.get(r.wavearc as usize).copied().flatten() { Some(a) => a, None => return false };
                let w = match self.src.wave(arc, r.wave as usize) { Some(w) => w, None => return false };
                ch.kind = Kind::Pcm;
                ch.wave = Some(w);
            }
            2 => {
                ch.kind = Kind::Psg;
                ch.duty = (r.wave & 7) as u8;
                ch.base_timer = 0x1f46 as f64;
            }
            3 => {
                ch.kind = Kind::Noise;
                ch.base_timer = 0x1f46 as f64;
            }
            _ => return false,
        }
        ch.key = key;
        ch.orig_key = r.original_key;
        ch.vel = vel;
        ch.note_len = len;
        ch.attack = attack_rate(r.attack as i32);
        ch.decay = calc_release(r.decay as i32);
        ch.sustain = r.sustain;
        ch.release = calc_release(rel as i32);
        ch.pan_inst = r.pan as i32 - 0x40;
        ch.amp = MIN_AMP;
        ch.env = 0;
        true
    }

    // ------------------------------------------------------------------ sequence ticks (PlayerSeqMain)
    /// run one tick of every track; returns true when no track is left running
    fn seq_tick(&mut self, si: usize) -> bool {
        let mut any_running = false;
        self.seqs[si].as_mut().unwrap().ticks += 1;
        for ti in 0..16 {
            let alive = self.seqs[si].as_ref().map_or(false, |s| s.tracks[ti].as_ref().map_or(false, |t| t.pc.is_some()));
            if !alive {
                continue;
            }
            // note length and tick-based sweep counters
            let chans = self.seqs[si].as_ref().unwrap().tracks[ti].as_ref().unwrap().chans.clone();
            for &ci in &chans {
                let c = &mut self.chans[ci];
                if c.note_len > 0 {
                    c.note_len -= 1;
                }
                if !c.sweep_by_frame && c.sweep_pos < c.sweep_len {
                    c.sweep_pos += 1;
                }
            }
            let mut finished_track = false;
            {
                let t = self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap();
                if t.wait_release {
                    if t.chans.is_empty() {
                        t.wait_release = false;
                    } else {
                        any_running = true;
                        continue;
                    }
                }
                if t.wait > 0 {
                    t.wait -= 1;
                }
                if t.wait >= 1 {
                    any_running = true;
                    continue;
                }
            }
            // execute commands until the track waits
            loop {
                let (waiting, wr) = {
                    let t = self.seqs[si].as_ref().unwrap().tracks[ti].as_ref().unwrap();
                    (t.wait != 0, t.wait_release)
                };
                if waiting || wr {
                    break;
                }
                if self.exec_command(si, ti) {
                    finished_track = true;
                    break;
                }
            }
            if finished_track {
                self.close_track(si, ti);
            } else {
                any_running = true;
            }
        }
        !any_running
    }

    fn close_track(&mut self, si: usize, ti: usize) {
        let chans = {
            let t = self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap();
            t.pc = None;
            std::mem::take(&mut t.chans)
        };
        for ci in chans {
            let c = &mut self.chans[ci];
            c.env = 3;
            c.prio = 1;
            c.owner = None;
        }
    }

    fn read_arg(&mut self, si: usize, ti: usize, kind: u8) -> i32 {
        let data = self.seqs[si].as_ref().unwrap().data.clone();
        let t = self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap();
        let rb = |pc: &mut usize| -> u32 {
            let v = data.get(*pc).copied().unwrap_or(0xff);
            *pc += 1;
            v as u32
        };
        let mut pc = t.pc.unwrap();
        let v = match kind {
            0 => rb(&mut pc) as i32,
            1 => (rb(&mut pc) | rb(&mut pc) << 8) as i32,
            2 => {
                let mut v = 0u32;
                loop {
                    let b = rb(&mut pc);
                    v = (b & 0x7f) | (v << 7);
                    if b & 0x80 == 0 {
                        break;
                    }
                }
                v as i32
            }
            3 => {
                let lo = (rb(&mut pc) | rb(&mut pc) << 8) as u16 as i16 as i32;
                let hi = (rb(&mut pc) | rb(&mut pc) << 8) as u16 as i16 as i32;
                t.pc = Some(pc);
                let r = self.calc_random() as i32;
                return (r * ((hi - lo) + 1) >> 16) + lo;
            }
            _ => {
                let idx = rb(&mut pc) as usize;
                t.pc = Some(pc);
                return self.var(si, idx);
            }
        };
        t.pc = Some(pc);
        v
    }

    fn var(&self, si: usize, idx: usize) -> i32 {
        if idx < 16 { self.seqs[si].as_ref().unwrap().vars[idx] as i32 } else { self.gvars[(idx - 16) & 15] as i32 }
    }

    fn set_var(&mut self, si: usize, idx: usize, v: i16) {
        if idx < 16 {
            self.seqs[si].as_mut().unwrap().vars[idx] = v;
        } else {
            self.gvars[(idx - 16) & 15] = v;
        }
    }

    fn byte(&mut self, si: usize, ti: usize) -> u32 {
        let data = self.seqs[si].as_ref().unwrap().data.clone();
        let t = self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap();
        let pc = t.pc.unwrap();
        t.pc = Some(pc + 1);
        data.get(pc).copied().unwrap_or(0xff) as u32
    }

    /// execute one command of a track; returns true when the track ended
    fn exec_command(&mut self, si: usize, ti: usize) -> bool {
        let mut op = self.byte(si, ti);
        let mut cond = true;
        if op == 0xa2 {
            op = self.byte(si, ti);
            cond = self.seqs[si].as_ref().unwrap().tracks[ti].as_ref().unwrap().cond;
        }
        let mut mode = 0u8; // 0 normal, 3 random, 4 variable
        if op == 0xa0 {
            op = self.byte(si, ti);
            mode = 3;
        }
        if op == 0xa1 {
            op = self.byte(si, ti);
            mode = 4;
        }
        let kind = |default: u8| if mode != 0 { mode } else { default };
        if op & 0x80 == 0 {
            // note: velocity, then length
            let vel = self.byte(si, ti) as u8;
            let len = self.read_arg(si, ti, kind(2));
            let transpose = self.seqs[si].as_ref().unwrap().tracks[ti].as_ref().unwrap().transpose as i32;
            let key = (op as i32 + transpose).clamp(0, 127) as u8;
            if cond {
                if self.note_log.is_some() {
                    let (tick, prg) = (self.seqs[si].as_ref().unwrap().ticks - 1, self.seqs[si].as_ref().unwrap().tracks[ti].as_ref().unwrap().prg);
                    self.note_log.as_mut().unwrap().push(NoteLog { tick, track: ti, key, vel, prg, len });
                }
                self.note_on(si, ti, key, vel, len);
            }
            return false;
        }
        match op {
            0x80 | 0x81 => {
                let v = self.read_arg(si, ti, kind(2));
                if cond {
                    let t = self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap();
                    if op == 0x80 {
                        t.wait = v;
                    } else if v < 0x10000 {
                        t.prg = v as u16;
                    }
                }
            }
            0x93 => {
                let no = self.byte(si, ti) as usize;
                let data = self.seqs[si].as_ref().unwrap().data.clone();
                let pc = self.seqs[si].as_ref().unwrap().tracks[ti].as_ref().unwrap().pc.unwrap();
                let off = read_be_u24(&data, pc);
                self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap().pc = Some(pc + 3);
                if cond && no < 16 && no != ti && self.seqs[si].as_ref().unwrap().tracks[no].is_some() {
                    self.close_track_keep(si, no);
                    let t = self.seqs[si].as_mut().unwrap().tracks[no].as_mut().unwrap();
                    *t = Track::new();
                    t.pc = Some(off);
                }
            }
            0x94 | 0x95 => {
                let data = self.seqs[si].as_ref().unwrap().data.clone();
                let t = self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap();
                let pc = t.pc.unwrap();
                let off = read_be_u24(&data, pc);
                t.pc = Some(pc + 3);
                if cond {
                    if op == 0x94 {
                        t.pc = Some(off);
                    } else if t.depth < 3 {
                        t.stack[t.depth] = pc + 3;
                        t.depth += 1;
                        t.pc = Some(off);
                    }
                }
            }
            0xb0..=0xbd => {
                let vi = self.byte(si, ti) as usize;
                let x = self.read_arg(si, ti, kind(1)) as u16 as i16 as i32;
                if cond {
                    let cur = self.var(si, vi);
                    match op {
                        0xb0 => self.set_var(si, vi, x as i16),
                        0xb1 => self.set_var(si, vi, (cur + x) as i16),
                        0xb2 => self.set_var(si, vi, (cur - x) as i16),
                        0xb3 => self.set_var(si, vi, (cur * x) as i16),
                        0xb4 => {
                            if x != 0 {
                                self.set_var(si, vi, (cur / x) as i16)
                            }
                        }
                        0xb5 => self.set_var(si, vi, if x < 0 { cur >> (-x).min(31) } else { cur << x.min(31) } as i16),
                        0xb6 => {
                            let neg = x < 0;
                            let m = (x.unsigned_abs() as u16) as u32;
                            let r = self.calc_random() as u32;
                            let v = ((r * (m + 1)) >> 16) as i32;
                            self.set_var(si, vi, if neg { -v } else { v } as i16);
                        }
                        0xb8..=0xbd => {
                            let c = match op {
                                0xb8 => cur == x,
                                0xb9 => x <= cur,
                                0xba => x < cur,
                                0xbb => cur <= x,
                                0xbc => cur < x,
                                _ => cur != x,
                            };
                            self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap().cond = c;
                        }
                        _ => {}
                    }
                }
            }
            0xc0..=0xdf => {
                let v = self.read_arg(si, ti, kind(0));
                if !cond {
                    return false;
                }
                let b = v as u8;
                match op {
                    0xc0 => self.tr(si, ti).pan = b.wrapping_sub(0x40) as i8,
                    0xc1 => self.tr(si, ti).vol = b,
                    0xc2 => self.seqs[si].as_mut().unwrap().volume = b,
                    0xc3 => self.tr(si, ti).transpose = b as i8,
                    0xc4 => self.tr(si, ti).bend = b as i8,
                    0xc5 => self.tr(si, ti).bend_range = b,
                    0xc6 => self.tr(si, ti).prio = b,
                    0xc7 => self.tr(si, ti).notewait = v & 1 != 0,
                    0xc8 => {
                        self.tr(si, ti).tie = v & 1 != 0;
                        // changing tie mode ends the track's sounding notes
                        let chans = std::mem::take(&mut self.tr(si, ti).chans);
                        for ci in chans {
                            let c = &mut self.chans[ci];
                            c.env = 3;
                            c.prio = 1;
                            c.owner = None;
                        }
                    }
                    0xc9 => {
                        let t = self.tr(si, ti);
                        t.last_key = b.wrapping_add(t.transpose as u8);
                        t.porta = true;
                    }
                    0xca => self.tr(si, ti).lfo.depth = b,
                    0xcb => self.tr(si, ti).lfo.speed = b,
                    0xcc => self.tr(si, ti).lfo.kind = b,
                    0xcd => self.tr(si, ti).lfo.range = b,
                    0xce => self.tr(si, ti).porta = v & 1 != 0,
                    0xcf => self.tr(si, ti).porta_time = b,
                    0xd0 => self.tr(si, ti).env[0] = b,
                    0xd1 => self.tr(si, ti).env[1] = b,
                    0xd2 => self.tr(si, ti).env[2] = b,
                    0xd3 => self.tr(si, ti).env[3] = b,
                    0xd4 => {
                        let t = self.tr(si, ti);
                        if t.depth < 3 {
                            let pc = t.pc.unwrap();
                            t.stack[t.depth] = pc;
                            t.counts[t.depth] = b;
                            t.depth += 1;
                        }
                    }
                    0xd5 => self.tr(si, ti).expr = b,
                    0xd7 => self.tr(si, ti).mute = b & 1 != 0,
                    _ => {}
                }
            }
            0xe0 | 0xe1 | 0xe3 => {
                let v = self.read_arg(si, ti, kind(1));
                if cond {
                    match op {
                        0xe0 => self.tr(si, ti).lfo.delay = v as u16,
                        0xe1 => self.seqs[si].as_mut().unwrap().tempo = v as u16,
                        _ => self.tr(si, ti).sweep = v as u16 as i16,
                    }
                }
            }
            0xfc => {
                if cond {
                    let t = self.tr(si, ti);
                    if t.depth != 0 {
                        let d = t.depth - 1;
                        let c = t.counts[d];
                        if c == 0 || c - 1 != 0 {
                            if c != 0 {
                                t.counts[d] = c - 1;
                            }
                            t.pc = Some(t.stack[d]);
                        } else {
                            t.depth -= 1;
                        }
                    }
                }
            }
            0xfd => {
                if cond {
                    let t = self.tr(si, ti);
                    if t.depth != 0 {
                        t.depth -= 1;
                        t.pc = Some(t.stack[t.depth]);
                    }
                }
            }
            0xff => {
                if cond {
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn tr(&mut self, si: usize, ti: usize) -> &mut Track {
        self.seqs[si].as_mut().unwrap().tracks[ti].as_mut().unwrap()
    }

    fn close_track_keep(&mut self, si: usize, ti: usize) {
        let chans = std::mem::take(&mut self.tr(si, ti).chans);
        for ci in chans {
            let c = &mut self.chans[ci];
            c.env = 3;
            c.prio = 1;
            c.owner = None;
        }
    }

    fn note_on(&mut self, si: usize, ti: usize, key: u8, vel: u8, len: i32) {
        let (muted, tie, head, prg, ch_prio, allowed, bank, arcs, porta, porta_time, sweep, last_key, notewait, env) = {
            let s = self.seqs[si].as_ref().unwrap();
            let t = s.tracks[ti].as_ref().unwrap();
            (t.mute, t.tie, t.chans.first().copied(), t.prg, s.prio as i32 + t.prio as i32, s.allowed & t.chan_mask(), s.bank.clone(), s.sdat_arcs, t.porta, t.porta_time, t.sweep, t.last_key, t.notewait, t.env)
        };
        let mut chan: Option<usize> = None;
        if !muted {
            if tie && head.is_some() {
                let c = head.unwrap();
                self.chans[c].key = key;
                self.chans[c].vel = vel;
                chan = Some(c);
            } else if let Some(r) = bank.region(prg as usize, key) {
                let type_mask: u16 = match r.kind {
                    2 => 0x3f00,
                    3 => 0xc000,
                    1 | 4 => 0xffff,
                    _ => 0,
                };
                if type_mask != 0 {
                    if let Some(ci) = self.alloc_channel(type_mask, allowed, ch_prio) {
                        let mut c = self.chans[ci].clone();
                        let ok = self.region_setup(&mut c, &arcs, &r, key, vel, if len > 0 { len } else { -1 });
                        if ok {
                            c.owner = Some((si, ti));
                            c.pan_range = 0x7f;
                            c.seq_gain = self.seqs[si].as_ref().unwrap().gain;
                            self.chans[ci] = c;
                            self.tr(si, ti).chans.insert(0, ci);
                            chan = Some(ci);
                        } else {
                            self.chans[ci] = Chan::free();
                        }
                    }
                }
            }
            if let Some(ci) = chan {
                let c = &mut self.chans[ci];
                if env[0] != 0xff {
                    c.attack = attack_rate(env[0] as i32);
                }
                if env[1] != 0xff {
                    c.decay = calc_release(env[1] as i32);
                }
                if env[2] != 0xff {
                    c.sustain = env[2];
                }
                if env[3] != 0xff {
                    c.release = calc_release(env[3] as i32);
                }
                c.sweep_pitch = sweep as i32;
                if porta {
                    c.sweep_pitch += ((last_key as i32) - key as i32) << 6;
                }
                if porta_time == 0 {
                    c.sweep_len = if len < 1 { 0x7fff_ffff } else { len };
                    c.sweep_by_frame = false;
                } else {
                    c.sweep_len = (porta_time as i32 * porta_time as i32 * (c.sweep_pitch.abs())) >> 11;
                    c.sweep_by_frame = true;
                }
                c.sweep_pos = 0;
            }
        }
        let t = self.tr(si, ti);
        t.last_key = key;
        if notewait {
            t.wait = len;
            if len == 0 {
                t.wait_release = true;
            }
        }
    }
}

impl Track {
    fn chan_mask(&self) -> u16 {
        self.ch_mask
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn square_decibel_table() {
        assert_eq!(sq_db(127), 0);
        assert_eq!(sq_db(1), -722);
        assert_eq!(sq_db(2), -721);
        assert_eq!(sq_db(3), -651);
        assert_eq!(sq_db(64), -119);
        assert_eq!(sq_db(0), -32768);
    }

    #[test]
    fn envelope_rate_conversions() {
        assert_eq!(calc_release(0), 1);
        assert_eq!(calc_release(49), 99);
        assert_eq!(calc_release(50), 0x1e00 / 76);
        assert_eq!(calc_release(0x7e), 0x3c00);
        assert_eq!(calc_release(0x7f), 0xffff);
        assert_eq!(attack_rate(0), 255);
        assert_eq!(attack_rate(108), 147);
        assert_eq!(attack_rate(127), 0);
        assert_eq!(attack_rate(0x6d), ATTACK_FAST[18]);
    }

    #[test]
    fn lfo_sine_and_phase() {
        assert_eq!(sin_idx(0), 0);
        assert_eq!(sin_idx(32), 127);
        assert_eq!(sin_idx(16), 90);
        assert_eq!(sin_idx(64), 0);
        assert_eq!(sin_idx(96), -127);
        let mut l = Lfo::init();
        l.depth = 10;
        l.update();
        assert!(l.phase > 0);
        assert_eq!(l.value(), 1 * 10 * sin_idx((l.phase >> 8) as usize));
        // the delay holds the LFO silent
        let mut l = Lfo::init();
        l.depth = 10;
        l.delay = 3;
        for _ in 0..3 {
            assert_eq!(l.value(), 0);
            l.update();
        }
    }

    #[test]
    fn frame_period_matches_the_hardware_timer() {
        assert!((1.0 / FRAME_SECONDS - 191.96).abs() < 0.05);
    }

    // ---------------------------------------------------------------- driver behaviour with a synthetic one-instrument bank
    use crate::sdat::{Inst, SeqInfo};

    /// bank 0 = a 50% pulse (PSG) instrument, original key 60, instant attack, full sustain, quick release
    struct Mock {
        release: u8,
        attack: u8,
    }
    impl Source for Mock {
        fn bank(&self, _id: usize) -> Option<Arc<Bank>> {
            let r = Region { kind: 2, wave: 3, wavearc: 0, original_key: 60, attack: self.attack, decay: 127, sustain: 127, release: self.release, pan: 64 };
            Some(Arc::new(Bank { instruments: vec![Inst::Single(r)], wavearcs: [None; 4] }))
        }
        fn bank_wavearcs(&self, _id: usize) -> [Option<usize>; 4] {
            [None; 4]
        }
        fn wave(&self, _a: usize, _n: usize) -> Option<Arc<Wave>> {
            None
        }
        fn player(&self, _n: usize) -> Option<PlayerInfo> {
            Some(PlayerInfo { max_seqs: 2, channel_mask: 0xffff })
        }
    }

    fn sd(bytes: &[u8]) -> SeqData {
        SeqData { data: Arc::new(bytes.to_vec()), start: 0, info: SeqInfo { bank: 0, volume: 127, channel_priority: 50, player_priority: 50, player: 0 } }
    }

    fn driver(release: u8, attack: u8) -> Driver {
        Driver::new(Arc::new(Mock { release, attack }), 48000)
    }

    /// render `secs` seconds; returns (left, right)
    fn run(d: &mut Driver, secs: f64) -> (Vec<f32>, Vec<f32>) {
        let mut buf = vec![0f32; (secs * 48000.0) as usize * 2];
        d.render(&mut buf);
        (buf.iter().step_by(2).copied().collect(), buf.iter().skip(1).step_by(2).copied().collect())
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn crossings(x: &[f32]) -> usize {
        x.windows(2).filter(|w| (w[0] <= 0.0) != (w[1] <= 0.0)).count()
    }

    fn hz(x: &[f32]) -> f64 {
        crossings(x) as f64 / 2.0 / (x.len() as f64 / 48000.0)
    }

    fn note(key: u8, len: u8) -> Vec<u8> {
        vec![key, 0x7f, len]
    }

    #[test]
    fn a_note_plays_at_its_pitch_and_for_its_length() {
        let mut prog = note(60, 96); // one second at 120 bpm
        prog.push(0xff);
        let mut d = driver(127, 127);
        d.start(&sd(&prog), 0, 0).unwrap();
        let (l, _) = run(&mut d, 1.5);
        let f = hz(&l[4800..4800 + 24000]);
        assert!((f - 261.6).abs() < 6.0, "C4 should be about 261.6 Hz, got {f}");
        assert!(rms(&l[4800..43200]) > 0.05, "sounding during the note");
        assert!(rms(&l[52000..60000]) < 1e-4, "silent after the note ends");
        assert_eq!(d.active_channels(), 0);
        assert_eq!(d.running(), 0, "the sequence finished");
        // an octave up doubles the frequency
        let mut p2 = note(72, 96);
        p2.push(0xff);
        let mut d2 = driver(127, 127);
        d2.start(&sd(&p2), 0, 0).unwrap();
        let (l2, _) = run(&mut d2, 0.8);
        assert!((hz(&l2[4800..28800]) / f - 2.0).abs() < 0.05);
    }

    #[test]
    fn tempo_command_changes_speed() {
        let mut prog = vec![0xe1, 0xf0, 0x00]; // tempo 240 (little-endian u16)
        prog.extend(note(60, 96));
        prog.push(0xff);
        let mut d = driver(127, 127);
        d.start(&sd(&prog), 0, 0).unwrap();
        let (l, _) = run(&mut d, 1.0);
        assert!(rms(&l[4800..20000]) > 0.05);
        assert!(rms(&l[30000..48000]) < 1e-4, "twice the tempo ends the note at 0.5 s");
    }

    #[test]
    fn pan_and_volume_follow_the_squared_law() {
        let run_with = |extra: Vec<u8>| {
            let mut prog = extra;
            prog.extend(note(60, 96));
            prog.push(0xff);
            let mut d = driver(127, 127);
            d.start(&sd(&prog), 0, 0).unwrap();
            run(&mut d, 0.5)
        };
        let (l0, r0) = run_with(vec![]);
        assert!((rms(&l0[4800..]) / rms(&r0[4800..]) - 1.0).abs() < 0.02, "centre pan is symmetric");
        let (l, r) = run_with(vec![0xc0, 0]); // hard left
        assert!(rms(&l[4800..]) > 0.05 && rms(&r[4800..]) < 1e-6);
        let (l, r) = run_with(vec![0xc0, 127]); // hard right
        assert!(rms(&r[4800..]) > 0.05 && rms(&l[4800..]) < rms(&r[4800..]) * 0.02);
        let (lv, _) = run_with(vec![0xc1, 64]); // track volume 64 of 127: -11.9 dB
        let ratio = rms(&lv[4800..]) / rms(&l0[4800..]);
        assert!((ratio as f64 - 10f64.powf(-119.0 / 200.0)).abs() < 0.02, "volume ratio {ratio}");
    }

    #[test]
    fn envelope_attack_ramps_and_release_decays_to_a_free_channel() {
        let mut prog = note(60, 48);
        prog.push(0xff);
        let mut d = driver(120, 100); // slow attack, release over about 0.4 s
        d.start(&sd(&prog), 0, 0).unwrap();
        let (l, _) = run(&mut d, 1.5);
        assert!(rms(&l[0..480]) < rms(&l[9600..12000]) * 0.5, "attack ramps up");
        // after the note off the level falls monotonically (per 20 ms window) and the channel is released
        let wins: Vec<f32> = (0..20).map(|i| rms(&l[24000 + i * 960..24000 + (i + 1) * 960])).collect();
        assert!(wins.windows(2).all(|w| w[1] <= w[0] + 1e-4), "release is monotone: {wins:?}");
        assert!(rms(&l[60000..]) < 1e-4);
        assert_eq!(d.active_channels(), 0);
    }

    #[test]
    fn loops_variables_and_conditionals() {
        // two passes of a loop body (one note each), then end: 2 x 24 ticks = 0.5 s
        let mut prog = vec![0xd4, 2];
        prog.extend(note(60, 24));
        prog.extend([0xfc, 0xff]);
        let mut d = driver(127, 127);
        d.start(&sd(&prog), 0, 0).unwrap();
        let (l, _) = run(&mut d, 1.0);
        assert!(rms(&l[4800..22000]) > 0.05 && rms(&l[30000..]) < 1e-4);
        assert_eq!(crossings(&l[2000..2400]) > 0, true);
        // var0 = 5; if var0 == 5 play; (second program tests != )
        let mk = |cmp_val: u8| {
            let mut p = vec![0xb0, 0, 0x05, 0x00]; // var0 = 5
            p.extend([0xb8, 0, cmp_val, 0x00]); // cond = (var0 == cmp_val)
            p.extend([0xa2]);
            p.extend(note(60, 48));
            p.push(0xff);
            p
        };
        for (cmp, expect_sound) in [(5u8, true), (6, false)] {
            let mut d = driver(127, 127);
            d.start(&sd(&mk(cmp)), 0, 0).unwrap();
            let (l, _) = run(&mut d, 0.4);
            assert_eq!(rms(&l[4800..]) > 0.05, expect_sound, "compare value {cmp}");
        }
    }

    #[test]
    fn portamento_glides_to_the_note() {
        // porta on, start key 48 (C3), note C4 for one second: pitch rises over the note
        let mut prog = vec![0xce, 1, 0xc9, 48];
        prog.extend(note(60, 96));
        prog.push(0xff);
        let mut d = driver(127, 127);
        d.start(&sd(&prog), 0, 0).unwrap();
        let (l, _) = run(&mut d, 1.0);
        let (early, late) = (hz(&l[2400..9600]), hz(&l[38000..45000]));
        assert!(early < 190.0 && late > 220.0, "glide from C3 up to C4: {early} -> {late}");
    }

    #[test]
    fn polyphony_is_limited_to_the_psg_channels_and_priority_decides() {
        // 10 simultaneous pulse notes (tracks 1..=9 plus track 0): only the six PSG channels exist
        let mut prog = vec![0xfe, 0xfe, 0x03]; // open tracks 1..9
        for t in 1..=9u8 {
            prog.extend([0x93, t, 0, 0, 0]); // open track t at offset 0 (patched below)
        }
        let body = prog.len();
        let mut full = prog.clone();
        full.extend(note(60, 96));
        full.push(0xff);
        // patch the track entry offsets to the body
        for t in 0..9 {
            let at = 3 + t * 5 + 2;
            full[at] = (body & 0xff) as u8;
            full[at + 1] = (body >> 8) as u8;
        }
        let mut d = driver(127, 127);
        d.start(&sd(&full), 0, 0).unwrap();
        let _ = run(&mut d, 0.3);
        assert!(d.active_channels() <= 6 && d.active_channels() >= 1, "PSG channels only: {}", d.active_channels());
    }

    #[test]
    fn stopping_with_a_fade_ramps_the_output_down() {
        let mut prog = vec![0xd4, 0]; // endless loop of a long note
        prog.extend(note(60, 96));
        prog.extend([0xfc, 0xff]);
        let mut d = driver(127, 127);
        let h = d.start(&sd(&prog), 0, 0).unwrap();
        let (_, _) = run(&mut d, 0.3);
        d.stop(h, 60); // one second fade (60 game frames)
        let (l, _) = run(&mut d, 1.6);
        let (a, b, c) = (rms(&l[2400..7200]), rms(&l[24000..28800]), rms(&l[60000..]));
        assert!(a > b && b > c * 2.0 && c < 1e-3, "fade out: {a} {b} {c}");
        assert!(!d.is_running(h));
    }

    #[test]
    fn a_player_limits_simultaneous_sequences() {
        // the mock player allows two sequences: starting a third stops the oldest
        let mut prog = vec![0xd4, 0];
        prog.extend(note(60, 96));
        prog.extend([0xfc, 0xff]);
        let mut d = driver(127, 127);
        let a = d.start(&sd(&prog), 0, 0).unwrap();
        let b = d.start(&sd(&prog), 0, 0).unwrap();
        let c = d.start(&sd(&prog), 0, 0).unwrap();
        assert!(!d.is_running(a) && d.is_running(b) && d.is_running(c));
    }

    #[test]
    fn voice_clips_mix_and_end() {
        let mut d = driver(127, 127);
        let id = d.play_clip(Arc::new(vec![8000i16; 4800]), 48000, 1.0, 1.0);
        assert!(d.clip_playing(id));
        let (l, r) = run(&mut d, 0.2);
        assert!(rms(&l[..4000]) > 0.1 && (rms(&l[..4000]) - rms(&r[..4000])).abs() < 1e-6);
        assert!(!d.clip_playing(id));
    }
}
