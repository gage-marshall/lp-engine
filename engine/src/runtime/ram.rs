//! The game's memory as its code sees it (the ARM9 memory map): main RAM 0x2000000..0x2400000 with the static image
//! (code and initialised data, content/engine/static.bin) and zeroed bss, the instruction TCM at 0x1ff8000
//! (itcm.bin, mirrored below 0x2000000) and the data TCM at 0x27e0000 (dtcm.bin; the stack lives there); an overlay's
//! image is copied in when it is loaded. I/O registers, palettes, VRAM and OAM are kept sparsely; the hardware divider
//! and square root unit (0x4000280..0x40002bf) compute their results when read.
//!
//! The same memory is the interpreter's bus (`arm::Bus`): addresses marked as traps are functions the engine answers
//! itself (see `runtime::interp`).
use crate::arm::Bus;
use crate::rom::Rom;
use std::collections::{HashMap, HashSet};

pub const MAIN: u32 = 0x2000000;
pub const MAIN_SIZE: u32 = 0x400000;
pub const ITCM: u32 = 0x1ff8000;
pub const ITCM_SIZE: u32 = 0x8000;
pub const DTCM: u32 = 0x27e0000;
pub const DTCM_SIZE: u32 = 0x4000;

/// palettes 0x5000000 (2 KB, mirrored to 0x6000000), VRAM 0x6000000..0x6900000 as the engines address it (BG/OBJ of
/// both engines and the LCDC window), OAM 0x7000000 (2 KB, mirrored); kept flat by address window (bank mapping is
/// the renderer's concern)
pub const VRAM: u32 = 0x6000000;
pub const VRAM_SIZE: u32 = 0x900000;

/// the cartridge bus as the ARM9 drives it directly (ROMCTRL 0x40001a4, command 0x40001a8, data 0x4100010): a
/// transfer started by ROMCTRL's start bit fills a buffer from the card (the virtual ROM); the data port returns it
#[derive(Default)]
pub struct CardBus {
    buf: Vec<u8>,
    pos: usize,
    pub card_id: u32,
    /// the commands the game sent: (command bytes, ROM address)
    pub log: Vec<([u8; 8], u32)>,
}

/// a copy of the memory and the geometry engine (`Ram::snapshot`)
pub struct RamSnapshot {
    main: Vec<u8>,
    dtcm: Vec<u8>,
    itcm: Vec<u8>,
    pal: Vec<u8>,
    oam: Vec<u8>,
    vram: super::vram::Vram,
    other: HashMap<u32, u8>,
    pub gx: super::gx::Gx,
    irq_raised: u32,
}

pub struct Ram {
    /// the geometry engine the GX FIFO and command ports feed
    pub gx: super::gx::Gx,
    /// interrupt request bits raised by the engine's hardware (DMA completion), not yet delivered
    pub irq_raised: u32,
    /// a watched word: interpreted writes to it fault (diagnostics)
    pub watch: Option<u32>,
    pub watch_itcm: bool,
    pub watch_value: Option<u32>,
    /// watched writes to let through before one faults
    pub watch_skip: u32,
    /// the program's entry point when it was loaded as the card holds it (its crt0 is still to run)
    pub boot_entry: Option<u32>,
    /// timer 0 was switched on (the OS tick's origin; taken by the interpreter)
    pub tm0_started: bool,
    /// the geometry FIFO interrupt is armed (GXSTAT bits 30..31)
    pub gx_fifo_irq: bool,
    /// the card ROM (content/fs), read by the SDK edge's card functions and by the card bus
    pub vrom: crate::runtime::sdk::VirtualRom,
    pub card: CardBus,
    main: Vec<u8>,
    pal: Vec<u8>,
    pub vram: super::vram::Vram,
    oam: Vec<u8>,
    itcm: Vec<u8>,
    dtcm: Vec<u8>,
    other: HashMap<u32, u8>,
    /// trapped main-RAM addresses, one bit per halfword
    traps: Vec<u64>,
    traps_other: HashSet<u32>,
}

