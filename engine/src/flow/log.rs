//! Replaying a call log recorded from the game (`sandbox/flow/calllog.py`) and checking the port against it: call
//! targets in order, the arguments the port gives, the loads and stores of each stretch between two calls (as a
//! multiset: the compiler's ordering is free), and the return value of every ported function.
//!
//! Records (one per line, hex):
//!     f FRAME                         a frame starts
//!     c SITE TARGET A0 A1 A2 A3       a call from SITE (in a ported function) and its first four arguments
//!     r SITE VALUE                    that call returned
//!     F SITE ORIGINAL FORCED          the explorer replaced the return value (the port must compute ORIGINAL)
//!     o SITE ARG W0 [W1 W2 W3]        after the return: words of the stack buffer passed as argument ARG
//!     l ADDR SIZE VALUE / s ADDR SIZE VALUE   a load / store made by a ported function
use super::{Arg, Cx, Flow};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug)]
enum Rec {
    Frame(u32),
    Call { site: u32, target: u32, args: [u32; 4] },
    Ret { site: u32, value: u32 },
    Forced { site: u32, original: u32 },
    Out { site: u32, arg: u32, n: u8, words: [u32; 4] },
    Load { addr: u32, size: u32, value: u32 },
    Store { addr: u32, size: u32, value: u32 },
}

/// how a replay ended
#[derive(Debug)]
pub enum Outcome {
    /// every frame matched (the log may end inside a frame)
    Ok,
    Mismatch(String),
}

pub struct LogCx {
    recs: Vec<Rec>,
    i: usize,
    used: HashSet<usize>,
    frame: Option<u32>,
    err: Option<String>,
    eof: bool,
    where_: Vec<&'static str>,
    /// ported functions whose call record was matched (stack parallel to `where_`): their sites
    sites: Vec<u32>,
    pub warnings: Vec<String>,
    pub calls: u64,
    pub ported: u64,
    pub loads: u64,
    pub stores: u64,
    pub forced: u64,
    pub frames: u64,
    /// (machine, state) -> frames that state ran
    pub coverage: HashMap<(&'static str, i32), u64>,
    /// the wireless code the replay reached (never, for single-player logs)
    pub not_ported: Vec<String>,
    /// call site -> the number of arguments the port passed there (checked against the game's call sites)
    pub site_args: HashMap<u32, usize>,
}

fn hex(s: &str) -> u32 {
    u32::from_str_radix(s, 16).unwrap_or_else(|_| panic!("bad hex {s:?}"))
}

impl LogCx {
    pub fn parse(text: &str) -> LogCx {
        let mut recs = Vec::new();
        for line in text.lines() {
            let p: Vec<&str> = line.split_ascii_whitespace().collect();
            if p.is_empty() {
                continue;
            }
            let h = |k: usize| hex(p[k]);
            let rec = match p[0] {
                "S" => continue, // the state stack when recording started
                "f" => Rec::Frame(h(1)),
                "c" => Rec::Call { site: h(1), target: h(2), args: [h(3), h(4), h(5), h(6)] },
                "r" => Rec::Ret { site: h(1), value: h(2) },
                "F" => Rec::Forced { site: h(1), original: h(2) },
                "o" => {
                    let mut words = [0; 4];
                    let n = (p.len() - 3).min(4);
                    for j in 0..n {
                        words[j] = h(3 + j);
                    }
                    Rec::Out { site: h(1), arg: p[2].parse().expect("o arg"), n: n as u8, words }
                }
                "l" | "s" => {
                    let addr = h(1);
                    if (0x27e0000..0x27e4000).contains(&addr) {
                        continue; // the stack (DTCM) reached through another register: locals
                    }
                    let (size, value) = (p[2].parse().expect("size"), h(3));
                    if p[0] == "l" { Rec::Load { addr, size, value } } else { Rec::Store { addr, size, value } }
                }
                k => panic!("unknown record {k:?}"),
            };
            recs.push(rec);
        }
        LogCx {
            recs,
            i: 0,
            used: HashSet::new(),
            frame: None,
            err: None,
            eof: false,
            where_: Vec::new(),
            sites: Vec::new(),
            warnings: Vec::new(),
            calls: 0,
            ported: 0,
            loads: 0,
            stores: 0,
            forced: 0,
            frames: 0,
            coverage: HashMap::new(),
            not_ported: Vec::new(),
            site_args: HashMap::new(),
        }
    }

