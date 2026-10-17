//! The ARM7's sound driver, as the ARM9 sees it: SDK sound commands arrive over PXI (tag 7, a word = the address of
//! a linked list of SNDCommand {next, id, arg[4]} in main RAM, or 0 = "process what you have"), the driver runs them
//! and publishes its state in the shared work (SNDSharedWork: finishCommandTag, playerStatus, channelStatus,
//! captureStatus, per-player variables and tick counters, global variables). Alarms it was asked to run come back
//! over PXI tag 7 as `alarmNo | id << 8`, which the game's callback (SNDi_CallAlarmHandler) dispatches.
//!
//! The protocol is the game's own code: the command builders (SND_* at 0x2023c24..0x2024054) give each command's
//! argument packing, SND_FlushCommand / SND_RecvCommandReply the list hand-over. The data the commands point at is
//! the game's: sequences, banks and wave archives as NNS loaded them into main RAM (banks link their wave archives,
//! a wave archive's offset table holds an offset from the archive, 0 for "not loaded", or the address of a wave
//! loaded on its own - SND_GetWaveDataAddress 0x2024e9c). The sequencer is `snd::Driver`; the channels the ARM9
//! drives itself (the NNS stream: SETUP_CHANNEL_PCM + START_TIMER) play straight from main RAM as the hardware
//! would.
//!
//! Time: the driver runs in the game's time (`Machine::tick`, OS ticks); `advance` renders what the elapsed ticks
//! cover and fires the alarms that came due.

use super::{ext, Machine, Subsystems, TICK_HZ};
use crate::flow::Arg;
use crate::sdat::{decode_swav, parse_sbnk, Bank, PlayerInfo, Wave};
use crate::snd::{Driver, Source};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

/// SNDCommandID
pub const NAMES: [&str; 34] = [
    "START_SEQ", "STOP_SEQ", "PREPARE_SEQ", "START_PREPARED_SEQ", "PAUSE_SEQ", "SKIP_SEQ", "PLAYER_PARAM", "TRACK_PARAM",
    "MUTE_TRACK", "ALLOCATABLE_CHANNEL", "PLAYER_LOCAL_VAR", "PLAYER_GLOBAL_VAR", "START_TIMER", "STOP_TIMER",
    "SETUP_CHANNEL_PCM", "SETUP_CHANNEL_PSG", "SETUP_CHANNEL_NOISE", "SETUP_CAPTURE", "SETUP_ALARM", "CHANNEL_TIMER",
    "CHANNEL_VOLUME", "CHANNEL_PAN", "SURROUND_DECAY", "MASTER_VOLUME", "MASTER_PAN", "OUTPUT_SELECTOR", "LOCK_CHANNEL",
    "UNLOCK_CHANNEL", "STOP_UNLOCKED_CHANNEL", "SHARED_WORK", "INVALIDATE_SEQ", "INVALIDATE_BANK", "INVALIDATE_WAVE",
    "READ_DRIVER_INFO",
];

/// the sound channels' timer clock
const CHANNEL_CLOCK: f64 = 16_756_991.0;
/// the ARM9's PXI callback for tag 7 (PxiFifoCallback -> SNDi_CallAlarmHandler)
const PXI_TAG_SOUND: u32 = 7;
/// how much rendered audio waits for the host before the oldest is dropped (seconds)
const SINK_LIMIT: f64 = 0.25;

/// banks and waves as the game loaded them into main RAM, copied when a sequence is prepared (ids are addresses)
#[derive(Default)]
pub struct RamSource {
    /// bank address -> (end address, instruments, wave archive addresses of its four slots)
    banks: Mutex<HashMap<usize, (u32, Arc<Bank>, [Option<usize>; 4])>>,
    /// wave archive address -> each wave's address (0: not loaded)
    tables: Mutex<HashMap<usize, Vec<u32>>>,
    /// wave address -> (end address, decoded wave)
    waves: Mutex<HashMap<u32, (u32, Arc<Wave>)>>,
}

