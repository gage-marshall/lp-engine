//! Parity with the verified sandbox drivers on the recorded scene 08143402 (docs/ENGINE_PORT_PLAN.md).
//! Fixtures: tests/data/s08143402 (derived numbers only; `sandbox/gx/export_fixtures.py`). Skipped when ../content or the
//! session's save file (local, not committed) is absent.
use lp_engine::rom::Rom;
use lp_engine::save::{Save, SaveFacts};
use lp_engine::scene_inputs as si;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn content() -> Option<&'static Path> {
    let p = Path::new("../content");
    if p.join("engine/static.bin").is_file() && p.join("assets/arc/n_block3d_01.arc").is_dir() { Some(p) } else { None }
}

fn fixture(name: &str) -> Value {
    serde_json::from_slice(&std::fs::read(format!("tests/data/s08143402/{name}.json")).unwrap()).unwrap()
}

fn save_path() -> Option<PathBuf> {
    let p = Path::new("../tools/emu_scripts/states/logs").join(fixture("inputs")["save"].as_str().unwrap());
    if p.is_file() { Some(p) } else { None }
}

#[test]
fn phase1_save_and_scene_inputs() {
    let (Some(c), Some(sp)) = (content(), save_path()) else { return };
    let rom = Rom::open(c).unwrap();
    let facts = SaveFacts::read(&Save::open(&sp).unwrap()).unwrap();
    assert_eq!(facts.levels, [15, 15, 15]);
    assert_eq!(facts.text_speed, 1);
    assert_eq!(facts.calendar_78, 0);
    assert_eq!(facts.record_0d[2], 1, "Nene's hair key");
    // name text variables (`TextVar_InitAllNameVars`): from the save's GameState and text bank 7
    let loaded = Save::open(&sp).unwrap();
    let v = si::name_vars(c, &rom, loaded.image(lp_engine::save::Kind::Game, 0), 2).unwrap();
    assert_eq!((v[&1].as_str(), v[&2].as_str(), v[&3].as_str(), v[&4].as_str(), v[&5].as_str(), v[&6].as_str(), v[&7].as_str()),
               ("俺", "わたし", "高嶺", "リンコ", "小早川", "私", "ネネさん"));
    assert_eq!(v[&0], loaded.player_name().unwrap());
    assert_eq!(v[&0x1a], format!("{}くん", v[&0]));
    let arc = c.join("assets/arc");
    let inp = fixture("inputs");
    let (month, hour) = (inp["month"].as_u64().unwrap() as u32, inp["hour"].as_u64().unwrap() as u32);
    let state = si::random_event_state(facts.calendar_78, facts.record_107[2]);
    assert_eq!(state, 0);
    let hair = si::hair_label(c, 2, facts.record_0d[2], state).unwrap();
    assert_eq!(hair, 0x3d);
    assert_eq!(si::label_number(&arc, 2, hair as u32, true).unwrap(), 100);
    assert_eq!(si::label_number(&arc, 2, 0xfc, false).unwrap(), 10, "outfit label from script 12e 4 252");
    assert_eq!(si::label_class(&arc, 0xfc).unwrap(), 1);
    assert_eq!(si::time_mode(&rom, month, hour, 3), 0, "15 h in October: day shading");
    assert_eq!(si::backdrop_tod(&rom, month, hour, facts.calendar_78, false, false, false), 1, "and an evening backdrop (game, frame 10559)");
    assert_eq!(si::char_intervals(&rom).map(|v| v >> 12), [10, 8, 4, 2, 1]);
}

use lp_engine::host::DefaultPolicy;
use lp_engine::machine::ScriptStore;
use lp_engine::sim::clock::Recorded;
use lp_engine::sim::scene::{SceneFrame, SceneSetup, SceneSim, SimEvent};
use lp_engine::sim::sched::ScriptEvent;
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

struct Replay {
    frames: Vec<SceneFrame>,
    events: Vec<SimEvent>,
    /// per text update: (typing start frame, last character frame, completed by a tap)
    typing: BTreeMap<u64, (Option<i64>, Option<i64>, bool)>,
}