    /// replay every frame of the log through the port
    pub fn run(&mut self, flow: &Flow) -> Outcome {
        let mut first = true;
        loop {
            match self.next_frame(first) {
                Ok(true) => {}
                Ok(false) => return Outcome::Ok,
                Err(e) => return Outcome::Mismatch(e),
            }
            first = false;
            flow.frame(self);
            if let Some(e) = self.take_error() {
                return Outcome::Mismatch(e);
            }
            if self.eof {
                return Outcome::Ok;
            }
            self.frames += 1;
        }
    }

    /// start the next frame of the log: true when one starts, false at the end (`first`: a log that starts inside a
    /// frame - a save state taken mid-frame - skips to the first frame boundary)
    pub fn next_frame(&mut self, first: bool) -> Result<bool, String> {
        if first {
            while self.i < self.recs.len() && !matches!(self.recs[self.i], Rec::Frame(_)) {
                self.i += 1;
            }
        }
        match self.recs.get(self.i) {
            None => Ok(false),
            Some(&Rec::Frame(f)) => {
                self.frame = Some(f);
                self.i += 1;
                Ok(true)
            }
            Some(r) => Err(format!("frame {:?}: records left over after the frame: {r:?}", self.frame)),
        }
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.err.take()
    }

    /// the log ended inside the frame just run
    pub fn at_end(&self) -> bool {
        self.eof
    }

    fn stopped(&self) -> bool {
        self.err.is_some() || self.eof
    }

    fn fail(&mut self, msg: String) {
        if !self.stopped() {
            let ctx = format!("frame {:?}, in {}", self.frame, if self.where_.is_empty() { "-".to_string() } else { self.where_.join(" > ") });
            let next: Vec<String> = self.recs[self.i..(self.i + 6).min(self.recs.len())].iter().map(|r| format!("{r:?}")).collect();
            self.err = Some(format!("{ctx}: {msg}\n  next records: {}", next.join(", ")));
        }
    }

    fn segment_end(&self) -> usize {
        let mut j = self.i;
        while j < self.recs.len() && matches!(self.recs[j], Rec::Load { .. } | Rec::Store { .. }) {
            j += 1;
        }
        j
    }

    fn end_segment(&mut self) {
        let end = self.segment_end();
        for j in self.i..end {
            if !self.used.contains(&j) {
                self.warnings.push(format!("frame {:?}: the game made {:?}; the port did not", self.frame, self.recs[j]));
            }
        }
        self.i = end;
    }

    /// past the return of the call made at `site` (whatever the callee did is not ours)
    fn skip_call(&mut self, site: u32) -> Option<u32> {
        let mut depth = 0;
        let mut j = self.i;
        while j < self.recs.len() {
            match self.recs[j] {
                Rec::Call { site: s, .. } if s == site => depth += 1,
                Rec::Ret { site: s, value } if s == site => {
                    if depth == 0 {
                        if j > 0 && matches!(self.recs[j - 1], Rec::Forced { .. }) {
                            self.forced += 1;
                        }
                        self.i = j + 1;
                        return Some(value);
                    }
                    depth -= 1;
                }
                _ => {}
            }
            j += 1;
        }
        self.eof = true;
        None
    }

    /// the 'o' records after a return: fill the Out arguments
    fn fill(&mut self, site: u32, args: &mut [Arg]) {
        while let Some(&Rec::Out { site: s, arg, n, words }) = self.recs.get(self.i) {
            if s != site {
                break;
            }
            if let Some(Arg::Out(buf)) = args.get_mut(arg as usize) {
                for j in 0..(n as usize).min(buf.len()) {
                    buf[j] = Some(words[j] as i32);
                }
            }
            self.i += 1;
        }
    }