impl Source for RamSource {
    fn bank(&self, id: usize) -> Option<Arc<Bank>> {
        self.banks.lock().unwrap().get(&id).map(|b| b.1.clone())
    }
    fn bank_wavearcs(&self, id: usize) -> [Option<usize>; 4] {
        self.banks.lock().unwrap().get(&id).map_or([None; 4], |b| b.2)
    }
    fn wave(&self, arc: usize, n: usize) -> Option<Arc<Wave>> {
        let addr = *self.tables.lock().unwrap().get(&arc)?.get(n)?;
        self.waves.lock().unwrap().get(&addr).map(|w| w.1.clone())
    }
    fn player(&self, _: usize) -> Option<PlayerInfo> {
        None
    }
}

impl RamSource {
    /// copy a bank (SNDBankData: the SBNK file with its wave archive links at +0x18 + 8k) and the waves its
    /// archives hold now
    fn load_bank(&self, m: &Machine, bank: u32) -> bool {
        if m.ram.main_slice(bank, 4) != Some(b"SBNK") {
            return false;
        }
        let size = m.ram.u32(bank + 8);
        let Some(d) = m.ram.main_slice(bank, size as usize) else { return false };
        let Ok(inst) = parse_sbnk(d) else { return false };
        let mut arcs = [None; 4];
        let mut wavearcs = [None; 4];
        for (k, a) in arcs.iter_mut().enumerate() {
            let p = m.ram.u32(bank + 0x18 + 8 * k as u32);
            if p != 0 {
                *a = Some(p as usize);
                wavearcs[k] = Some(k as u16);
                self.load_wavearc(m, p);
            }
        }
        self.banks.lock().unwrap().insert(bank as usize, (bank + size, Arc::new(Bank { instruments: inst, wavearcs }), arcs));
        true
    }

    /// read a wave archive's offset table (SNDWaveArc: count at +0x38, entries at +0x3c) and copy waves not held yet
    fn load_wavearc(&self, m: &Machine, arc: u32) {
        if m.ram.main_slice(arc, 4) != Some(b"SWAR") {
            return;
        }
        let n = m.ram.u32(arc + 0x38).min(0x1000);
        let table: Vec<u32> = (0..n)
            .map(|i| match m.ram.u32(arc + 0x3c + 4 * i) {
                0 => 0,
                v if v < 0x200_0000 => arc + v,
                v => v,
            })
            .collect();
        let mut waves = self.waves.lock().unwrap();
        for &a in &table {
            if a == 0 || waves.contains_key(&a) {
                continue;
            }
            // SNDWaveData: SNDWaveParam {format, loopflag, rate, timer, loopstart (words), looplen (words)}, samples
            let (ls, ll) = (m.ram.u16(a + 6) as usize, m.ram.u32(a + 8) as usize);
            let size = 12 + (ls + ll) * 4;
            if let Some(w) = m.ram.main_slice(a, size).and_then(|d| decode_swav(d, 0, size)) {
                waves.insert(a, (a + size as u32, Arc::new(w)));
            }
        }
        self.tables.lock().unwrap().insert(arc as usize, table);
    }

    /// the banks held (address, instruments, wave archive addresses), for checks against the extracted data
    pub fn banks_held(&self) -> Vec<(usize, Arc<Bank>, [Option<usize>; 4])> {
        self.banks.lock().unwrap().iter().map(|(&a, b)| (a, b.1.clone(), b.2)).collect()
    }

    /// INVALIDATE_BANK / INVALIDATE_WAVE: the game freed this memory
    fn forget(&self, start: u32, end: u32) {
        let hit = |a: u32, e: u32| a < end && e > start;
        self.banks.lock().unwrap().retain(|&a, b| !hit(a as u32, b.0));
        self.waves.lock().unwrap().retain(|&a, w| !hit(a, w.0));
        self.tables.lock().unwrap().retain(|&a, _| !hit(a as u32, a as u32 + 0x3c));
    }
}