impl Ram {
    /// a write hits the watched word (and value): faults unless `watch_skip` lets it through
    fn watched(&mut self, a: u32, v: Option<u32>) -> bool {
        if self.watch != Some(a & !3) || matches!((self.watch_value, v), (Some(w), Some(v)) if w != v) {
            return false;
        }
        if self.watch_skip > 0 {
            self.watch_skip -= 1;
            return false;
        }
        true
    }

    pub fn new(rom: &Rom) -> Ram {
        let mut main = vec![0u8; MAIN_SIZE as usize];
        let (base, img) = rom.static_image();
        // the ARM9 program as the card holds it (content/fs/arm9.bin) is loaded where the header says and its own
        // crt0 runs at power-on (Game::power_on); older content without it gets the unpacked image and `crt0` below
        let header = std::fs::read(rom.content_dir().join("fs/header.bin")).unwrap_or_default();
        let raw = std::fs::read(rom.content_dir().join("fs/arm9.bin")).ok().filter(|_| header.len() >= 0x30);
        let h = |o: usize| u32::from_le_bytes(header[o..o + 4].try_into().unwrap());
        let boot_entry = raw.as_ref().map(|_| h(0x24));
        let (load, image): (u32, &[u8]) = match &raw {
            Some(r) => (h(0x28), r.as_slice()),
            None => (base, img),
        };
        let o = (load - MAIN) as usize;
        main[o..o + image.len()].copy_from_slice(image);
        let mut r = Ram { gx: Default::default(), watch: None, watch_itcm: false, watch_value: None, watch_skip: 0, tm0_started: false, boot_entry: None, irq_raised: 0, gx_fifo_irq: false, vrom: Default::default(), card: CardBus::default(), main, pal: vec![0; 0x800], vram: Default::default(), oam: vec![0; 0x800], itcm: vec![0; ITCM_SIZE as usize], dtcm: vec![0; DTCM_SIZE as usize], other: HashMap::new(), traps: vec![0; (MAIN_SIZE / 2 / 64) as usize], traps_other: HashSet::new() };
        r.boot_entry = boot_entry;
        if boot_entry.is_none() {
            r.crt0(base, img);
        }
        r
    }

    /// what the startup code does before main: the autoload blocks (ITCM, DTCM) copied to their places with their bss
    /// cleared, then the static bss cleared (which reuses the autoload blocks' source area). The layout comes from
    /// the image's module parameters (found by their 0xdec00621 marker): autoload list, autoload start, bss bounds.
    fn crt0(&mut self, base: u32, img: &[u8]) {
        let w = |o: usize| u32::from_le_bytes(img[o..o + 4].try_into().unwrap());
        let Some(k) = img.windows(4).position(|x| x == 0xdec00621u32.to_le_bytes()) else {
            eprintln!("ram: no module parameters in the static image");
            return;
        };
        let (list, list_end, mut src) = (w(k - 0x1c), w(k - 0x18), w(k - 0x14));
        let (bss_start, bss_end) = (w(k - 0x10), w(k - 0xc));
        let mut e = list;
        while e + 12 <= list_end {
            let o = (e - base) as usize;
            let (dst, size, bss) = (w(o), w(o + 4), w(o + 8));
            let data = img[(src - base) as usize..(src - base + size) as usize].to_vec();
            self.put(dst, &data);
            self.fill(dst + size, 0, bss);
            src += size;
            e += 12;
        }
        self.fill(bss_start, 0, bss_end - bss_start);
    }

    // ---- traps -------------------------------------------------------------------------------------------------
    pub fn set_trap(&mut self, a: u32) {
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE {
            let i = (o / 2) as usize;
            self.traps[i / 64] |= 1 << (i % 64);
        } else {
            self.traps_other.insert(a & !1);
        }
    }
    pub fn unset_trap(&mut self, a: u32) {
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE {
            let i = (o / 2) as usize;
            self.traps[i / 64] &= !(1 << (i % 64));
        } else {
            self.traps_other.remove(&(a & !1));
        }
    }
    pub fn clear_traps(&mut self) {
        self.traps.iter_mut().for_each(|w| *w = 0);
        self.traps_other.clear();
    }

