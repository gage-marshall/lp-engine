//! The scene player on a real recorded event (skipped when ../content is absent).
use lp_engine::player::*;
use lp_engine::render::*;
use std::path::Path;

fn content() -> Option<&'static Path> {
    let p = Path::new("../content");
    if p.join("assets/arc/m_block3d_01.arc").is_dir() && p.join("assets/test_font").is_dir() { Some(p) } else { None }
}

#[test]
fn frames_carry_camera_pose_and_dialogue_ui() {
    let Some(c) = content() else { return };
    let mut p = ScenePlayer::new(c, 0xa44c_1b00, PlayerOptions::default()).unwrap();
    assert_eq!(p.slot, 0);
    assert!(p.duration > 10.0 && p.timeline.len() > 50);
    // before the first line: heroine with the idle pose, no dialogue box
    let f0 = p.frame(0.5, 600, 800).unwrap();
    assert!(f0.data.verts.len() > 5_000, "tessellated heroine: {}", f0.data.verts.len());
    assert!(f0.view.camera.is_some() && f0.ui.is_none());
    // a few seconds in, the first line is on screen with the name tag and some revealed text
    let f1 = p.frame(2.5, 600, 800).unwrap();
    let ui = f1.ui.expect("dialogue box drawn");
    let opaque = ui.buf.chunks_exact(4).filter(|px| px[3] > 0).count();
    assert!(opaque > 20_000, "box pixels {opaque}");
    // asking for the same moment again does not redraw the UI
    assert!(p.frame(2.5, 600, 800).unwrap().ui.is_none());
    // going back in time replays the scene from the start
    let f2 = p.frame(0.5, 600, 800).unwrap();
    assert!(f2.clear_ui);
}

#[test]
fn a_scene_renders_with_backdrop_dialogue_and_fade_on_the_gpu() {
    let Some(c) = content() else { return };
    let Some((device, queue)) = headless_device() else { return };
    let mut p = ScenePlayer::new(c, 0xa44c_1b00, PlayerOptions::default()).unwrap();
    let (w, h) = (300u32, 400u32);
    let mut r = Renderer::new(device, queue, w * 2, h * 2, wgpu::TextureFormat::Rgba8Unorm);
    r.set_opts(p.render_opts.clone());
    r.set_backdrop(&gradient_backdrop(w as usize, h as usize));
    let f = p.frame(2.5, w, h).unwrap();
    if let Some(ui) = &f.ui { r.set_overlay(ui); }
    r.set_fade(0.0);
    let img = r.render_to_image(&f.data, &f.view, w, h);
    let px = |x: usize, y: usize| [img.buf[(y * w as usize + x) * 4], img.buf[(y * w as usize + x) * 4 + 1], img.buf[(y * w as usize + x) * 4 + 2]];
    // backdrop top is blue, the lower box region is dark, the middle has the heroine (not backdrop colored)
    assert!(px(5, 5)[2] > px(5, 5)[0], "sky is blue-ish: {:?}", px(5, 5));
    assert!(px(150, 330).iter().map(|&v| v as u32).sum::<u32>() < 250, "dialogue box is dark: {:?}", px(150, 330));
    // fading to black darkens everything
    r.set_fade(1.0);
    let black = r.render_to_image(&f.data, &f.view, w, h);
    assert!(black.buf.chunks_exact(4).all(|p| p[0] < 6 && p[1] < 6 && p[2] < 6));
}
