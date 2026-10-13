//! Export NitroSystem 2D resource sets (PNG + JSON) with the Rust loaders; mirrors tools/nns2d_export.py so the two can be diffed.
//! usage: nns2d_dump ASSETS_DIR OUT_DIR
use lp_engine::nns2d::*;
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn write_png(path: &Path, im: &Image) {
    let f = fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), im.w as u32, im.h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header().unwrap().write_image_data(&im.buf).unwrap();
}

fn split_index(f: &str) -> &str {
    let digits = f.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && f[digits..].starts_with('_') {
        &f[digits + 1..]
    } else {
        f
    }
}

type Group = (String, BTreeMap<String, Vec<String>>);

fn groups(files: &[String]) -> Vec<Group> {
    let named = files.iter().any(|f| {
        let d = f.chars().take_while(|c| c.is_ascii_digit()).count();
        d > 0 && f[d..].starts_with('_')
    });
    let mut out: Vec<Group> = Vec::new();
    if named {
        let mut index: std::collections::HashMap<String, usize> = Default::default();
        for f in files {
            let (stem, ext) = f.rsplit_once('.').unwrap();
            let stem = split_index(stem).to_string();
            let key = stem.to_lowercase();
            let i = *index.entry(key).or_insert_with(|| {
                out.push((stem.clone(), BTreeMap::new()));
                out.len() - 1
            });
            out[i].1.entry(ext.to_lowercase()).or_default().push(f.clone());
        }
    } else {
        let mut n = 0;
        let mut cur: Option<usize> = None;
        for f in files {
            let ext = f.rsplit_once('.').unwrap().1.to_lowercase();
            if cur.is_none() || (ext == "ncgr" && out[cur.unwrap()].1.contains_key("ncgr")) {
                n += 1;
                out.push((format!("set{:03}", n), BTreeMap::new()));
                cur = Some(out.len() - 1);
            }
            out[cur.unwrap()].1.entry(ext).or_default().push(f.clone());
        }
    }
    out
}

fn oam_json(o: &Oam) -> Value {
    json!({"x": o.x, "y": o.y, "rot": o.rot, "hide": o.hide, "c256": o.c256, "w": o.w, "h": o.h, "hflip": o.hflip, "vflip": o.vflip, "tile": o.tile, "prio": o.prio, "pal": o.pal})
}

fn anim_json(seqs: &[AnimSeq]) -> Value {
    let s: Vec<Value> = seqs
        .iter()
        .map(|s| {
            let fr: Vec<Value> = s
                .frames
                .iter()
                .map(|f| {
                    let mut m = serde_json::Map::new();
                    m.insert("index".into(), json!(f.index));
                    if let Some(v) = f.rot { m.insert("rot".into(), json!(v)); }
                    if let Some(v) = f.sx { m.insert("sx".into(), json!(v)); }
                    if let Some(v) = f.sy { m.insert("sy".into(), json!(v)); }
                    if let Some(v) = f.x { m.insert("x".into(), json!(v)); }
                    if let Some(v) = f.y { m.insert("y".into(), json!(v)); }
                    m.insert("frames".into(), json!(f.frames));
                    Value::Object(m)
                })
                .collect();
            json!({"loop_start": s.loop_start, "type": s.kind, "element": s.element, "play_mode": s.play_mode, "frames": fr})
        })
        .collect();
    json!({"sequences": s})
}

fn sanitize(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() || c == '_' || c == '.' || c == '-' { c } else { '_' }).collect()
}

