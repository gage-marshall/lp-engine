use lp_engine::heroine::*;
use lp_engine::render::*;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (stem, frame) = (a[1].clone(), a[2].parse::<usize>().unwrap());
    let c = Path::new("../content");
    let w = Wardrobe::load(&c.join("assets/arc"), "m");
    let tables = FaceTables::from_json(&serde_json::from_slice(&std::fs::read(c.join("engine/face_tables.json")).unwrap()).unwrap()).unwrap();
    let (device, queue) = headless_device().unwrap();
    let (pw, ph) = (400u32, 500u32);
    let mut r = Renderer::new(device, queue, pw * 2, ph * 2, wgpu::TextureFormat::Rgba8Unorm);
    let (ro, _) = look("clean").unwrap();
    r.set_opts(ro);
    let mut sheet = lp_engine::nns2d::Image::new(pw as usize * 4, ph as usize);
    for (i, kinds) in [vec!["body", "larm", "rarm", "waist", "leg", "hair", "face", "lhand", "rhand"], vec!["face"], vec!["hair"], vec!["body", "face"]].iter().enumerate() {
        let all = w.part_sets(0, 0);
        let parts: Vec<(&str, String)> = all.into_iter().filter(|(k, _)| kinds.contains(k)).collect();
        let (tris, mut mats) = w.build(&parts, &Some(stem.clone()), frame).unwrap();
        w.apply_expression(&mut mats, &tables.face_for(0, 5, 0, false));
        let sh = w.shading(0, 0, 0, 0, 0).unwrap();
        let mut cache = ImageCache::default();
        let f = build_frame(&tris, &mats, &sh.0, &sh.1, &w.tex, &mut cache);
        let mut v = fit_view(&tris, std::f64::consts::PI, 0.0, [(-48, -18), (-180, 122), (-180, 180)]);
        v.center = [0.0, 125.0, 0.0];
        v.ext = 45.0; v.ext_y = 45.0;
        let img = r.render_to_image(&f, &v, pw, ph);
        for y in 0..ph as usize { for x in 0..pw as usize { let s = (y * pw as usize + x) * 4; let d = (y * pw as usize * 4 + i * pw as usize + x) * 4; sheet.buf[d..d + 4].copy_from_slice(&img.buf[s..s + 4]); } }
    }
    let f = std::fs::File::create(&a[3]).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), sheet.w as u32, sheet.h as u32);
    enc.set_color(png::ColorType::Rgba); enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&sheet.buf).unwrap();
}
