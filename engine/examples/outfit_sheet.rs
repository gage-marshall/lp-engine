//! contact sheet of every outfit id of a heroine: outfit_sheet PREFIX OUT.png
use lp_engine::heroine::*;
use lp_engine::render::*;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let prefix = a[1].clone();
    let c = Path::new("../content");
    let w = Wardrobe::load(&c.join("assets/arc"), &prefix);
    let tables = FaceTables::from_json(&serde_json::from_slice(&std::fs::read(c.join("engine/face_tables.json")).unwrap()).unwrap()).unwrap();
    let mut outfits: Vec<u32> = w.parts.iter().filter_map(|(k, _)| { let rest = k.strip_prefix(&format!("{prefix}_01_"))?; rest.strip_suffix("_body")?.parse::<u32>().ok() }).collect();
    outfits.sort(); outfits.dedup();
    println!("outfits: {:?}", outfits);
    // OUTFIT env: show one outfit across its colour variants instead
    if let Ok(o) = std::env::var("OUTFIT") { let o: u32 = o.parse().unwrap(); outfits = (0..16).map(|c| o * 100 + c).collect(); }
    let (device, queue) = headless_device().unwrap();
    let (pw, ph) = (160u32, 240u32);
    let mut r = Renderer::new(device, queue, pw, ph, wgpu::TextureFormat::Rgba8Unorm);
    let (ro, _) = look("clean").unwrap();
    r.set_opts(ro);
    let cols = 10usize;
    let rows = (outfits.len() + cols - 1) / cols;
    let mut sheet = lp_engine::nns2d::Image::new(pw as usize * cols, ph as usize * rows);
    for (i, &o) in outfits.iter().enumerate() {
        let opts = if std::env::var("OUTFIT").is_ok() { ComposeOpts { outfit: o / 100, outfit_color: o % 100, ..ComposeOpts::default() } } else { ComposeOpts { outfit: o, ..ComposeOpts::default() } };
        let cm = match w.compose(&opts, Some(&tables)) { Ok(x) => x, Err(_) => continue };
        let fr = build_frame(&cm.tris, &cm.mats, &cm.toon, &cm.edge, &w.tex, &mut ImageCache::default());
        let mut v = fit_view(&cm.tris, std::f64::consts::PI, 0.0, [(-48, -18), (-180, 122), (-180, 180)]);
        v.center = [0.0, 100.0, 0.0]; v.ext = 70.0; v.ext_y = 70.0 * ph as f64 / pw as f64;
        let img = r.render_to_image(&fr, &v, pw, ph);
        let (ox, oy) = ((i % cols) * pw as usize, (i / cols) * ph as usize);
        for y in 0..ph as usize { for x in 0..pw as usize { let s = (y * pw as usize + x) * 4; let d = ((oy + y) * pw as usize * cols + ox + x) * 4; sheet.buf[d..d + 4].copy_from_slice(&img.buf[s..s + 4]); } }
    }
    let f = std::fs::File::create(&a[2]).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), sheet.w as u32, sheet.h as u32);
    enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&sheet.buf).unwrap();
}