/// a channel the ARM9 set up itself (SETUP_CHANNEL_PCM): plays from main RAM as the hardware does
#[derive(Clone, Default)]
struct HwChan {
    set: bool,
    on: bool,
    /// 0 PCM8, 1 PCM16, 2 IMA-ADPCM
    format: u8,
    addr: u32,
    /// 0 manual, 1 repeat, 2 one shot
    looping: u8,
    /// in words
    loop_start: u32,
    loop_len: u32,
    volume: u8,
    shift: u8,
    /// the sample period in channel clocks (SND_TIMER_CLOCK / rate; the register holds 0x10000 minus this)
    timer: u16,
    pan: u8,
    /// position in samples, ADPCM decoder state (sample index decoded, predictor, step index) and the state saved
    /// at the loop start
    pos: f64,
    adpcm: (usize, i32, i32),
    adpcm_loop: Option<(i32, i32)>,
    last: i16,
    /// diagnostics: the last sample index recorded into `Snd7::tap` (+1; 0 = none)
    tapped: usize,
}

impl HwChan {
    fn samples_per_word(&self) -> usize {
        match self.format {
            0 => 4,
            1 => 2,
            _ => 8,
        }
    }
    /// (loop start, end) in samples (ADPCM: the first word is the decoder's header)
    fn bounds(&self) -> (usize, usize) {
        let spw = self.samples_per_word();
        let hdr = if self.format == 2 { 8 } else { 0 };
        let ls = (self.loop_start as usize * spw).saturating_sub(hdr);
        (ls, ls + self.loop_len as usize * spw)
    }
    fn start(&mut self, m: &Machine) {
        self.on = true;
        self.pos = 0.0;
        self.adpcm = (0, m.ram.u16(self.addr) as i16 as i32, (m.ram.u16(self.addr + 2) as i32).min(88));
        self.adpcm_loop = None;
        self.last = self.adpcm.1 as i16;
    }
    /// the sample at index `i` (sequential for ADPCM)
    fn sample(&mut self, m: &Machine, i: usize) -> i16 {
        match self.format {
            0 => (m.ram.u8(self.addr + i as u32) as i8 as i16) << 8,
            1 => m.ram.u16(self.addr + 2 * i as u32) as i16,
            _ => {
                let (ls, _) = self.bounds();
                if i < self.adpcm.0 {
                    // looped back: the state saved at the loop start
                    if let Some((p, x)) = self.adpcm_loop {
                        self.adpcm = (ls, p, x);
                    }
                }
                while self.adpcm.0 <= i {
                    let k = self.adpcm.0;
                    if k == ls && self.adpcm_loop.is_none() {
                        self.adpcm_loop = Some((self.adpcm.1, self.adpcm.2));
                    }
                    let byte = m.ram.u8(self.addr + 4 + (k / 2) as u32);
                    let nib = if k & 1 == 0 { byte & 15 } else { byte >> 4 };
                    let (mut p1, mut x1) = (self.adpcm.1, self.adpcm.2);
                    let v = adpcm_step(nib, &mut p1, &mut x1);
                    self.adpcm = (k + 1, p1, x1);
                    self.last = v;
                }
                self.last
            }
        }
    }
}

/// one IMA-ADPCM nibble (the hardware's decoder)
fn adpcm_step(nib: u8, pred: &mut i32, idx: &mut i32) -> i16 {
    const STEP: [i32; 89] = [
        7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107,
        118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371, 408, 449, 494, 544, 598, 658, 724, 796, 876,
        963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871,
        5358, 5894, 6484, 7132, 7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385,
        24623, 27086, 29794, 32767,
    ];
    const IDX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];
    let n = nib as i32;
    let step = STEP[*idx as usize];
    let mut diff = step >> 3;
    if n & 1 != 0 {
        diff += step >> 2;
    }
    if n & 2 != 0 {
        diff += step >> 1;
    }
    if n & 4 != 0 {
        diff += step;
    }
    *pred = if n & 8 != 0 { *pred - diff } else { *pred + diff }.clamp(-32767, 32767);
    *idx = (*idx + IDX[(n & 7) as usize]).clamp(0, 88);
    *pred as i16
}

