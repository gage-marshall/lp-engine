//! The eye state machine at chara+0x488 (reference `sandbox/gx/motion_sim.py` EyeSim + `scene_data.blink_step`; eye texture
//! and mode 1,135 / 1,177 updates of the recorded scene).
//!
//! Per update (`UiAsset_UpdateSceneVariants` 0x2094e90): the blink (0x2094aa4) when no transition runs, else the mode transition
//! (0x2094cdc: request textures, wait for the load, closed eye, then the new mode). Requests come from the motion's `eye`
//! visibility track (`Model_RequestMotionState` 0x2079eb4, issued by `Gfx3D_UpdateActorState` after the eye step); every mood
//! set starts a type-2 reload of the current mode. Start state from model creation (`Model_ResetMotionFlagsEntry`,
//! `Model_ClearLoaderSet`, eye = 0x2103540[0]) plus the creation mood set. Blinks under mood 0x1a draw `Rand_Next`.

use crate::game::Rng;
use crate::rom::Rom;

pub struct EyeTables {
    /// s8[10] blink intervals (0x2103554) and holds (0x210355e)
    interval: Vec<i8>,
    hold: Vec<i8>,
    /// s8[46][18] eye texture per (mood, column) (0x2127ae4)
    table: Vec<i8>,
    /// s8[] column after a blink, per eye mode (0x2103542)
    mode_byte: Vec<i8>,
    /// resident eye textures (0x2103540[0..1])
    fixed: [i32; 2],
}

impl EyeTables {
    pub fn from_rom(rom: &Rom) -> EyeTables {
        EyeTables {
            interval: rom.i8s(0x2103554, 10),
            hold: rom.i8s(0x210355e, 10),
            table: rom.i8s(0x2127ae4, 46 * 18),
            mode_byte: rom.i8s(0x2103542, 32),
            fixed: [rom.i8(0x2103540) as i32, rom.i8(0x2103541) as i32],
        }
    }

    /// 0x2094898: eye texture for column `col` under `mood` (mood -1 = row 0)
    pub fn eye(&self, col: i32, mood: i32) -> i32 {
        let m = if mood == -1 { 0 } else { mood };
        self.table.get((m * 18 + col) as usize).map_or(-1, |&v| v as i32)
    }

    fn mode_col(&self, mode: i32) -> i32 {
        self.mode_byte.get(mode as usize).map_or(0, |&v| v as i32)
    }

