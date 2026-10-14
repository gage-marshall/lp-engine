//! Render game audio offline to a WAV file with the Rust sound driver.
//!   audio_render CONTENT OUT.wav [--bgm N | --arc A:I | --voice N] [--seconds S] [--rate HZ]
use lp_engine::sdat::Sdat;
use lp_engine::siren::{decode_clip, Tables};
use lp_engine::snd::Driver;
use std::fs;
use std::path::Path;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = Path::new(&a[1]);
    let out = &a[2];
    let (mut bgm, mut arc, mut voice, mut seconds, mut rate) = (None, None, None, 20.0f64, 48000u32);
    let mut i = 3;
    while i < a.len() {
        match a[i].as_str() {
            "--bgm" => { i += 1; bgm = a[i].parse::<usize>().ok(); }
            "--arc" => { i += 1; let (x, y) = a[i].split_once(':').unwrap(); arc = Some((x.parse::<usize>().unwrap(), y.parse::<usize>().unwrap())); }
            "--voice" => { i += 1; voice = a[i].parse::<usize>().ok(); }
            "--seconds" => { i += 1; seconds = a[i].parse().unwrap_or(20.0); }
            "--rate" => { i += 1; rate = a[i].parse().unwrap_or(48000); }
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    let sdat = Arc::new(Sdat::parse(fs::read(c.join("assets/sys/sound_data.sdat")).unwrap()).unwrap());
    let mut drv = Driver::new(sdat.clone(), rate);
    if let Some(n) = bgm {
        let sd = sdat.sequence(n).expect("sequence");
        drv.start(&sd, sd.info.player as usize, 0).expect("start");
    }
    if let Some((x, y)) = arc {
        let sd = sdat.archive_sequence(x, y).expect("archive sequence");
        drv.start(&sd, sd.info.player as usize, 0).expect("start");
    }
    if let Some(v) = voice {
        let t = Tables::from_pack(&fs::read(c.join("engine/g7221.pack")).unwrap()).unwrap();
        let vtb = fs::read(c.join("assets/sys/Siren14Voice.vtb")).unwrap();
        let spp = fs::read(c.join("assets/sys/Siren14Voice.spp")).unwrap();
        let (off, size) = (u32::from_le_bytes(vtb[v * 8..v * 8 + 4].try_into().unwrap()) as usize / 16, u32::from_le_bytes(vtb[v * 8 + 4..v * 8 + 8].try_into().unwrap()) as usize / 16);
        let (pcm, _) = decode_clip(&t, &spp[off..off + size], 16000, 14000);
        drv.play_clip(Arc::new(pcm), 32000, 1.0, 1.0);
    }
    let n = (seconds * rate as f64) as usize;
    let mut buf = vec![0f32; n * 2];
    for chunk in buf.chunks_mut(2 * 4800) {
        drv.render(chunk);
    }
    let peak = buf.iter().fold(0f32, |m, x| m.max(x.abs()));
    let rms = (buf.iter().map(|x| x * x).sum::<f32>() / buf.len() as f32).sqrt();
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
    fs::write(out, w).unwrap();
    println!("{} s, peak {:.3}, rms {:.3}, {} channels active at end, {} sequences running", seconds, peak, rms, drv.active_channels(), drv.running());
}
