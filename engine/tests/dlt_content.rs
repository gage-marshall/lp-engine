//! The handwriting recogniser (the game's own code under the engine's ARM interpreter) against the same code under
//! Unicorn: tests/data/dlt/cases.json (drawn on sandbox/dlt/pad.py) and generated.json (sandbox/dlt/export_cases.py)
//! hold stroke sets and the 12 candidate codes Unicorn gave. Skipped when content/ is absent.
use lp_engine::dlt::{display, Recogniser};
use serde_json::Value;
use std::path::Path;

fn cases(name: &str) -> Vec<(String, Vec<Vec<(u16, u16)>>, Vec<u16>)> {
    let p = Path::new("tests/data/dlt").join(name);
    let Ok(b) = std::fs::read(&p) else { return vec![] };
    let v: Value = serde_json::from_slice(&b).unwrap();
    v.as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let strokes = c["strokes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_array().unwrap().iter().map(|p| (p[0].as_u64().unwrap() as u16, p[1].as_u64().unwrap() as u16)).collect())
                .collect();
            let codes = c["codes"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u16).collect();
            (c["note"].as_str().unwrap_or("").to_string(), strokes, codes)
        })
        .collect()
}

fn show(codes: &[u16]) -> String {
    codes.iter().map(|&c| display(c).unwrap_or('·')).collect()
}

#[test]
fn recogniser_matches_the_oracle() {
    let content = Path::new("../content");
    if !content.join("assets/dltjdb.dat").is_file() {
        eprintln!("content/ absent: skipped");
        return;
    }
    let mut r = Recogniser::open(content).unwrap();
    let mut failed = 0;
    let mut total = 0;
    for file in ["cases.json", "generated.json"] {
        for (i, (note, strokes, want)) in cases(file).into_iter().enumerate() {
            total += 1;
            let s0 = r.steps();
            let t = std::time::Instant::now();
            let got = r.recognise_raw(&strokes).unwrap();
            let ok = got[..] == want[..];
            eprintln!("{file} #{i} {note} {} strokes: {} {} ({} instr, {:.0} ms)", strokes.len(), show(&got),
                      if ok { "ok" } else { "MISMATCH" }, r.steps() - s0, t.elapsed().as_secs_f64() * 1000.0);
            if !ok {
                eprintln!("    want {}", show(&want));
                failed += 1;
            }
        }
    }
    assert_eq!(failed, 0, "{failed} of {total} cases differ");
}