fn replay(c: &Path, sp: &Path) -> Replay {
    let rom = Arc::new(Rom::open(c).unwrap());
    let store = Arc::new(ScriptStore::open(c).unwrap());
    let inp = fixture("inputs");
    let loaded = Save::open(sp).unwrap();
    let save = SaveFacts::read(&loaded).unwrap();
    let event = inp["event"].as_u64().unwrap() as u32;
    // the session's own state from its save (flags, counters, records, name), as the game had it
    let game = Some(loaded.game_state(1, lp_engine::scene_inputs::heroine_of_event(event).unwrap()).unwrap());
    let (f0, f1) = (inp["frames"][0].as_i64().unwrap(), inp["frames"][1].as_i64().unwrap());
    let setup = SceneSetup {
        content: c.to_path_buf(),
        event_id: event,
        save,
        month: inp["month"].as_u64().unwrap() as u32,
        hour: inp["hour"].as_u64().unwrap() as u32,
        phases: Box::new(Recorded::from_digits(f0, inp["phases"].as_str().unwrap())),
        first_frame: f0,
        start_update: inp["script_start_update"].as_u64().unwrap(),
        prev_preset: None,
        rng: None,
        policy: Box::new(DefaultPolicy),
        vars: HashMap::new(),
        game,
    };
    let mut sim = SceneSim::new(rom, store, setup).unwrap();
    let mut rel: HashMap<i64, usize> = HashMap::new();
    for r in inp["releases"].as_array().unwrap() {
        *rel.entry(r.as_i64().unwrap()).or_default() += 1;
    }
    let (mut frames, mut typing) = (Vec::new(), BTreeMap::new());
    for f in f0..f1 {
        if let Some(sf) = sim.step_frame(rel.get(&f).copied().unwrap_or(0)) {
            frames.push(sf);
        }
        if let Some(t) = &sim.sched.text {
            typing.insert(t.update, (t.typing_from, t.typed_frame, t.completed));
        }
    }
    Replay { frames, events: std::mem::take(&mut sim.events), typing }
}

#[test]
fn phase2_3_scene_sim_matches_the_sandbox_and_the_recording() {
    let (Some(c), Some(sp)) = (content(), save_path()) else { return };
    let r = replay(c, &sp);
    // --- script pacing: every command in the sandbox's update (which is the recorded one, 70 / 70)
    let sched = fixture("sched");
    let cmds: Vec<(u64, u32, Vec<u32>)> = r.events.iter().filter_map(|e| match e { SimEvent::Script(ScriptEvent::Cmd { update, id, args, .. }) => Some((*update, *id, args.clone())), _ => None }).collect();
    let want: Vec<(u64, u32, Vec<u32>)> = sched["cmds"].as_array().unwrap().iter().map(|x| (x[0].as_u64().unwrap(), x[1].as_u64().unwrap() as u32, x[2].as_array().unwrap().iter().map(|a| a.as_u64().unwrap() as u32).collect())).collect();
    assert_eq!(cmds.len(), want.len(), "command count");
    for (g, w) in cmds.iter().zip(&want) {
        assert_eq!(g, w, "command placement");
    }
    let rec: Vec<(u64, u32)> = sched["recorded_cmds"].as_array().unwrap().iter().map(|x| (x[0].as_u64().unwrap(), x[1].as_u64().unwrap() as u32)).collect();
    let exact = cmds.iter().zip(&rec).filter(|((u, id, _), (ru, rid))| u == ru && id == rid).count();
    assert_eq!(exact, 70, "recorded command updates");
    // --- texts and the typewriter
    let texts: Vec<(u64, usize)> = r.events.iter().filter_map(|e| match e { SimEvent::Script(ScriptEvent::Text { update, chars, .. }) => Some((*update, *chars)), _ => None }).collect();
    let want_t: Vec<(u64, usize)> = sched["texts"].as_array().unwrap().iter().map(|x| (x[0].as_u64().unwrap(), x[1].as_u64().unwrap() as usize)).collect();
    assert_eq!(texts, want_t, "text commits");
    for ((u, _), ty) in texts.iter().zip(sched["typing"].as_array().unwrap()) {
        let (from, typed, completed) = r.typing[u];
        assert_eq!(from, Some(ty[0].as_i64().unwrap()), "typing start of the line at update {u}");
        if !completed {
            assert_eq!(typed, Some(ty[2].as_i64().unwrap()), "last character of the line at update {u}");
        }
    }
    // --- scene state: exact against the sandbox simulation, update by update
    let sim = fixture("sim");
    let rows = sim["rows"].as_array().unwrap();
    assert!(r.frames.len() >= rows.len(), "{} updates simulated, {} expected", r.frames.len(), rows.len());
    for (sf, w) in r.frames.iter().zip(rows) {
        let chain: Vec<(String, u64, i64, i64)> = sf.chain.iter().map(|l| (l.stem.clone(), l.bundle as u64, l.frame as i64, l.ratio as i64)).collect();
        let wchain: Vec<(String, u64, i64, i64)> = w["chain"].as_array().unwrap().iter().map(|x| (x[0].as_str().unwrap().to_string(), x[1].as_u64().unwrap(), x[2].as_i64().unwrap(), x[3].as_i64().unwrap())).collect();
        let t = sf.update;
        assert_eq!(chain, wchain, "motion chain at update {t}");
        assert_eq!(sf.mood as i64, w["mood"].as_i64().unwrap(), "mood at {t}");
        assert_eq!((sf.eye_tex as i64, sf.eye_mode as i64, sf.eye_trans as i64), (w["eye"].as_i64().unwrap(), w["eye_mode"].as_i64().unwrap(), w["trans"].as_i64().unwrap()), "eye at {t}");
        let wh: Vec<Option<i64>> = w["hands"].as_array().unwrap().iter().map(|x| x.as_i64()).collect();
        assert_eq!(sf.hands.map(|h| h.map(|v| v as i64)).to_vec(), wh, "hands at {t}");
        assert_eq!(sf.mouth as i64, w["mouth"].as_i64().unwrap(), "mouth at {t}");
        assert_eq!((sf.fade_cur as i64, sf.seen_cur as i64), (w["fade_cur"].as_i64().unwrap(), w["seen_cur"].as_i64().unwrap()), "fade at {t}");
        assert_eq!(sf.preset as i64, w["preset"].as_i64().unwrap(), "preset at {t}");
    }
    // --- the recorded game (RAM): the parity counts of docs/RENDER_PRIMITIVES.md
    let ram = fixture("ram");
    let hb = ram["hand_base"].as_array().unwrap().iter().map(|x| x.as_i64().unwrap()).collect::<Vec<_>>();
    let (mut motion, mut eyes, mut hands, mut bright, mut n) = (0, 0, 0, 0, 0);
    for (sf, samples) in r.frames.iter().zip(ram["rows"].as_array().unwrap()) {
        n += 1;
        let s = samples.as_array().unwrap();
        let frames: Vec<i64> = sf.chain.iter().map(|l| l.frame as i64).collect();
        let ratios: Vec<i64> = sf.chain.iter().map(|l| l.ratio as i64).collect();
        let ok = |x: &Value, k: &str, v: &Vec<i64>| x[k].as_array().unwrap().iter().map(|y| y.as_i64().unwrap()).collect::<Vec<_>>() == *v;
        if s.iter().any(|x| ok(x, "frames", &frames) && ok(x, "ratios", &ratios) && x["mood"].as_i64() == Some(sf.mood as i64)) {
            motion += 1;
        }
        if s.iter().any(|x| x["eye"].as_i64() == Some(sf.eye_tex as i64) && x["eye_mode"].as_i64() == Some(sf.eye_mode as i64)) {
            eyes += 1;
        }
        let hv: Vec<i64> = (0..2).map(|k| sf.hands[k].map_or(-1, |v| hb[k] + v as i64)).collect();
        if s.iter().any(|x| ok(x, "hands", &hv)) {
            hands += 1;
        }
        if s[0]["bright"].as_i64() == Some((sf.seen_cur / 0x1000) as i64) {
            bright += 1;
        }
    }
    eprintln!("RAM parity over {n} updates: motion {motion}, eyes {eyes}, hands {hands}, brightness {bright}");
    assert!(motion >= 1173 && eyes >= 1135 && hands >= 1148 && bright == n, "motion {motion} eyes {eyes} hands {hands} brightness {bright}/{n}");
}

