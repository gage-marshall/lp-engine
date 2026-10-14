//! Sequence part of a scene soundtrack: every sequence in ONE engine driver (they share the 14 channels of mask 0xfcff, so
//! channel stealing happens as on the DS), with the NitroSystem fader stepped once per game frame.
//!   audio_mix CONTENT EVENTS.json OUT.wav
//! EVENTS.json: {"rate": 48000, "seconds": S, "frame_hz": 59.8261, "levels": [bgm, se, voice],
//!               "events": [{"t": sec, "op": "start"|"stop", "slot": 0|1, "seq": N | "arc": [A, I], "fade": frames}]}
//! Fader (TwlSystem snd fader.c / player.c): volume v moves linearly to its target over `fade` frames (start: 0 -> 127,
//! stop: current -> 0, then the sequence ends); its level is SND_CalcDecibel(v) = squared law, i.e. amplitude (v/127)^2.
//! Slot volume (GameSnd_ApplyChannelVolume 0x20724f8): 105 * level / 15, applied the same way (extVolume).
use lp_engine::sdat::Sdat;
use lp_engine::snd::{Driver, SeqHandle};
use serde_json::Value;
use std::{fs, path::Path, sync::Arc};

struct Fader { origin: f64, target: f64, frame: u32, counter: u32 }
impl Fader {
    fn get(&self) -> f64 { if self.counter >= self.frame { self.target } else { self.origin + (self.target - self.origin) * self.counter as f64 / self.frame as f64 } }
    fn set(&mut self, target: f64, frame: u32) { self.origin = self.get(); self.target = target; self.frame = frame; self.counter = 0; }
    fn update(&mut self) { if self.counter < self.frame { self.counter += 1; } }
    fn done(&self) -> bool { self.counter >= self.frame }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let content = Path::new(&a[1]);
    let ev: Value = serde_json::from_slice(&fs::read(&a[2]).unwrap()).unwrap();
    let rate = ev["rate"].as_u64().unwrap_or(48000) as u32;
    let seconds = ev["seconds"].as_f64().unwrap();
    let fhz = ev["frame_hz"].as_f64().unwrap_or(59.8261);
    let levels: Vec<f64> = ev["levels"].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
    let sdat = Arc::new(Sdat::parse(fs::read(content.join("assets/sys/sound_data.sdat")).unwrap()).unwrap());
    let mut drv = Driver::new(sdat.clone(), rate);
    let mut events: Vec<Value> = ev["events"].as_array().unwrap().clone();
    events.sort_by(|x, y| x["t"].as_f64().partial_cmp(&y["t"].as_f64()).unwrap());
    // per slot: (handle, fader, stopping)
    let mut slots: Vec<Option<(SeqHandle, Fader, bool)>> = vec![None, None];
    let sq = |v: f64| (v / 127.0).powi(2);
    let ext = |slot: usize| { let lvl = if slot == 0 { levels[0] } else { levels[1] }; sq((105.0 * lvl / 15.0).floor().min(127.0)) };
    let total_frames = (seconds * fhz) as usize;
    let spf = rate as f64 / fhz;
    let mut out = vec![0f32; ((total_frames as f64 * spf) as usize + 16) * 2];
    let mut pos = 0usize; let mut ei = 0;
    for gf in 0..total_frames {
        let t = gf as f64 / fhz;
        while ei < events.len() && events[ei]["t"].as_f64().unwrap() <= t {
            let e = &events[ei]; let slot = e["slot"].as_u64().unwrap() as usize; let fade = e["fade"].as_u64().unwrap_or(0) as u32;
            match e["op"].as_str().unwrap() {
                "start" => {
                    if let Some((h, _, _)) = slots[slot].take() { drv.stop(h, 0); }
                    let sd = if let Some(n) = e["seq"].as_u64() { sdat.sequence(n as usize) } else {
                        let ar = e["arc"].as_array().unwrap(); sdat.archive_sequence(ar[0].as_u64().unwrap() as usize, ar[1].as_u64().unwrap() as usize) };
                    if let Some(sd) = sd {
                        if let Some(h) = drv.start(&sd, sd.info.player as usize, 0) {
                            let mut f = Fader { origin: 0.0, target: 0.0, frame: 0, counter: 0 };
                            f.set(127.0, 1);                               // player init: 0 -> 127 in one frame
                            if fade > 0 { f.set(127.0, fade); }            // GameSnd_PlaySeq: NNS_SndPlayerMoveVolume(127, fade)
                            drv.fade_to(h, 0.0, 0);
                            slots[slot] = Some((h, f, false));
                        }
                    }
                }
                "stop" => {
                    if let Some((h, mut f, _)) = slots[slot].take() {
                        if fade == 0 { drv.stop(h, 0); } else { f.set(0.0, fade); slots[slot] = Some((h, f, true)); }
                    }
                }
                _ => {}
            }
            ei += 1;
        }
        for s in 0..2 {
            let mut end = false;
            if let Some((h, f, stopping)) = slots[s].as_mut() {
                f.update();
                drv.fade_to(*h, (sq(f.get()) * ext(s)) as f32, 0);
                if *stopping && f.done() { drv.stop(*h, 0); end = true; }
            }
            if end { slots[s] = None; }
        }
        let n = ((gf + 1) as f64 * spf) as usize - (gf as f64 * spf) as usize;
        drv.render(&mut out[pos * 2..(pos + n) * 2]);
        pos += n;
    }
    out.truncate(pos * 2);
    let pcm: Vec<u8> = out.iter().flat_map(|x| ((x.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect();
    let mut w = Vec::new();
    w.extend_from_slice(b"RIFF"); w.extend_from_slice(&(36 + pcm.len() as u32).to_le_bytes()); w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); for v in [1u16, 2] { w.extend_from_slice(&v.to_le_bytes()); }
    w.extend_from_slice(&rate.to_le_bytes()); w.extend_from_slice(&(rate * 4).to_le_bytes());
    for v in [4u16, 16] { w.extend_from_slice(&v.to_le_bytes()); }
    w.extend_from_slice(b"data"); w.extend_from_slice(&(pcm.len() as u32).to_le_bytes()); w.extend_from_slice(&pcm);
    fs::write(&a[3], w).unwrap();
    eprintln!("sequences: {} game frames, peak {:.3}", total_frames, out.iter().fold(0f32, |m, x| m.max(x.abs())));
}
