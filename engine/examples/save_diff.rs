//! Compare two battery saves structure by structure: every kind and slot unpacked to the game's image, then the
//! differing bytes listed as runs (offset, length, bytes of each).
//!     cargo run --release --example save_diff -- A.sav B.sav
use lp_engine::save::{Kind, Save};
use std::path::Path;

fn main() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    let (x, y) = (Save::open(Path::new(&a[1]))?, Save::open(Path::new(&a[2]))?);
    let mut total = 0;
    for kind in Kind::ALL {
        for slot in 0..kind.slots() {
            let (p, q) = (x.image(kind, slot), y.image(kind, slot));
            match (p, q) {
                (None, None) => continue,
                (Some(_), None) | (None, Some(_)) => {
                    println!("{} slot {slot}: present in only {}", kind.letter(), if p.is_some() { "the first" } else { "the second" });
                    total += 1;
                    continue;
                }
                (Some(p), Some(q)) => {
                    let n = p.len().max(q.len());
                    let mut i = 0;
                    let mut runs = Vec::new();
                    while i < n {
                        if p.get(i) == q.get(i) {
                            i += 1;
                            continue;
                        }
                        let s = i;
                        while i < n && (p.get(i) != q.get(i) || (i + 1 < n && p.get(i + 1) != q.get(i + 1))) {
                            i += 1;
                        }
                        runs.push((s, i));
                    }
                    println!("{} slot {slot}: {} bytes, {} differing runs", kind.letter(), n, runs.len());
                    for (s, e) in runs.iter().take(40) {
                        let h = |d: &[u8]| d.get(*s..(*e).min(d.len())).unwrap_or(&[]).iter().map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ");
                        println!("  +{s:#06x} len {:3}: {}  |  {}", e - s, h(p), h(q));
                    }
                    total += runs.len();
                }
            }
        }
    }
    println!("{total} differences");
    Ok(())
}
