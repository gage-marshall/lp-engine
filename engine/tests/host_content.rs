//! The host against the real extracted scripts (skipped when ../content is absent).
use lp_engine::game::GameState;
use lp_engine::host::*;
use lp_engine::machine::ScriptStore;
use std::path::Path;
use std::sync::Arc;

fn store() -> Option<Arc<ScriptStore>> {
    let p = Path::new("../content");
    if p.join("assets/arc/tmx_event.arc").is_dir() { Some(Arc::new(ScriptStore::open(p).unwrap())) } else { None }
}

#[test]
fn a_recorded_play_event_runs_to_the_end_with_its_dialogue() {
    let Some(store) = store() else { return };
    // an afternoon hallway encounter seen in a play session (event 0xa44c1b00)
    let mut h = Host::new(store, GameState::new(1));
    let tr = h.run_event(0xa44c_1b00, &mut DefaultPolicy).unwrap();
    assert_eq!(tr.end, EndReason::Finished(0));
    let lines: Vec<&str> = tr.steps.iter().filter_map(|s| if let Effect::Dialogue { text, .. } = &s.effect { Some(text.as_str()) } else { None }).collect();
    assert!(lines.iter().any(|l| l.contains("あの、そろそろ行かないと")), "{lines:?}");
    assert!(tr.steps.iter().any(|s| matches!(s.effect, Effect::Voice { .. })));
    assert!(tr.steps.iter().any(|s| matches!(s.effect, Effect::Face { .. })));
    // the script waits (0x44) somewhere along the way
    assert!(tr.steps.iter().any(|s| matches!(s.effect, Effect::Wait { ticks } if ticks > 0)));
}

#[test]
fn choices_change_the_script_path() {
    let Some(store) = store() else { return };
    // find an event with a mode-2 choice and show that the two answers diverge
    let mut ids: Vec<u32> = store.directory.keys().copied().collect();
    ids.sort();
    for id in ids.into_iter().take(3000) {
        let mut h = Host::new(store.clone(), GameState::new(1));
        h.max_steps = 20_000;
        h.max_effects = 400;
        let a = h.run_event(id, &mut Scripted::new(vec![1, 1, 1])).unwrap();
        if !a.steps.iter().any(|s| matches!(s.effect, Effect::Dialogue { mode: Some(2), .. })) {
            continue;
        }
        let mut h = Host::new(store.clone(), GameState::new(1));
        h.max_steps = 20_000;
        h.max_effects = 400;
        let b = h.run_event(id, &mut Scripted::new(vec![2, 2, 2])).unwrap();
        let text = |t: &Transcript| t.steps.iter().filter_map(|s| if let Effect::Dialogue { text, .. } = &s.effect { Some(text.clone()) } else { None }).collect::<Vec<_>>();
        if text(&a) != text(&b) {
            return; // found an event whose dialogue depends on the choice
        }
    }
    panic!("no event with choice-dependent dialogue found in the first 3000 events");
}

#[test]
fn most_events_finish_headlessly() {
    let Some(store) = store() else { return };
    let mut ids: Vec<u32> = store.directory.keys().copied().collect();
    ids.sort();
    let sample: Vec<u32> = ids.iter().step_by(40).copied().collect(); // ~400 events across the whole directory
    let mut finished = 0;
    for id in &sample {
        let mut h = Host::new(store.clone(), GameState::new(1));
        h.max_steps = 100_000;
        h.max_effects = 5_000;
        let tr = h.run_event(*id, &mut DefaultPolicy).unwrap();
        assert!(!matches!(tr.end, EndReason::Fault(_) | EndReason::Unresolved(_)), "{id:08x}: {:?}", tr.end);
        finished += matches!(tr.end, EndReason::Finished(_)) as usize;
    }
    assert!(finished as f64 / sample.len() as f64 > 0.97, "{finished}/{}", sample.len());
}
