//! The flow machines (`flow`) against call logs recorded from the game: every log under `../sandbox/flow/logs`
//! (recorded by sandbox/flow/calllog.py; git-ignored, like content/) is replayed through the port, which must make the
//! same calls in the same order with the same arguments, the same global loads and stores, and return what the game
//! returned. Skipped when content/ or the logs are absent.
use lp_engine::flow::log::{LogCx, Outcome};
use lp_engine::flow::Flow;
use lp_engine::rom::Rom;
use std::path::Path;

#[test]
fn flow_replays_every_recorded_log() {
    let Ok(rom) = Rom::open(Path::new("../content")) else {
        eprintln!("skipped: no content/");
        return;
    };
    let mut logs: Vec<_> = match std::fs::read_dir("../sandbox/flow/logs") {
        Ok(d) => d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt") && !p.ends_with("verify_report.txt")).collect(),
        Err(_) => {
            eprintln!("skipped: no sandbox/flow/logs");
            return;
        }
    };
    logs.sort();
    let flow = Flow::new(&rom);
    let results: Vec<_> = std::thread::scope(|s| {
        let flow = &flow;
        let handles: Vec<_> = logs
            .chunks(logs.len().div_ceil(8).max(1))
            .map(|chunk| {
                s.spawn(move || {
                    chunk
                        .iter()
                        .map(|path| {
                            let text = std::fs::read_to_string(path).unwrap();
                            let mut cx = LogCx::parse(&text);
                            let outcome = cx.run(flow);
                            (path.file_name().unwrap().to_string_lossy().to_string(), outcome, cx)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let (mut frames, mut calls, mut loads, mut stores, mut forced, mut bad) = (0, 0, 0, 0, 0, 0);
    for (name, outcome, cx) in &results {
        let status = match outcome {
            Outcome::Ok => "OK".to_string(),
            Outcome::Mismatch(e) => {
                bad += 1;
                format!("MISMATCH: {e}")
            }
        };
        let warn = if cx.warnings.is_empty() { String::new() } else { format!(" ({} warnings)", cx.warnings.len()) };
        eprintln!("{name:<40} {:>7} frames  {status}{warn}", cx.frames);
        frames += cx.frames;
        calls += cx.calls;
        loads += cx.loads;
        stores += cx.stores;
        forced += cx.forced;
        assert!(cx.not_ported.is_empty(), "{name}: reached {:?}", cx.not_ported);
    }
    // every argument register the game sets up at a call site must be one the port passes
    if let Ok(t) = std::fs::read_to_string("../sandbox/flow/logs/callsite_args.json") {
        let need: serde_json::Value = serde_json::from_str(&t).unwrap();
        let mut given: std::collections::HashMap<u32, usize> = Default::default();
        for (_, _, cx) in &results {
            for (&site, &n) in &cx.site_args {
                let e = given.entry(site).or_insert(0);
                *e = (*e).max(n);
            }
        }
        let mut short = Vec::new();
        for (site, v) in need.as_object().unwrap() {
            let site = u32::from_str_radix(site, 16).unwrap();
            let n = v[1].as_u64().unwrap() as usize;
            if let Some(&g) = given.get(&site) {
                if g < n {
                    short.push(format!("{site:#x} -> {}: port passes {g}, the game sets up {n}", v[0].as_str().unwrap()));
                }
            }
        }
        short.sort();
        for s in &short {
            eprintln!("  SHORT CALL {s}");
        }
        eprintln!("{} call sites pass fewer arguments than the game sets up", short.len());
    }
    eprintln!("{} logs, {bad} diverged; {frames} frames, {calls} calls, {loads} loads, {stores} stores, {forced} forced returns", results.len());
    assert_eq!(bad, 0, "some logs diverged");
}
