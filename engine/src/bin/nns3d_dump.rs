//! Canonical text dump of every G3D file under a directory; diffed against tools/nns3d_dump.py.
//! usage: nns3d_dump ASSETS_DIR
use lp_engine::nns3d::*;
use sha1::{Digest, Sha1};
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn f7(x: f64) -> String {
    let s = format!("{:.7}", x);
    if s == "-0.0000000" { "0.0000000".into() } else { s }
}

fn h(lines: &[String]) -> String {
    let mut s = Sha1::new();
    s.update(lines.join("\n").as_bytes());
    let r = s.finalize();
    r.iter().take(8).map(|b| format!("{:02x}", b)).collect()
}

fn node_lines(n: &Node) -> Vec<String> {
    let mut v = vec![n.name.clone(), n.flag.to_string()];
    v.extend(n.trans.iter().map(|&x| f7(x)));
    v.extend(n.rot.iter().flatten().map(|&x| f7(x)));
    v.extend(n.scale.iter().map(|&x| f7(x)));
    v
}

fn tex_digest(t0: &Tex0, tx: &TexEntry) -> String {
    let want = format!("{}_pl", tx.name);
    let pal = t0.palettes.iter().rev().find(|p| p.name == want).or(t0.palettes.first());
    let sel = match pal {
        Some(p) => PalSel::Offset(p.offset),
        None => PalSel::None,
    };
    match decode_texture(&t0.src(), tx, sel) {
        Ok(img) => {
            let mut s = Sha1::new();
            s.update(&img.buf);
            s.finalize().iter().take(8).map(|b| format!("{:02x}", b)).collect()
        }
        Err(_) => "ERR".into(),
    }
}

fn dump_tex(t0: &Tex0, out: &mut Vec<String>) {
    for tx in &t0.textures {
        out.push(format!(" tex {} {}x{} fmt={} {}", tx.name, tx.w, tx.h, tx.fmt, tex_digest(t0, tx)));
    }
    out.push(format!(" pals {}", h(&t0.palettes.iter().map(|p| format!("{}|{}", p.name, p.offset)).collect::<Vec<_>>())));
}

fn dump_bmd(d: Vec<u8>, out: &mut Vec<String>) -> Result<(), String> {
    let r = parse_file(Arc::new(d))?;
    for m in &r.models {
        let i = &m.info;
        out.push(format!("M {} nodes={} mats={} shapes={} ps={} rule={} tm={} fu={} nv={} np={}", m.name, i.num_node, i.num_mat, i.num_shp, f7(i.pos_scale), i.scaling_rule, i.tex_mtx_mode, i.first_unused_mtx, i.num_vertex, i.num_polygon));
        out.push(format!(" nodes {}", h(&m.nodes.iter().flat_map(node_lines).collect::<Vec<_>>())));
        let ml: Vec<String> = m
            .materials
            .iter()
            .map(|x| {
                let o = |v: &Option<String>| v.clone().unwrap_or("None".into());
                format!("{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}", x.name, x.diffuse, x.ambient, x.polyattr, x.alpha, x.cull, x.tex_param, x.pltt_base, x.flag, x.orig_w, x.orig_h, o(&x.tex), o(&x.pal))
            })
            .collect();
        out.push(format!(" mats {}", h(&ml)));
        out.push(format!(" shapes {}", h(&m.shapes.iter().map(|s| format!("{}|{}|{}|{}", s.name, s.flag, s.dl_off, s.dl_size)).collect::<Vec<_>>())));
        match build_mesh(m, None, true, None) {
            Ok(tris) => {
                let mut lines = Vec::new();
                for t in &tris {
                    lines.push(t.material.to_string());
                    for v in &t.verts {
                        let a: Vec<String> = v.pos.iter().chain(v.nrm.iter()).chain(v.uv.iter()).chain(v.col.iter()).map(|&x| f7(x)).collect();
                        let j: Vec<String> = v.joints.as_ref().unwrap().iter().map(|(a, b)| format!("{}:{}", a, f7(*b))).collect();
                        lines.push(format!("{};{}", a.join(","), j.join(",")));
                    }
                }
                out.push(format!(" mesh tris={} {}", tris.len(), h(&lines)));
            }
            Err(_) => out.push(" mesh ERR".into()),
        }
    }
    if let Some(t0) = &r.tex0 {
        dump_tex(t0, out);
    }
    Ok(())
}

fn spec_s(s: &Spec) -> String {
    match s {
        Spec::ConstF(v) => format!("const:{}", f7(*v)),
        Spec::ConstRaw(v) => format!("const:{}", v),
        Spec::Anim(a, b) => format!("anim:{},{}", a, b),
    }
}