#[derive(Clone, Copy, Default)]
struct Alarm {
    tick: u32,
    period: u32,
    id: u8,
    active: bool,
    due: u64,
}

pub struct Snd7 {
    /// the shared work's address (SHARED_WORK command)
    pub shared: u32,
    /// command lists received and not yet run
    pending: Vec<u32>,
    /// commands run, per id (diagnostics)
    pub counts: Vec<u64>,
    /// the output rate and where rendered audio goes (interleaved stereo; None: rendered and dropped)
    pub rate: u32,
    pub sink: Option<Arc<Mutex<VecDeque<f32>>>>,
    pub src: Arc<RamSource>,
    seq: Option<Driver>,
    /// per player: the sequence data and bank addresses it was started with
    players: [Option<(u32, u32)>; 16],
    hw: Vec<HwChan>,
    alarms: [Alarm; 8],
    master: f32,
    /// OS tick rendered up to, and the fraction of a sample carried
    rendered_to: Option<u64>,
    carry: f64,
    /// diagnostics: when set, the samples the lowest sounding ARM9 channel consumes (one per sample index)
    pub tap: Option<Vec<i16>>,
}

impl Default for Snd7 {
    fn default() -> Self {
        Snd7 {
            shared: 0,
            pending: Vec::new(),
            counts: Vec::new(),
            rate: 32_768,
            sink: None,
            src: Default::default(),
            seq: None,
            players: [None; 16],
            hw: vec![HwChan::default(); 16],
            alarms: [Alarm::default(); 8],
            master: 1.0,
            rendered_to: None,
            carry: 0.0,
            tap: None,
        }
    }
}

impl Snd7 {
    fn seq(&mut self) -> &mut Driver {
        if self.seq.is_none() {
            self.seq = Some(Driver::new(self.src.clone(), self.rate));
        }
        self.seq.as_mut().unwrap()
    }

    /// a word arrived over PXI tag 7
    pub fn receive(m: &mut Machine, data: u32) {
        // what played before the commands take effect
        render_to(m, m.tick);
        if data != 0 {
            m.snd.pending.push(data);
        }
        // the ARM7 runs its command lists as soon as it is told (its sound thread wakes on the PXI interrupt)
        Self::process(m);
    }

    /// run the received command lists in order; each finished list advances finishCommandTag by one
    pub fn process(m: &mut Machine) {
        let lists = std::mem::take(&mut m.snd.pending);
        for head in lists {
            let mut c = head;
            let mut guard = 0;
            while c != 0 && guard < 4096 {
                let id = m.ram.u32(c + 4);
                let arg = [m.ram.u32(c + 8), m.ram.u32(c + 12), m.ram.u32(c + 16), m.ram.u32(c + 20)];
                Self::command(m, id, arg);
                c = m.ram.u32(c);
                guard += 1;
            }
            // the players' state goes to the shared work with the list's tag: the ARM9's sound library checks a
            // started player's active bit once the tag is finished, and drops a player whose bit is still clear
            update_shared(m);
            if m.snd.shared != 0 {
                let v = m.ram.u32(m.snd.shared).wrapping_add(1);
                m.ram.set_u32(m.snd.shared, v);
            }
        }
    }

