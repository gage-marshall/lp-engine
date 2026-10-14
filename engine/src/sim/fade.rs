//! ScreenFade (0x213e4cc): the master-brightness fade of both screens.
//! `ScreenFade_ToBlack` 0x20451f8: step = (-1.0 - cur) / max(n, 1); `ScreenFade_ToClear` 0x20452a4: step = -cur / max(n, 1), delay
//! counter 0; `ScreenFade_Process` 0x2045390 runs once per main-loop pass (3 per update): to black steps and clamps at -1.0;
//! to clear counts up and steps once counter >= delay, idle when cur reaches 0. 16.16 fixed point, truncating division,
//! so n = 20 takes 21 steps (RAM: idle after 26 = 5 + 21 passes). Brightness register = cur / 0x1000 toward zero.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenFade {
    pub cur: i32,
    step: i32,
    /// 0 to black, 2 to clear, 3 idle
    state: u8,
    counter: i32,
    delay: i32,
}

fn div_trunc(a: i32, b: i32) -> i32 {
    a / b // Rust integer division truncates toward zero, like _s32_div_f
}

impl ScreenFade {
    /// a scene starts black
    pub fn black() -> ScreenFade {
        ScreenFade { cur: -0x10000, step: 0, state: 3, counter: 0, delay: 0 }
    }

    pub fn to_black(&mut self, n: i32) {
        self.state = 0;
        self.step = div_trunc(-0x10000 - self.cur, n.max(1));
    }

    pub fn to_clear(&mut self, n: i32, delay: i32) {
        self.state = 2;
        self.step = div_trunc(-self.cur, n.max(1));
        self.counter = 0;
        self.delay = delay;
    }

    pub fn process(&mut self) {
        match self.state {
            0 => {
                self.cur += self.step;
                if self.cur <= -0x10000 {
                    self.cur = -0x10000;
                    self.state = 3;
                }
            }
            2 => {
                self.counter += 1;
                if self.counter >= self.delay {
                    self.cur += self.step;
                    if (self.step >= 0 && self.cur >= 0) || (self.step < 0 && self.cur <= 0) {
                        self.cur = 0;
                        self.state = 3;
                    }
                }
            }
            _ => {}
        }
    }

    pub fn active(&self) -> bool {
        self.state != 3
    }

    /// master brightness level -16..0 (`ScreenFade_Process`: cur / 0x1000 rounded toward zero)
    pub fn brightness(&self) -> i32 {
        self.cur / 0x1000
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_in_of_20_takes_26_passes() {
        let mut f = ScreenFade::black();
        f.to_clear(20, 6);
        let mut n = 0;
        while f.active() {
            f.process();
            n += 1;
        }
        assert_eq!(n, 26, "5 delay passes + 21 truncated steps");
        assert_eq!(f.brightness(), 0);
    }

    #[test]
    fn fade_out_clamps_at_black() {
        let mut f = ScreenFade { cur: 0, ..ScreenFade::black() };
        f.to_black(20);
        for _ in 0..21 {
            f.process();
        }
        assert_eq!(f.cur, -0x10000);
        assert!(!f.active());
        assert_eq!(ScreenFade { cur: -0x1fff, ..ScreenFade::black() }.brightness(), -1);
    }
}
