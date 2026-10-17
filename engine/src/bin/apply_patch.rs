//! `apply_patch CONTENT TRANSLATION.lpdelta PATCH`: the translation rebuilt into PATCH (`lp_engine::patchfile`).
fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 4 {
        eprintln!("usage: apply_patch CONTENT TRANSLATION.lpdelta PATCH");
        std::process::exit(1)
    }
    let d = std::fs::read(&a[2]).expect("translation file");
    let t0 = std::time::Instant::now();
    if let Err(e) = lp_engine::patchfile::apply(std::path::Path::new(&a[1]), &d, std::path::Path::new(&a[3]), &mut |_, _, _| {}) {
        eprintln!("error: {e}");
        std::process::exit(1)
    }
    eprintln!("done in {:.1} s", t0.elapsed().as_secs_f64());
}
