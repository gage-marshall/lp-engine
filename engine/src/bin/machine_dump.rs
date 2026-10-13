//! Canonical event trace of every directory event with the deterministic reply policy of tools/event_trace.py (`TraceHost`);
//! diffed against tools/machine_dump.py.   usage: machine_dump CONTENT_DIR
use lp_engine::machine::*;
use lp_engine::script::Seg;
use std::collections::HashSet;
use std::sync::Arc;

fn segs(v: &[Seg]) -> String {
    v.iter().map(|s| match s { Seg::Lit(t) => format!("s:{}", t), Seg::Var(n) => format!("v:{}", n) }).collect::<Vec<_>>().join(";")
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let store = Arc::new(ScriptStore::open(std::path::Path::new(&a[1])).unwrap());
    let mut ids: Vec<u32> = store.directory.keys().copied().collect();
    ids.sort();
    for id in ids {
        println!("== {:08x}", id);
        let mut m = match Machine::start(store.clone(), id) {
            Ok(m) => m,
            Err(e) => {
                println!("START-ERR {}", e);
                continue;
            }
        };
        m.max_steps = 20000;
        let mut flags: HashSet<u32> = HashSet::new();
        let mut reply: Option<u32> = None;
        let mut n = 0;
        loop {
            n += 1;
            if n > 3000 {
                println!("CAP");
                break;
            }
            let ev = m.step(reply.take());
            match ev {
                MEvent::Cmd { id, args } => {
                    let mut r = 0u32;
                    if id == 0x55 && !args.is_empty() {
                        flags.insert(args[0]);
                    } else if id == 0x59 && !args.is_empty() {
                        r = flags.contains(&args[0]) as u32;
                    }
                    println!("C {:x} {} -> {}", id, args.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","), r);
                    reply = Some(r);
                }
                MEvent::Text { boxed, mode } => println!("T {:?} {}|{}|{}", mode, segs(&boxed.name), segs(&boxed.sub), segs(&boxed.text)),
                MEvent::WaitFade => println!("W"),
                MEvent::ReturnById(i) => println!("R {:x}", i),
                MEvent::Call { event_id, target } => println!("K {:x} {}/{}/{}", event_id, target.slot, target.script, target.entry),
                MEvent::CallRef { raw, target } => println!("KR {:x} {}/{}/{}", raw, target.slot, target.script, target.entry),
                MEvent::UnresolvedCall(i) => {
                    println!("U {:x}", i);
                    break;
                }
                MEvent::End(acc) => {
                    println!("E {}", acc);
                    break;
                }
                MEvent::Limit(pc) => {
                    println!("L {}", pc);
                    break;
                }
                MEvent::Fault(f) => {
                    println!("F");
                    let _ = f;
                    break;
                }
                MEvent::BadOp { op, pc } => {
                    println!("B {} {}", op, pc);
                    break;
                }
            }
        }
    }
}
