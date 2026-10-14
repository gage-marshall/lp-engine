//! Run every directory event through the host and summarise how it ends and which commands are not modelled yet.
//!   host_survey CONTENT [--seed S]
use lp_engine::game::GameState;
use lp_engine::host::*;
use lp_engine::machine::ScriptStore;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let store = Arc::new(ScriptStore::open(Path::new(&a[1])).unwrap());
    let mut ids: Vec<u32> = store.directory.keys().copied().collect();
    ids.sort();
    let mut ends: BTreeMap<String, usize> = BTreeMap::new();
    let mut unmodelled_q: BTreeMap<u32, (usize, String)> = BTreeMap::new();
    let mut untyped: BTreeMap<u32, (usize, String, String)> = BTreeMap::new();
    let (mut dialogues, mut effects, mut total_ticks) = (0usize, 0usize, 0.0f64);
    let mut loops: BTreeMap<String, usize> = BTreeMap::new();
    let mut big: Vec<(u32, f64)> = Vec::new();
    let mut typed: BTreeMap<&'static str, usize> = BTreeMap::new();
    for id in &ids {
        let mut host = Host::new(store.clone(), GameState::new(1));
        host.max_steps = 100_000;
        host.max_effects = 5_000;
        let tr = host.run_event(*id, &mut DefaultPolicy).unwrap();
        let key = match &tr.end {
            EndReason::Finished(_) => "finished".to_string(),
            EndReason::StepLimit => "blocked: step limit (polling loop)".to_string(),
            EndReason::EffectLimit => "blocked: command loop (input / native state)".to_string(),
            EndReason::Unresolved(_) => "unresolved call".to_string(),
            EndReason::Fault(f) => format!("fault: {}", f.split(' ').next().unwrap_or("")),
        };
        if matches!(tr.end, EndReason::EffectLimit | EndReason::StepLimit) {
            // what repeats at the tail?
            let tail: Vec<String> = tr.steps.iter().rev().take(6).map(|s| format!("{}", s.effect)).collect();
            *loops.entry(tail.join(" | ")).or_default() += 1;
        }
        let waits: f64 = tr.steps.iter().map(|s| if let Effect::Wait { ticks } = s.effect { ticks as f64 } else { 0.0 }).sum();
        if waits > 100_000.0 {
            big.push((*id, waits));
        }
        *ends.entry(key).or_default() += 1;
        total_ticks += waits;
        effects += tr.steps.len();
        for s in &tr.steps {
            match &s.effect {
                Effect::Dialogue { .. } => dialogues += 1,
                Effect::Query { id, name, modelled: false, .. } => unmodelled_q.entry(*id).or_insert((0, name.clone())).0 += 1,
                Effect::Command { id, name, conf, .. } => {
                    let e = untyped.entry(*id).or_insert((0, name.clone(), conf.clone()));
                    e.0 += 1;
                }
                Effect::Voice { .. } => *typed.entry("voice").or_default() += 1,
                Effect::Face { .. } | Effect::FaceAfter { .. } => *typed.entry("face").or_default() += 1,
                Effect::Motion { .. } | Effect::MotionQueue { .. } => *typed.entry("motion").or_default() += 1,
                _ => {}
            }
        }
    }
    println!("{} events, {} effects, {} dialogue boxes, {:.0} s of scripted time at 3.8-frame ticks", ids.len(), effects, dialogues, total_ticks * 3.8 / 60.0);
    println!("how events end: {:?}", ends);
    println!("typed effects: {:?}", typed);
    let mut l: Vec<_> = loops.into_iter().collect();
    l.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    for (k, n) in l.iter().take(8) {
        println!("  loop x{}: {}", n, &k[..k.len().min(300)]);
    }
    println!("events with > 100000 ticks: {} e.g. {:?}", big.len(), big.iter().take(3).collect::<Vec<_>>());
    let mut q: Vec<_> = unmodelled_q.into_iter().collect();
    q.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    println!("unmodelled queries (answered 0): {} commands; top:", q.len());
    for (id, (n, name)) in q.iter().take(12) {
        println!("  {:#x} {} x{}", id, name, n);
    }
    let mut u: Vec<_> = untyped.into_iter().collect();
    u.sort_by_key(|(_, (n, _, _))| std::cmp::Reverse(*n));
    println!("commands logged without a typed effect: {}; top:", u.len());
    for (id, (n, name, conf)) in u.iter().take(20) {
        println!("  {:#x} {} [{}] x{}", id, name, conf, n);
    }
}