    // ---- byte access across the map -------------------------------------------------------------------------------
    #[inline]
    fn locate(&self, a: u32) -> Option<(u8, usize)> {
        let o = a.wrapping_sub(DTCM);
        if o < DTCM_SIZE {
            return Some((1, o as usize));
        }
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE {
            return Some((0, o as usize));
        }
        if o < 0x1000000 {
            // main RAM is mirrored up to 0x2ffffff (the firmware's system area at 0x27ff000.. is its last 4 KB)
            return Some((0, (o & (MAIN_SIZE - 1)) as usize));
        }
        if a < MAIN {
            return Some((2, (a.wrapping_sub(ITCM) & (ITCM_SIZE - 1)) as usize));
        }
        match a >> 24 {
            5 => Some((3, (a & 0x7ff) as usize)),
            6 if a.wrapping_sub(VRAM) < VRAM_SIZE => Some((4, (a - VRAM) as usize)),
            7 => Some((5, (a & 0x7ff) as usize)),
            _ => None,
        }
    }

    pub fn u8(&self, a: u32) -> u8 {
        match self.locate(a) {
            Some((0, o)) => self.main.get(o).copied().unwrap_or(0),
            Some((1, o)) => self.dtcm.get(o).copied().unwrap_or(0),
            Some((2, o)) => self.itcm.get(o).copied().unwrap_or(0),
            Some((3, o)) => self.pal[o],
            Some((4, o)) => self.vram.read(VRAM + o as u32),
            Some((_, o)) => self.oam[o],
            None => self.other.get(&a).copied().unwrap_or(0),
        }
    }
    pub fn set_u8(&mut self, a: u32, v: u8) {
        if a < MAIN && (a.wrapping_sub(ITCM) & (ITCM_SIZE - 1)) & !3 == 0x120 && self.watch_itcm {
            eprintln!("write {v:#x} to ITCM 0x120 via {a:#x}:\n{}", std::backtrace::Backtrace::force_capture());
            std::process::exit(3);
        }
        if self.watch == Some(a & !3) && self.watch_value.is_none() && std::env::var("LP_WATCH_NATIVE").is_ok() {
            eprintln!("native write of 0 to watched {a:#x}:\n{}", std::backtrace::Backtrace::force_capture());
            std::process::exit(3);
        }
        match self.locate(a) {
            Some((0, o)) => self.main[o] = v,
            Some((1, o)) => self.dtcm[o] = v,
            Some((2, o)) => self.itcm[o] = v,
            Some((3, o)) => self.pal[o] = v,
            Some((4, o)) => self.vram.write(VRAM + o as u32, v),
            Some((_, o)) => self.oam[o] = v,
            None => {
                self.other.insert(a, v);
                if (0x4000240..0x400024a).contains(&a) && a != 0x4000247 {
                    let bank = if a < 0x4000247 { a - 0x4000240 } else { a - 0x4000241 };
                    self.vram.set_cnt(bank as usize, v);
                }
            }
        }
    }
    /// palette RAM, VRAM and OAM as the renderer reads them
    pub fn palettes(&self) -> &[u8] {
        &self.pal
    }
    pub fn vram(&self) -> &super::vram::Vram {
        &self.vram
    }
    pub fn oam(&self) -> &[u8] {
        &self.oam
    }
    pub fn read(&self, a: u32, size: u32) -> u32 {
        if (0x4000280..0x40002c0).contains(&a) {
            return math_read(self, a, size);
        }
        if (0x4000620..0x40006a4).contains(&a) {
            if let Some(v) = self.gx.read(a & !3) {
                return (v >> (8 * (a & 3))) & if size >= 4 { u32::MAX } else { (1 << (8 * size)) - 1 };
            }
        }
        if (0x4000600..0x4000604).contains(&a) {
            // GXSTAT: the engine's renderer consumes geometry commands at once: FIFO empty and below half, idle;
            // bit 1 the last box test (1: inside), bit 15 a matrix stack error
            let v = (self.u8(0x4000603) as u32 & 0xc0) << 24 | 0x0600_0000 | (self.gx.box_result as u32) << 1 | (self.gx.stack_error as u32) << 15;
            return (v >> (8 * (a - 0x4000600))) & if size >= 4 { u32::MAX } else { (1 << (8 * size)) - 1 };
        }
        if a == 0x40001a4 || a == 0x40001a6 || a == 0x40001a7 {
            // ROMCTRL: data ready (bit 23) while the transfer has data, busy (bit 31) until it is read out
            let busy = self.card.pos < self.card.buf.len();
            let v = (self.u8(0x40001a4) as u32 | (self.u8(0x40001a5) as u32) << 8 | (self.u8(0x40001a6) as u32 & 0x7f) << 16 | (self.u8(0x40001a7) as u32 & 0x7f) << 24)
                | if busy { 0x8080_0000 } else { 0 };
            return (v >> (8 * (a - 0x40001a4))) & if size >= 4 { u32::MAX } else { (1 << (8 * size)) - 1 };
        }
        (0..size).fold(0, |v, i| v | (self.u8(a.wrapping_add(i)) as u32) << (8 * i))
    }
    pub fn write(&mut self, a: u32, v: u32, size: u32) {
        if (0x4000400..0x4000600).contains(&a) && size == 4 {
            if a < 0x4000440 {
                self.gx.fifo(v);
            } else {
                self.gx.port(((a - 0x4000400) / 4) as u8, v);
            }
            return;
        }
        for i in 0..size {
            self.set_u8(a.wrapping_add(i), (v >> (8 * i)) as u8);
        }
        if (0x40000b0..0x40000e0).contains(&a) {
            self.dma_check(a, size);
        }
        if (0x4000600..0x4000604).contains(&a) && a + size > 0x4000603 {
            // GXSTAT bits 30..31 arm the geometry FIFO interrupt (below half / empty): with the engine's FIFO always
            // empty, the (level-triggered) interrupt is asserted while armed
            self.gx_fifo_irq = (self.u8(0x4000603) >> 6) != 0;
        }
        if (0x4000100..0x4000104).contains(&a) && a + size > 0x4000102 && self.u8(0x4000102) & 0x80 != 0 {
            // timer 0 started (OS_InitTick: prescaler /64, the OS tick): the machine takes the time as its origin
            self.tm0_started = true;
        }
        if (0x40001a4..0x40001a8).contains(&a) && a + size > 0x40001a7 && v >> (8 * (0x40001a7 - a)) & 0x80 != 0 {
            self.card_start();
        }
    }

