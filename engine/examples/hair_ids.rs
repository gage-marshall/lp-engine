use lp_engine::heroine::*;
use std::path::Path;
fn main() {
    let w = Wardrobe::load(&Path::new("../content/assets/arc"), &std::env::args().nth(1).unwrap());
    let p = format!("{}_00_", std::env::args().nth(1).unwrap());
    let mut ids: Vec<u32> = w.parts.iter().filter_map(|(k, _)| k.strip_prefix(&p)?.strip_suffix("_hair")?.parse::<u32>().ok()).collect();
    ids.sort(); ids.dedup();
    println!("{:?}", ids);
}
