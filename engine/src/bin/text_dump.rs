//! Dump every TXRC resource in a directory as JSON [group][sub][line] (same shape as tools/lp_export.py's text stage).
//! usage: text_dump TEXT_DIR OUT_DIR
use lp_engine::text::Txrc;
use std::fs;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    fs::create_dir_all(&a[2]).unwrap();
    let mut files: Vec<_> = fs::read_dir(&a[1]).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "txrc")).collect();
    files.sort();
    let (mut n, mut lines) = (0, 0);
    for f in files {
        let t = Txrc::parse(&fs::read(&f).unwrap()).unwrap();
        let doc: Vec<Vec<Vec<String>>> = t.groups.iter().map(|g| g.iter().map(|s| s.iter().map(|l| String::from_utf8_lossy(l).into_owned()).collect()).collect()).collect();
        let out = std::path::Path::new(&a[2]).join(f.file_stem().unwrap()).with_extension("json");
        fs::write(out, serde_json::to_string(&doc).unwrap()).unwrap();
        n += 1;
        lines += t.line_count();
    }
    println!("{} files, {} lines", n, lines);
}