    /// the data port (0x4100010): the next word of the transfer
    pub fn card_data(&mut self) -> u32 {
        let c = &mut self.card;
        let mut w = [0u8; 4];
        for b in w.iter_mut() {
            *b = c.buf.get(c.pos).copied().unwrap_or(0xff);
            c.pos += 1;
        }
        u32::from_le_bytes(w)
    }

    /// ROMCTRL written with its start bit: the command in 0x40001a8.. runs. B7 (data read) returns 0x200-byte blocks
    /// from the card (block size from ROMCTRL bits 24..26), B8 the chip ID; others return 0xff
    fn card_start(&mut self) {
        let mut cmd = [0u8; 8];
        for (i, b) in cmd.iter_mut().enumerate() {
            *b = self.u8(0x40001a8 + i as u32);
        }
        let ctl = self.u8(0x40001a7) as u32;
        let bs = (ctl & 7) as u32;
        let len = match bs {
            0 => 0,
            7 => 4,
            n => 0x100 << n,
        };
        let addr = u32::from_be_bytes([cmd[1], cmd[2], cmd[3], cmd[4]]);
        self.card.log.push((cmd, addr));
        self.card.buf = match cmd[0] {
            0xb7 => self.vrom.read(addr, len),
            0xb8 => (0..len / 4).flat_map(|_| self.card.card_id.to_le_bytes()).collect(),
            _ => vec![0xff; len as usize],
        };
        self.card.pos = 0;
    }

