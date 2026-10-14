//! Run a script event through the headless host and print a readable transcript.
//!   host_run CONTENT EVENT_ID [--choose N,N,...] [--seed S] [--reply CMD=VALUE ...] [--player NAME] [--save SAV] [--slot N] [--quiet]
//! EVENT_ID accepts 0x... hex.  --quiet hides the non-dialogue effects.
use lp_engine::game::GameState;
use lp_engine::host::*;
use lp_engine::machine::ScriptStore;
use std::path::Path;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 3 {
        eprintln!("usage: host_run CONTENT EVENT_ID [--choose N,N] [--seed S] [--reply CMD=VALUE] [--player NAME] [--save SAV] [--slot N] [--quiet]");
        return;
    }
    let content = Path::new(&a[1]);
    let event = u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("event id");
    let mut raw = false;
    let (mut month, mut hour) = (None::<u32>, None::<u32>);
    let (mut choices, mut seed, mut quiet, mut player) = (Vec::new(), 1u64, false, None);
    let mut replies = Vec::new();
    let (mut save, mut slot) = (None::<std::path::PathBuf>, None::<u32>);
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
            "--save" => {
                i += 1;
                save = Some(std::path::PathBuf::from(&a[i]));
            }
            "--slot" => {
                i += 1;
                slot = a[i].parse().ok();
            }
            "--quiet" => quiet = true,
            "--cmds" => raw = true,
            "--month" => {
                i += 1;
                month = a[i].parse().ok();
            }
            "--hour" => {
                i += 1;
                hour = a[i].parse().ok();
            }
            x => eprintln!("ignoring {x}"),
        }
        i += 1;
    }
    let store = Arc::new(ScriptStore::open(content).expect("script store"));
    // --save: flags, counters, heroine records and name from the save (`Save::game_state`), else a fresh state
    let cur = slot.or_else(|| GameState::slot_of_event_id(event)).unwrap_or(0);
    let mut state = match &save {
        Some(p) => lp_engine::save::Save::open(p).and_then(|s| s.game_state(seed, cur)).expect("save"),
        None => GameState::new(seed),
    };
    if let Some(sv) = &save {
        eprintln!("save {}: {} flags, {} counters, name {:?}, status {:?}", sv.display(), state.flags.count(), state.counters.len(), state.player_name,
                  [0, 1, 2].map(|s| state.slot_status(s)));
    }
    if let Some(p) = player {
        state.player_name = p;
    }
    if let Some(m) = month {
        state.clock.month = m;
    }
    if let Some(h) = hour {
        state.clock.hour = h;
    }
    let mut host = Host::new(store, state);
    host.slot_tables = lp_engine::rom::Rom::open(content).ok().map(|r| lp_engine::game::SlotTables::from_rom(&r));
    if let (Some(sv), Ok(rom)) = (&save, lp_engine::rom::Rom::open(content)) {
        let s = lp_engine::save::Save::open(sv).expect("save");
        let vars = lp_engine::scene_inputs::name_vars(content, &rom, s.image(lp_engine::save::Kind::Game, 0), cur).expect("name vars");
        host.vars.extend(vars);
    }
    let mut pol = Scripted::new(choices);
    pol.queries.extend(replies);
    let tr = host.run_event(event, &mut pol).expect("run");
    if raw {
        for (i, (id, args)) in tr.cmds.iter().enumerate() {
            println!("{:6}  cmd {:x} {}", i, id, args.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(" "));
        }
        return;
    }
    for s in &tr.steps {
        let dialogue = matches!(s.effect, Effect::Dialogue { .. });
        if quiet && !dialogue && !matches!(s.effect, Effect::Ended { .. } | Effect::Stopped { .. }) {
            continue;
        }
        println!("{:6}  {}", s.seq, s.effect);
    }
    println!("-- {:?}, {} effects (untimed: scene timing comes from the simulation, see scene_player)", tr.end, tr.steps.len());
}
