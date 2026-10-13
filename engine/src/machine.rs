//! SBIN interpreter with the game's master-directory call convention (reference: tools/event_trace.py `TraceMachine`).
//!
//! Unlike `script::Run`, which executes one script file, the machine owns the active script context: opcode 0x16 resolves an event id
//! through the master directory (script 899) and the resource pools (`engine/script_resource_map.json`), so it can call into another
//! SBIN before returning; opcode 0x14 ("return by id") and a non-zero root return value hand off to another event after unwinding.

use crate::script::{read_cell, write_cell, Script, Seg, TextBox};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ref {
    pub slot: u32,
    pub script: u32,
    pub entry: u32,
}

pub struct Loaded {
    pub slot: u32,
    pub script_id: u32,
    /// file number in tmx_event.arc
    pub resource: u32,
    pub script: Script,
}

pub struct ScriptStore {
    /// event id -> (slot, script, entry)
    pub directory: HashMap<u32, Ref>,
    pools: Vec<Vec<u16>>,
    root: PathBuf,
    cache: Mutex<HashMap<(u32, u32), Arc<Loaded>>>,
}

impl ScriptStore {
    /// `content` = the layer-0 content tree
    pub fn open(content: &Path) -> Result<ScriptStore, String> {
        let root = content.join("assets/arc/tmx_event.arc");
        let rm: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(content.join("engine/script_resource_map.json")).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let mut pools = Vec::new();
        for slot in 0..4 {
            let e = rm["slots"][slot.to_string()]["entries"].as_array().ok_or("resource map slot")?;
            pools.push(e.iter().map(|x| x.as_u64().unwrap_or(0) as u16).collect());
        }
        // the master directory is the script holding > 1000 id records
        let mut files: Vec<_> = std::fs::read_dir(&root).map_err(|e| e.to_string())?.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "sbin")).collect();
        files.sort();
        let mut directory = HashMap::new();
        for f in files {
            let d = std::fs::read(&f).map_err(|e| e.to_string())?;
            if d.len() < 0x34 {
                continue;
            }
            let cnt = u32::from_be_bytes([d[0x30], d[0x31], d[0x32], d[0x33]]) as usize;
            if cnt > 1000 {
                let off = u32::from_be_bytes([d[0x2c], d[0x2d], d[0x2e], d[0x2f]]) as usize;
                for k in 0..cnt {
                    let p = off + 8 * k;
                    let id = u32::from_be_bytes([d[p], d[p + 1], d[p + 2], d[p + 3]]);
                    let r = u32::from_be_bytes([d[p + 4], d[p + 5], d[p + 6], d[p + 7]]);
                    directory.insert(id, Ref { slot: (r >> 24) & 0xf, script: (r >> 12) & 0xfff, entry: r & 0xfff });
                }
                break;
            }
        }
        if directory.is_empty() {
            return Err("no master directory script found".into());
        }
        Ok(ScriptStore { directory, pools, root, cache: Mutex::new(HashMap::new()) })
    }

    pub fn load(&self, r: Ref) -> Result<Arc<Loaded>, String> {
        let pool = self.pools.get(r.slot as usize).ok_or("invalid slot")?;
        let resource = *pool.get(r.script as usize).ok_or_else(|| format!("invalid script reference slot/script {}/{}", r.slot, r.script))? as u32;
        let key = (r.slot, r.script);
        let loaded = {
            let mut c = self.cache.lock().unwrap();
            if let Some(l) = c.get(&key) {
                l.clone()
            } else {
                let data = std::fs::read(self.root.join(format!("{:04}.sbin", resource))).map_err(|e| e.to_string())?;
                let script = Script::from_decrypted(data).ok_or("bad SBIN header")?;
                let l = Arc::new(Loaded { slot: r.slot, script_id: r.script, resource, script });
                c.insert(key, l.clone());
                l
            }
        };
        if r.entry as usize >= loaded.script.entries.len() {
            return Err(format!("{:04}.sbin entry {} is out of range", loaded.resource, r.entry));
        }
        Ok(loaded)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MEvent {
    Cmd { id: u32, args: Vec<u32> },
    Text { boxed: TextBox, mode: Option<u16> },
    WaitFade,
    ReturnById(u32),
    Call { event_id: u32, target: Ref },
    CallRef { raw: u32, target: Ref },
    UnresolvedCall(u32),
    End(u32),
    Limit(usize),
    Fault(String),
    BadOp { op: u8, pc: usize },
}

#[derive(PartialEq)]
enum Last {
    None,
    Cmd,
    TextMode,
}

pub struct Machine {
    store: Arc<ScriptStore>,
    pub current: Arc<Loaded>,
    pub pc: usize,
    pub acc: u32,
    flag: bool,
    stack: Vec<u32>,
    frames: Vec<(Arc<Loaded>, usize)>,
    pub regs: HashMap<u16, u32>,
    pub gvars: HashMap<u16, u32>,
    local_memory: HashMap<u32, Vec<u8>>,
    bufs: [Vec<Seg>; 3],
    pending_return_event: Option<u32>,
    ret_by_id: Option<u32>,
    last: Last,
    steps: usize,
    pub max_steps: usize,
    done: bool,
}

fn s16(v: u32) -> i64 {
    v as u16 as i16 as i64
}

impl Machine {
    pub fn start(store: Arc<ScriptStore>, event_id: u32) -> Result<Machine, String> {
        let r = *store.directory.get(&event_id).ok_or_else(|| format!("event id {event_id:08x} is not in the script directory"))?;
        let current = store.load(r)?;
        let pc = current.script.entries[r.entry as usize] as usize;
        Ok(Machine {
            store,
            current,
            pc,
            acc: 0,
            flag: false,
            stack: Vec::new(),
            frames: Vec::new(),
            regs: HashMap::new(),
            gvars: HashMap::new(),
            local_memory: HashMap::new(),
            bufs: [Vec::new(), Vec::new(), Vec::new()],
            pending_return_event: None,
            ret_by_id: None,
            last: Last::None,
            steps: 0,
            max_steps: 200_000,
            done: false,
        })
    }

    fn enter(&mut self, event_id: u32) -> Option<Ref> {
        let r = *self.store.directory.get(&event_id)?;
        self.current = self.store.load(r).ok()?;
        self.pc = self.current.script.entries[r.entry as usize] as usize;
        Some(r)
    }

    fn enter_ref(&mut self, raw: u32) -> Option<Ref> {
        let r = Ref { slot: (raw >> 24) & 0xf, script: (raw >> 12) & 0xfff, entry: raw & 0xfff };
        self.current = self.store.load(r).ok()?;
        self.pc = self.current.script.entries[r.entry as usize] as usize;
        Some(r)
    }

    /// return one frame, or perform a deferred opcode-14 handoff / the root register handoff
    fn finish_return(&mut self) -> Option<MEvent> {
        if let Some((c, pc)) = self.frames.pop() {
            self.current = c;
            self.pc = pc;
            return None;
        }
        if let Some(ev) = self.pending_return_event.take() {
            return Some(match self.enter(ev) {
                Some(t) => MEvent::Call { event_id: ev, target: t },
                None => MEvent::UnresolvedCall(ev),
            });
        }
        if self.acc != 0 {
            if let Some(t) = self.enter_ref(self.acc) {
                return Some(MEvent::CallRef { raw: self.acc, target: t });
            }
        }
        Some(MEvent::End(self.acc))
    }

    fn fin(&mut self, e: MEvent) -> MEvent {
        if matches!(e, MEvent::End(_) | MEvent::UnresolvedCall(_) | MEvent::Limit(_) | MEvent::Fault(_) | MEvent::BadOp { .. }) {
            self.done = true;
        }
        e
    }

    /// Continue after the previous event; `reply` answers a `Cmd` (accumulator) or a mode-text choice (register 0).
    pub fn step(&mut self, reply: Option<u32>) -> MEvent {
        if self.done {
            return MEvent::End(self.acc);
        }
        if let Some(r) = reply {
            match self.last {
                Last::Cmd => self.acc = r,
                Last::TextMode => {
                    self.regs.insert(0, r);
                }
                Last::None => {}
            }
        }
        self.last = Last::None;
        if let Some(ev) = self.ret_by_id.take() {
            self.pending_return_event = Some(ev);
            if let Some(e) = self.finish_return() {
                return self.fin(e);
            }
        }
        loop {
            self.steps += 1;
            if self.steps > self.max_steps {
                return self.fin(MEvent::Limit(self.pc));
            }
            let cur = self.current.clone();
            let data = &cur.script.data;
            let pc = self.pc;
            if pc + 2 > data.len() {
                return self.fin(MEvent::Fault(format!("instruction outside script at {pc}")));
            }
            let (op, count) = (data[pc], data[pc + 1] as usize);
            let end = pc + 2 + 2 * count;
            if end > data.len() {
                return self.fin(MEvent::Fault(format!("truncated instruction at {pc}")));
            }
            let a: Vec<u32> = (0..count).map(|i| u16::from_be_bytes([data[pc + 2 + 2 * i], data[pc + 3 + 2 * i]]) as u32).collect();
            let mut nxt = end;
            macro_rules! arg {
                ($i:expr) => {
                    match a.get($i) {
                        Some(&v) => v,
                        None => return self.fin(MEvent::Fault(format!("missing operand at {pc}"))),
                    }
                };
            }
            match op {
                0x00 => {}
                0x01 => self.stack.push(self.acc),
                0x02 => self.acc = self.stack.pop().unwrap_or(0),
                0x03 => {
                    for _ in 0..arg!(0) {
                        self.stack.pop();
                    }
                }
                0x10 => nxt = (nxt as i64 + s16(arg!(0))) as usize,
                0x11 => {
                    if self.flag {
                        nxt = (nxt as i64 + s16(arg!(0))) as usize;
                    }
                }
                0x12 => {
                    if !self.flag {
                        nxt = (nxt as i64 + s16(arg!(0))) as usize;
                    }
                }
                0x13 => match self.finish_return() {
                    None => continue,
                    Some(ev) => {
                        let ev = self.fin(ev);
                        return ev;
                    }
                },
                0x14 => {
                    let ev = (arg!(0) << 16) | arg!(1);
                    // the native VM reports the return-by-id first, then unwinds on the next resume
                    self.ret_by_id = Some(ev);
                    return MEvent::ReturnById(ev);
                }
                0x16 => {
                    let ev = (arg!(0) << 16) | arg!(1);
                    let r = match self.store.directory.get(&ev) {
                        Some(&r) => r,
                        None => return self.fin(MEvent::UnresolvedCall(ev)),
                    };
                    let target = match self.store.load(r) {
                        Ok(t) => t,
                        Err(e) => return self.fin(MEvent::Fault(e)),
                    };
                    self.frames.push((self.current.clone(), nxt));
                    self.pc = target.script.entries[r.entry as usize] as usize;
                    self.current = target;
                    return MEvent::Call { event_id: ev, target: r };
                }
                0x20 => self.flag = self.acc != 0,
                0x21 => self.flag = self.stack.last().map_or(false, |&t| t == self.acc),
                0x30 => self.acc = 0,
                0x31 => self.acc = 1,
                0x32 => self.acc = arg!(0),
                0x33 => self.acc = (arg!(0) << 16) | arg!(1),
                0x34 => self.acc = (self.acc != 0) as u32,
                0x35 => self.acc = self.acc.wrapping_neg(),
                0x40 => self.acc = self.regs.get(&(arg!(0) as u16)).copied().unwrap_or(0),
                0x41 => self.acc = self.gvars.get(&(arg!(0) as u16)).copied().unwrap_or(0),
                0x42 => {
                    let i = arg!(0) as usize;
                    self.acc = match cur.script.local_descriptors.get(i) {
                        Some(&d) => read_cell(self.local_memory.entry(cur.slot).or_insert_with(|| vec![0; 0x400]), d),
                        None => 0,
                    };
                }
                0x43 => {
                    self.regs.insert(arg!(0) as u16, self.acc);
                }
                0x44 => {
                    self.gvars.insert(arg!(0) as u16, self.acc);
                }
                0x45 => {
                    let i = arg!(0) as usize;
                    if let Some(&d) = cur.script.local_descriptors.get(i) {
                        let acc = self.acc;
                        write_cell(self.local_memory.entry(cur.slot).or_insert_with(|| vec![0; 0x400]), d, acc);
                    }
                }
                0x50..=0x5c => {
                    let b = self.stack.pop().unwrap_or(0);
                    let x = self.stack.pop().unwrap_or(0);
                    self.acc = match op {
                        0x50 => x.wrapping_add(b),
                        0x51 => x.wrapping_sub(b),
                        0x52 => x.wrapping_mul(b),
                        0x53 => if b == 0 { 0 } else { (x as i32 as i64 / b as i32 as i64) as u32 },
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
                    let b = arg!(0) as usize;
                    if b >= 3 {
                        return self.fin(MEvent::Fault(format!("text buffer {b}")));
                    }
                    self.bufs[b].clear();
                }
                0x81 | 0x82 => {
                    let b = arg!(0) as usize;
                    if b >= 3 {
                        return self.fin(MEvent::Fault(format!("text buffer {b}")));
                    }
                    if op == 0x81 {
                        match cur.script.string((arg!(1) << 16) | arg!(2)) {
                            Some(t) => self.bufs[b].push(Seg::Lit(t)),
                            None => return self.fin(MEvent::Fault("string outside script".into())),
                        }
                    } else {
                        self.bufs[b].push(Seg::Var(arg!(1) as u16));
                    }
                }
                0x83 | 0x84 => {
                    let mode = if op == 0x84 { Some(arg!(0) as u16) } else { None };
                    let boxed = TextBox { text: self.bufs[0].clone(), sub: self.bufs[1].clone(), name: self.bufs[2].clone() };
                    self.pc = nxt;
                    self.last = if mode.is_some() { Last::TextMode } else { Last::None };
                    return MEvent::Text { boxed, mode };
                }
                0x87 => {
                    let arity = self.stack.last().copied().unwrap_or(0) as usize;
                    let len = self.stack.len();
                    let e = len.saturating_sub(1);
                    let args = if arity != 0 && len > 0 { self.stack[e.saturating_sub(arity)..e].to_vec() } else { Vec::new() };
                    let id = (arg!(0) << 16) | arg!(1);
                    self.pc = nxt;
                    self.last = Last::Cmd;
                    return MEvent::Cmd { id, args };
                }
                0x88 => {
                    self.pc = nxt;
                    return MEvent::WaitFade;
                }
                0x85 | 0x86 | 0x5d..=0x7f | 0x04 | 0x15 | 0x17..=0x1f => {}
                _ => return self.fin(MEvent::BadOp { op, pc }),
            }
            self.pc = nxt;
        }
    }
}