    fn command(m: &mut Machine, id: u32, a: [u32; 4]) {
        let c = &mut m.snd.counts;
        if c.len() <= id as usize {
            c.resize(id as usize + 1, 0);
        }
        c[id as usize] += 1;
        if std::env::var("LP_SND_LOG").map_or(false, |v| v == "all" || id != 33) {
            eprintln!("snd f{} t{} {} {:x?}", m.frame, m.tick, NAMES.get(id as usize).copied().unwrap_or("?"), a);
        }
        let each = |mask: u32| (0..16usize).filter(move |i| mask >> i & 1 == 1);
        match id {
            // START_SEQ / PREPARE_SEQ (player, sequence data base, offset, bank)
            0 | 2 => {
                let (p, base, off, bank) = (a[0] as usize, a[1], a[2] as usize, a[3]);
                if p >= 16 {
                    return;
                }
                let Some(data) = seq_data(m, base) else { return };
                let src = m.snd.src.clone();
                if !src.load_bank(m, bank) {
                    return;
                }
                m.snd.players[p] = Some((base, bank));
                m.snd.seq().start_player(p, Arc::new(data), off, bank as usize, id == 2);
            }
            1 => {
                m.snd.seq().stop_player(a[0] as usize);
            }
            3 => m.snd.seq().start_prepared(a[0] as usize),
            4 => m.snd.seq().pause_player(a[0] as usize, a[1] != 0),
            6 => m.snd.seq().set_player_param(a[0] as usize, a[1], a[2], a[3]),
            7 => m.snd.seq().set_track_param((a[0] & 0xffffff) as usize, a[1], a[2], a[3], a[0] >> 24),
            8 => m.snd.seq().mute_tracks(a[0] as usize, a[1], a[2]),
            9 => m.snd.seq().set_allocatable(a[0] as usize, a[1], a[2] as u16),
            10 => m.snd.seq().set_player_var(a[0] as usize, a[1] as usize, a[2] as i16),
            11 => m.snd.seq().set_global_var(a[0] as usize, a[1] as i16),
            // START_TIMER / STOP_TIMER (channels, captures, alarms, flags)
            12 => {
                // the channel starts now: the next output sample comes (1 - carry) output periods later, so its
                // first read is that far into the channel's samples (keeps channel and alarms phase-exact)
                let rate = m.snd.rate as f64;
                let lead = 1.0 - m.snd.carry;
                for ch in each(a[0]) {
                    let mut h = std::mem::take(&mut m.snd.hw[ch]);
                    if h.set {
                        h.start(m);
                        h.pos = lead * CHANNEL_CLOCK / h.timer.max(1) as f64 / rate;
                    }
                    m.snd.hw[ch] = h;
                }
                let now = m.tick;
                for n in each(a[2]).filter(|&n| n < 8) {
                    let al = &mut m.snd.alarms[n];
                    al.active = true;
                    al.due = now + al.tick.max(1) as u64;
                }
            }
            13 => {
                for ch in each(a[0]) {
                    m.snd.hw[ch].on = false;
                }
                for n in each(a[2]).filter(|&n| n < 8) {
                    m.snd.alarms[n].active = false;
                }
            }
            // SETUP_CHANNEL_PCM: chNo | timer << 16, data, loopLen | shift << 22 | volume << 24,
            // loopStart | pan << 16 | format << 24 | loop << 26
            14 => {
                let ch = (a[0] & 0xffff) as usize;
                if ch < 16 {
                    m.snd.hw[ch] = HwChan {
                        set: true,
                        on: false,
                        format: (a[3] >> 24 & 3) as u8,
                        addr: a[1],
                        looping: (a[3] >> 26 & 3) as u8,
                        loop_start: a[3] & 0xffff,
                        loop_len: a[2] & 0x3fffff,
                        volume: (a[2] >> 24 & 0x7f) as u8,
                        shift: (a[2] >> 22 & 3) as u8,
                        timer: (a[0] >> 16) as u16,
                        pan: (a[3] >> 16 & 0x7f) as u8,
                        ..Default::default()
                    };
                }
            }
            // SETUP_ALARM (alarm, first tick, period, id)
            18 => {
                if let Some(al) = m.snd.alarms.get_mut(a[0] as usize) {
                    *al = Alarm { tick: a[1], period: a[2], id: a[3] as u8, active: false, due: 0 };
                }
            }
            19 => each(a[0]).for_each(|ch| m.snd.hw[ch].timer = a[1] as u16),
            20 => each(a[0]).for_each(|ch| {
                m.snd.hw[ch].volume = a[1] as u8;
                m.snd.hw[ch].shift = a[2] as u8;
            }),
            21 => each(a[0]).for_each(|ch| m.snd.hw[ch].pan = a[1] as u8),
            23 => m.snd.master = a[0].min(127) as f32 / 127.0,
            // LOCK_CHANNEL / UNLOCK_CHANNEL: the sequencer keeps off these channels
            26 => {
                let l = m.snd.seq().locked | a[0] as u16;
                m.snd.seq().locked = l;
            }
            27 => {
                let l = m.snd.seq().locked & !(a[0] as u16);
                m.snd.seq().locked = l;
            }
            29 => m.snd.shared = a[0],
            // INVALIDATE_SEQ / BANK / WAVE (start, end): players using that memory stop, copies are dropped
            30 | 31 => {
                for p in 0..16 {
                    if let Some((base, bank)) = m.snd.players[p] {
                        let a0 = if id == 30 { base } else { bank };
                        if a0 >= a[0] && a0 < a[1] {
                            m.snd.seq().stop_player(p);
                            m.snd.players[p] = None;
                        }
                    }
                }
                if id == 31 {
                    m.snd.src.forget(a[0], a[1]);
                }
            }
            32 => m.snd.src.forget(a[0], a[1]),
            33 => write_driver_info(m, a[0]),
            _ => {}
        }
    }

