//! Run a script event through the headless host and print a readable transcript.
//!   host_run CONTENT EVENT_ID [--choose N,N,...] [--seed S] [--reply CMD=VALUE ...] [--player NAME] [--quiet]
//! EVENT_ID accepts 0x... hex.  --quiet hides the non-dialogue effects.
use lp_engine::game::GameState;
use lp_engine::host::*;
use lp_engine::machine::ScriptStore;
use std::path::Path;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: host_run CONTENT EVENT_ID [--choose N,N] [--seed S] [--reply CMD=VALUE] [--player NAME] [--quiet]");
        return;
    }
    let content = Path::new(&a[1]);
    let event = u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("event id");
    let (mut choices, mut seed, mut quiet, mut player) = (Vec::new(), 1u64, false, None);
    let mut replies = Vec::new();
    let mut i = 3;
    while i < a.len() {
        match a[i].as_str() {
            "--choose" => {
                i += 1;
                choices = a[i].split(',').filter_map(|x| x.parse().ok()).collect();
            }
            "--seed" => {
                i += 1;
                seed = a[i].parse().unwrap_or(1);
            }
            "--reply" => {
                i += 1;
                if let Some((c, v)) = a[i].split_once('=') {
                    replies.push((u32::from_str_radix(c.trim_start_matches("0x"), 16).unwrap_or(0), v.parse::<u32>().unwrap_or(0)));
                }
            }
            "--player" => {
                i += 1;
                player = Some(a[i].clone());
            }
            "--quiet" => quiet = true,
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    let store = Arc::new(ScriptStore::open(content).expect("script store"));
    let mut state = GameState::new(seed);
    if let Some(p) = player {
        state.player_name = p;
    }
    let mut host = Host::new(store, state);
    if let Ok(vtb) = std::fs::read(content.join("assets/sys/Siren14Voice.vtb")) {
        host.load_voice_lengths(&vtb);
    }
    let mut pol = Scripted::new(choices);
    pol.queries.extend(replies);
    let tr = host.run_event(event, &mut pol).expect("run");
    for s in &tr.steps {
        let dialogue = matches!(s.effect, Effect::Dialogue { .. });
        if quiet && !dialogue && !matches!(s.effect, Effect::Ended { .. } | Effect::Stopped { .. }) {
            continue;
        }
        println!("{:8.1}  {}", s.t, s.effect);
    }
    println!("-- {:?}, {} effects, {:.1} ticks ({:.1} s)", tr.end, tr.steps.len(), tr.ticks, tr.ticks * 3.8 / 60.0);
}
