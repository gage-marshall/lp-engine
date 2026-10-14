//! The profile screen's handwriting recogniser: the game's own engine (overlay 6's Thumb block 0x216a920..0x217bc20,
//! plus the static division helpers it calls), run by the ARM interpreter (`arm`) from the user's ROM image, with
//! its dictionary `dltjdb.dat`. Nothing of it is reimplemented: this module only sets memory up the way the profile
//! screen's glue does (Ov06ProfileDlt_ProcessProfileDltData 0x217c084, ShutdownProfileDlt 0x217be9c) and makes the
//! same three calls.
//!
//!   GetDltGlobalValue 0x216aa38() -> work buffer size
//!   FlushDltGlyphProcessor 0x216c870(ctx) -> 0;  ctx = {dictionary, work buffer, 500-byte buffer, 500}
//!   RenderDltGlyphBuffer 0x216c8a4(ctx, params, strokes, out u16[12], work[48]) -> 0
//!     params (0x1c bytes): size 0x1c, character set mask 0xf302f, the box 9 / 0x81 / 0x74 / 0xec, 0, 12, 1, 0
//!     strokes: {count, table}; table entries {point count, points}; points (u16 a, u16 b) as RunProfileDlt 0x217bcd0
//!     stores touches: a = y - 9, b = 0x81 - (x - 0x74) (the DS held sideways: a runs left to right, b top to bottom)
//!   The candidates are UCS-2; MeasureProfileDltData maps ASCII 0x21..0x7e to full width.

use crate::arm::Cpu;
use crate::rom::Rom;
use std::path::Path;

const GET_WORK_SIZE: u32 = 0x216aa38 | 1;
const INIT: u32 = 0x216c870 | 1;
const RECOGNISE: u32 = 0x216c8a4 | 1;
const OVERLAY: u32 = 6;
const HEAP: u32 = 0x228_0000;
const STACK: u32 = 0x23f_e000;
const BUDGET: u64 = 2_000_000_000;
pub const CANDIDATES: usize = 12;
/// RunProfileDlt's limits
pub const MAX_POINTS: usize = 0x400;
pub const MAX_STROKES: usize = 0x32;

pub struct Recogniser {
    cpu: Cpu,
    heap: u32,
    ctx: u32,
    params: u32,
    mark: u32,
}

impl Recogniser {
    pub fn open(content: &Path) -> Result<Recogniser, String> {
        let rom = Rom::open(content)?;
        let dict = std::fs::read(content.join("assets/dltjdb.dat")).map_err(|e| format!("dltjdb.dat: {e}"))?;
        Recogniser::new(&rom, &dict)
    }

    pub fn new(rom: &Rom, dict: &[u8]) -> Result<Recogniser, String> {
        let mut cpu = Cpu::new();
        let (base, img) = rom.static_image();
        cpu.write(base, img).map_err(|e| e.to_string())?;
        let (ob, ov) = rom.overlay_image(OVERLAY).ok_or("overlay 6 not extracted")?;
        cpu.write(ob, ov).map_err(|e| e.to_string())?;
        let mut r = Recogniser { cpu, heap: HEAP, ctx: 0, params: 0, mark: 0 };
        let e = |f: crate::arm::Fault| f.to_string();
        let d = r.alloc(dict.len() as u32);
        r.cpu.write(d, dict).map_err(e)?;
        let work_size = r.cpu.call(GET_WORK_SIZE, &[], STACK, BUDGET).map_err(e)?;
        r.ctx = r.alloc(0x10);
        let work = r.alloc(work_size);
        let buf = r.alloc(500);
        for (i, v) in [d, work, buf, 500].into_iter().enumerate() {
            r.cpu.w32(r.ctx + 4 * i as u32, v).map_err(e)?;
        }
        let ok = r.cpu.call(INIT, &[r.ctx], STACK, BUDGET).map_err(e)?;
        if ok != 0 {
            return Err(format!("recogniser init returned {}", ok as i32));
        }
        r.params = r.alloc(0x1c);
        let p = r.params;
        r.cpu.w32(p, 0x1c).map_err(e)?;
        r.cpu.w32(p + 4, 0xf302f).map_err(e)?;
        for (i, v) in [9u32, 0x81, 0x74, 0xec, 0, 0xc].into_iter().enumerate() {
            r.cpu.w16(p + 8 + 2 * i as u32, v).map_err(e)?;
        }
        r.cpu.w8(p + 0x14, 1).map_err(e)?;
        r.cpu.w8(p + 0x15, 0).map_err(e)?;
        r.mark = r.heap;
        Ok(r)
    }

    fn alloc(&mut self, n: u32) -> u32 {
        let a = (self.heap + 31) & !31;
        self.heap = a + n;
        let o = (a - crate::arm::RAM_BASE) as usize;
        self.cpu.bus.0[o..o + n as usize].fill(0);
        a
    }

    /// strokes of (a, b) points as the game stores them -> the 12 candidate codes (0 = none)
    pub fn recognise_raw(&mut self, strokes: &[Vec<(u16, u16)>]) -> Result<[u16; CANDIDATES], String> {
        let e = |f: crate::arm::Fault| f.to_string();
        self.heap = self.mark;
        let npts: usize = strokes.iter().map(|s| s.len()).sum();
        let pts = self.alloc(4 * npts.max(1) as u32);
        let table = self.alloc(8 * strokes.len().max(1) as u32);
        let desc = self.alloc(8);
        let out = self.alloc(2 * CANDIDATES as u32);
        let work = self.alloc(48);
        let mut i = 0u32;
        for (k, s) in strokes.iter().enumerate() {
            self.cpu.w32(table + 8 * k as u32, s.len() as u32).map_err(e)?;
            self.cpu.w32(table + 8 * k as u32 + 4, pts + 4 * i).map_err(e)?;
            for &(a, b) in s {
                self.cpu.w16(pts + 4 * i, a as u32).map_err(e)?;
                self.cpu.w16(pts + 4 * i + 2, b as u32).map_err(e)?;
                i += 1;
            }
        }
        self.cpu.w32(desc, strokes.len() as u32).map_err(e)?;
        self.cpu.w32(desc + 4, table).map_err(e)?;
        let r = self.cpu.call(RECOGNISE, &[self.ctx, self.params, desc, out, work], STACK, BUDGET).map_err(e)?;
        if r != 0 {
            return Err(format!("recognise returned {}", r as i32));
        }
        let mut codes = [0u16; CANDIDATES];
        for (k, c) in codes.iter_mut().enumerate() {
            *c = self.cpu.r16(out + 2 * k as u32).map_err(e)? as u16;
        }
        Ok(codes)
    }

    /// the candidates as the profile screen shows them (MeasureProfileDltData: ASCII to full width)
    pub fn recognise(&mut self, strokes: &[Vec<(u16, u16)>]) -> Result<Vec<Option<char>>, String> {
        Ok(self.recognise_raw(strokes)?.iter().map(|&c| display(c)).collect())
    }

    /// instructions executed so far (for timing)
    pub fn steps(&self) -> u64 {
        self.cpu.steps
    }
}

/// a candidate code as shown (0 = none)
pub fn display(c: u16) -> Option<char> {
    match c {
        0 => None,
        0x21..=0x7e => char::from_u32(c as u32 + 0xfee0),
        _ => char::from_u32(c as u32),
    }
}
