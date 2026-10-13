//! Integration tests against a real extracted content tree (skipped when ../content is absent).
use lp_engine::heroine::*;
use lp_engine::render::*;
use lp_engine::{camera, nns2d, nns3d, text};
use std::path::Path;
use std::sync::Arc;

fn content() -> Option<&'static Path> {
    let p = Path::new("../content");
    if p.join("assets/arc/m_block3d_01.arc").is_dir() { Some(p) } else { None }
}

#[test]
fn every_model_in_a_costume_archive_parses_and_meshes() {
    let Some(c) = content() else { return };
    let dir = c.join("assets/arc/m_block3d_01.arc");
    let (mut models, mut tris) = (0, 0);
    for e in std::fs::read_dir(dir).unwrap() {
        let b = std::fs::read(e.unwrap().path()).unwrap();
        if b.get(0..4) != Some(b"BMD0") {
            continue;
        }
        let f = nns3d::parse_file(Arc::new(b)).unwrap();
        for m in &f.models {
            tris += nns3d::build_mesh(m, None, true, None).unwrap().len();
            models += 1;
        }
    }
    assert!(models > 50 && tris > 10_000, "{models} models, {tris} triangles");
}

#[test]
fn heroines_assemble_and_animate() {
    let Some(c) = content() else { return };
    let tables = FaceTables::from_json(&serde_json::from_str(&std::fs::read_to_string(c.join("engine/face_tables.json")).unwrap()).unwrap()).unwrap();
    for prefix in ["m", "r", "n"] {
        let w = Wardrobe::load(&c.join("assets/arc"), prefix);
        assert!(w.parts.iter().count() > 300, "{prefix}: parts");
        assert!(w.motions.len() > 50, "{prefix}: motions");
        let rest = w.compose(&ComposeOpts { outfit: 3, ..Default::default() }, Some(&tables)).unwrap();
        assert!(rest.tris.len() > 1000, "{prefix}: {} triangles", rest.tris.len());
        let motion = w.motions.keys().nth(10).cloned();
        let posed = w.compose(&ComposeOpts { outfit: 3, motion, frame: 7, ..Default::default() }, Some(&tables)).unwrap();
        assert_eq!(posed.tris.len() >= 1000, true);
        // animation really moves the mesh
        let moved = rest.tris.iter().zip(&posed.tris).any(|(a, b)| a.verts[0].pos != b.verts[0].pos);
        assert!(moved, "{prefix}: motion did not move any vertex");
        assert_eq!(rest.toon.len(), 32);
        assert_eq!(rest.edge.len(), 8);
    }
}

#[test]
fn camera_presets_and_system_text_load() {
    let Some(c) = content() else { return };
    let p = camera::load_presets(&c.join("assets/arc"));
    assert_eq!(p.len(), 150);
    assert_eq!(p[&1000].eye, [0.0, 144.0, 150.0]);
    assert_eq!(p[&1000].lights[0], (-48, -18));
    let t = text::Txrc::parse(&std::fs::read(c.join("assets/text/gd_textres_jpn.txrc")).unwrap()).unwrap();
    assert_eq!(t.line_str(0, 0, 0).unwrap(), "あいざわ");
    let nclr = nns2d::parse_nclr(&std::fs::read(c.join("assets/gift/card_bg_arc.bin/0121.nclr")).unwrap()).unwrap();
    assert_eq!(nclr.colors.len(), 256);
}

#[test]
fn heroine_renders_on_the_gpu() {
    let Some(c) = content() else { return };
    let Some((device, queue)) = headless_device() else { return };
    let w = Wardrobe::load(&c.join("assets/arc"), "m");
    let comp = w.compose(&ComposeOpts::default(), None).unwrap();
    let mut cache = ImageCache::default();
    let f = build_frame(&comp.tris, &comp.mats, &comp.toon, &comp.edge, &w.tex, &mut cache);
    let mut r = Renderer::new(device, queue, 512, wgpu::TextureFormat::Rgba8Unorm);
    let v = fit_view(&comp.tris, std::f64::consts::PI, 0.0, [(-48, -18), (-180, 122), (-180, 180)]);
    let img = r.render_to_image(&f, &v, 256);
    let bg = [46i32, 46, 58];
    let fg = (0..256 * 256).filter(|i| (0..3).any(|k| (img.buf[i * 4 + k] as i32 - bg[k]).abs() > 12)).count();
    assert!(fg > 3000 && fg < 40_000, "figure covers {fg} pixels");
}
