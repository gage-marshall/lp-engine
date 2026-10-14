//! Event backdrops: which BG picture a scene shows.
//!
//! Chain (decoded from the game, see docs/SCENE.md and tools/bg_model.py): the event's id has a record in `arc/tmx_bbin.arc` giving a background id;
//! ids below 10001 pick an archive by range (`BgTransition_DispatchState`), ids from 10001 are file numbers of archive `id / 10000`.
//! `ScheduleText_UpdateState` turns the id into a file number from a 20-byte record (`engine/bg_select.json`) plus the season and the time of day;
//! the file number's position in `arc/bg_file_no_0N.bin` is the picture (entries come as NCGR, NCLR, NSCR).
//! Verified against every backdrop the real game loaded in the recorded play (tools/bg_samples.py).

use crate::nns2d::{bg_panorama, parse_nclr, parse_ncgr, parse_nscr, Image};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug)]
struct Rec {
    base: i64,
    flags: [u8; 3],
    mode: i64,
    param: i64,
}

pub struct BackdropSelect {
    recs: Vec<Rec>,
    pub time_state: Vec<Vec<u32>>,
    pub archives: Vec<String>,
}

/// where a backdrop comes from
#[derive(Clone, Debug, PartialEq)]
pub struct BgChoice {
    pub bg_id: u32,
    pub archive: String,
    /// position in the archive's entry list; the picture is files `3 * entry ..` (NCGR, NCLR, NSCR)
    pub entry: usize,
    pub file_number: u32,
}

impl BackdropSelect {
    pub fn load(content: &Path) -> Result<BackdropSelect, String> {
        let v: Value = serde_json::from_slice(&fs::read(content.join("engine/bg_select.json")).map_err(|e| format!("bg_select.json: {e}"))?).map_err(|e| e.to_string())?;
        let recs = v["records"].as_array().ok_or("records")?.iter().map(|r| Rec {
            base: r["base"].as_i64().unwrap_or(0),
            flags: {
                let f = r["flags"].as_array().cloned().unwrap_or_default();
                [0, 1, 2].map(|i| f.get(i).and_then(|x| x.as_u64()).unwrap_or(0) as u8)
            },
            mode: r["mode"].as_i64().unwrap_or(-1),
            param: r["param"].as_i64().unwrap_or(0),
        }).collect();
        let grid = |k: &str| -> Vec<Vec<u32>> { v[k].as_array().map(|a| a.iter().map(|r| r.as_array().map(|x| x.iter().map(|y| y.as_u64().unwrap_or(0) as u32).collect()).unwrap_or_default()).collect()).unwrap_or_default() };
        Ok(BackdropSelect {
            recs,
            time_state: grid("time_state"),
            archives: v["archives"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
        })
    }

    /// archive number (1 center, 2 town, 3 suburb, 4 sea side, 5 mountain, 6 school, 7 trip, 8 room) of a background id (`BgTransition_DispatchState`)
    pub fn archive_of(id: u32) -> Option<usize> {
        if id >= 10001 {
            return Some((id / 10000) as usize);
        }
        Some(match id {
            0..=0x2e | 0xa7..=0xa9 => 1,
            0x2f..=0x47 => 2,
            0x48..=0x60 => 3,
            0x61..=0x77 | 0xaa => 4,
            0x78..=0x84 => 5,
            0x85..=0xa6 | 0xab..=0xaf => 6,
            0xb0..=0xd4 => 7,
            0xd5..=0xd7 => 8,
            _ => return None,
        })
    }

    fn state_field(cond: bool, param: i64, p3: i64) -> i64 {
        match param {
            2 => if !cond { 30 } else { 0 },
            3 => if cond { 0 } else { ((if p3 == 2 { 1 } else { p3 }) + 1) * 30 },
            4 => ((if p3 == 2 { 1 } else { p3 }) + if !cond { 2 } else { 0 }) * 30,
            6 => (p3 + if !cond { 3 } else { 0 }) * 30,
            _ => 0,
        }
    }

    /// `ScheduleText_UpdateState` for a generic id: season index 0 spring .. 3 winter, tod 0 day / 1 evening / 2 night
    fn generic_file_number(&self, id: usize, s: i64, tod: usize) -> Option<i64> {
        let r = self.recs.get(id)?;
        let (b, f, mode, p) = (r.base, r.flags, r.mode, r.param);
        let any = f.iter().any(|&x| x != 0);
        let sf = |cond: bool, t: i64| Self::state_field(cond, p, t);
        Some(if !any {
            match mode {
                5 => b + sf(s == 1, tod as i64),
                6 => b + sf(s == 2, tod as i64),
                7 => b + sf(s == 0, tod as i64),
                _ => b,
            }
        } else if f[tod] == 0 {
            if tod == 2 && f[1] != 0 {
                match mode {
                    -1 => b + 0x1e,
                    8 => (s * 2 + 1) * 0x1e + b,
                    4 => b + sf(s < 2, 2),
                    5 => b + sf(s == 1, 2),
                    6 => b + sf(s == 2, 2),
                    7 => b + sf(s == 0, 2),
                    _ => b,
                }
            } else {
                b
            }
        } else {
            match mode {
                -1 => if p != 1 && p < 4 { tod as i64 * 0x1e + b } else { b },
                8 => match p {
                    12 => (tod as i64 + s * 3) * 0x1e + b,
                    8 => (tod as i64 + s * 2) * 0x1e + b,
                    4 => s * 0x1e + b,
                    _ => b,
                },
                4 => b + sf(s < 2, tod as i64),
                5 => b + sf(s == 1, tod as i64),
                6 => b + sf(s == 2, tod as i64),
                7 => b + sf(s == 0, tod as i64),
                _ => b,
            }
        })
    }

    /// ids whose picture `ScheduleText_RefreshDateState` fixes without the generic record (calendar-independent ones; weekday/holiday-type 0 assumed)
    fn special(&self, id: u32, s: i64, tod: usize) -> Option<i64> {
        Some(match id {
            0x56 => if s == 1 && tod == 0 { 30991 } else { 31021 },
            0x84 => if tod == 0 { 51081 } else { 51112 },
            0x88 => if tod == 2 { 60210 } else { 60181 },
            0x92 => if s == 1 { 60841 } else if tod == 0 { 60871 } else { 60902 },
            0xa8 => 13871,
            0xa9 => 13961,
            0xcc | 0xcd => if s == 3 { 72581 } else { 72551 },
            _ => return None,
        })
    }

    /// the backdrop for background id `bg_id` at game `month` and time of day `tod` (0 day, 1 evening, 2 night:
    /// `scene_inputs::backdrop_tod`, which follows `ScheduleText_UpdateState`)
    pub fn choose(&self, bg_id: u32, month: u32, tod: u32, list: &dyn Fn(usize) -> Option<Vec<u32>>) -> Option<BgChoice> {
        let archive_no = Self::archive_of(bg_id)?;
        let sea = [2u32, 5, 8, 11].iter().position(|&m| month <= m).unwrap_or(0); // 0 winter, 1 spring, 2 summer, 3 autumn
        let s = [3i64, 0, 1, 2][sea];
        let tod = tod.min(2) as usize;
        let fnum = if bg_id >= 10001 {
            bg_id as i64
        } else if let Some(f) = self.special(bg_id, s, tod) {
            f
        } else {
            self.generic_file_number(bg_id as usize, s, tod)?
        };
        let lst = list(archive_no)?;
        let entry = lst.iter().position(|&v| v as i64 >= fnum)?;
        Some(BgChoice { bg_id, archive: self.archives.get(archive_no - 1)?.clone(), entry, file_number: fnum as u32 })
    }
}

/// event id -> background id from `arc/tmx_bbin.arc` (records of {event id u32, background id u16, 2 flag bytes})
pub fn load_bbin(arc: &Path) -> HashMap<u32, u32> {
    let mut out = HashMap::new();
    let dir = arc.join("tmx_bbin.arc");
    let mut files: Vec<_> = match fs::read_dir(&dir) { Ok(d) => d.filter_map(|e| e.ok()).map(|e| e.path()).collect(), Err(_) => return out };
    files.sort();
    for f in files {
        let b = match fs::read(&f) { Ok(b) => b, Err(_) => continue };
        if b.len() < 4 {
            continue;
        }
        let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
        for i in 0..n {
            let o = 4 + 8 * i;
            if o + 8 > b.len() {
                break;
            }
            let ev = u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]);
            let bg = u16::from_le_bytes([b[o + 4], b[o + 5]]) as u32;
            out.entry(ev).or_insert(bg);
        }
    }
    out
}

