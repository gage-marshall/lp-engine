//! The scene player on a real recorded event (skipped when ../content is absent).
use lp_engine::player::*;
use lp_engine::render::*;
use std::path::Path;

fn content() -> Option<&'static Path> {
    let p = Path::new("../content");
    if p.join("assets/arc/m_block3d_01.arc").is_dir() && p.join("assets/test_font").is_dir() { Some(p) } else { None }
}

/// run until the first line has `n` characters on screen
fn to_first_line(p: &mut ScenePlayer, n: usize) {
    for _ in 0..20_000 {
        p.tick();
        if p.script.iter().any(|e| matches!(e, lp_engine::sim::sched::ScriptEvent::Text { .. })) && p.revealed().map_or(false, |r| r >= n) {
            return;
        }
    }
    panic!("no line with {n} characters");
}

#[test]
fn frames_carry_camera_pose_and_dialogue_ui() {
    let Some(c) = content() else { return };
    let mut p = ScenePlayer::new(c, 0xa44c_1b00, PlayerOptions::default()).unwrap();
    assert_eq!(p.slot, 0);
    // before the actor setup no model is drawn (the game creates the heroine at cmd 2)
    while p.state().is_none() {
        p.tick();
    }
    if p.sim.last.as_ref().map_or(false, |f| f.chain.is_empty()) {
        assert!(p.frame(600, 800, 0.0).unwrap().data.verts.is_empty(), "no bind pose before the actor setup");
    }
    // once her first motion runs: the heroine, the game camera, no dialogue window yet
    while p.sim.last.as_ref().map_or(true, |f| f.chain.is_empty()) {
        p.tick();
    }
    let f0 = p.frame(600, 800, 0.0).unwrap();
    assert!(f0.data.verts.len() > 5_000, "tessellated heroine: {}", f0.data.verts.len());
    assert!(f0.view.game.is_some() && f0.ui.is_none());
    // the first line typing: window with name tag and some characters
    to_first_line(&mut p, 3);
    let f1 = p.frame(600, 800, 0.0).unwrap();
    let ui = f1.ui.expect("dialogue box drawn");
    let opaque = ui.buf.chunks_exact(4).filter(|px| px[3] > 0).count();
    assert!(opaque > 20_000, "box pixels {opaque}");
    // the same state again does not redraw the window
    assert!(p.frame(600, 800, 0.0).unwrap().ui.is_none());
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
    to_first_line(&mut p, 3);
    let f = p.frame(w, h, 0.0).unwrap();
    if let Some(ui) = &f.ui { r.set_overlay(ui); }
    r.set_fade(0.0);
    let img = r.render_to_image(&f.data, &f.view, w, h);
    let px = |x: usize, y: usize| [img.buf[(y * w as usize + x) * 4], img.buf[(y * w as usize + x) * 4 + 1], img.buf[(y * w as usize + x) * 4 + 2]];
    assert!(px(5, 5)[2] > px(5, 5)[0], "sky is blue-ish: {:?}", px(5, 5));
    assert!(px(150, 330).iter().map(|&v| v as u32).sum::<u32>() < 250, "dialogue box is dark: {:?}", px(150, 330));
    r.set_fade(1.0);
    let black = r.render_to_image(&f.data, &f.view, w, h);
    assert!(black.buf.chunks_exact(4).all(|p| p[0] < 6 && p[1] < 6 && p[2] < 6));
}

/// backdrops the real game loaded in recorded play (tools/bg_samples.py): (event id, month, time period, archive, entry)
#[test]
fn backdrop_selection_matches_the_recorded_game() {
    use lp_engine::backdrop::backdrop_for;
    let content = std::path::Path::new("../content");
    if !content.join("engine/bg_select.json").exists() {
        return;
    }
    let samples: &[(u32, u32, u32, &str, usize)] = &[
        (0x5c4c1b00, 10, 1, "bg_school", 16),
        (0x58143403, 10, 3, "bg_school", 17),
        (0x80286400, 10, 1, "bg_school", 20),
        (0x58143402, 10, 3, "bg_school", 30),
        (0x58143404, 10, 3, "bg_school", 61),
        (0xa0143400, 10, 3, "bg_school", 9),
        (0x802a6400, 10, 1, "bg_school", 37),
        (0x5c4c1600, 10, 2, "bg_school", 50),
        (0xa0146600, 10, 3, "bg_school", 42),
        (0x08143400, 10, 3, "bg_center", 34),
        (0x08143402, 10, 3, "bg_center", 41),
        (0x58146600, 10, 4, "bg_town", 2),
    ];
    for &(ev, month, period, arc, entry) in samples {
        let (c, img) = backdrop_for(content, ev, month, lp_engine::scene_inputs::tod_of_period(period)).unwrap_or_else(|| panic!("no backdrop for {ev:08x}"));
        assert_eq!((c.archive.as_str(), c.entry), (arc, entry), "event {ev:08x}");
        assert_eq!((img.w, img.h), (384, 256), "the BG1 panorama");
    }
}

/// every backdrop in the recorded play (engine/tests/data/bg_samples.json, from tools/bg_samples.py): the Rust selection must name the same archive entry
#[test]
fn backdrop_selection_matches_all_recorded_samples() {
    use lp_engine::backdrop::backdrop_for;
    let content = std::path::Path::new("../content");
    let data = match std::fs::read("tests/data/bg_samples.json") { Ok(d) => d, Err(_) => return };
    if !content.join("engine/bg_select.json").exists() {
        return;
    }
    let v: serde_json::Value = serde_json::from_slice(&data).unwrap();
    let mut bad = Vec::new();
    for s in v.as_array().unwrap() {
        let ev = u32::from_str_radix(s["event"].as_str().unwrap(), 16).unwrap();
        let (m, p) = (s["month"].as_u64().unwrap() as u32, s["period"].as_u64().unwrap() as u32);
        match backdrop_for(content, ev, m, lp_engine::scene_inputs::tod_of_period(p)) {
            Some((c, _)) if c.archive == s["archive"].as_str().unwrap() && c.entry as u64 == s["entry"].as_u64().unwrap() => {}
            other => bad.push(format!("{ev:08x} m{m} p{p}: game {} {} vs {:?}", s["archive"], s["entry"], other.map(|(c, _)| (c.archive, c.entry)))),
        }
    }
    assert!(bad.is_empty(), "{} mismatches:\n{}", bad.len(), bad.join("\n"));
}
