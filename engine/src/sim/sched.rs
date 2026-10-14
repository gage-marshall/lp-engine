//! Script pacing, one game update at a time (reference `sandbox/gx/script_sim.py`; 70 / 70 commands of the recorded scene in
//! their exact update). The script VM (`Machine`) runs inside an update until it blocks; what blocks it, and for how long:
//!
//! * tasks, ticked once per update after the fade passes: `0x44 n` countdown (`TaskList_FreeAfterCountdown`); a fade
//!   command's wait task (`TaskList_FreeAfterFadesIdleThreeTicks` 0x206c6ec: frees on the 3rd update with the fades idle,
//!   counting the update the fade ends in). `0x1` (yield) with tasks pending: the script resumes in the update after the one
//!   in which the task list empties.
//! * text: the message box takes the text in the next update (the commit). The dialog Transform state machine then opens
//!   the box: the update that handles state 3 waits for the line's voice to be loaded (`GameSnd_AdvanceStreamStartState`
//!   0x2073990; cmd 0x5c in update c is loaded in c + 2), the slide-in takes 8 VBlanks (2 updates), and the typewriter
//!   starts 2 frames after that update. `TextControl_ProcessText` 0x208bf4c runs per frame (12.12 accumulator, reload =
//!   interval[speed] / VBlank divisor). The first tap release while typing completes the text; the next closes the box:
//!   consumed in the cycle after the release (`DialogState_Update`), slide-out 8 VBlanks, the script resumes in
//!   cycle_of(release) + 5. Releases up to the commit frame are ignored.

use crate::host::{Effect, Host, Policy};
use crate::machine::{MEvent, Machine};
use crate::sim::clock::Clock;
use crate::sim::fade::ScreenFade;

/// update of the voice load (`IsLoaded` seen) after a 0x5c request, relative to the request's update
pub const VOICE_LOADED: u64 = 2;
/// the voice stream plays from this many updates after its request (`GameSnd_UpdateStreamSlots` phase-2 pass after c + 3)
pub const VOICE_START: u64 = 4;
/// box slide-in: 8 VBlanks = 2 updates of the 4-frame cycle; the typewriter starts 2 frames after that update's phase-0 run
pub const TYPE_START: u64 = 2;
/// release consumed in cycle + 1, slide-out, resume in cycle + 5
pub const CLOSE_UPDATES: i64 = 5;

#[derive(Clone, Debug, PartialEq)]
pub enum ScriptEvent {
    Cmd { update: u64, id: u32, args: Vec<u32>, effects: Vec<Effect> },
    /// the message box took a line (`chars` = characters the typewriter will draw)
    Text { update: u64, name: String, text: String, chars: usize, mode: Option<u16> },
    /// the box closed and the script continues
    TextClosed { update: u64 },
    Ended { update: u64 },
}

#[derive(Clone, Debug)]
enum Task {
    Count(i64),
    FadeWait(u32),
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Run,
    /// yield with tasks pending
    WaitTasks,
    ResumeAt(u64),
    /// a text was hit in the previous update; the box takes it in this one
    Commit,
    Text,
    Done,
}

#[derive(Clone, Debug)]
pub struct TextBox {
    pub update: u64,
    pub name: String,
    pub text: String,
    pub chars: usize,
    pub mode: Option<u16>,
    pub choice: Option<u32>,
    /// frame of the commit update (releases up to here are ignored)
    pub shown_frame: i64,
    /// update after which the typewriter starts
    pub type_start_update: u64,
    pub typing_from: Option<i64>,
    /// typewriter accumulator (12.12) and characters drawn
    acc: i32,
    pub drawn: usize,
    /// frame in which the last character was drawn
    pub typed_frame: Option<i64>,
    pub completed: bool,
    /// update in which the script resumes after the close
    pub resume_update: Option<u64>,
}

pub struct Scheduler {
    machine: Machine,
    pub host: Host,
    policy: Box<dyn Policy + Send>,
    state: State,
    tasks: Vec<Task>,
    voice_loaded: Option<u64>,
    pending: Option<(String, String, usize, Option<u16>, Option<u32>)>,
    pub text: Option<TextBox>,
    reply: Option<u32>,
    intervals: [i32; 5],
    speed: i8,
}

fn text_chars(s: &str) -> usize {
    s.chars().filter(|&c| c != '\n').count()
}

impl Scheduler {
    pub fn new(machine: Machine, host: Host, policy: Box<dyn Policy + Send>, intervals: [i32; 5], speed: i8) -> Scheduler {
        Scheduler { machine, host, policy, state: State::Run, tasks: Vec::new(), voice_loaded: None, pending: None, text: None, reply: None, intervals, speed }
    }

    pub fn done(&self) -> bool {
        self.state == State::Done
    }

    fn tick_tasks(&mut self, fade: &ScreenFade) {
        for t in self.tasks.iter_mut() {
            match t {
                Task::Count(n) => *n -= 1,
                Task::FadeWait(idle) => {
                    if !fade.active() {
                        *idle += 1;
                    }
                }
            }
        }
        self.tasks.retain(|t| match t {
            Task::Count(n) => *n > 0,
            Task::FadeWait(idle) => *idle <= 2,
        });
    }

