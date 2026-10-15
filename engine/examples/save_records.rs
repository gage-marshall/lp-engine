//! The three heroine records of each file in a battery save, as the game unpacks them: flags, phase, state, affection, points.
//!     cargo run --release --example save_records -- A.sav [B.sav ...]
use lp_engine::save::{game_state, Kind, Save};
use std::path::Path;

fn main() -> Result<(), String> {
    for p in std::env::args().skip(1) {
        let save = Save::open(Path::new(&p))?;
        for slot in 0..Kind::Game.slots() {
        let Some(img) = save.image(Kind::Game, slot) else {
            continue;
        };
        let u32_at = |o: usize| u32::from_le_bytes(img[o..o + 4].try_into().unwrap());
        println!("{p} file {slot}: GameState +0x78 {} +0x98 {}", img[0x78], img[0x98]);
        for k in 0..3 {
            let r = game_state::RECORDS + game_state::RECORD_SIZE * k;
            println!(
                "  heroine {k}: flags {:#x} phase {} state {} affection {} +7 {} points {} +0xc {}",
                u32_at(r),
                img[r + 4] as i8,
                img[r + 5],
                img[r + 6],
                img[r + 7],
                u32_at(r + 8),
                img[r + 0xc]
            );
        }
        }
    }
    Ok(())
}
