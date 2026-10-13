//! SBIN script container and bytecode VM (docs/SCRIPT_VM.md; reference: tools/lpvm.py).
//!
//! The VM is a pull-style state machine: `Run::step` executes until the script needs the host
//! (a game command, a text box, a fade wait) or ends.

use std::collections::HashMap;

/// TXRC/SBIN stream cipher: `plain[i] = c[i] ^ ((mult*(i/3) + key[i%3]) & 0xff)` for i >= 8.
pub fn decrypt(raw: &[u8]) -> Vec<u8> {
    let mut out = raw.to_vec();
    if raw.len() < 8 {
        return out;
    }
    let m = raw[7] as u32;
    let key = [raw[4] as u32, raw[5] as u32, raw[6] as u32];
    for i in 8..raw.len() {
        let k = (m.wrapping_mul((i / 3) as u32).wrapping_add(key[i % 3])) & 0xff;
        out[i] = raw[i] ^ k as u8;
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Seg {
    Lit(String),
    Var(u16),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TextBox {
    pub text: Vec<Seg>,
    pub sub: Vec<Seg>,
    pub name: Vec<Seg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Run game command `id`; the host may answer through `step(Some(value))`, which sets the accumulator.
    Cmd { id: u32, args: Vec<u32> },
    /// Text box commit (op 0x83 has no mode, 0x84 carries one).
    Text { boxed: TextBox, mode: Option<u16> },
    WaitFade,
    End(u32),
    Limit(usize),
    BadOp { op: u8, pc: usize },
    Fault(usize),
}

pub struct Script {
    pub data: Vec<u8>,
    pub entries: Vec<u32>,
    str_base: usize,
    ids: HashMap<u32, u32>,
    local_descriptors: Vec<u32>,
}

fn be32(d: &[u8], p: usize) -> Option<u32> {
    d.get(p..p + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
}

impl Script {
    pub fn from_encrypted(raw: &[u8]) -> Option<Script> {
        Self::from_decrypted(decrypt(raw))
    }

    pub fn from_decrypted(data: Vec<u8>) -> Option<Script> {
        if data.len() < 0x40 {
            return None;
        }
        let h: Vec<u32> = (0..11).map(|i| be32(&data, 8 + 4 * i).unwrap()).collect();
        let n = h[0] as usize;
        let mut entries = Vec::with_capacity(n);
        for k in 0..n {
            entries.push(be32(&data, 0x40 + 4 * k)?);
        }
        let mut ids = HashMap::new();
        let (off, cnt) = (h[9] as usize, h[10] as usize);
        for k in 0..cnt {
            if off != 0 && off + 8 * k + 8 <= data.len() {
                let id = be32(&data, off + 8 * k).unwrap();
                let r = be32(&data, off + 8 * k + 4).unwrap();
                ids.insert(id, r);
            }
        }
        let (loff, lcnt) = (h[6] as usize, h[8] as usize);
        let mut local_descriptors = Vec::new();
        if lcnt != 0 && loff + 4 * lcnt <= data.len() {
            for k in 0..lcnt {
                local_descriptors.push(be32(&data, loff + 4 * k).unwrap());
            }
        }
        Some(Script { data, entries, str_base: h[1] as usize, ids, local_descriptors })
    }

    /// Length-prefixed UTF-8 string from the string table.
    pub fn string(&self, off: u32) -> Option<String> {
        let p = self.str_base + (off & 0x7fff_ffff) as usize;
        let b0 = *self.data.get(p)? as usize;
        let (n, q) = if b0 & 0x80 != 0 {
            let b1 = *self.data.get(p + 1)? as usize;
            (b1 + 0x80, p + 2)
        } else {
            (b0, p + 1)
        };
        let bytes = self.data.get(q..q + n).unwrap_or(&self.data[q.min(self.data.len())..]);
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    pub fn run(&self, entry: usize, max_steps: usize) -> Run<'_> {
        Run {
            s: self,
            pc: self.entries.get(entry).copied().unwrap_or(0) as usize,
            acc: 0,
            flag: false,
            stack: Vec::new(),
            frames: Vec::new(),
            regs: HashMap::new(),
            gvars: HashMap::new(),
            local_memory: vec![0; 0x400],
            bufs: [Vec::new(), Vec::new(), Vec::new()],
            steps: 0,
            max_steps,
            done: false,
        }
    }
}

pub struct Run<'a> {
    s: &'a Script,
    pub pc: usize,
    pub acc: u32,
    flag: bool,
    stack: Vec<u32>,
    frames: Vec<usize>,
    regs: HashMap<u16, u32>,
    gvars: HashMap<u16, u32>,
    local_memory: Vec<u8>,
    bufs: [Vec<Seg>; 3],
    steps: usize,
    max_steps: usize,
    done: bool,
}

fn read_cell(mem: &[u8], desc: u32) -> u32 {
    let kind = (desc >> 28) & 0xf;
    let w = ((desc >> 24) & 0xf) as usize;
    let off = (desc & 0xff_ffff) as usize;
    if kind == 4 && matches!(w, 1 | 2 | 4) && off + w <= mem.len() {
        let mut v = 0u32;
        for i in (0..w).rev() {
            v = (v << 8) | mem[off + i] as u32;
        }
        v
    } else if kind == 8 && off < mem.len() {
        ((mem[off] >> w) & 1) as u32
    } else {
        0
    }
}

fn write_cell(mem: &mut Vec<u8>, desc: u32, value: u32) {
    let kind = (desc >> 28) & 0xf;
    let w = ((desc >> 24) & 0xf) as usize;
    let off = (desc & 0xff_ffff) as usize;
    if kind == 4 && matches!(w, 1 | 2 | 4) {
        if off + w > mem.len() {
            mem.resize(off + w, 0);
        }
        for i in 0..w {
            mem[off + i] = (value >> (8 * i)) as u8;
        }
    } else if kind == 8 {
        if off >= mem.len() {
            mem.resize(off + 1, 0);
        }
        let mask = 1u8 << (w & 7);
        if value != 0 {
            mem[off] |= mask;
        } else {
            mem[off] &= !mask;
        }
    }
}

impl<'a> Run<'a> {
    /// Continue after the previous event. `reply` answers a `Cmd` (written to the accumulator).
    pub fn step(&mut self, reply: Option<u32>) -> Event {
        if self.done {
            return Event::End(self.acc);
        }
        if let Some(r) = reply {
            self.acc = r;
        }
        let d = &self.s.data;
        loop {
            self.steps += 1;
            if self.steps > self.max_steps {
                self.done = true;
                return Event::Limit(self.pc);
            }
            let pc = self.pc;
            let (op, n) = match (d.get(pc), d.get(pc + 1)) {
                (Some(&o), Some(&n)) => (o, n as usize),
                _ => {
                    self.done = true;
                    return Event::Fault(pc);
                }
            };
            if pc + 2 + 2 * n > d.len() {
                self.done = true;
                return Event::Fault(pc);
            }
            let a: Vec<u32> = (0..n).map(|i| u16::from_be_bytes([d[pc + 2 + 2 * i], d[pc + 3 + 2 * i]]) as u32).collect();
            let arg = |i: usize| a.get(i).copied();
            let mut nxt = pc + 2 + 2 * n;
            macro_rules! need {
                ($i:expr) => {
                    match arg($i) {
                        Some(v) => v,
                        None => {
                            self.done = true;
                            return Event::Fault(pc);
                        }
                    }
                };
            }
            let rel = |v: u32| -> usize { (nxt as i64 + (v as u16 as i16) as i64) as usize };
            match op {
                0x00 => {}
                0x01 => self.stack.push(self.acc),
                0x02 => self.acc = self.stack.pop().unwrap_or(0),
                0x03 => {
                    for _ in 0..need!(0) {
                        self.stack.pop();
                    }
                }
                0x10 => nxt = rel(need!(0)),
                0x11 => {
                    if self.flag {
                        nxt = rel(need!(0));
                    }
                }
                0x12 => {
                    if !self.flag {
                        nxt = rel(need!(0));
                    }
                }
                0x13 | 0x14 => {
                    if op == 0x14 {
                        let key = (need!(0) << 16) | need!(1);
                        self.acc = self.s.ids.get(&key).copied().unwrap_or(0);
                    }
                    match self.frames.pop() {
                        None => {
                            self.done = true;
                            return Event::End(self.acc);
                        }
                        Some(f) => nxt = f,
                    }
                }
                0x16 => {
                    let key = (need!(0) << 16) | need!(1);
                    let r = self.s.ids.get(&key).copied().unwrap_or(0);
                    if r != 0 {
                        if let Some(&t) = self.s.entries.get((r & 0xfff) as usize) {
                            self.frames.push(nxt);
                            nxt = t as usize;
                        }
                    }
                }
                0x20 => self.flag = self.acc != 0,
                0x21 => self.flag = self.stack.last().map_or(false, |&t| t == self.acc),
                0x30 => self.acc = 0,
                0x31 => self.acc = 1,
                0x32 => self.acc = need!(0),
                0x33 => self.acc = (need!(0) << 16) | need!(1),
                0x34 => self.acc = (self.acc != 0) as u32,
                0x35 => self.acc = self.acc.wrapping_neg(),
                0x40 => self.acc = self.regs.get(&(need!(0) as u16)).copied().unwrap_or(0),
                0x41 => self.acc = self.gvars.get(&(need!(0) as u16)).copied().unwrap_or(0),
                0x42 => {
                    let i = need!(0) as usize;
                    self.acc = self.s.local_descriptors.get(i).map_or(0, |&ds| read_cell(&self.local_memory, ds));
                }
                0x43 => {
                    self.regs.insert(need!(0) as u16, self.acc);
                }
                0x44 => {
                    self.gvars.insert(need!(0) as u16, self.acc);
                }
                0x45 => {
                    let i = need!(0) as usize;
                    if let Some(&ds) = self.s.local_descriptors.get(i) {
                        write_cell(&mut self.local_memory, ds, self.acc);
                    }
                }
                0x50..=0x5c => {
                    let b = self.stack.pop().unwrap_or(0);
                    let x = self.stack.pop().unwrap_or(0);
                    self.acc = match op {
                        0x50 => x.wrapping_add(b),
                        0x51 => x.wrapping_sub(b),
                        0x52 => x.wrapping_mul(b),
                        0x53 => {
                            if b == 0 {
                                0
                            } else {
                                (x as i32 as i64 / b as i32 as i64) as u32
                            }
                        }
                        0x54 => x & b,
                        0x55 => x | b,
                        0x56 => x ^ b,
                        0x57 => (x == b) as u32,
                        0x58 => (x != b) as u32,
                        0x59 => (x < b) as u32,
                        0x5a => (x <= b) as u32,
                        0x5b => (x > b) as u32,
                        _ => (x >= b) as u32,
                    };
                }
                0x80 => {
                    let b = need!(0) as usize;
                    if b >= 3 {
                        self.done = true;
                        return Event::Fault(pc);
                    }
                    self.bufs[b].clear();
                }
                0x81 | 0x82 => {
                    let b = need!(0) as usize;
                    if b >= 3 {
                        self.done = true;
                        return Event::Fault(pc);
                    }
                    if op == 0x81 {
                        let off = (need!(1) << 16) | need!(2);
                        match self.s.string(off) {
                            Some(s) => self.bufs[b].push(Seg::Lit(s)),
                            None => {
                                self.done = true;
                                return Event::Fault(pc);
                            }
                        }
                    } else {
                        self.bufs[b].push(Seg::Var(need!(1) as u16));
                    }
                }
                0x83 | 0x84 => {
                    let mode = if op == 0x84 { Some(need!(0) as u16) } else { None };
                    let boxed = TextBox { text: self.bufs[0].clone(), sub: self.bufs[1].clone(), name: self.bufs[2].clone() };
                    self.pc = nxt;
                    return Event::Text { boxed, mode };
                }
                0x85 | 0x86 | 0x5d..=0x7f | 0x04 => {}
                0x87 => {
                    let k = self.stack.last().copied().unwrap_or(0) as usize;
                    let len = self.stack.len();
                    let end = len.saturating_sub(1);
                    let start = end.saturating_sub(k);
                    let args = if k != 0 && len > 0 { self.stack[start..end].to_vec() } else { Vec::new() };
                    let id = (need!(0) << 16) | need!(1);
                    self.pc = nxt;
                    return Event::Cmd { id, args };
                }
                0x88 => {
                    self.pc = nxt;
                    return Event::WaitFade;
                }
                _ => {
                    self.done = true;
                    return Event::BadOp { op, pc };
                }
            }
            self.pc = nxt;
        }
    }
}