    /// One logic update `u` ending at frame `frame` (call after the update's ScreenFade passes). Returns what the script did.
    pub fn update(&mut self, u: u64, frame: i64, fade: &ScreenFade) -> Vec<ScriptEvent> {
        let mut out = Vec::new();
        self.tick_tasks(fade);
        loop {
            match self.state.clone() {
                State::Done => break,
                State::WaitTasks => {
                    if self.tasks.is_empty() {
                        self.state = State::ResumeAt(u + 1);
                    }
                    break;
                }
                State::ResumeAt(t) => {
                    if u >= t {
                        self.state = State::Run;
                        continue;
                    }
                    break;
                }
                State::Commit => {
                    let (name, text, chars, mode, choice) = self.pending.take().unwrap();
                    let start = u.max(self.voice_loaded.take().unwrap_or(0)) + TYPE_START;
                    self.text = Some(TextBox { update: u, name: name.clone(), text: text.clone(), chars, mode, choice, shown_frame: frame, type_start_update: start, typing_from: None, acc: 0, drawn: 0, typed_frame: None, completed: false, resume_update: None });
                    out.push(ScriptEvent::Text { update: u, name, text, chars, mode });
                    self.state = State::Text;
                    break;
                }
                State::Text => {
                    let tb = self.text.as_mut().unwrap();
                    if tb.typing_from.is_none() && u == tb.type_start_update {
                        tb.typing_from = Some(frame + 2);
                    }
                    if tb.resume_update == Some(u) {
                        self.reply = tb.choice;
                        self.text = None;
                        out.push(ScriptEvent::TextClosed { update: u });
                        self.state = State::Run;
                        continue;
                    }
                    break;
                }
                State::Run => {
                    self.run(u, &mut out);
                    break;
                }
            }
        }
        out
    }

    /// run the script until it blocks
    fn run(&mut self, u: u64, out: &mut Vec<ScriptEvent>) {
        for _ in 0..100_000 {
            let ev = self.machine.step(self.reply.take());
            match ev {
                MEvent::Cmd { id, args, strs } => {
                    let (reply, effects) = self.host.command(id, &args, &strs, self.policy.as_mut());
                    self.reply = reply;
                    let mut block = false;
                    for e in &effects {
                        match e {
                            // 0x44 n: n updates (1 without an argument)
                            Effect::Wait { .. } => self.tasks.push(Task::Count(args.first().map_or(1, |&n| n as i64))),
                            Effect::Fade { .. } => self.tasks.push(Task::FadeWait(0)),
                            Effect::Voice { clip, .. } if *clip >= 0 => self.voice_loaded = Some(u + VOICE_LOADED),
                            Effect::Yield if !self.tasks.is_empty() => block = true,
                            _ => {}
                        }
                    }
                    out.push(ScriptEvent::Cmd { update: u, id, args, effects });
                    if block {
                        self.state = State::WaitTasks;
                        return;
                    }
                }
                MEvent::Text { boxed, mode } => {
                    let (name, text) = (self.host.render(&boxed.name), self.host.render(&boxed.text));
                    let choice = mode.and_then(|md| self.policy.text_reply(&name, &text, md));
                    let chars = text_chars(&text);
                    self.pending = Some((name, text, chars, mode, choice));
                    self.state = State::Commit;
                    return;
                }
                MEvent::WaitFade | MEvent::Call { .. } | MEvent::CallRef { .. } | MEvent::ReturnById(_) => {}
                MEvent::End(_) | MEvent::UnresolvedCall(_) | MEvent::Limit(_) | MEvent::Fault(_) | MEvent::BadOp { .. } => {
                    out.push(ScriptEvent::Ended { update: u });
                    self.state = State::Done;
                    return;
                }
            }
        }
    }

    /// the typewriter at frame `f` (once per frame, after the frame's update)
    pub fn frame(&mut self, f: i64, clock: &Clock) {
        let (intervals, speed) = (self.intervals, self.speed);
        let Some(tb) = self.text.as_mut() else { return };
        let Some(from) = tb.typing_from else { return };
        if f < from || tb.typed_frame.is_some() {
            return;
        }
        if speed < 0 {
            tb.drawn = tb.chars;
            tb.typed_frame = Some(f);
            return;
        }
        if tb.acc >> 12 > 0 {
            tb.acc -= 0x1000;
        }
        if tb.acc >> 12 <= 0 {
            tb.acc = intervals[speed as usize] / clock.vblank_div(f);
            tb.drawn += 1;
            if tb.drawn >= tb.chars {
                tb.drawn = tb.chars;
                tb.typed_frame = Some(f);
            }
        }
    }

    /// a tap release at frame `f` (`cycle` = index of the update cycle containing it)
    pub fn release(&mut self, f: i64, cycle: i64) {
        if self.state != State::Text {
            return;
        }
        let tb = self.text.as_mut().unwrap();
        if f <= tb.shown_frame || tb.resume_update.is_some() {
            return;
        }
        if !tb.completed && tb.typed_frame.map_or(true, |t| f <= t) {
            tb.completed = true;
            tb.drawn = tb.chars;
            tb.typed_frame = Some(f);
            return;
        }
        tb.resume_update = Some((cycle + CLOSE_UPDATES).max(0) as u64);
    }

    /// characters of the current line on screen
    pub fn revealed(&self) -> Option<(&TextBox, usize)> {
        self.text.as_ref().map(|t| (t, t.drawn))
    }
}
