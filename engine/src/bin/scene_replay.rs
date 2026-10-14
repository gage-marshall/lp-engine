//! Replay a recorded session through the engine (the scene simulation + presentation, same path as `scene_player`) and
//! write what it draws and plays, for comparison with the sandbox and the emulator.
//!   scene_replay CONTENT INPUTS.json SAVE OUT_DIR [--updates A..B] [--every N] [--size WxH] [--ss N] [--no-interp]
//!                [--no-backdrop] [--wav]
//! INPUTS.json: engine/tests/data/<scene>/inputs.json. Writes OUT_DIR/u<update>.png (the state after each listed update, no
//! dialogue window) and with --wav OUT_DIR/soundtrack.wav (48 kHz) on the same frame clock.
use lp_engine::audio::{render_events, AudioDirector};
use lp_engine::player::*;
use lp_engine::render::*;
use std::path::{Path, PathBuf};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: scene_replay CONTENT INPUTS.json SAVE OUT_DIR [--updates A..B] [--every N] [--size WxH] [--ss N] [--no-interp] [--wav]");
        return;
    }
    let content = Path::new(&a[1]);
    let inp: serde_json::Value = serde_json::from_slice(&std::fs::read(&a[2]).expect("inputs")).expect("json");
    let out = PathBuf::from(&a[4]);
    std::fs::create_dir_all(&out).unwrap();
    let (mut range, mut every, mut w, mut h, mut ss, mut wav, mut backdrop) = ((0u64, u64::MAX), 1u64, 192u32, 256u32, 1u32, false, true);
    let mut opts = PlayerOptions { save: Some(PathBuf::from(&a[3])), month: inp["month"].as_u64().map(|x| x as u32), hour: inp["hour"].as_u64().map(|x| x as u32), interpolate: false, auto_advance: None, replay: Replay::from_json(&inp), ..PlayerOptions::default() };
    let mut i = 5;
    while i < a.len() {
        let v = a.get(i + 1).cloned().unwrap_or_default();
        match a[i].as_str() {
            "--updates" => { if let Some((x, y)) = v.split_once("..") { range = (x.parse().unwrap_or(0), y.parse().unwrap_or(u64::MAX)); } i += 1; }
            "--every" => { every = v.parse().unwrap_or(1).max(1); i += 1; }
            "--size" => { if let Some((x, y)) = v.split_once('x') { w = x.parse().unwrap_or(192); h = y.parse().unwrap_or(256); } i += 1; }
            "--ss" => { ss = v.parse().unwrap_or(1); i += 1; }
            "--no-interp" => opts.interpolate = false,
            "--no-backdrop" => backdrop = false,
            "--wav" => wav = true,
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    let last = inp["frames"][1].as_i64().unwrap();
    let event = inp["event"].as_u64().unwrap() as u32;
    let mut p = ScenePlayer::new(content, event, opts).expect("scene");
    let (device, queue) = headless_device().expect("no GPU adapter");
    let mut r = Renderer::new(device, queue, w * ss, h * ss, wgpu::TextureFormat::Rgba8Unorm);
    r.set_opts(p.render_opts.clone());
    let mut events = Vec::new();
    let mut last_update = None;
    let mut written = 0;
    while p.frame_no() < last {
        p.tick();
        events.extend(p.take_audio());
        let Some(st) = p.state() else { continue };
        let u = st.update;
        if last_update == Some(u) {
            continue;
        }
        last_update = Some(u);
        if u < range.0 || u > range.1 || (u - range.0) % every != 0 {
            continue;
        }
        if backdrop {
            if let Some(bg) = p.backdrop_image(w as usize, h as usize) { r.set_backdrop(&bg); }
        } else {
            r.set_backdrop(&gradient_backdrop(w as usize, h as usize));
        }
        let f = p.frame(w, h, 0.0).expect("frame");
        r.set_fade(f.fade);
        let img = r.render_to_image(&f.data, &f.view, w, h);
        let file = std::fs::File::create(out.join(format!("u{u:05}.png"))).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.w as u32, img.h as u32);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&img.buf).unwrap();
        written += 1;
    }
    println!("{written} frames -> {}", out.display());
    if wav {
        let rate = 48000;
        let mut audio = AudioDirector::open(content, rate).expect("audio");
        audio.levels = p.levels();
        let first = inp["frames"][0].as_i64().unwrap();
        let buf = render_events(&mut audio, &events, first, last - first + (2.0 * lp_engine::sim::clock::FRAME_HZ) as i64, p.slot);
        let pcm: Vec<u8> = buf.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        let mut wv = Vec::new();
        wv.extend_from_slice(b"RIFF");
        wv.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        wv.extend_from_slice(b"WAVEfmt ");
        wv.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 2] { wv.extend_from_slice(&v.to_le_bytes()); }
        wv.extend_from_slice(&rate.to_le_bytes());
        wv.extend_from_slice(&(rate * 4).to_le_bytes());
        for v in [4u16, 16] { wv.extend_from_slice(&v.to_le_bytes()); }
        wv.extend_from_slice(b"data");
        wv.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wv.extend_from_slice(&pcm);
        std::fs::write(out.join("soundtrack.wav"), wv).unwrap();
        let json: Vec<serde_json::Value> = events.iter().map(|(f, e)| serde_json::json!([f, format!("{e:?}")])).collect();
        std::fs::write(out.join("audio_events.json"), serde_json::to_vec(&json).unwrap()).unwrap();
        println!("soundtrack: {} events -> {}", events.len(), out.join("soundtrack.wav").display());
    }
}
