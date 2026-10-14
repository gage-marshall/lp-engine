use lp_engine::text::Txrc;
fn main() {
    for f in ["gd_textres_jpn", "gd_textres01_jpn", "gd_textres02_jpn", "gd_textres03_jpn", "gd_textres04_jpn", "gd_textres05_jpn", "gd_textres06_jpn", "gd_textres07_jpn"] {
        let t = match Txrc::parse(&std::fs::read(format!("../content/assets/text/{f}.txrc")).unwrap()) { Ok(t) => t, Err(e) => { println!("{f}: {e}"); continue } };
        let row = |s: usize| (0..6).filter_map(|l| t.line_str(0x1c0, s, l)).collect::<Vec<_>>();
        println!("{f}: sub0 {:?} sub1 {:?} sub2 {:?} sub4 {:?}", row(0), row(1), row(2), row(4));
    }
}