    /// the commands run so far, by name (diagnostics)
    /// the sequence players playing now: (player, its sequence base and bank, ticks played) (diagnostics)
    pub fn active_players(&mut self) -> Vec<(usize, Option<(u32, u32)>, u64)> {
        let players = self.players;
        let d = self.seq();
        (0..16).filter(|&p| d.player_active(p)).map(|p| (p, players[p], d.player_ticks(p))).collect()
    }
    pub fn summary(&self) -> String {
        self.counts.iter().enumerate().filter(|(_, &n)| n > 0).map(|(i, n)| format!("{} {n}", NAMES.get(i).copied().unwrap_or("?"))).collect::<Vec<_>>().join(", ")
    }

    /// bring the driver up to the game's time: the alarms that came due fire in order (the game's PXI callback runs
    /// them; the audio before each is rendered first, as it played before the handler ran), then the rest of the
    /// audio, and the state goes to the shared work
    pub fn advance(m: &mut Machine, subs: &mut dyn Subsystems) {
        let now = m.tick;
        loop {
            let next = (0..8).filter(|&n| m.snd.alarms[n].active && m.snd.alarms[n].due <= now).min_by_key(|&n| m.snd.alarms[n].due);
            let Some(n) = next else { break };
            let due = m.snd.alarms[n].due;
            render_to(m, due);
            let al = &mut m.snd.alarms[n];
            let word = n as u32 | (al.id as u32) << 8;
            if al.period > 0 {
                al.due += al.period as u64;
            } else {
                al.active = false;
            }
            if let Some(&cb) = m.pxi_callbacks.get(&PXI_TAG_SOUND) {
                let mut args = [Arg::I(PXI_TAG_SOUND as i32), Arg::I(word as i32), Arg::I(0)];
                ext::call(m, subs, cb, &mut args);
            }
        }
        render_to(m, now);
        update_shared(m);
    }
}

/// render the audio from where the last render ended up to OS tick `t`
fn render_to(m: &mut Machine, t: u64) {
    let from = *m.snd.rendered_to.get_or_insert(t);
    if t <= from {
        return;
    }
    let exact = (t - from) as f64 * m.snd.rate as f64 / TICK_HZ + m.snd.carry;
    let n = exact.floor() as usize;
    m.snd.carry = exact - n as f64;
    m.snd.rendered_to = Some(t);
    if n == 0 {
        return;
    }
    let mut buf = vec![0f32; 2 * n];
    m.snd.seq().render(&mut buf);
    mix_hw(m, &mut buf);
    let master = m.snd.master;
    for v in buf.iter_mut() {
        *v *= master;
    }
    if let Some(sink) = &m.snd.sink {
        let mut q = sink.lock().unwrap();
        q.extend(buf);
        let limit = (SINK_LIMIT * m.snd.rate as f64) as usize * 2;
        if q.len() > limit {
            let drop = q.len() - limit;
            q.drain(..drop);
        }
    }
}

