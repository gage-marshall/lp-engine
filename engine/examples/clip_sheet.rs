use lp_engine::heroine::*;
use lp_engine::render::*;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let stem = a[1].clone();
    let c = Path::new("../content");
    let w = Wardrobe::load(&c.join("assets/arc"), "m");
    let nf = w.motions[&stem].bca.iter().map(|x| x.frames).max().unwrap();
    println!("{stem}: {nf} frames");
    let (device, queue) = headless_device().unwrap();
    let (pw, ph) = (200u32, 300u32);
    let mut r = Renderer::new(device, queue, pw, ph, wgpu::TextureFormat::Rgba8Unorm);
    let (ro, _) = look("clean").unwrap();
    r.set_opts(ro);
    let n = 12usize;
    let mut sheet = lp_engine::nns2d::Image::new(pw as usize * 6, ph as usize * 2);
    for i in 0..n {
        let f = i * (nf - 1) / (n - 1);
        let o = ComposeOpts { motion: Some(stem.clone()), frame: f, ..ComposeOpts::default() };
        let cm = w.compose(&o, None).unwrap();
        let fr = build_frame(&cm.tris, &cm.mats, &cm.toon, &cm.edge, &w.tex, &mut ImageCache::default());
        let mut v = fit_view(&cm.tris, std::f64::consts::PI, 0.0, [(-48, -18), (-180, 122), (-180, 180)]);
        v.center = [0.0, 70.0, 0.0]; v.ext = 100.0; v.ext_y = 100.0 * ph as f64 / pw as f64 * 1.0;
        let img = r.render_to_image(&fr, &v, pw, ph);
        let (ox, oy) = ((i % 6) * pw as usize, (i / 6) * ph as usize);
        for y in 0..ph as usize { for x in 0..pw as usize { let s = (y * pw as usize + x) * 4; let d = ((oy + y) * pw as usize * 6 + ox + x) * 4; sheet.buf[d..d + 4].copy_from_slice(&img.buf[s..s + 4]); } }
    }
    let f = std::fs::File::create(&a[2]).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), sheet.w as u32, sheet.h as u32);
    enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&sheet.buf).unwrap();
}
