//! Play or render the audio of a script event: host transcript -> paced timeline -> sound driver.
//!   scene_audio CONTENT EVENT_ID [--wav OUT.wav] [--play] [--choose N,N] [--seed S] [--slot N]
//! --wav renders offline (48 kHz stereo); --play plays through the default output device in real time, printing each dialogue line as it starts.
use lp_engine::audio::*;
use lp_engine::game::GameState;
use lp_engine::host::*;
use lp_engine::machine::ScriptStore;
use std::path::Path;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: scene_audio CONTENT EVENT_ID [--wav OUT.wav] [--play] [--choose N,N] [--seed S]");
        return;
    }
    let content = Path::new(&a[1]);
    let event = u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("event id");
    let (mut wav, mut play, mut choices, mut seed) = (None, false, Vec::new(), 1u64);
    let mut i = 3;
    while i < a.len() {
        match a[i].as_str() {
            "--wav" => { i += 1; wav = Some(a[i].clone()); }
            "--play" => play = true,
            "--choose" => { i += 1; choices = a[i].split(',').filter_map(|x| x.parse().ok()).collect(); }
            "--seed" => { i += 1; seed = a[i].parse().unwrap_or(1); }
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    let store = Arc::new(ScriptStore::open(content).unwrap());
    let mut host = Host::new(store, GameState::new(seed));
    if let Ok(vtb) = std::fs::read(content.join("assets/sys/Siren14Voice.vtb")) {
        host.load_voice_lengths(&vtb);
    }
    let tr = host.run_event(event, &mut Scripted::new(choices)).expect("run event");
    let slot = host.state.current_slot;
    if let Some(path) = wav {
        let rate = 48000;
        let mut audio = AudioDirector::open(content, rate).expect("audio");
        let tl = pace(&mut audio, &tr.steps);
        let buf = render_timeline(&mut audio, &tl, slot, rate, 2.5);
        let peak = buf.iter().fold(0f32, |m, x| m.max(x.abs()));
        let pcm: Vec<u8> = buf.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
        let mut w = Vec::new();
        w.extend_from_slice(b"RIFF");
        w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes());
        w.extend_from_slice(b"WAVEfmt ");
        w.extend_from_slice(&16u32.to_le_bytes());
        for v in [1u16, 2] { w.extend_from_slice(&v.to_le_bytes()); }
        w.extend_from_slice(&rate.to_le_bytes());
        w.extend_from_slice(&(rate * 4).to_le_bytes());
        for v in [4u16, 16] { w.extend_from_slice(&v.to_le_bytes()); }
        w.extend_from_slice(b"data");
        w.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        w.extend_from_slice(&pcm);
        std::fs::write(&path, w).unwrap();
        for t in &tl {
            if let Effect::Dialogue { name, text, .. } = &t.effect {
                println!("{:7.2}s  {}{}", t.sec, if name.is_empty() { String::new() } else { format!("{name}: ") }, text.replace('\n', " / "));
            }
        }
        println!("wrote {path}: {:.1} s, peak {:.2}", buf.len() as f64 / 2.0 / rate as f64, peak);
    }
    if play {
        let out = AudioOutput::start(content).expect("audio output");
        let tl = {
            let mut d = out.director.lock().unwrap();
            pace(&mut d, &tr.steps)
        };
        let t0 = std::time::Instant::now();
        for t in &tl {
            let wait = t.sec - t0.elapsed().as_secs_f64();
            if wait > 0.0 {
                std::thread::sleep(std::time::Duration::from_secs_f64(wait));
            }
            if let Effect::Dialogue { name, text, .. } = &t.effect {
                println!("{}{}", if name.is_empty() { String::new() } else { format!("{name}: ") }, text.replace('\n', " / "));
            }
            out.director.lock().unwrap().apply(&t.effect, slot, false);
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    }
}