    /// 0x20949f8: special eye for (mode, mood), -1 if none
    pub fn special(mode: i32, mood: i32) -> i32 {
        if mood == -1 {
            return -1;
        }
        match mode {
            8 => match mood {
                0x13 => 0x13,
                0x1b => 3,
                _ => -1,
            },
            9 => match mood {
                9 | 0x1b | 0xf => 1,
                0xc | 0xd => 3,
                0x13 | 0x29 => 0x11,
                _ => -1,
            },
            _ => -1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EyeState {
    pub counter: i32,
    pub idx: i32,
    pub eye: i32,
    pub phase: i32,
    pub hold: i32,
    pub extra: i32,
    pub mode: i32,
}

#[derive(Clone)]
pub struct EyeSim {
    pub st: EyeState,
    /// transition type (+0x21: 0 none, 1 request, 2 reload) and its sub-state (+0x23)
    pub trans: i32,
    pub sub: i32,
    req: i32,
    cache: Vec<i32>,
    pending: Vec<i32>,
    /// `Rand_Next` with the RNG state at the scene start (runtime state; only blinks under mood 0x1a draw)
    pub rng: Option<Rng>,
}

impl EyeSim {
    pub fn new(t: &EyeTables, rng: Option<Rng>) -> EyeSim {
        EyeSim { st: EyeState { counter: 1, idx: 0, eye: t.fixed[0], phase: 0, hold: 0, extra: 0, mode: 0 }, trans: 2, sub: 0, req: 0, cache: Vec::new(), pending: Vec::new(), rng }
    }

    /// `Model_SetMotionIdAndInitFlags`: +0x4a9 = 2, +0x4ab = 0, +0x4b4 = +0x4b0
    pub fn mood_set(&mut self) {
        self.trans = 2;
        self.sub = 0;
        self.req = self.st.mode;
    }

    /// `Model_RequestMotionState`
    pub fn request(&mut self, code: i32) {
        let code = if (12..=14).contains(&code) { 2 } else { code };
        if self.trans != 0 {
            return;
        }
        self.req = code;
        if code != self.st.mode {
            self.trans = 1;
            self.sub = 0;
        }
    }

    pub fn update(&mut self, t: &EyeTables, mood: i32) {
        if self.trans == 0 {
            self.blink(t, mood);
        } else if self.trans == 1 || self.trans == 2 {
            match self.sub {
                0 => {
                    let r = self.req;
                    self.pending = vec![t.eye(r, mood), t.eye(t.mode_col(r), mood)];
                    let sp = EyeTables::special(r, mood);
                    if sp != -1 {
                        self.pending.push(sp);
                    }
                    self.sub = 1;
                }
                1 => self.sub = 3, // loaded (assumed at once)
                3 => {
                    self.st.eye = t.eye(2, mood);
                    self.sub = 2;
                }
                2 => {
                    self.st.mode = self.req;
                    self.req = -1;
                    self.trans = 0;
                    self.sub = 0;
                    self.st.eye = t.eye(t.mode_col(self.st.mode), mood);
                    self.cache = self.pending.clone();
                    self.cache.extend_from_slice(&t.fixed);
                }
                _ => {}
            }
        }
        // residency check (0x2094ee8..0x2094fe8): a non-resident eye texture forces a reload of the current mode
        let e = self.st.eye;
        if e != -1 && !self.cache.contains(&e) && self.trans != 2 {
            self.sub = 0;
            self.trans = 2;
            self.req = self.st.mode;
        }
    }

    /// one update of the blink (0x2094aa4)
    fn blink(&mut self, t: &EyeTables, mood: i32) {
        let st = &mut self.st;
        let mode = st.mode;
        if (0..=1).contains(&(mode - 10)) {
            return;
        }
        let step = if mood == 0x18 { 2 } else { 1 };
        match st.phase {
            0 => {
                st.eye = t.eye(mode, mood);
                if st.counter >= 0 {
                    st.counter -= step;
                } else {
                    let i = st.idx;
                    st.counter = t.interval[i as usize] as i32;
                    st.extra = if mood == 0x1a && [1, 2, 4, 5].contains(&i) {
                        let r = self.rng.as_mut().expect("mood 0x1a blinks draw Rand_Next: give the scene its RNG state").next(30);
                        r as i32 + 0x2d
                    } else {
                        0
                    };
                    st.phase = 1;
                    st.idx = if i + 1 >= 10 { 0 } else { i + 1 };
                }
                if EyeTables::special(mode, mood) == -1 {
                    return;
                }
                if st.hold >= 0 {
                    st.hold -= 1;
                    return;
                }
                let i = st.idx;
                st.idx = if i + 1 >= 10 { 0 } else { i + 1 };
                st.phase = 0x65;
                st.hold = t.hold[i as usize] as i32;
            }
            1 => {
                st.eye = t.eye(2, mood);
                st.phase = if st.extra == 0 { st.phase + 1 } else { 10 };
            }
            2 => {
                st.eye = t.eye(t.mode_col(mode), mood);
                st.phase = 0;
            }
            10 => {
                if st.extra <= 0 {
                    st.phase = 2;
                    st.extra = 0;
                } else {
                    st.extra -= 1;
                }
            }
            0x65 => {
                st.eye = EyeTables::special(mode, mood);
                st.phase += 1;
            }
            0x66 => {
                st.eye = EyeTables::special(mode, mood);
                st.phase = 0;
            }
            _ => {}
        }
    }
}
