//! The game's copy protection must find a real console. On the title screen (ov13, about frame 311) the game runs
//! three checks of its protection overlay (ov01): one reads the console's Wi-Fi MAC address from the system area,
//! one reads the card's secure area with command B7 (a real card redirects it to 8000h + (addr & 1ffh)). Their
//! results move a counter; unless it is a multiple of 97, the title screen installs hooks that slow the heroines'
//! affection (gains / 5, losses * 5) and two more replacements (docs/RUNTIME.md). This boots with an empty save and
//! checks the three hooks the title screen installed are the ordinary ones. Needs ../content.
use lp_engine::rom::Rom;
use lp_engine::runtime::{sdk, Game, NoSubsystems, TICKS_PER_FRAME};
use std::path::Path;

#[test]
fn the_title_screen_installs_the_ordinary_hooks() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    let mut g = Game::new(rom, NoSubsystems);
    g.m.backup = vec![0xff; 0x40000];
    let base = sdk::local_seconds(2026, 10, 15, 21, 0, 0);
    g.m.rtc_base = Some(base);
    g.m.rtc = sdk::rtc_from_local_seconds(base);
    assert!(g.power_on().is_empty());
    while g.m.tick / TICKS_PER_FRAME < 400 {
        assert_eq!(g.frame(), 0);
        assert!(g.m.faults.is_empty(), "{:?}", g.m.faults);
    }
    // (hook, ordinary, penalty)
    for (at, ordinary, penalty) in [(0x2142c74u32, 0x2061204u32, 0x20612b4u32), (0x2142c78, 0x2061034, 0x2061064), (0x2156914, 0x20cade4, 0x20cae1c)] {
        let v = g.m.ram.u32(at);
        assert_ne!(v, penalty, "hook {at:#x}: the copy-protection penalty {penalty:#x} is installed");
        assert_eq!(v, ordinary, "hook {at:#x}: {v:#x}, the ordinary one is {ordinary:#x}");
    }
}
