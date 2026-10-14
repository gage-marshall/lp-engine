use lp_engine::player::{PlayerOptions, ScenePlayer};
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let ev = u32::from_str_radix(&a[1], 16).unwrap();
    let mut o = PlayerOptions::default();
    o.save = a.get(2).map(|s| s.into());
    let mut p = ScenePlayer::new(Path::new("../content"), ev, o).unwrap();
    let mut last = String::new();
    for t in 1..700 {
        p.run_to(t as f64 * 0.1);
        if let Some(f) = &p.sim.last {
            let sum: i32 = f.chain.iter().map(|l| l.ratio).sum();
            let s = format!("{} chain {:?}{}", if f.chain.is_empty() { "EMPTY" } else if sum != 4096 { "SUM!=4096" } else { "" },
                f.chain.iter().map(|l| format!("{}r{}", l.stem, l.ratio)).collect::<Vec<_>>(), "");
            if s != last { println!("t {:.1} upd {} {}", t as f64 * 0.1, f.update, s); last = s; }
        }
    }
}
