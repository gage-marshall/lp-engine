//! The audio stack against the real extracted sound archive and voice bank (skipped when ../content is absent).
use lp_engine::audio::*;
use lp_engine::game::GameState;
use lp_engine::host::*;
use lp_engine::machine::ScriptStore;
use lp_engine::sdat::Sdat;
use lp_engine::snd::Driver;
use std::path::Path;
use std::sync::Arc;

fn content() -> Option<&'static Path> {
    let p = Path::new("../content");
    if p.join("assets/sys/sound_data.sdat").is_file() { Some(p) } else { None }
}

fn sdat(c: &Path) -> Arc<Sdat> {
    Arc::new(Sdat::parse(std::fs::read(c.join("assets/sys/sound_data.sdat")).unwrap()).unwrap())
}

fn peak(buf: &[f32]) -> f32 {
    buf.iter().fold(0.0, |m, x| m.max(x.abs()))
}

#[test]
fn the_archive_has_the_expected_contents() {
    let Some(c) = content() else { return };
    let s = sdat(c);
    assert_eq!(s.seqs.iter().flatten().count(), 152);
    assert_eq!(s.seqarc_len(0), Some(70));
    assert_eq!(s.seqarc_len(2), Some(198));
    assert_eq!(s.players.iter().flatten().count(), 5);
    assert!(s.wave(0, 0).is_some() || s.wavearcs.iter().flatten().count() > 100);
}

#[test]
fn every_bgm_sequence_plays_sound() {
    let Some(c) = content() else { return };
    let s = sdat(c);
    for id in 1..152 {
        let sd = s.sequence(id).unwrap();
        let mut d = Driver::new(s.clone(), 32000);
        d.start(&sd, sd.info.player as usize, 0).unwrap();
        let mut buf = vec![0f32; 32000 * 2 * 4];
        d.render(&mut buf);
        assert!(peak(&buf) > 0.001, "BGM {id} is silent in its first 4 seconds");
        assert!(peak(&buf) < 8.0, "BGM {id} is absurdly loud");
    }
}

#[test]
fn every_sequence_archive_entry_starts() {
    let Some(c) = content() else { return };
    let s = sdat(c);
    let mut silent = 0;
    let mut total = 0;
    for arc in 0..5 {
        for i in 0..s.seqarc_len(arc).unwrap() {
            let sd = s.archive_sequence(arc, i).unwrap();
            let mut d = Driver::new(s.clone(), 32000);
            if d.start(&sd, sd.info.player as usize, 0).is_none() {
                continue;
            }
            let mut buf = vec![0f32; 32000 * 2];
            d.render(&mut buf);
            total += 1;
            silent += (peak(&buf) < 1e-4) as usize;
        }
    }
    assert!(total > 300, "{total} entries started");
    assert!(silent * 10 < total, "{silent} of {total} sound effects are silent");
}

#[test]
fn bgm_aliases_resolve_per_heroine() {
    let Some(c) = content() else { return };
    let a = AudioDirector::open(c, 32000).unwrap();
    assert_eq!(a.resolve_bgm(1000, 0, false), 48);
    assert_eq!(a.resolve_bgm(1000, 2, false), 50);
    assert_eq!(a.resolve_bgm(1001, 1, true), 16);
    assert_eq!(a.resolve_bgm(5, 1, false), 5);
}

#[test]
fn a_real_scene_gets_voices_on_the_game_clock_and_an_audible_mix() {
    use lp_engine::player::{PlayerOptions, ScenePlayer};
    use lp_engine::sim::sched::ScriptEvent;
    let Some(c) = content() else { return };
    let mut p = ScenePlayer::new(c, 0xa000_6600, PlayerOptions::default()).unwrap();
    let mut events = Vec::new();
    while !p.finished() && p.seconds() < 300.0 {
        p.tick();
        events.extend(p.take_audio());
    }
    // lines come one after another (each waits for the tap that closes the previous one)
    let lines: Vec<u64> = p.script.iter().filter_map(|e| match e { ScriptEvent::Text { update, .. } => Some(*update), _ => None }).collect();
    assert!(lines.len() >= 3 && lines.windows(2).all(|w| w[1] > w[0] + 5), "{lines:?}");
    // the teacher's voice is started by the simulation (4 updates after its request) and is in the mix from that frame
    let (vf, _) = *events.iter().find(|(_, e)| *e == AudioEvent::Voice(3206)).expect("voice 3206");
    let mut audio = AudioDirector::open(c, 32000).unwrap();
    let buf = render_events(&mut audio, &events, 0, p.frame_no() + 60, p.slot);
    let at = (vf as f64 / lp_engine::sim::clock::FRAME_HZ * 32000.0) as usize * 2;
    assert!(peak(&buf[at..at + 32000]) > 0.05, "voice audible at its start");
}

#[test]
fn voice_clips_decode_with_few_errors() {
    let Some(c) = content() else { return };
    let mut a = AudioDirector::open(c, 32000).unwrap();
    for clip in [0u32, 1, 3206, 5736, 19202, 40000] {
        let pcm = a.voice_clip(clip).unwrap();
        assert!(pcm.len() > 3200, "clip {clip}: {} samples", pcm.len());
        let p = pcm.iter().map(|&x| x.unsigned_abs()).max().unwrap();
        assert!(p > 500, "clip {clip} is silent");
    }
    assert!(a.voice_clip(99_999).is_none());
}