fn export_set(src: &Path, dst: &Path, stem: &str, g: &BTreeMap<String, Vec<String>>, last_pal: &mut Option<Nclr>) -> Result<bool, String> {
    let rd = |n: &String| fs::read(src.join(n)).map_err(|e| e.to_string());
    let pal = match g.get("nclr") {
        Some(v) => {
            let p = parse_nclr(&rd(&v[0])?)?;
            *last_pal = Some(Nclr { fmt: p.fmt, colors: p.colors.clone(), bpp: p.bpp });
            Some(p)
        }
        None => last_pal.as_ref().map(|p| Nclr { fmt: p.fmt, colors: p.colors.clone(), bpp: p.bpp }),
    };
    let ch = match g.get("ncgr") {
        Some(v) => Some(parse_ncgr(&rd(&v[0])?)?),
        None => None,
    };
    let od = dst.join(sanitize(stem));
    let mut made = false;
    if let Some(ch) = &ch {
        fs::create_dir_all(&od).map_err(|e| e.to_string())?;
        write_png(&od.join("chars.png"), &ncgr_image(ch, pal.as_ref()));
        made = true;
        for (k, f) in g.get("nscr").into_iter().flatten().enumerate() {
            let sc = parse_nscr(&rd(f)?)?;
            let name = if k == 0 { "screen.png".to_string() } else { format!("screen{}.png", k) };
            write_png(&od.join(name), &nscr_image(&sc, ch, pal.as_ref()));
        }
        if let Some(v) = g.get("ncer") {
            let ce = parse_ncer(&rd(&v[0])?)?;
            fs::create_dir_all(od.join("cells")).map_err(|e| e.to_string())?;
            let mut cells = Vec::new();
            for (i, c) in ce.cells.iter().enumerate() {
                let oams: Result<Vec<Value>, String> = c.oams.iter().map(|o| oam_decode(o[0], o[1], o[2]).map(|x| oam_json(&x))).collect();
                let mut entry = serde_json::Map::new();
                entry.insert("oams".into(), json!(oams?));
                entry.insert("bounds".into(), json!(c.bounds));
                if let Some((im, (ox, oy))) = render_cell(c, ce.mapping, ch, pal.as_ref())? {
                    write_png(&od.join("cells").join(format!("{:03}.png", i)), &im);
                    entry.insert("origin".into(), json!([ox, oy]));
                    entry.insert("size".into(), json!([im.w, im.h]));
                }
                cells.push(Value::Object(entry));
            }
            fs::write(od.join("cells.json"), json!({"mapping": ce.mapping, "cells": cells}).to_string()).map_err(|e| e.to_string())?;
        }
    }
    if let Some(v) = g.get("nanr") {
        fs::create_dir_all(&od).map_err(|e| e.to_string())?;
        fs::write(od.join("anim.json"), anim_json(&parse_nanr(&rd(&v[0])?)?).to_string()).map_err(|e| e.to_string())?;
        made = true;
    }
    Ok(made)
}

fn export_dir(src: &Path, dst: &Path, log: &mut Vec<String>) -> usize {
    let mut files: Vec<String> = fs::read_dir(src)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|f| matches!(f.rsplit_once('.').map(|x| x.1.to_lowercase()).as_deref(), Some("ncgr" | "nclr" | "nscr" | "ncer" | "nanr")))
        .collect();
    files.sort();
    if files.is_empty() {
        return 0;
    }
    let mut last_pal = None;
    let mut n = 0;
    for (stem, g) in groups(&files) {
        match export_set(src, dst, &stem, &g, &mut last_pal) {
            Ok(m) => n += m as usize,
            Err(e) => log.push(format!("{}/{}: {}", src.display(), stem, e)),
        }
    }
    n
}

fn walk(src: &Path, dst: &Path, log: &mut Vec<String>) -> usize {
    let mut total = export_dir(src, dst, log);
    let mut dirs: Vec<PathBuf> = fs::read_dir(src).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for d in dirs {
        total += walk(&d, &dst.join(d.file_name().unwrap()), log);
    }
    total
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let mut log = Vec::new();
    let n = walk(Path::new(&a[1]), Path::new(&a[2]), &mut log);
    println!("{} resource sets exported; {} failures", n, log.len());
    for l in log.iter().take(15) {
        println!("  {}", l);
    }
}
