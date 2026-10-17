//! `extract ROM.nds CONTENT`: the player's ROM read into a content tree (`lp_engine::extract`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: extract ROM.nds CONTENT");
        std::process::exit(1)
    }
    let rom = std::fs::read(&a[1]).unwrap_or_else(|e| {
        eprintln!("{}: {e}", a[1]);
        std::process::exit(1)
    });
    let t0 = std::time::Instant::now();
    let mut last = String::new();
    let r = lp_engine::extract::extract(&rom, std::path::Path::new(&a[2]), &mut |stage, i, n| {
        if stage != last {
            eprintln!("{stage}...");
            last = stage.to_string();
        }
        let _ = (i, n);
    });
    if let Err(e) = r {
        eprintln!("error: {e}");
        std::process::exit(1)
    }
    eprintln!("done in {:.1} s", t0.elapsed().as_secs_f64());
}