/// a sequence's data from its base address to the end of the file it lies in: an SSEQ (base = file + 0x1c) or a
/// sequence archive SSAR (base = file + its data offset at +0x18)
fn seq_data(m: &Machine, base: u32) -> Option<Vec<u8>> {
    let file_end = |f: u32| f + m.ram.u32(f + 8);
    let end = if m.ram.main_slice(base.wrapping_sub(0x1c), 4) == Some(b"SSEQ") && m.ram.u32(base - 0x1c + 0x18) == 0x1c {
        file_end(base - 0x1c)
    } else {
        (1..0x4000u32)
            .map(|k| base.wrapping_sub(4 * k))
            .find(|&f| m.ram.main_slice(f, 4) == Some(b"SSAR") && m.ram.u32(f + 0x18) == base - f)
            .map(file_end)
            .unwrap_or(base + 0x10000)
    };
    m.ram.main_slice(base, end.checked_sub(base)? as usize).map(|d| d.to_vec())
}

/// the ARM9's own channels, added to `out` (interleaved stereo at the driver's rate)
fn mix_hw(m: &mut Machine, out: &mut [f32]) {
    let rate = m.snd.rate as f64;
    let mut hw = std::mem::take(&mut m.snd.hw);
    let mut tap = m.snd.tap.take();
    let tapped = hw.iter().position(|h| h.on);
    for (ci, h) in hw.iter_mut().enumerate().filter(|(_, h)| h.on) {
        let step = CHANNEL_CLOCK / h.timer.max(1) as f64 / rate;
        let div = [1.0, 0.5, 0.25, 1.0 / 16.0][h.shift as usize & 3];
        let g = h.volume as f32 / 127.0 * div as f32;
        let (gl, gr) = (g * (127 - h.pan as i32) as f32 / 127.0, g * h.pan as f32 / 127.0);
        let (ls, end) = h.bounds();
        for i in 0..out.len() / 2 {
            let mut k = h.pos as usize;
            if k >= end {
                match h.looping {
                    1 if end > ls => {
                        h.pos -= (end - ls) as f64;
                        k = h.pos as usize;
                    }
                    0 => {}
                    _ => {
                        h.on = false;
                        break;
                    }
                }
            }
            let v = h.sample(m, k);
            if let (Some(t), true) = (tap.as_mut(), Some(ci) == tapped && k + 1 != h.tapped) {
                t.push(v);
                h.tapped = k + 1;
            }
            let s = v as f32 / 32768.0;
            out[2 * i] += s * gl;
            out[2 * i + 1] += s * gr;
            h.pos += step;
        }
    }
    m.snd.hw = hw;
    m.snd.tap = tap;
}

/// SNDSharedWork: finishCommandTag +0, playerStatus +4, channelStatus +8, captureStatus +0xa, per player
/// {variable[16], tickCounter} at +0x20 + 0x24 p, globalVariable[16] at +0x260
fn update_shared(m: &mut Machine) {
    let w = m.snd.shared;
    if w == 0 {
        return;
    }
    let hw_on = m.snd.hw.iter().enumerate().filter(|(_, h)| h.on).fold(0u16, |a, (i, _)| a | 1 << i);
    let seq = m.snd.seq();
    let status = (0..16).filter(|&p| seq.player_active(p)).fold(0u32, |a, p| a | 1 << p);
    let chans = seq.channel_mask() | hw_on;
    let per: Vec<(Option<[i16; 16]>, u64)> = (0..16).map(|p| (seq.player_vars(p), seq.player_ticks(p))).collect();
    let g = seq.global_vars();
    m.ram.set_u32(w + 4, status);
    m.ram.set_u16(w + 8, chans);
    m.ram.set_u16(w + 0xa, 0);
    for (p, (vars, ticks)) in per.into_iter().enumerate() {
        let b = w + 0x20 + 0x24 * p as u32;
        if let Some(v) = vars {
            for (i, x) in v.iter().enumerate() {
                m.ram.set_u16(b + 2 * i as u32, *x as u16);
            }
            m.ram.set_u32(b + 0x20, ticks as u32);
        }
    }
    for (i, x) in g.iter().enumerate() {
        m.ram.set_u16(w + 0x260 + 2 * i as u32, *x as u16);
    }
}

