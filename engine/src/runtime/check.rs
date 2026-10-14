//! The live check: the flow replays a call log recorded from the game (the recording decides every branch, as in
//! `flow::log`), while the engine's machine runs alongside and answers every call itself. For each external the
//! registry ports natively, the value the engine computes is compared with the value the game returned; every load
//! the flow makes is compared with the engine's RAM. A log recorded from power-on (sandbox/flow/explore_calls.py with a
//! battery save) starts both from the same state, so the comparison covers the natives' state as well as their logic.
use super::ext::{self, Kind, Ret};
use super::{Machine, Subsystems};
use crate::flow::log::{LogCx, Outcome};
use crate::flow::{Arg, Cx, Flow};
use std::collections::BTreeMap;

#[derive(Default, Debug, Clone)]
pub struct Tally {
    pub ok: u64,
    pub bad: u64,
    /// the first few disagreements: (frame, engine value, game value)
    pub first: Vec<(u64, i64, i64)>,
}

impl Tally {
    fn add(&mut self, frame: u64, engine: i64, game: i64) {
        if engine == game {
            self.ok += 1;
        } else {
            self.bad += 1;
            if self.first.len() < 5 {
                self.first.push((frame, engine, game));
            }
        }
    }
}

pub struct LiveCheck<'a> {
    pub log: LogCx,
    pub m: &'a mut Machine,
    pub subs: &'a mut dyn Subsystems,
    /// per native: return values compared
    pub returns: BTreeMap<&'static str, Tally>,
    /// per address range of a flow load: values compared with the engine's RAM
    pub loads: BTreeMap<u32, Tally>,
    frame: u64,
}

impl<'a> LiveCheck<'a> {
    pub fn new(log: LogCx, m: &'a mut Machine, subs: &'a mut dyn Subsystems) -> LiveCheck<'a> {
        // replayed calls run out of their real context, where some never finish: stop those early (the game's own
        // long calls, such as saving, do not occur in the replayed logs)
        m.budget = 20_000_000;
        LiveCheck { log, m, subs, returns: BTreeMap::new(), loads: BTreeMap::new(), frame: 0 }
    }

    /// replay the log; the machine follows. Between frames the machine runs what Main_UpdateFrame runs around the
    /// flow (the fade).
    pub fn run(&mut self, flow: &Flow) -> Outcome {
        let mut n = 0u64;
        let mut first = true;
        loop {
            // the log drives frames; LogCx::run handles one log at a time, so step it frame by frame here
            match self.step(flow, first) {
                Some(Ok(())) => {}
                Some(Err(e)) => return Outcome::Mismatch(e),
                None => return Outcome::Ok,
            }
            first = false;
            n += 1;
            self.frame = n;
        }
    }

    fn step(&mut self, flow: &Flow, first: bool) -> Option<Result<(), String>> {
        match self.log.next_frame(first) {
            Ok(true) => {}
            Ok(false) => return None,
            Err(e) => return Some(Err(e)),
        }
        flow.frame(self);
        if let Some(e) = self.log.take_error() {
            return Some(Err(e));
        }
        if self.log.at_end() {
            return None;
        }
        super::natives::screen_fade_process(self.m);
        self.m.frame += 1;
        Some(Ok(()))
    }
}

impl Cx for LiveCheck<'_> {
    fn external(&mut self, target: u32, args: &mut [Arg]) -> i32 {
        let en = ext::entry(target);
        // the engine answers first (its Out buffers are then overwritten by the recording's)
        let mine = ext::call(self.m, self.subs, target, args);
        let game = self.log.external(target, args);
        if let Some(en) = en {
            if matches!(en.kind, Kind::Native(_)) && en.ret == Ret::Value {
                self.returns.entry(en.name).or_default().add(self.frame, mine as i64, game as i64);
            }
        }
        game
    }
    fn enter(&mut self, target: u32, name: &'static str, args: &mut [Arg]) {
        self.log.enter(target, name, args)
    }
    fn leave(&mut self, target: u32, ret: Option<i32>, args: &mut [Arg]) -> i32 {
        self.log.leave(target, ret, args)
    }
    fn load(&mut self, addr: u32, size: u32) -> u32 {
        let game = self.log.load(addr, size);
        let mine = self.m.ram.read(addr, size);
        self.loads.entry(addr & !0xff).or_default().add(self.frame, mine as i64, game as i64);
        // the machine follows the game where they disagree (one difference must not cascade)
        if mine != game {
            self.m.ram.write(addr, game, size);
        }
        game
    }
    fn store(&mut self, addr: u32, value: Option<u32>, size: u32) {
        self.log.store(addr, value, size);
        if let Some(v) = value {
            self.m.ram.write(addr, v, size);
        }
    }
    fn pick(&mut self, cond: Option<bool>, targets: &[(bool, u32)]) -> bool {
        self.log.pick(cond, targets)
    }
    fn not_ported(&mut self, what: String) {
        self.log.not_ported(what)
    }
}