#[test]
fn phase4_game_camera_and_backdrop_scroll() {
    let Some(c) = content() else { return };
    let rom = Rom::open(c).unwrap();
    let presets = lp_engine::camera::load_presets(&c.join("assets/arc"));
    let p = &presets[&3014];
    let gc = lp_engine::camera::GameCamera::new(p, &rom, lp_engine::camera::heroine_height(&rom, 2));
    // the captured BG1 scroll of this preset (docs/RENDER_PRIMITIVES.md "Backdrop scroll")
    assert_eq!(gc.bg_scroll(p), (64, 32));
    // world points through the game's own fixed-point chain (sandbox nnsgen: base x LookAt x PerspectiveW), landscape pixels
    for (w, want) in [([0.0, 140.0 + 2.0459, 0.0], (161.5, 163.5)), ([0.0, 100.0 + 2.0459, 0.0], (-33.6, 158.3))] {
        // screen_pos takes world points; the chain's model point (0, y, 0) sits at world (0, y + height, 0)
        let (x, y) = gc.screen_pos(w);
        assert!((x - want.0).abs() < 0.6 && (y - want.1).abs() < 0.6, "{w:?}: ({x:.1}, {y:.1}) vs {want:?}");
    }
}

#[test]
fn phase5_voices_start_four_updates_after_their_request() {
    let (Some(c), Some(sp)) = (content(), save_path()) else { return };
    let r = replay(c, &sp);
    let frame_of: HashMap<u64, i64> = r.frames.iter().map(|f| (f.update, f.frame)).collect();
    let requests: Vec<(u64, u32)> = r.events.iter().filter_map(|e| match e { SimEvent::Script(ScriptEvent::Cmd { update, id: 0x5c, args, .. }) if args.first().map_or(false, |&a| (a as i32) >= 0) => Some((*update, args[0])), _ => None }).collect();
    let starts: Vec<(i64, u32)> = r.events.iter().filter_map(|e| match e { SimEvent::Voice { frame, clip } => Some((*frame, *clip)), _ => None }).collect();
    assert!(!requests.is_empty());
    for ((u, clip), (f, c2)) in requests.iter().zip(&starts) {
        assert_eq!((frame_of[&(u + 4)], *clip), (*f, *c2), "voice {clip} requested in update {u}");
    }
}
