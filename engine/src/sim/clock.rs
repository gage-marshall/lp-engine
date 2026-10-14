//! The game's frame clock. The DS shows 59.8261 frames per second; a heroine scene renders through Capture3D in a 3-phase
//! cycle (`Capture3D_OnVBlank`: phase byte 0x213d2e4+0xa3 = 0, 1, 2; the phase-2 main-loop pass waits two VBlanks), so the
//! nominal cycle is 4 frames with phases 0, 1, 2, 2. One logic update runs per cycle, at the end of the phase-0 run.
//! Under CPU load a pass can take longer (the recorded session has 3-5 frame cycles); a replay injects the recorded phases.

pub const FRAME_HZ: f64 = 59.8261;

pub trait PhaseSource {
    /// Capture3D phase of frame `f`
    fn phase(&self, f: i64) -> u8;
}

/// the nominal cycle: frame 0 is a phase-0 frame
pub struct Nominal;

impl PhaseSource for Nominal {
    fn phase(&self, f: i64) -> u8 {
        [0, 1, 2, 2][f.rem_euclid(4) as usize]
    }
}

/// recorded phases for frames `first ..`; frames outside continue the nominal cycle
pub struct Recorded {
    pub first: i64,
    pub phases: Vec<u8>,
}

impl Recorded {
    pub fn from_digits(first: i64, digits: &str) -> Recorded {
        Recorded { first, phases: digits.bytes().map(|b| b - b'0').collect() }
    }
}

impl PhaseSource for Recorded {
    fn phase(&self, f: i64) -> u8 {
        match self.phases.get((f - self.first) as usize) {
            Some(&p) if f >= self.first => p,
            _ => Nominal.phase(f),
        }
    }
}

/// The frame/update counters the simulation runs on.
pub struct Clock {
    pub src: Box<dyn PhaseSource + Send>,
    /// current frame
    pub frame: i64,
    /// updates so far (the update in progress has index `updates - 1` once `update_end` fired)
    pub updates: u64,
    /// index of the cycle containing the current frame (cycle t = the phase-0 run of update t and the frames after it)
    pub cycle: i64,
}

impl Clock {
    pub fn new(src: Box<dyn PhaseSource + Send>, first_frame: i64) -> Clock {
        Clock { src, frame: first_frame, updates: 0, cycle: -1 }
    }

    pub fn phase(&self, f: i64) -> u8 {
        self.src.phase(f)
    }

    /// does a cycle start at frame `f` (first frame of a phase-0 run)?
    pub fn cycle_starts(&self, f: i64, first_frame: i64) -> bool {
        self.phase(f) == 0 && (f == first_frame || self.phase(f - 1) != 0)
    }

    /// is frame `f` the last frame of a phase-0 run (the frame in which the logic update runs)?
    pub fn update_ends(&self, f: i64) -> bool {
        self.phase(f) == 0 && self.phase(f + 1) != 0
    }

    /// VBlank divisor the typewriter reloads with at frame `f` (0x213d2e4+0xa0, set by `VramBank_Process` 0x2044ee8 to 2 after
    /// a phase-2 pass, else 1): 2 iff the previous frame was phase 2
    pub fn vblank_div(&self, f: i64) -> i32 {
        if self.phase(f - 1) == 2 { 2 } else { 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nominal_cycle_is_four_frames_with_one_update() {
        let c = Clock::new(Box::new(Nominal), 0);
        let ends: Vec<i64> = (0..16).filter(|&f| c.update_ends(f)).collect();
        assert_eq!(ends, vec![0, 4, 8, 12]);
        assert_eq!(c.vblank_div(1), 1);
        assert_eq!(c.vblank_div(4), 2, "after the second phase-2 frame");
    }

    #[test]
    fn recorded_phases() {
        let r = Recorded::from_digits(100, "0012201220");
        let c = Clock::new(Box::new(r), 100);
        assert!(!c.update_ends(100) && c.update_ends(101));
        assert!(c.cycle_starts(100, 100) && c.cycle_starts(105, 100) && !c.cycle_starts(101, 100));
    }
}