/// where the driver's work would sit in the ARM7's memory (the info's channel links are addresses there)
const WORK_ADDRESS: u32 = 0x380_0000;

/// READ_DRIVER_INFO: a copy of the driver's work (SNDDriverInfo) as the ARM9's readers use it (SND_ReadTrackInfo
/// 0x20248cc): SNDExChannel[16] (0x54 each: myNo +0, nextLink +0x50) at +0, SNDPlayer[16] (0x24 each: flags +0,
/// myNo +1, prio +4, volume +5, extFader +6, tracks[16] +8, tempo +0x18) at +0x540, SNDTrack[32] (0x40 each: prgNo
/// +2, volume +4, volume2 +5, pitch_bend +6, bend_range +7, pan +8, transpose +0x13, channel_list +0x3c) at +0x780,
/// workAddress at +0x11c0, lockedChannels at +0x11c4
fn write_driver_info(m: &mut Machine, info: u32) {
    if info == 0 {
        return;
    }
    let locked = m.snd.seq().locked;
    let mut players = Vec::new();
    {
        let seq = m.snd.seq();
        for p in 0..16 {
            let tracks: Vec<_> = (0..16).map(|t| seq.track_info(p, t)).collect();
            players.push((seq.player_active(p), tracks));
        }
    }
    for c in 0..16u32 {
        m.ram.set_u8(info + 0x54 * c, c as u8);
        m.ram.set_u32(info + 0x54 * c + 0x50, 0);
    }
    let mut next_track = 0u32;
    for (p, (active, tracks)) in players.into_iter().enumerate() {
        let pb = info + 0x540 + 0x24 * p as u32;
        m.ram.set_u8(pb, active as u8);
        m.ram.set_u8(pb + 1, p as u8);
        for (t, tr) in tracks.into_iter().enumerate() {
            let Some((prg, vol, vol2, bend, range, pan, transpose, chans)) = tr.filter(|_| active && next_track < 32) else {
                m.ram.set_u8(pb + 8 + t as u32, 0xff);
                continue;
            };
            let tb = info + 0x780 + 0x40 * next_track;
            m.ram.set_u8(pb + 8 + t as u32, next_track as u8);
            next_track += 1;
            m.ram.set_u16(tb + 2, prg);
            m.ram.set_u8(tb + 4, vol);
            m.ram.set_u8(tb + 5, vol2);
            m.ram.set_u8(tb + 6, bend as u8);
            m.ram.set_u8(tb + 7, range);
            m.ram.set_u8(tb + 8, pan as u8);
            m.ram.set_u8(tb + 0x13, transpose as u8);
            // the track's channels, most recent first, linked through nextLink
            let link = |c: usize| WORK_ADDRESS + 0x54 * c as u32;
            m.ram.set_u32(tb + 0x3c, chans.first().map_or(0, |&c| link(c)));
            for w in chans.windows(2) {
                m.ram.set_u32(info + 0x54 * w[0] as u32 + 0x50, link(w[1]));
            }
        }
    }
    m.ram.set_u32(info + 0x11c0, WORK_ADDRESS);
    m.ram.set_u32(info + 0x11c4, locked as u32);
}
