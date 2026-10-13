//! Differential test driver: prints a canonical event trace for every entry of every script in a directory.
//! usage: script_trace SCRIPT_DIR ACC_CMDS.txt   (ACC_CMDS: hex command ids that write the accumulator, one per line)
use lp_engine::script::{Event, Script, Seg};
use std::collections::HashSet;

fn segs(v: &[Seg]) -> String {
    v.iter()
        .map(|s| match s {
            Seg::Lit(t) => format!("s:{}", t),
            Seg::Var(n) => format!("v:{}", n),
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let acc_cmds: HashSet<u32> = std::fs::read_to_string(&args[2])
        .unwrap()
        .lines()
        .filter_map(|l| u32::from_str_radix(l.trim(), 16).ok())
        .collect();
    let mut files: Vec<_> = std::fs::read_dir(&args[1]).unwrap().map(|e| e.unwrap().path()).filter(|p| p.extension().map_or(false, |x| x == "sbin")).collect();
    files.sort();
    for f in files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let sc = match Script::from_decrypted(std::fs::read(&f).unwrap()) {
            Some(s) => s,
            None => continue,
        };
        for e in 0..sc.entries.len() {
            let mut run = sc.run(e, 20000);
            let mut reply = None;
            let mut count = 0u32;
            let mut out = String::new();
            loop {
                match run.step(reply.take()) {
                    Event::Cmd { id, args } => {
                        out += &format!("C {:x} {}\n", id, args.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(","));
                        if acc_cmds.contains(&id) {
                            reply = Some(((id.wrapping_mul(31) + count * 17 + e as u32) >> 2) & 1);
                        }
                        count += 1;
                    }
                    Event::Text { boxed, mode } => {
                        out += &format!("T {:?} {}|{}|{}\n", mode, segs(&boxed.name), segs(&boxed.sub), segs(&boxed.text));
                    }
                    Event::WaitFade => out += "W\n",
                    Event::End(a) => {
                        out += &format!("E {}\n", a);
                        break;
                    }
                    Event::Limit(p) => {
                        out += &format!("L {}\n", p);
                        break;
                    }
                    Event::BadOp { op, pc } => {
                        out += &format!("B {} {}\n", op, pc);
                        break;
                    }
                    Event::Fault(_) => {
                        out += "F\n";
                        break;
                    }
                }
            }
            print!("== {} {}\n{}", name, e, out);
        }
    }
}
