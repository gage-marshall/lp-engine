//! Edit one byte of a heroine record in a save and write it back the way the game does.
//!     cargo run --release --example save_edit -- IN.sav OUT.sav SLOT OFFSET VALUE
use lp_engine::save::{game_state, Card, Kind, Save};
use std::path::Path;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let num = |s: &str| if let Some(h) = s.strip_prefix("0x") { usize::from_str_radix(h, 16).unwrap() } else { s.parse().unwrap() };
    let (slot, off, val) = (num(&a[3]), num(&a[4]), num(&a[5]) as u8);
    let mut card = Card::open(Path::new(&a[1]))?;
    let mut save = Save::load(&card)?;
    let o = game_state::RECORDS + game_state::RECORD_SIZE * slot + off;
    let img = save.image_mut(Kind::Game, 0).ok_or("no game state")?;
    println!("record {slot} +{off:#x}: {} -> {val}", img[o]);
    img[o] = val;
    let w = save.store(&mut card, Kind::Game, 0)?.unwrap();
    println!("written: copy {} generation {} ({} bytes stored)", w.copy, w.generation, w.stored_size);
    card.save(Path::new(&a[2]))
}