fn dump_bca(d: Vec<u8>, out: &mut Vec<String>) -> Result<(), String> {
    for a in parse_bca(&Arc::new(d))? {
        let tl: Vec<String> = a
            .tracks
            .iter()
            .map(|t| {
                format!(
                    "{}|{}|{}|{}|{}",
                    t.node,
                    t.flag,
                    t.t.as_ref().map(|v| v.iter().map(spec_s).collect::<Vec<_>>().join(";")).unwrap_or_default(),
                    t.r.as_ref().map(spec_s).unwrap_or_default(),
                    t.s.as_ref().map(|v| v.iter().map(spec_s).collect::<Vec<_>>().join(";")).unwrap_or_default()
                )
            })
            .collect();
        let nn = a.tracks.iter().map(|t| t.node).max().unwrap_or(0) + 1;
        let base: Vec<Node> = (0..nn).map(|i| Node { name: format!("n{}", i), flag: 0, trans: [0.1 * i as f64, 0.0, 0.0], rot: IDENT.0, scale: [1.0; 3] }).collect();
        let mut frames = vec![0, a.frames / 3, a.frames / 2, a.frames.saturating_sub(1)];
        frames.sort();
        frames.dedup();
        let samples: Vec<String> = frames
            .iter()
            .map(|&f| match sample_anim(&a, &base, f) {
                Ok(res) => h(&res.iter().flat_map(node_lines).collect::<Vec<_>>()),
                Err(_) => "ERR".into(),
            })
            .collect();
        out.push(format!(" bca {} frames={} tracks={} samples={}", a.name, a.frames, h(&tl), samples.join(",")));
    }
    Ok(())
}

fn dump_bva(d: &[u8], out: &mut Vec<String>) -> Result<(), String> {
    for a in parse_bva(d)? {
        let mut frames = vec![0, a.frames / 2, a.frames.saturating_sub(1)];
        frames.sort();
        frames.dedup();
        let vis: String = frames.iter().flat_map(|&f| (0..a.nodes.min(40)).map(move |n| (f, n))).map(|(f, n)| if visible(&a, f, n) { '1' } else { '0' }).collect();
        out.push(format!(" bva {} frames={} nodes={} bits={} vis={}", a.name, a.frames, a.nodes, h(&a.bits.iter().map(|b| b.to_string()).collect::<Vec<_>>()), vis));
    }
    Ok(())
}

fn bundle_entries(b: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    if b.len() < 8 {
        return None;
    }
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let esz = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    if !(1..=4096).contains(&n) || esz != 0x28 || 8 + n * esz > b.len() {
        return None;
    }
    let mut out = Vec::new();
    for k in 0..n {
        let e = 8 + k * esz;
        let off = u32::from_le_bytes([b[e], b[e + 1], b[e + 2], b[e + 3]]) as usize;
        let size = u32::from_le_bytes([b[e + 4], b[e + 5], b[e + 6], b[e + 7]]) as usize;
        let nm = &b[e + 8..e + 40];
        let end = nm.iter().position(|&c| c == 0).unwrap_or(32);
        let name: String = nm[..end].iter().map(|&c| c as char).collect();
        out.push((name, b.get(off..(off + size).min(b.len())).unwrap_or(&[]).to_vec()));
    }
    Some(out)
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let mut ents: Vec<_> = fs::read_dir(dir).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
    ents.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let (dirs, files): (Vec<_>, Vec<_>) = ents.into_iter().partition(|p| p.is_dir());
    for f in files {
        let b = fs::read(&f).unwrap();
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().to_string();
        match b.get(0..4) {
            Some(b"BMD0") => {
                out.push(format!("F {}", rel));
                if dump_bmd(b, out).is_err() {
                    out.push(" ERR".into());
                }
            }
            Some(b"BTX0") => {
                out.push(format!("F {}", rel));
                match parse_file(Arc::new(b)) {
                    Ok(r) if r.tex0.is_some() => dump_tex(r.tex0.as_ref().unwrap(), out),
                    _ => out.push(" ERR".into()),
                }
            }
            _ => {
                if let Some(ents) = bundle_entries(&b) {
                    if ents.iter().any(|(_, d)| d.get(0..4) == Some(b"BCA0") || d.get(0..4) == Some(b"BVA0")) {
                        out.push(format!("F {}", rel));
                        for (name, d) in ents {
                            match d.get(0..4) {
                                Some(b"BCA0") => {
                                    out.push(format!(" E {}", name));
                                    if dump_bca(d, out).is_err() {
                                        out.push(" ERR".into());
                                    }
                                }
                                Some(b"BVA0") => {
                                    out.push(format!(" E {}", name));
                                    if dump_bva(&d, out).is_err() {
                                        out.push(" ERR".into());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }
    }
    for d in dirs {
        walk(root, &d, out);
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let root = Path::new(&a[1]);
    let mut out = Vec::new();
    walk(root, root, &mut out);
    println!("{}", out.join("\n"));
}