/// `arc/bg_file_no_0N.bin`: count, then one file number per entry
pub fn load_list(arc: &Path, archive_no: usize) -> Option<Vec<u32>> {
    let b = fs::read(arc.join(format!("bg_file_no_{archive_no:02}.bin"))).ok()?;
    let n = u32::from_le_bytes([*b.first()?, *b.get(1)?, *b.get(2)?, *b.get(3)?]) as usize;
    Some((0..n).filter_map(|i| b.get(4 + 4 * i..8 + 4 * i).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))).collect())
}

/// decode the picture of a choice: the BG1 panorama (384 x 256, landscape; `nns2d::bg_panorama`)
pub fn load_picture(arc: &Path, c: &BgChoice) -> Result<Image, String> {
    let dir = arc.join(format!("{}.bin", c.archive));
    let mut files: Vec<_> = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    files.sort();
    let get = |k: usize| files.get(3 * c.entry + k).ok_or_else(|| format!("{} has no entry {}", c.archive, 3 * c.entry + k)).and_then(|p| fs::read(p).map_err(|e| e.to_string()));
    let (g, p, s) = (parse_ncgr(&get(0)?)?, parse_nclr(&get(1)?)?, parse_nscr(&get(2)?)?);
    Ok(bg_panorama(&s, &g, Some(&p)))
}

/// place of heroine-agnostic scene families without a `tmx_bbin` record, learned from recorded play (`data/bg_observed.json`, compiled in)
fn observed_place(event_id: u32) -> Option<u32> {
    let v: Value = serde_json::from_str(include_str!("../data/bg_observed.json")).ok()?;
    if event_id >> 24 & 7 != 7 {
        return None;
    }
    v["by_low24"][format!("{:06x}", event_id & 0xff_ffff)].as_u64().map(|x| x as u32)
}

/// the backdrop panorama for an event at a game month and time of day, if the event has a background record
pub fn backdrop_for(content: &Path, event_id: u32, month: u32, tod: u32) -> Option<(BgChoice, Image)> {
    let arc = content.join("assets/arc");
    let sel = BackdropSelect::load(content).ok()?;
    let bg = match load_bbin(&arc).get(&event_id) {
        Some(&b) => b,
        None => observed_place(event_id)?,
    };
    let c = sel.choose(bg, month, tod, &|n| load_list(&arc, n))?;
    let img = load_picture(&arc, &c).ok()?;
    Some((c, img))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archives_follow_the_dispatch_ranges() {
        assert_eq!(BackdropSelect::archive_of(9), Some(1));
        assert_eq!(BackdropSelect::archive_of(141), Some(6));
        assert_eq!(BackdropSelect::archive_of(0xa8), Some(1));
        assert_eq!(BackdropSelect::archive_of(0xaa), Some(4));
        assert_eq!(BackdropSelect::archive_of(20060), Some(2));
    }
}
