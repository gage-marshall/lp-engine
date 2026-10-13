//! Script-visible game state: scoped flags, counters, clock, RNG, heroine records.
//! Formats are documented in docs/CONDITIONS.md (flag ids), docs/SAVE_RECORD.md (records), docs/EVENT_STAGES.md (RNG).

use std::collections::{HashMap, HashSet};

/// Scoped flag id: scope = bits 12-13 (0 global, 1..=3 = heroine slot + 1), bit index = (id & 0xfff) | ((id & 0x4000) >> 2).
pub fn flag_key(id: u32) -> (u32, u32) {
    ((id >> 12) & 3, (id & 0xfff) | ((id & 0x4000) >> 2))
}

#[derive(Default, Clone, Debug)]
pub struct Flags {
    set: HashSet<(u32, u32)>,
}

impl Flags {
    pub fn set(&mut self, id: u32, on: bool) {
        let k = flag_key(id);
        if on {
            self.set.insert(k);
        } else {
            self.set.remove(&k);
        }
    }
    pub fn test(&self, id: u32) -> bool {
        self.set.contains(&flag_key(id))
    }
    pub fn count(&self) -> usize {
        self.set.len()
    }
}

/// `Rand_Next` of the game: 64-bit LCG, high word scaled by n (docs/EVENT_STAGES.md).  Seeded once at boot from the RTC.
#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Rng {
        Rng { state: seed }
    }
    pub fn next(&mut self, n: u32) -> u32 {
        self.state = self.state.wrapping_mul(0x5D58_8B65_6C07_8965).wrapping_add(0x26_9EC3);
        let hi = (self.state >> 32) as u32;
        if n == 0 { hi } else { ((hi as u64 * n as u64) >> 32) as u32 }
    }
}

#[derive(Clone, Debug)]
pub struct Clock {
    pub year: u32,
    pub month: u32,
    pub day: u32,
    pub weekday: u32,
    pub hour: u32,
}

impl Default for Clock {
    fn default() -> Self {
        Clock { year: 26, month: 4, day: 1, weekday: 0, hour: 12 }
    }
}

/// per-heroine record fields the scripts touch (layout: docs/SAVE_RECORD.md)
#[derive(Clone, Debug)]
pub struct Heroine {
    /// status the slot-status hook returns to cmd 0x17: 3 when the slot's bit 0 is set, 2 / 1 / 0 from the scoped progress bits
    /// (`SaveSlot_ProcessStorageStatus`); 3 matches the recorded play (a heroine you are dating)
    pub status: u32,
    /// stage flags set by cmd 0x1d (1..=10)
    pub stage_flags: u32,
    /// last state set by cmd 0x1e
    pub state: u32,
    pub stat_a: u32,
    pub stat_b: u32,
}

impl Default for Heroine {
    fn default() -> Self {
        Heroine { status: 3, stage_flags: 0, state: 0, stat_a: 0, stat_b: 0 }
    }
}

#[derive(Clone, Debug)]
pub struct GameState {
    pub flags: Flags,
    /// scoped byte counters (cmd 0x107), clamped to 0..=255
    pub counters: HashMap<u32, u32>,
    pub heroines: [Heroine; 3],
    pub clock: Clock,
    pub rng: Rng,
    /// heroine slot of the running scene (0 Manaka, 1 Rinko, 2 Nene)
    pub current_slot: u32,
    pub player_name: String,
}

impl GameState {
    pub fn new(seed: u64) -> GameState {
        GameState { flags: Flags::default(), counters: HashMap::new(), heroines: Default::default(), clock: Clock::default(), rng: Rng::new(seed), current_slot: 0, player_name: "あなた".into() }
    }

    /// season of the current month (`Calendar_MonthToSeason`, table 2/5/8/11 at 0x2101cfc): 0 winter (Dec-Feb), 1 spring, 2 summer, 3 autumn
    pub fn season(&self) -> u32 {
        [2u32, 5, 8, 11].iter().position(|&m| self.clock.month <= m).unwrap_or(0) as u32
    }

    /// time period of the current hour (`TimePeriod_FromHour`: before 5 -> 5, then the first of 8/12/15/18/21 above the hour, else 5)
    pub fn time_period(&self) -> u32 {
        if self.clock.hour < 5 {
            return 5;
        }
        [8u32, 12, 15, 18, 21].iter().position(|&h| self.clock.hour < h).unwrap_or(5) as u32
    }

    /// heroine slot encoded in the high byte of a script id / character ref (0xa0 Manaka, 0x58 Rinko, 0x08 Nene)
    pub fn slot_of_event_id(id: u32) -> Option<u32> {
        match id >> 24 {
            0xa0 | 0xa4 | 0xa7 => Some(0),
            0x58 | 0x5c | 0x5f => Some(1),
            0x08 | 0x0c | 0x0f => Some(2),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_ids_scope_and_normalise() {
        assert_eq!(flag_key(0x1005), (1, 5));
        assert_eq!(flag_key(0x3123), (3, 0x123));
        assert_eq!(flag_key(0x4005), (0, 0x1005)); // bit 14 folds down to bit 12 of the index
        let mut f = Flags::default();
        f.set(0x2010, true);
        assert!(f.test(0x2010) && !f.test(0x1010) && !f.test(0x3010)); // scoped per heroine
        f.set(0x2010, false);
        assert_eq!(f.count(), 0);
    }

    #[test]
    fn rng_is_deterministic_and_bounded() {
        let mut a = Rng::new(1234);
        let mut b = Rng::new(1234);
        let xs: Vec<u32> = (0..50).map(|_| a.next(10)).collect();
        assert_eq!(xs, (0..50).map(|_| b.next(10)).collect::<Vec<_>>());
        assert!(xs.iter().all(|&x| x < 10));
        assert!(xs.iter().collect::<HashSet<_>>().len() > 5, "outputs vary");
        let mut c = Rng::new(0);
        assert_eq!(c.next(0), (0x26_9EC3u64 >> 32) as u32); // first state is just the increment
        // known first value for state 0 with n = 100: hi32(0x269EC3) = 0
        assert_eq!(Rng::new(0).next(100), 0);
    }

    #[test]
    fn heroine_slot_from_event_id() {
        assert_eq!(GameState::slot_of_event_id(0xa063_f380), Some(0));
        assert_eq!(GameState::slot_of_event_id(0x5800_6600), Some(1));
        assert_eq!(GameState::slot_of_event_id(0x0814_0200), Some(2));
        assert_eq!(GameState::slot_of_event_id(0x8004_0500), None);
    }

    #[test]
    fn season_and_period_follow_the_game_tables() {
        let mut g = GameState::new(1);
        for (month, season) in [(1, 0), (2, 0), (3, 1), (5, 1), (6, 2), (8, 2), (9, 3), (11, 3), (12, 0)] {
            g.clock.month = month;
            assert_eq!(g.season(), season, "month {month}");
        }
        for (hour, period) in [(0, 5), (4, 5), (5, 0), (7, 0), (8, 1), (11, 1), (12, 2), (14, 2), (15, 3), (17, 3), (18, 4), (20, 4), (21, 5), (23, 5)] {
            g.clock.hour = hour;
            assert_eq!(g.time_period(), period, "hour {hour}");
        }
    }
}
