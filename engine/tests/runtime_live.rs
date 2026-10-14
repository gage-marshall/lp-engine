//! The runtime's native ports against the game. A call log is replayed through the flow while the engine's machine
//! answers every call itself; each native's return values and every flow load from main RAM are compared with the
//! game's. The machine starts from the game's own state: the log's RAM snapshot (LOG.txt.ram, written by
//! sandbox/flow/calllog.py at the first frame) or, for a power-on log (xp_boot, xp_cont, xp_tm_*), the image as the
//! game starts. Logs: ../sandbox/flow/logs/ram/*.txt and the power-on xp_*.txt (git-ignored). Skipped without them.
use lp_engine::flow::log::{LogCx, Outcome};
use lp_engine::flow::Flow;
use lp_engine::rom::Rom;
use lp_engine::runtime::check::{LiveCheck, Tally};
use lp_engine::runtime::{Machine, NoSubsystems};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn logs() -> Vec<PathBuf> {
    if let Ok(p) = std::env::var("LOG") {
        return vec![PathBuf::from(p)];
    }
    let mut v = Vec::new();
    if let Ok(d) = std::fs::read_dir("../sandbox/flow/logs/ram") {
        v.extend(d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "txt")));
    }
    if let Ok(d) = std::fs::read_dir("../sandbox/flow/logs") {
        v.extend(d.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy();
            n.starts_with("xp_") && n.ends_with(".txt") && std::fs::read_to_string(p).map(|t| t.starts_with("S 0 0 0 0 ")).unwrap_or(false)
        }));
    }
    v.sort();
    v
}

#[test]
fn natives_agree_with_the_game() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    let flow = Flow::new(&rom);
    let mut total: BTreeMap<&'static str, Tally> = BTreeMap::new();
    let mut load_bad: BTreeMap<u32, Tally> = BTreeMap::new();
    let mut pending: BTreeMap<&'static str, u64> = BTreeMap::new();
    let mut failed = Vec::new();
    for path in logs() {
        let text = std::fs::read_to_string(&path).unwrap();
        let mut m = Machine::new(Rom::open(Path::new("../content")).unwrap());
        // the program as the game starts: the game's crt0 unpacks it (a log's RAM snapshot then replaces main RAM)
        if let Some(entry) = m.ram.boot_entry {
            m.run_crt0(&mut NoSubsystems, entry);
        }
        let ram = PathBuf::from(format!("{}.ram", path.display()));
        if let Ok(img) = std::fs::read(&ram) {
            m.ram.put(0x2000000, &img);
        }
        let mut subs = NoSubsystems;
        let mut lc = LiveCheck::new(LogCx::parse(&text), &mut m, &mut subs);
        let out = lc.run(&flow);
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        let bad: u64 = lc.returns.values().map(|t| t.bad).sum();
        eprintln!("{name:<36} {out:?}  native disagreements {bad}");
        if !matches!(out, Outcome::Ok) {
            failed.push(name.clone());
        }
        for (k, t) in &lc.returns {
            let e = total.entry(k).or_default();
            e.ok += t.ok;
            e.bad += t.bad;
            if e.first.len() < 3 {
                e.first.extend(t.first.iter().take(3 - e.first.len()));
            }
            if t.bad > 0 {
                eprintln!("    {k}: {} bad, first {:x?}", t.bad, t.first);
            }
        }
        for (a, t) in &lc.loads {
            if t.bad > 0 && *a < 0x4000000 {
                let e = load_bad.entry(*a).or_default();
                e.ok += t.ok;
                e.bad += t.bad;
                eprintln!("    load {a:#x}: {} bad, first {:x?}", t.bad, t.first);
            }
        }
        drop(lc);
        for f in m.faults.iter().take(12) {
            eprintln!("    fault: {f}");
        }
        if m.faults.len() > 12 {
            eprintln!("    ... {} faults", m.faults.len());
        }
        eprintln!("    interpreted {} functions, {} instructions", m.interpreted.len(), m.steps);
        for (k, v) in &m.pending {
            *pending.entry(k).or_default() += v;
        }
    }
    eprintln!("natives:");
    for (k, t) in &total {
        eprintln!("  {k:<40} agree {:>9}  disagree {:>6}", t.ok, t.bad);
    }
    eprintln!("still answered by no one (pending): {pending:?}");
    assert!(failed.is_empty(), "replays failed: {failed:?}");
}
