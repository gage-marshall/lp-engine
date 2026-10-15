//! A whole new game on the engine against the real game: power on with an empty battery save, set the clock, write
//! the profile by hand (the game's own handwriting recognizer reads the strokes), play the prologue with its two
//! choices, page the tutorial and save from the room - the input in tests/data/newgame/script.json, timed in
//! VBlanks as the emulator driver times it (tap: held 4, released 20; drag: 2 per point, released 8). The emulator
//! ran the same script (sandbox/flow/explore_calls.py, RTC pinned to 2026-10-12 17:00) and its save is
//! ../tools/emu_scripts/states/newgame-emu.sav (git-ignored). Every save structure must be identical, except
//! GameState +0x180: `GameState_Load` 0x20632f0 stores OS_GetTick() there. The OS tick starts when the game's
//! OS_InitTick starts timer 0, after crt0 has unpacked the program; on the DS (emulator) that is about 21 frames
//! after power-on, on the engine 13.4 (its flat instructions-per-tick estimate runs crt0's memory-heavy unpacking
//! fast), so the stamp is about 10 VBlanks lower on the DS. The load itself happens at the same frame on both.
//! The run also shadow-checks the native ports: every native getter call is followed by the game's own function on
//! the same state, and the two must agree (about 690,000 calls).
//!
//! About 3 minutes in release mode, so ignored by default:
//!     cargo test --release --test newgame_live -- --ignored
use lp_engine::rom::Rom;
use lp_engine::runtime::{sdk, Game, NoSubsystems, TICKS_PER_FRAME};
use lp_engine::save::{Kind, Save};
use std::collections::HashMap;
use std::path::Path;

#[test]
#[ignore]
fn a_new_game_saves_what_the_game_saves() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    let Ok(want) = std::fs::read("../tools/emu_scripts/states/newgame-emu.sav") else { return };
    let script: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("tests/data/newgame/script.json").unwrap()).unwrap();
    let mut touch: HashMap<u64, (u16, u16)> = HashMap::new();
    let mut f = 0u64;
    let pt = |p: &serde_json::Value| (p[0].as_u64().unwrap() as u16, p[1].as_u64().unwrap() as u16);
    for st in script.as_array().unwrap() {
        if let Some(n) = st.get("wait").and_then(|w| w.as_u64()) {
            f += n;
        } else if let Some(pts) = st.get("drag").and_then(|d| d.as_array()) {
            for p in pts {
                touch.insert(f, pt(p));
                touch.insert(f + 1, pt(p));
                f += 2;
            }
            f += 8;
        } else if let Some(t) = st.get("tap") {
            for k in 0..4 {
                touch.insert(f + k, pt(t));
            }
            f += 24;
        }
    }
    let mut g = Game::new(rom, NoSubsystems);
    g.m.shadow = Some(Default::default());
    g.m.backup = vec![0xff; 0x40000];
    let base = sdk::local_seconds(2026, 10, 12, 17, 0, 0);
    g.m.rtc_base = Some(base);
    g.m.rtc = sdk::rtc_from_local_seconds(base);
    assert!(g.power_on().is_empty());
    loop {
        let vb = g.m.tick / TICKS_PER_FRAME;
        if vb >= f {
            break;
        }
        g.m.input.touch = touch.get(&vb).copied();
        g.frame();
        assert!(g.m.faults.is_empty(), "frame {vb}: {:?}", g.m.faults);
    }
    if let Some(t) = g.m.check_native {
        // LP_CHECK_NATIVE: a native with side effects against the game's function over the whole run
        let (n, eq, d) = &g.m.check_counts;
        eprintln!("check-native {t:#x}: {n} calls compared, {eq} identical {d:?}");
        assert_eq!(n, eq, "{t:#x}: the native differs from the game's function");
    }
    assert!(g.m.backup_dirty, "the game did not save");
    let shadow = g.m.shadow.take().unwrap();
    let bad: Vec<_> = shadow.iter().filter(|(k, v)| v.1 > 0 && !k.starts_with('(')).collect();
    assert!(bad.is_empty(), "native ports disagree with the game: {bad:?}");
    assert!(shadow.values().map(|v| v.0).sum::<u64>() > 100_000, "too few getter calls checked");

    let (ours, theirs) = (Save::from_bytes(&g.m.backup).unwrap(), Save::from_bytes(&want).unwrap());
    let mut kinds = 0;
    for kind in Kind::ALL {
        for slot in 0..kind.slots() {
            let (a, b) = (ours.image(kind, slot), theirs.image(kind, slot));
            assert_eq!(a.is_some(), b.is_some(), "{} slot {slot}: present in only one save", kind.letter());
            let (Some(a), Some(b)) = (a, b) else { continue };
            kinds += 1;
            assert_eq!(a.len(), b.len());
            let diff: Vec<usize> = (0..a.len()).filter(|&i| a[i] != b[i]).filter(|&i| !(kind == Kind::Game && (0x180..0x184).contains(&i))).collect();
            assert!(diff.is_empty(), "{} slot {slot}: {} bytes differ, first at {:#x}", kind.letter(), diff.len(), diff[0]);
        }
    }
    assert_eq!(kinds, 3, "settings, game state and header");
}
