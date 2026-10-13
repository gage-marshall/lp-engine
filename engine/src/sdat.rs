//! NitroSystem sound archive (SDAT): sequences (SSEQ), sequence archives (SSAR), banks (SBNK), wave archives (SWAR/SWAV) and players.
//! Reference: tools/sdat.py (diffed by `sdat_dump` / tools/sdat_dump.py); layouts from the NITRO-Composer manuals.

use crate::bytes::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const STEP: [i32; 89] = [
    7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 21, 23, 25, 28, 31, 34, 37, 41, 45, 50, 55, 60, 66, 73, 80, 88, 97, 107, 118, 130, 143, 157, 173, 190, 209, 230, 253, 279, 307, 337, 371,
    408, 449, 494, 544, 598, 658, 724, 796, 876, 963, 1060, 1166, 1282, 1411, 1552, 1707, 1878, 2066, 2272, 2499, 2749, 3024, 3327, 3660, 4026, 4428, 4871, 5358, 5894, 6484, 7132,
    7845, 8630, 9493, 10442, 11487, 12635, 13899, 15289, 16818, 18500, 20350, 22385, 24623, 27086, 29794, 32767,
];
const IDX: [i32; 8] = [-1, -1, -1, -1, 2, 4, 6, 8];

/// IMA-ADPCM (NDS flavour): `data` starts with an s16 predictor and u16 step index, then nibbles, low nibble first.  Returns the samples after the header.
pub fn adpcm_block(data: &[u8]) -> Vec<i16> {
    if data.len() < 4 {
        return Vec::new();
    }
    let mut pred = i16::from_le_bytes([data[0], data[1]]) as i32;
    let mut idx = (u16::from_le_bytes([data[2], data[3]]) as i32).min(88);
    adpcm_body(&data[4..], &mut pred, &mut idx)
}