    /// match the call record for a call the port makes; its site
    fn match_call(&mut self, target: u32, args: &[Arg]) -> Option<u32> {
        if self.stopped() {
            return None;
        }
        self.end_segment();
        let Some(&r) = self.recs.get(self.i) else {
            self.eof = true;
            return None;
        };
        let Rec::Call { site, target: t, args: ga } = r else {
            self.fail(format!("port calls {target:#x}; the game: {r:?}"));
            return None;
        };
        if t != target {
            self.fail(format!("port calls {target:#x}; the game called {t:#x} (site {site:#x})"));
            return None;
        }
        for (k, a) in args.iter().take(4).enumerate() {
            if let Arg::I(v) = a {
                if *v as u32 != ga[k] {
                    self.fail(format!("call {target:#x} (site {site:#x}) argument {k}: port {:#x}, game {:#x}", *v as u32, ga[k]));
                    return None;
                }
            }
        }
        self.i += 1;
        self.calls += 1;
        let n = self.site_args.entry(site).or_insert(0);
        *n = (*n).max(args.len());
        Some(site)
    }
}

impl Cx for LogCx {
    fn external(&mut self, target: u32, args: &mut [Arg]) -> i32 {
        let Some(site) = self.match_call(target, args) else { return 0 };
        let Some(v) = self.skip_call(site) else { return 0 };
        self.fill(site, args);
        if target == 0x203dbd4 {
            // Main_GetStateAtDepth: which state ran
            if let Some(&w) = self.where_.last() {
                *self.coverage.entry((w, v as i32)).or_default() += 1;
            }
        }
        v as i32
    }

    fn enter(&mut self, target: u32, name: &'static str, args: &mut [Arg]) {
        // a stopped replay still keeps the stacks balanced
        let site = self.match_call(target, args).unwrap_or(0);
        self.where_.push(name);
        self.sites.push(site);
    }

    fn leave(&mut self, _target: u32, ret: Option<i32>, args: &mut [Arg]) -> i32 {
        let name = self.where_.pop().unwrap_or("-");
        let site = self.sites.pop().unwrap_or(0);
        if self.stopped() {
            return 0;
        }
        self.end_segment();
        let mut forced = None;
        if let Some(&Rec::Forced { site: s, original }) = self.recs.get(self.i) {
            if s == site {
                forced = Some(original);
                self.i += 1;
            }
        }
        let Some(&rr) = self.recs.get(self.i) else {
            self.eof = true;
            return 0;
        };
        let value = match rr {
            Rec::Ret { site: s, value } if s == site => value,
            _ => {
                self.fail(format!("{name} returned; the game still had {rr:?}"));
                return 0;
            }
        };
        self.i += 1;
        self.fill(site, args);
        self.ported += 1;
        let game = forced.unwrap_or(value);
        if let Some(r) = ret {
            if r as u32 != game {
                self.fail(format!("{name} returned {:#x}; the game {game:#x}", r as u32));
                return 0;
            }
        }
        if forced.is_some() {
            self.forced += 1;
        }
        value as i32
    }

    fn load(&mut self, addr: u32, size: u32) -> u32 {
        if self.stopped() {
            return 0;
        }
        for j in self.i..self.segment_end() {
            if let Rec::Load { addr: a, size: n, value } = self.recs[j] {
                if a == addr && n == size && !self.used.contains(&j) {
                    self.used.insert(j);
                    self.loads += 1;
                    return value;
                }
            }
        }
        self.fail(format!("port loads {addr:#x} ({size} bytes); the game did not"));
        0
    }

    fn store(&mut self, addr: u32, value: Option<u32>, size: u32) {
        if self.stopped() {
            return;
        }
        let mask = if size >= 4 { u32::MAX } else { (1 << (8 * size)) - 1 };
        for j in self.i..self.segment_end() {
            if let Rec::Store { addr: a, size: n, value: g } = self.recs[j] {
                if a == addr && n == size && !self.used.contains(&j) {
                    self.used.insert(j);
                    self.stores += 1;
                    if let Some(v) = value {
                        if v & mask != g {
                            self.fail(format!("port stores {:#x} to {addr:#x}; the game stored {g:#x}", v & mask));
                        }
                    }
                    return;
                }
            }
        }
        self.fail(format!("port stores {value:x?} to {addr:#x} ({size} bytes); the game did not"));
    }

    fn pick(&mut self, cond: Option<bool>, targets: &[(bool, u32)]) -> bool {
        if self.stopped() {
            return cond.unwrap_or(false);
        }
        self.end_segment();
        match self.recs.get(self.i) {
            Some(&Rec::Call { target, .. }) => {
                if let Some(&(v, _)) = targets.iter().find(|&&(_, t)| t == target) {
                    if cond.is_some_and(|c| c != v) {
                        self.fail(format!("the port's branch condition is {cond:?}; the game took the other way"));
                    }
                    return v;
                }
                let r = self.recs[self.i];
                self.fail(format!("next call {r:?} matches none of {targets:x?}"));
            }
            Some(&r) => self.fail(format!("next record {r:?} is not a call")),
            None => self.eof = true,
        }
        false
    }

    fn not_ported(&mut self, what: String) {
        self.not_ported.push(what);
    }
}
