//! Canonical text dump of assembled heroines; diffed against tools/heroine_dump.py.
//! usage: heroine_dump ARC_ROOT FACE_TABLES.json
use lp_engine::heroine::*;
use sha1::{Digest, Sha1};
use std::path::Path;

fn f7(x: f64) -> String {
    let s = format!("{:.7}", x);
    if s == "-0.0000000" { "0.0000000".into() } else { s }
}

fn h(lines: &[String]) -> String {
    let mut s = Sha1::new();
    s.update(lines.join("\n").as_bytes());
    s.finalize().iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

fn configs(motions: &[String]) -> Vec<(ComposeOpts, bool)> {
    let n = motions.len();
    let d = ComposeOpts::default;
    vec![
        (d(), false),
        (ComposeOpts { outfit: 17, mood: 7, viseme: 2, ..d() }, true),
        (ComposeOpts { outfit: 3, hair: 1, mood: 12, blink: true, ..d() }, true),
        (ComposeOpts { motion: Some(motions[0].clone()), frame: 5, mood: 3, ..d() }, true),
        (ComposeOpts { motion: Some(motions[n / 3].clone()), frame: 40, hair_color: 2, outfit_variant: 1, state: 1, mood: 20, viseme: 4, eye_column: 2, ..d() }, true),
        (ComposeOpts { motion: Some(motions[n / 2].clone()), frame: 12, ribbon: 3, outfit_color: 1, state: 2, outfit: 5, hair: 2, ..d() }, true),
        (ComposeOpts { motion: Some(motions[(2 * n) / 3].clone()), frame: 90, outfit: 8, mood: 30, viseme: 0, ..d() }, true),
    ]
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let tables = FaceTables::from_json(&serde_json::from_str(&std::fs::read_to_string(&a[2]).unwrap()).unwrap()).unwrap();
    let mut out = Vec::new();
    for prefix in ["m", "r", "n"] {
        let w = Wardrobe::load(Path::new(&a[1]), prefix);
        let motions: Vec<String> = w.motions.keys().cloned().collect();
        out.push(format!("W {} parts={} motions={} sd={} pl={} vt={} tex={}", prefix, w.parts.iter().count(), motions.len(), w.sd.len(), w.pl.len(), w.vt.len(), w.tex.by_name.len()));
        for (i, (c, face)) in configs(&motions).into_iter().enumerate() {
            match w.compose(&c, if face { Some(&tables) } else { None }) {
                Err(_) => out.push(format!(" C{} ERR", i)),
                Ok(r) => {
                    let mut tl = Vec::new();
                    for t in &r.tris {
                        tl.push(t.material.to_string());
                        for v in &t.verts {
                            let s: Vec<String> = v.pos.iter().chain(v.nrm.iter()).chain(v.uv.iter()).chain(v.col.iter()).map(|&x| f7(x)).collect();
                            tl.push(s.join(","));
                        }
                    }
                    let ml: Vec<String> = r
                        .mats
                        .iter()
                        .map(|m| {
                            let img = match &m.image {
                                Some(im) => { let mut s = Sha1::new(); s.update(&im.buf); s.finalize().iter().take(6).map(|b| format!("{:02x}", b)).collect::<String>() }
                                None => "None".into(),
                            };
                            format!("{}|{}|{}", m.tex.clone().unwrap_or("None".into()), m.pal.clone().unwrap_or("None".into()), img)
                        })
                        .collect();
                    out.push(format!(" C{} tris={} {} mats={} {} toon={} edge={}", i, r.tris.len(), h(&tl), r.mats.len(), h(&ml), h(&r.toon.iter().map(|x| x.to_string()).collect::<Vec<_>>()), h(&r.edge.iter().map(|x| x.to_string()).collect::<Vec<_>>())));
                }
            }
        }
    }
    println!("{}", out.join("\n"));
}