pub fn adpcm_body(body: &[u8], pred: &mut i32, idx: &mut i32) -> Vec<i16> {
    let mut out = Vec::with_capacity(body.len() * 2);
    for &byte in body {
        for nib in [(byte & 15) as i32, (byte >> 4) as i32] {
            let step = STEP[*idx as usize];
            let mut diff = step >> 3;
            if nib & 1 != 0 {
                diff += step >> 2;
            }
            if nib & 2 != 0 {
                diff += step >> 1;
            }
            if nib & 4 != 0 {
                diff += step;
            }
            *pred = if nib & 8 != 0 { *pred - diff } else { *pred + diff }.clamp(-32768, 32767);
            *idx = (*idx + IDX[(nib & 7) as usize]).clamp(0, 88);
            out.push(*pred as i16);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    /// 1 PCM, 2 PSG, 3 noise, 4 direct PCM, 5 null
    pub kind: u8,
    pub wave: u16,
    pub wavearc: u16,
    pub original_key: u8,
    pub attack: u8,
    pub decay: u8,
    pub sustain: u8,
    pub release: u8,
    pub pan: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inst {
    None,
    Single(Region),
    DrumSet { min: u8, max: u8, regions: Vec<Region> },
    KeySplit { keys: Vec<u8>, regions: Vec<Region> },
    Other(u8),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Wave {
    /// 0 PCM8, 1 PCM16, 2 ADPCM
    pub format: u8,
    pub rate: u32,
    /// decoded samples (i16)
    pub pcm: Vec<i16>,
    /// loop as (start, end) sample indices
    pub looped: Option<(usize, usize)>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SeqInfo {
    pub bank: u16,
    pub volume: u8,
    pub channel_priority: u8,
    pub player_priority: u8,
    pub player: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayerInfo {
    pub max_seqs: u16,
    pub channel_mask: u16,
}

pub struct Bank {
    pub instruments: Vec<Inst>,
    pub wavearcs: [Option<u16>; 4],
}

impl Bank {
    /// region for a program/key (drum sets and key splits resolved)
    pub fn region(&self, prg: usize, key: u8) -> Option<Region> {
        match self.instruments.get(prg)? {
            Inst::Single(r) => Some(*r),
            Inst::DrumSet { min, max, regions } => {
                if key < *min || key > *max {
                    None
                } else {
                    regions.get((key - min) as usize).copied()
                }
            }
            Inst::KeySplit { keys, regions } => {
                for (i, k) in keys.iter().enumerate() {
                    if key <= *k {
                        return regions.get(i).copied();
                    }
                }
                None
            }
            _ => None,
        }
    }
}

/// a sequence's event data: `data` is the whole blob, `start` the offset of the first command; jump targets are relative to `data`
#[derive(Clone)]
pub struct SeqData {
    pub data: Arc<Vec<u8>>,
    pub start: usize,
    pub info: SeqInfo,
}

pub struct Sdat {
    d: Vec<u8>,
    files: Vec<(usize, usize)>,
    pub seqs: Vec<Option<(u32, SeqInfo)>>,
    pub seqarcs: Vec<Option<u32>>,
    pub banks: Vec<Option<(u32, [Option<u16>; 4])>>,
    pub wavearcs: Vec<Option<u32>>,
    pub players: Vec<Option<PlayerInfo>>,
    pub seq_names: Vec<Option<String>>,
    bank_cache: Mutex<HashMap<usize, Arc<Bank>>>,
    wave_cache: Mutex<HashMap<(usize, usize), Option<Arc<Wave>>>>,
    seq_cache: Mutex<HashMap<u32, Arc<Vec<u8>>>>,
}

fn cstr(d: &[u8], o: usize) -> Option<String> {
    let e = d.get(o..)?.iter().position(|&b| b == 0)?;
    Some(d[o..o + e].iter().map(|&b| b as char).collect())
}

impl Sdat {
    pub fn parse(d: Vec<u8>) -> Result<Sdat, String> {
        if d.get(0..4) != Some(b"SDAT") {
            return Err("not an SDAT".into());
        }
        let (symb, info, fat) = (u32at(&d, 0x10)? as usize, u32at(&d, 0x18)? as usize, u32at(&d, 0x20)? as usize);
        let nfiles = u32at(&d, fat + 8)? as usize;
        let mut files = Vec::new();
        for i in 0..nfiles {
            files.push((u32at(&d, fat + 12 + 16 * i)? as usize, u32at(&d, fat + 16 + 16 * i)? as usize));
        }
        let inforecs: Vec<usize> = (0..8).map(|k| u32at(&d, info + 8 + 4 * k).map(|v| v as usize)).collect::<Result<_, _>>()?;
        let symrecs: Vec<usize> = (0..8).map(|k| u32at(&d, symb + 8 + 4 * k).map(|v| v as usize)).collect::<Result<_, _>>()?;
        let rec = |kind: usize| -> Result<Vec<Option<usize>>, String> {
            let ir = info + inforecs[kind];
            let cnt = u32at(&d, ir)? as usize;
            let mut v = Vec::new();
            for i in 0..cnt {
                let o = u32at(&d, ir + 4 + 4 * i)? as usize;
                v.push(if o != 0 { Some(info + o) } else { None });
            }
            Ok(v)
        };
        let mut seqs = Vec::new();
        for r in rec(0)? {
            seqs.push(match r {
                Some(io) => Some((
                    u32at(&d, io)? & 0xff_ffff,
                    SeqInfo { bank: u16at(&d, io + 4)? as u16, volume: u8at(&d, io + 6)? as u8, channel_priority: u8at(&d, io + 7)? as u8, player_priority: u8at(&d, io + 8)? as u8, player: u8at(&d, io + 9)? as u8 },
                )),
                None => None,
            });
        }
        let seqarcs = rec(1)?.into_iter().map(|r| r.map(|io| u32at(&d, io).unwrap_or(0) & 0xff_ffff)).collect();
        let mut banks = Vec::new();
        for r in rec(2)? {
            banks.push(match r {
                Some(io) => {
                    let mut w = [None; 4];
                    for (k, slot) in w.iter_mut().enumerate() {
                        let v = u16at(&d, io + 4 + 2 * k)? as u16;
                        *slot = if v != 0xffff { Some(v) } else { None };
                    }
                    Some((u32at(&d, io)? & 0xff_ffff, w))
                }
                None => None,
            });
        }
        let wavearcs = rec(3)?.into_iter().map(|r| r.map(|io| u32at(&d, io).unwrap_or(0) & 0xff_ffff)).collect();
        let mut players = Vec::new();
        for r in rec(4)? {
            players.push(match r {
                Some(io) => Some(PlayerInfo { max_seqs: u16at(&d, io)? as u16, channel_mask: u16at(&d, io + 2)? as u16 }),
                None => None,
            });
        }
        // sequence names
        let sr = symb + symrecs[0];
        let cnt = u32at(&d, sr)? as usize;
        let mut seq_names = Vec::new();
        for i in 0..cnt {
            let o = u32at(&d, sr + 4 + 4 * i)? as usize;
            seq_names.push(if o != 0 { cstr(&d, symb + o) } else { None });
        }
        Ok(Sdat { d, files, seqs, seqarcs, banks, wavearcs, players, seq_names, bank_cache: Default::default(), wave_cache: Default::default(), seq_cache: Default::default() })
    }

    fn file(&self, fid: u32) -> Option<&[u8]> {
        let (o, s) = *self.files.get(fid as usize)?;
        self.d.get(o..o + s)
    }

    /// a plain sequence (SSEQ): `seq` is the SEQ id
    pub fn sequence(&self, seq: usize) -> Option<SeqData> {
        let (fid, info) = (*self.seqs.get(seq)?)?;
        let data = {
            let mut c = self.seq_cache.lock().unwrap();
            c.entry(fid).or_insert_with(|| Arc::new(self.file(fid).map(|f| f.to_vec()).unwrap_or_default())).clone()
        };
        if data.get(0..4) != Some(b"SSEQ") || data.get(16..20) != Some(b"DATA") {
            return None;
        }
        let off = u32at(&data, 24).ok()? as usize;
        // keep the file blob; offsets inside the sequence are relative to the data start, so expose a re-based copy
        let rebased = Arc::new(data[off..].to_vec());
        Some(SeqData { data: rebased, start: 0, info })
    }

    /// number of sub-sequences in a sequence archive
    pub fn seqarc_len(&self, arc: usize) -> Option<usize> {
        let f = self.file((*self.seqarcs.get(arc)?)?)?;
        u32at(f, 0x1c).ok().map(|v| v as usize)
    }

    /// a sub-sequence of a sequence archive (SSAR)
    pub fn archive_sequence(&self, arc: usize, index: usize) -> Option<SeqData> {
        let fid = (*self.seqarcs.get(arc)?)?;
        let f = self.file(fid)?;
        if f.get(0..4) != Some(b"SSAR") {
            return None;
        }
        let (data_off, count) = (u32at(f, 0x18).ok()? as usize, u32at(f, 0x1c).ok()? as usize);
        if index >= count {
            return None;
        }
        let r = 0x20 + 12 * index;
        let off = u32at(f, r).ok()? as usize;
        let info = SeqInfo { bank: u16at(f, r + 4).ok()? as u16, volume: u8at(f, r + 6).ok()? as u8, channel_priority: u8at(f, r + 7).ok()? as u8, player_priority: u8at(f, r + 8).ok()? as u8, player: u8at(f, r + 9).ok()? as u8 };
        let data = f.get(data_off..)?.to_vec();
        Some(SeqData { data: Arc::new(data), start: off, info })
    }

    pub fn bank(&self, id: usize) -> Option<Arc<Bank>> {
        if let Some(b) = self.bank_cache.lock().unwrap().get(&id) {
            return Some(b.clone());
        }
        let (fid, wavearcs) = (*self.banks.get(id)?)?;
        let f = self.file(fid)?;
        let b = Arc::new(Bank { instruments: parse_sbnk(f).ok()?, wavearcs });
        self.bank_cache.lock().unwrap().insert(id, b.clone());
        Some(b)
    }

    /// wave `n` of wave archive `arc` (SDAT wave-archive id), decoded
    pub fn wave(&self, arc: usize, n: usize) -> Option<Arc<Wave>> {
        if let Some(w) = self.wave_cache.lock().unwrap().get(&(arc, n)) {
            return w.clone();
        }
        let w = (|| {
            let f = self.file((*self.wavearcs.get(arc)?)?)?;
            let cnt = u32at(f, 0x38).ok()? as usize;
            if n >= cnt {
                return None;
            }
            let off = u32at(f, 0x3c + 4 * n).ok()? as usize;
            let end = if n + 1 < cnt { u32at(f, 0x3c + 4 * (n + 1)).ok()? as usize } else { f.len() };
            decode_swav(f, off, end - off).map(Arc::new)
        })();
        self.wave_cache.lock().unwrap().insert((arc, n), w.clone());
        w
    }

    /// wave archive id for slot `slot` (0..4) of a bank
    pub fn bank_wavearc(&self, bank: usize, slot: usize) -> Option<usize> {
        self.banks.get(bank)?.as_ref()?.1.get(slot).copied().flatten().map(|v| v as usize)
    }
}

pub fn parse_sbnk(f: &[u8]) -> Result<Vec<Inst>, String> {
    let cnt = u32at(f, 0x38)? as usize;
    let param = |o: usize, kind: u8| -> Result<Region, String> {
        Ok(Region {
            kind,
            wave: u16at(f, o)? as u16,
            wavearc: u16at(f, o + 2)? as u16,
            original_key: u8at(f, o + 4)? as u8,
            attack: u8at(f, o + 5)? as u8,
            decay: u8at(f, o + 6)? as u8,
            sustain: u8at(f, o + 7)? as u8,
            release: u8at(f, o + 8)? as u8,
            pan: u8at(f, o + 9)? as u8,
        })
    };
    let mut out = Vec::new();
    for i in 0..cnt {
        let v = u32at(f, 0x3c + 4 * i)?;
        let (typ, off) = ((v & 0xff) as u8, (v >> 8) as usize);
        out.push(match typ {
            0 => Inst::None,
            1..=5 => Inst::Single(param(off, typ)?),
            0x10 => {
                let (lo, hi) = (u8at(f, off)? as u8, u8at(f, off + 1)? as u8);
                let mut regions = Vec::new();
                for k in 0..=(hi.saturating_sub(lo)) as usize {
                    regions.push(param(off + 2 + 12 * k + 2, u16at(f, off + 2 + 12 * k)? as u8)?);
                }
                Inst::DrumSet { min: lo, max: hi, regions }
            }
            0x11 => {
                let keys: Vec<u8> = (0..8).map(|k| u8at(f, off + k).map(|v| v as u8)).collect::<Result<_, _>>()?;
                let n = keys.iter().position(|&k| k == 127).map(|p| p + 1).unwrap_or_else(|| keys.iter().filter(|&&k| k != 0).count());
                let mut regions = Vec::new();
                for k in 0..n {
                    regions.push(param(off + 8 + 12 * k + 2, u16at(f, off + 8 + 12 * k)? as u8)?);
                }
                Inst::KeySplit { keys: keys[..n].to_vec(), regions }
            }
            t => Inst::Other(t),
        });
    }
    Ok(out)
}

/// SWAV at `off` (12-byte header + data) -> decoded wave
pub fn decode_swav(f: &[u8], off: usize, size: usize) -> Option<Wave> {
    if size < 12 {
        return None;
    }
    let (fmt, loopflag) = (*f.get(off)?, *f.get(off + 1)?);
    let rate = u16at(f, off + 2).ok()? as u32;
    let (loopstart, looplen) = (u16at(f, off + 6).ok()? as usize, u32at(f, off + 8).ok()? as usize);
    let data = f.get(off + 12..off + size)?;
    let total_words = loopstart + looplen;
    let raw = if total_words != 0 { &data[..(total_words * 4).min(data.len())] } else { data };
    let (pcm, ls, ll) = match fmt {
        0 => (raw.iter().map(|&b| ((b as i8) as i16) << 8).collect::<Vec<_>>(), loopstart * 4, looplen * 4),
        1 => (raw.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect(), loopstart * 2, looplen * 2),
        _ => (adpcm_block(raw), (loopstart * 4).saturating_sub(4) * 2, looplen * 4 * 2),
    };
    let looped = if loopflag != 0 { Some((ls, ls + ll)) } else { None };
    Some(Wave { format: fmt, rate, pcm, looped })
}