    /// DMA (channels 0..3: source 0x40000b0 + 12k, destination +4, control +8): a channel whose control word is
    /// written with the enable bit transfers at once (every start timing: the engine has no display timing to wait
    /// for; GX FIFO transfers feed the 3D engine, which the engine's renderer replaces) and clears its enable bit
    fn dma_check(&mut self, a: u32, size: u32) {
        let k = (a - 0x40000b0) / 12;
        let base = 0x40000b0 + 12 * k;
        if a + size <= base + 8 {
            return;
        }
        let cnt = self.u32(base + 8);
        if cnt & 0x8000_0000 == 0 {
            return;
        }
        let (mut src, mut dst) = (self.u32(base), self.u32(base + 4));
        let unit = if cnt & (1 << 26) != 0 { 4 } else { 2 };
        let mut n = cnt & 0x1fffff;
        if n == 0 {
            n = 0x200000;
        }
        let (dctl, sctl) = ((cnt >> 21) & 3, (cnt >> 23) & 3);
        let timing = (cnt >> 27) & 7;
        for _ in 0..n {
            let v = self.read(src, unit);
            if timing == 7 {
                self.gx.fifo(v); // the geometry FIFO
            } else if unit == 4 && (0x4000400..0x4000600).contains(&dst) {
                self.write(dst, v, 4); // into the FIFO / a command port
            } else {
                for i in 0..unit {
                    self.set_u8(dst.wrapping_add(i), (v >> (8 * i)) as u8);
                }
            }
            src = match sctl {
                0 => src.wrapping_add(unit),
                1 => src.wrapping_sub(unit),
                _ => src,
            };
            dst = match dctl {
                0 | 3 => dst.wrapping_add(unit),
                1 => dst.wrapping_sub(unit),
                _ => dst,
            };
        }
        let c = self.u32(base + 8) & 0x7fff_ffff;
        for i in 0..4 {
            self.set_u8(base + 8 + i, (c >> (8 * i)) as u8);
        }
        if cnt & (1 << 30) != 0 {
            self.irq_raised |= 1 << (8 + k); // the channel's completion interrupt
        }
    }
    /// the whole memory and the geometry engine, to run something twice from the same state (`restore`)
    pub fn snapshot(&self) -> RamSnapshot {
        RamSnapshot { main: self.main.clone(), dtcm: self.dtcm.clone(), itcm: self.itcm.clone(), pal: self.pal.clone(), oam: self.oam.clone(), vram: self.vram.clone(), other: self.other.clone(), gx: self.gx.clone(), irq_raised: self.irq_raised }
    }
    pub fn restore(&mut self, s: &RamSnapshot) {
        self.main.copy_from_slice(&s.main);
        self.dtcm.copy_from_slice(&s.dtcm);
        self.itcm.copy_from_slice(&s.itcm);
        self.pal.copy_from_slice(&s.pal);
        self.oam.copy_from_slice(&s.oam);
        self.vram = s.vram.clone();
        self.other = s.other.clone();
        self.gx = s.gx.clone();
        self.irq_raised = s.irq_raised;
    }
    /// where the memory differs from a snapshot: (region, first differing address, bytes differing); `skip` is an
    /// address range left out (a function's stack frame)
    pub fn differences(&self, s: &RamSnapshot, skip: std::ops::Range<u32>) -> Vec<(&'static str, u32, usize)> {
        let mut out = Vec::new();
        for (name, base, a, b) in [("main", MAIN, &self.main, &s.main), ("dtcm", DTCM, &self.dtcm, &s.dtcm), ("itcm", ITCM, &self.itcm, &s.itcm), ("pal", 0x5000000, &self.pal, &s.pal), ("oam", 0x7000000, &self.oam, &s.oam)] {
            let d: Vec<u32> = a.iter().zip(b.iter()).enumerate().filter(|(i, (x, y))| x != y && !skip.contains(&(base + *i as u32))).map(|(i, _)| base + i as u32).collect();
            if let Some(&first) = d.first() {
                out.push((name, first, d.len()));
            }
        }
        for k in 0..self.vram.banks.len() {
            let n = self.vram.banks[k].iter().zip(&s.vram.banks[k]).filter(|(x, y)| x != y).count();
            if n > 0 {
                out.push(("vram", k as u32, n));
            }
        }
        let mut io: Vec<u32> = self.other.keys().chain(s.other.keys()).copied().filter(|a| self.other.get(a) != s.other.get(a)).collect();
        io.sort();
        if let Some(&first) = io.first() {
            out.push(("io", first, io.len()));
        }
        out
    }

