//! Render the soundtrack of a script event offline: the scene simulation (headless, lines closed automatically) -> sound and
//! voice events on the game's frame clock -> sound driver.
//!   scene_audio CONTENT EVENT_ID --wav OUT.wav [--save BACKUP.sav] [--month M] [--hour H] [--choose N,N] [--advance FRAMES]
//!                [--seconds S]
use lp_engine::audio::*;
use lp_engine::player::{PlayerOptions, ScenePlayer};
use lp_engine::sim::clock::FRAME_HZ;
use lp_engine::sim::sched::ScriptEvent;
use std::path::{Path, PathBuf};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: scene_audio CONTENT EVENT_ID --wav OUT.wav [--save BACKUP.sav] [--month M] [--hour H] [--choose N,N] [--advance FRAMES] [--seconds S]");
        return;
    }
    let content = Path::new(&a[1]);
    let event = u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("event id");
    let mut opts = PlayerOptions::default();
    let (mut wav, mut limit) = (None, 600.0f64);
    let mut i = 3;
    while i < a.len() {
        let v = a.get(i + 1).cloned().unwrap_or_default();
        match a[i].as_str() {
            "--wav" => { wav = Some(v); i += 1; }
            "--save" => { opts.save = Some(PathBuf::from(v)); i += 1; }
            "--month" => { opts.month = v.parse().ok(); i += 1; }
            "--hour" => { opts.hour = v.parse().ok(); i += 1; }
            "--choose" => { opts.choices = v.split(',').filter_map(|x| x.parse().ok()).collect(); i += 1; }
            "--advance" => { opts.auto_advance = v.parse().ok(); i += 1; }
            "--seconds" => { limit = v.parse().unwrap_or(600.0); i += 1; }
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    let Some(path) = wav else { eprintln!("--wav OUT.wav is required"); return };
    let mut p = ScenePlayer::new(content, event, opts).expect("scene");
    let mut events = Vec::new();
    while !p.finished() && p.seconds() < limit {
        p.tick();
        events.extend(p.take_audio());
    }
    let frames = p.frame_no() + (2.5 * FRAME_HZ) as i64;
    let rate = 48000;
    let mut audio = AudioDirector::open(content, rate).expect("audio");
    audio.levels = p.levels();
    let buf = render_events(&mut audio, &events, 0, frames, p.slot);
    let peak = buf.iter().fold(0f32, |m, x| m.max(x.abs()));
    let pcm: Vec<u8> = buf.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    for v in [1u16, 2] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate * 4).to_le_bytes());
    for v in [4u16, 16] {
        w.extend_from_slice(&v.to_le_bytes());
    }
    w.extend_from_slice(b"data");
    w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
    w.extend_from_slice(&pcm);
    std::fs::write(&path, w).unwrap();
    for e in &p.script {
        if let ScriptEvent::Text { update, name, text, .. } = e {
            println!("update {update:5}  {}{}", if name.is_empty() { String::new() } else { format!("{name}: ") }, text.replace('\n', " / "));
        }
    }
    println!("wrote {path}: {:.1} s, peak {:.2}, {} sound events", buf.len() as f64 / 2.0 / rate as f64, peak, events.len());
}