    /// what the display reads (I/O registers, palettes, OAM, VRAM, the 3D engine's frame), for drawing elsewhere:
    /// main RAM, the card and the diagnostics are left out (main RAM reads as 0)
    pub fn display_copy(&self) -> Ram {
        Ram {
            gx: self.gx.clone(),
            watch: None,
            watch_itcm: false,
            watch_value: None,
            watch_skip: 0,
            tm0_started: self.tm0_started,
            boot_entry: None,
            irq_raised: 0,
            gx_fifo_irq: false,
            vrom: Default::default(),
            card: CardBus::default(),
            main: Vec::new(),
            pal: self.pal.clone(),
            vram: self.vram.clone(),
            oam: self.oam.clone(),
            itcm: Vec::new(),
            dtcm: Vec::new(),
            other: self.other.clone(),
            traps: Vec::new(),
            traps_other: HashSet::new(),
        }
    }
    pub fn u16(&self, a: u32) -> u16 {
        self.read(a, 2) as u16
    }
    pub fn u32(&self, a: u32) -> u32 {
        self.read(a, 4)
    }
    pub fn i32(&self, a: u32) -> i32 {
        self.read(a, 4) as i32
    }
    pub fn set_u16(&mut self, a: u32, v: u16) {
        self.write(a, v as u32, 2)
    }
    pub fn set_u32(&mut self, a: u32, v: u32) {
        self.write(a, v, 4)
    }
    pub fn fill(&mut self, a: u32, v: u8, n: u32) {
        for i in 0..n {
            self.set_u8(a + i, v);
        }
    }
    /// a span of main RAM as a slice (None when it leaves main RAM)
    pub fn main_slice(&self, a: u32, n: usize) -> Option<&[u8]> {
        let o = a.checked_sub(MAIN)? as usize;
        self.main.get(o..o.checked_add(n)?)
    }
    pub fn bytes(&self, a: u32, n: usize) -> Vec<u8> {
        (0..n as u32).map(|i| self.u8(a + i)).collect()
    }
    pub fn put(&mut self, a: u32, d: &[u8]) {
        match self.locate(a) {
            Some((0, o)) if o + d.len() <= self.main.len() => self.main[o..o + d.len()].copy_from_slice(d),
            _ => {
                for (i, &b) in d.iter().enumerate() {
                    self.set_u8(a + i as u32, b);
                }
            }
        }
    }
    /// a NUL-terminated string at `a`
    pub fn cstr(&self, a: u32) -> Vec<u8> {
        let mut v = Vec::new();
        let mut p = a;
        loop {
            let b = self.u8(p);
            if b == 0 || v.len() > 4096 {
                return v;
            }
            v.push(b);
            p += 1;
        }
    }
}

/// the divider (DIVCNT 0x280, numerator 0x290, denominator 0x298, quotient 0x2a0, remainder 0x2a8) and the square
/// root unit (SQRTCNT 0x2b0, result 0x2b4, parameter 0x2b8)
fn math_read(r: &Ram, a: u32, size: u32) -> u32 {
    let raw = |a: u32, n: u32| (0..n).fold(0u64, |v, i| v | (r.u8(a + i) as u64) << (8 * i));
    let mut out = [0u8; 0x40];
    for i in 0..0x40 {
        out[i] = r.u8(0x4000280 + i as u32);
    }
    let cnt = raw(0x4000280, 4) as u32;
    let (num, den) = (raw(0x4000290, 8), raw(0x4000298, 8));
    let (q, rem): (i64, i64) = match cnt & 3 {
        0 => {
            let (n, d) = (num as u32 as i32, den as u32 as i32);
            if d == 0 {
                ((if n < 0 { 1 } else { -1 }) as i64, n as i64)
            } else if n == i32::MIN && d == -1 {
                (n as i64 ^ ((-1i64) << 32), 0)
            } else {
                ((n / d) as i64, (n % d) as i64)
            }
        }
        1 | 3 => {
            let (n, d) = (num as i64, den as u32 as i32 as i64);
            if d == 0 { (if n < 0 { 1 } else { -1 }, n) } else { (n.wrapping_div(d), n.wrapping_rem(d)) }
        }
        _ => {
            let (n, d) = (num as i64, den as i64);
            if d == 0 { (if n < 0 { 1 } else { -1 }, n) } else { (n.wrapping_div(d), n.wrapping_rem(d)) }
        }
    };
    out[0x20..0x28].copy_from_slice(&q.to_le_bytes());
    out[0x28..0x30].copy_from_slice(&rem.to_le_bytes());
    let scnt = raw(0x40002b0, 4) as u32;
    let p = raw(0x40002b8, 8);
    let s = if scnt & 1 != 0 { isqrt(p) } else { isqrt(p & 0xffff_ffff) } as u32;
    out[0x34..0x38].copy_from_slice(&s.to_le_bytes());
    out[0] &= !0x80; // never busy
    out[0x30] &= !0x80;
    let o = (a - 0x4000280) as usize;
    (0..size as usize).fold(0, |v, i| v | (out[(o + i) & 0x3f] as u32) << (8 * i))
}

fn isqrt(v: u64) -> u64 {
    if v == 0 {
        return 0;
    }
    let mut x = (v as f64).sqrt() as u64;
    while x * x > v {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= v {
        x += 1;
    }
    x
}

impl Bus for Ram {
    #[inline]
    fn r8(&mut self, a: u32) -> Option<u32> {
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE && a.wrapping_sub(DTCM) >= DTCM_SIZE {
            return Some(self.main[o as usize] as u32);
        }
        Some(self.u8(a) as u32)
    }
    #[inline]
    fn r16(&mut self, a: u32) -> Option<u32> {
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE - 1 && a.wrapping_sub(DTCM) >= DTCM_SIZE {
            let o = o as usize;
            return Some(u16::from_le_bytes([self.main[o], self.main[o + 1]]) as u32);
        }
        Some(self.read(a, 2))
    }
    #[inline]
    fn r32(&mut self, a: u32) -> Option<u32> {
        if a == 0x4100010 {
            return Some(self.card_data());
        }
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE - 3 && a.wrapping_sub(DTCM) >= DTCM_SIZE {
            let o = o as usize;
            return Some(u32::from_le_bytes(self.main[o..o + 4].try_into().unwrap()));
        }
        let o = a.wrapping_sub(DTCM);
        if o < DTCM_SIZE - 3 {
            let o = o as usize;
            return Some(u32::from_le_bytes(self.dtcm[o..o + 4].try_into().unwrap()));
        }
        Some(self.read(a, 4))
    }
    #[inline]
    fn w8(&mut self, a: u32, v: u32) -> Option<()> {
        if a < 0x1000000 || self.watched(a, None) {
            return None; // a write through a null pointer (ITCM's low mirror): reported, never done
        }
        self.set_u8(a, v as u8);
        Some(())
    }
    #[inline]
    fn w16(&mut self, a: u32, v: u32) -> Option<()> {
        if a < 0x1000000 || self.watched(a, None) {
            return None;
        }
        self.write(a, v, 2);
        Some(())
    }
    #[inline]
    fn w32(&mut self, a: u32, v: u32) -> Option<()> {
        if self.watched(a, Some(v)) || (a < MAIN && (a.wrapping_sub(ITCM) & (ITCM_SIZE - 1)) & !3 == 0x120 && self.watch_itcm) {
            return None;
        }
        let o = a.wrapping_sub(MAIN);
        if o < MAIN_SIZE - 3 && a.wrapping_sub(DTCM) >= DTCM_SIZE {
            let o = o as usize;
            self.main[o..o + 4].copy_from_slice(&v.to_le_bytes());
            return Some(());
        }
        let o = a.wrapping_sub(DTCM);
        if o < DTCM_SIZE - 3 {
            let o = o as usize;
            self.dtcm[o..o + 4].copy_from_slice(&v.to_le_bytes());
            return Some(());
        }
        if a < 0x1000000 {
            return None;
        }
        self.write(a, v, 4);
        Some(())
    }
    #[inline]
    fn irq_pending(&self) -> bool {
        self.irq_raised != 0 || (self.gx_fifo_irq && self.u32(0x4000210) & (1 << 21) != 0)
    }
    #[inline]
    fn is_trap(&self, pc: u32) -> bool {
        let o = pc.wrapping_sub(MAIN);
        if o < MAIN_SIZE {
            let i = (o / 2) as usize;
            return self.traps[i / 64] >> (i % 64) & 1 != 0;
        }
        !self.traps_other.is_empty() && self.traps_other.contains(&pc)
    }
}
