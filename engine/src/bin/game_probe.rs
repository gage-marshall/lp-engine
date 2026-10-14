//! Bring-up probe: boots the game on the engine runtime as the game boots (Main_InitializeRuntime, Main_Startup, then
//! the frame loop) with no engine subsystems, so everything the registry does not answer runs as the game's code.
//! Prints each fault as it happens and stops at the first. Labels come from fid/labels_all.csv when present.
//!   game_probe CONTENT [FRAMES] [--budget N]
use lp_engine::rom::Rom;
use lp_engine::runtime::{Game, NoSubsystems};
use std::path::Path;

/// labels: static ones, and per overlay (ovNN rows)
fn labels() -> (std::collections::BTreeMap<u32, String>, std::collections::HashMap<u32, std::collections::BTreeMap<u32, String>>) {
    let mut m = std::collections::BTreeMap::new();
    let mut ov: std::collections::HashMap<u32, std::collections::BTreeMap<u32, String>> = Default::default();
    if let Ok(t) = std::fs::read_to_string("../fid/labels_all.csv") {
        for l in t.lines() {
            let p: Vec<&str> = l.split(',').collect();
            if p.len() > 2 {
                if let Ok(a) = u32::from_str_radix(p[1], 16) {
                    if p[0] == "static" {
                        m.insert(a, p[2].to_string());
                    } else if let Some(n) = p[0].strip_prefix("ov").and_then(|n| n.parse::<u32>().ok()) {
                        ov.entry(n).or_default().insert(a, p[2].to_string());
                    }
                }
            }
        }
    }
    (m, ov)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rom = Rom::open(Path::new(&a[1])).expect("content");
    let frames: u64 = a.get(2).and_then(|x| x.parse().ok()).unwrap_or(10);
    let (names, ov_names) = labels();
    let loaded = std::cell::RefCell::new(Vec::<u32>::new());
    let name = |x: u32| {
        let pick = |t: &std::collections::BTreeMap<u32, String>| match t.range(..=x).next_back() {
            Some((&a, n)) if x - a < 0x2000 => Some(if a == x { n.clone() } else { format!("{n}+{:#x}", x - a) }),
            _ => None,
        };
        if x >= 0x2158000 {
            for o in loaded.borrow().iter() {
                if let Some(t) = ov_names.get(o) {
                    if let Some(n) = pick(t) {
                        return format!("ov{o}:{n}");
                    }
                }
            }
        }
        pick(&names).unwrap_or_else(|| format!("{x:#x}"))
    };
    let mut g = Game::new(rom, NoSubsystems);
    g.m.rtc = lp_engine::runtime::sdk::rtc_from_local_seconds(lp_engine::runtime::sdk::host_local_seconds());
    // --rtc Y-M-D-h-m-s: the clock starts there and runs with the game's time (repeatable runs)
    if let Some(i) = a.iter().position(|x| x == "--rtc") {
        let v: Vec<i64> = a[i + 1].split('-').map(|x| x.parse().unwrap()).collect();
        let b = lp_engine::runtime::sdk::local_seconds(v[0], v[1], v[2], v[3], v[4], v[5]);
        g.m.rtc_base = Some(b);
        g.m.rtc = lp_engine::runtime::sdk::rtc_from_local_seconds(b);
    }
    let mut breaks = Vec::new();
    for (i, x) in a.iter().enumerate() {
        if x == "--break" {
            breaks.push(u32::from_str_radix(a[i + 1].trim_start_matches("0x"), 16).unwrap());
        }
    }
    let break_frame: u64 = a.iter().position(|x| x == "--break-frame").map(|i| a[i + 1].parse().unwrap()).unwrap_or(0);
    let arm = |g: &mut Game<NoSubsystems>| {
        for &b in &breaks {
            g.m.breakpoints.push(b);
            g.m.ram.set_trap(b);
        }
    };
    if break_frame == 0 {
        arm(&mut g);
    }
    if let Some(i) = a.iter().position(|x| x == "--save") {
        g.m.backup = std::fs::read(&a[i + 1]).expect("save file");
    }
    if let Some(i) = a.iter().position(|x| x == "--snapshot") {
        // --snapshot FRAME FUNCTION DIR
        let f = a[i + 1].parse().unwrap();
        let t = u32::from_str_radix(a[i + 2].trim_start_matches("0x"), 16).unwrap();
        g.m.snapshot_at = Some((f, t, a[i + 3].clone()));
    }
    let shots: Vec<u64> = a.iter().position(|x| x == "--shot").map(|i| a[i + 1].split(',').map(|v| v.parse().unwrap()).collect()).unwrap_or_default();
    let shot_prefix = a.iter().position(|x| x == "--out").map(|i| a[i + 1].clone()).unwrap_or("shot".into());
    // --script JSON: the emulator driver's steps ({"wait": n}, {"tap": [x, y]} held 4 frames then released 20,
    // {"drag": [[x, y], ...]} each point held 2 frames then released 8)
    let mut touch_at: std::collections::HashMap<u64, (u16, u16)> = Default::default();
    if let Some(i) = a.iter().position(|x| x == "--script") {
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&a[i + 1]).unwrap()).unwrap();
        let mut f = 0u64;
        for st in v.as_array().unwrap() {
            if let Some(n) = st.get("wait").and_then(|w| w.as_u64()) {
                f += n;
            } else if let Some(pts) = st.get("drag").and_then(|d| d.as_array()) {
                // a stroke: each point held 2 frames, then released for 8 (emu_driver.drag)
                for p in pts {
                    for k in 0..2 {
                        touch_at.insert(f + k, (p[0].as_u64().unwrap() as u16, p[1].as_u64().unwrap() as u16));
                    }
                    f += 2;
                }
                f += 8;
            } else if let Some(t) = st.get("tap") {
                for k in 0..4 {
                    touch_at.insert(f + k, (t[0].as_u64().unwrap() as u16, t[1].as_u64().unwrap() as u16));
                }
                f += 24;
            }
        }
    }
    // --peek ADDR,WORDS,FROM,TO
    let peek: Option<(u32, u32, u64, u64)> = a.iter().position(|x| x == "--peek").map(|i| {
        let p: Vec<&str> = a[i + 1].split(',').collect();
        (u32::from_str_radix(p[0].trim_start_matches("0x"), 16).unwrap(), p[1].parse().unwrap(), p[2].parse().unwrap(), p[3].parse().unwrap())
    });
    let watch = a.iter().position(|x| x == "--watch").map(|i| u32::from_str_radix(a[i + 1].trim_start_matches("0x"), 16).unwrap());
    if a.iter().any(|x| x == "--trace") {
        g.m.trace = Some(Default::default());
    }
    if let Some(i) = a.iter().position(|x| x == "--budget") {
        g.m.budget = a[i + 1].parse().unwrap();
    }
    let report = |g: &Game<NoSubsystems>, stage: &str| {
        *loaded.borrow_mut() = (0..2).filter_map(|i| lp_engine::runtime::natives::overlay_in_slot(&g.m, i)).collect();
        if !g.m.faults.is_empty() {
            for f in &g.m.faults {
                let f = f.split_whitespace().map(|w| {
                    let t = w.trim_end_matches(|c| c == ']' || c == ';' || c == ')');
                    let t = t.split("->").next().unwrap_or(t);
                    u32::from_str_radix(t.trim_start_matches("0x"), 16).ok().filter(|_| t.starts_with("0x")).map_or(w.to_string(), |v| format!("{}{}", name(v), &w[t.len()..]))
                });
                println!("FAULT during {stage}: {}", f.collect::<Vec<_>>().join(" "));
            }
            println!("{} instructions; pxi sent {:x?}; callbacks {:x?}; snd commands: {}", g.m.steps, g.m.pxi_sent, g.m.pxi_callbacks, g.m.snd.summary());
            println!("SND_Init flag {:#x}; cmd work {:x?}", g.m.ram.u32(0x2135dd4), (0..10).map(|i| g.m.ram.u32(0x2135df0 + 4 * i)).collect::<Vec<_>>());
            println!("card requests {:?}", g.m.card_requests);
            println!("interrupts by bit {:?}; ITCM pool word 0x1ff8120 = {:#x}; entry 0 there {:#x}; 0x27e0000 = {:#x}", g.m.irq_counts, g.m.ram.u32(0x1ff8120), g.m.ram.u32(g.m.ram.u32(0x1ff8120)), g.m.ram.u32(0x27e0000));
            println!("IME {:#x} IE {:#x} IF {:#x} check {:#x}; tick {} vblank {}; irqs {} idle waits {} switches {}", g.m.ram.u32(0x4000208), g.m.ram.u32(0x4000210), g.m.ram.u32(0x4000214), g.m.ram.u32(0x27e3ff8), g.m.tick, g.m.vblank_tick, g.m.irqs, g.m.idle_waits, g.m.switches);
            // the SDK's threads (list head at 0x2135990 + 0x24, linked by +0x68; +0x64 state 1 ready / 0 waiting)
            let mut t = g.m.ram.u32(0x2135990 + 0x24);
            let cur = g.m.ram.u32(0x2135990 + 0x20);
            while t != 0 {
                let (pc, lr, sp) = (g.m.ram.u32(t + 0x40).wrapping_sub(4), g.m.ram.u32(t + 0x3c), g.m.ram.u32(t + 0x38));
                println!("thread {:#x}{} prio {} state {}: resume {} (lr {}, sp {:#x}); waiting on queue {:#x}", t, if t == cur { " (current)" } else { "" }, g.m.ram.u32(t + 0x70), g.m.ram.u32(t + 0x64), name(pc), name(lr), sp, g.m.ram.u32(t + 0x78));
                if std::env::var("STACKS").is_ok() {
                    let ret: Vec<String> = (0..96).map(|i| g.m.ram.u32(sp + 4 * i)).filter(|&w| (0x2000000..0x20fc000).contains(&(w & !1)) || (0x2158000..0x21d0000).contains(&(w & !1))).map(|w| name(w)).collect();
                    println!("    stack: {}", ret.join(" < "));
                }
                t = g.m.ram.u32(t + 0x68);
            }
            println!("virtual ROM reads (last): {:x?}", g.m.vrom_reads.iter().rev().take(8).collect::<Vec<_>>());
            println!("card bus commands (last): {:x?}", g.m.ram.card.log.iter().rev().take(6).collect::<Vec<_>>());
            println!("virtual ROM misses: {:x?}", g.m.ram.vrom.misses.iter().take(10).collect::<Vec<_>>());
            let sh = g.m.ram.u32(0x2137920);
            println!("snd shared {:#x}: finished {:#x}; command work +4 {:#x} +0x20 {:#x}", sh, g.m.ram.u32(sh), g.m.ram.u32(0x2135df4), g.m.ram.u32(0x2135e10));
            std::process::exit(1);
        }
    };
    g.m.faults = g.power_on();
    g.m.ram.watch_itcm = std::env::var("LP_WATCH_ITCM").is_ok();
    report(&g, "power on");
    println!("power on: ok ({} instructions; {} switches, {} interrupts, {} idle waits)", g.m.steps, g.m.switches, g.m.irqs, g.m.idle_waits);
    let slots = |g: &Game<NoSubsystems>| {
        let base = g.m.ram.u32(g.m.lit(0x2040410));
        (0..20).filter(|i| g.m.ram.u32(base + i * 0x21c + 0x208) != 0 || g.m.ram.u32(base + i * 0x21c + 0x214) & 1 != 0).map(|i| g.m.ram.u32(base + i * 0x21c + 0x208)).collect::<Vec<_>>()
    };
    println!("task slots after power on: {:x?}", slots(&g));
    let base = g.m.ram.u32(g.m.lit(0x2040410));
    println!("pool at {:#x} (pool var {:#x}); +0x214 words {:x?}", base, g.m.lit(0x2040410), (0..20).map(|i| g.m.ram.u32(base + i * 0x21c + 0x214)).collect::<Vec<_>>());
    // --wav PATH: the sound driver's output, written at the end
    let wav = a.iter().position(|x| x == "--wav").map(|i| a[i + 1].clone());
    let sink = std::sync::Arc::new(std::sync::Mutex::new(std::collections::VecDeque::new()));
    let mut audio: Vec<f32> = Vec::new();
    if wav.is_some() {
        g.m.snd.sink = Some(sink.clone());
    }
    if a.iter().any(|x| x == "--check-sound") {
        g.m.snd.tap = Some(Vec::new());
    }
    // time is counted in VBlanks (the game's clock / TICKS_PER_FRAME), as the emulator counts frames: a game frame
    // that overruns takes two, so script steps, shots and peeks line up with emulator runs
    let mut armed = false;
    loop {
        let f = g.m.tick / lp_engine::runtime::TICKS_PER_FRAME;
        if f >= frames {
            break;
        }
        g.m.input.touch = touch_at.get(&f).copied();
        audio.extend(sink.lock().unwrap().drain(..));
        if f >= break_frame && break_frame > 0 && !armed {
            armed = true;
            arm(&mut g);
            g.m.ram.watch = watch;
            g.m.ram.watch_skip = std::env::var("LP_WATCH_SKIP").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
            g.m.ram.watch_value = a.iter().position(|x| x == "--watch-value").map(|i| u32::from_str_radix(a[i + 1].trim_start_matches("0x"), 16).unwrap());
        }
        g.frame();
        let f_end = g.m.tick / lp_engine::runtime::TICKS_PER_FRAME;
        if let Some((a0, n, lo, hi)) = peek {
            if f >= lo && f <= hi {
                let w: Vec<String> = (0..n).map(|i| format!("{:x}", g.m.ram.u32(a0 + 4 * i))).collect();
                println!("peek f{f} touch={:?}: {}", g.m.input.touch, w.join(" "));
            }
        }
        if let Some(&shot) = shots.iter().find(|&&v| v > f && v <= f_end.max(f + 1)) {
            let l3 = g.m.layer3d();
            let (top, bottom) = g.m.screens(Some(&l3));
            let mut img = top.clone();
            img.extend_from_slice(&bottom);
            let path = format!("{shot_prefix}_{:05}.png", shot);
            let file = std::fs::File::create(&path).unwrap();
            let mut e = png::Encoder::new(std::io::BufWriter::new(file), 256, 384);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            e.write_header().unwrap().write_image_data(&img).unwrap();
            if std::env::var("REGS").is_ok() {
                let r = |a: u32| g.m.ram.u32(a);
                println!("  POWCNT {:#x} DISPCNT A {:#x} B {:#x} DISP3DCNT {:#x} DISPCAPCNT {:#x}", g.m.ram.u16(0x4000304), r(0x4000000), r(0x4001000), r(0x4000060), r(0x4000064));
                println!("  A BGCNT {:x?}  B BGCNT {:x?}", (0..4).map(|i| g.m.ram.u16(0x4000008 + 2 * i)).collect::<Vec<_>>(), (0..4).map(|i| g.m.ram.u16(0x4001008 + 2 * i)).collect::<Vec<_>>());
                println!("  VRAMCNT {:x?}  places {:?}", (0..10).map(|i| g.m.ram.u8(0x4000240 + i)).collect::<Vec<_>>(), g.m.ram.vram().place);
                println!("  BLDCNT A {:#x} BLDALPHA {:#x} MASTER_BRIGHT A {:#x} B {:#x}", g.m.ram.u16(0x4000050), g.m.ram.u16(0x4000052), g.m.ram.u16(0x400006c), g.m.ram.u16(0x400106c));
            }
            let gx = &g.m.ram.gx;
            println!("  gx: {} fifo words, {} port writes, {} vertices; commands {:?}", gx.n_fifo, gx.n_port, gx.n_vtx, gx.n_cmd.iter().enumerate().filter(|(_, &n)| n > 0).map(|(c, n)| format!("{c:x}:{n}")).collect::<Vec<_>>());
            println!("frame {shot}: {path} ({} polygons in the 3D frame, {} swaps)", g.m.ram.gx.frame.polys.len(), g.m.ram.gx.frames);
        }
        if f < 3 || (f < 60 && f % 10 == 0) || (f > 280 && f < 300) {
            println!("frame {f}: vblank handler {:#x}", g.m.ram.u32(0x27e0000));
        }
        if f % 60 == 59 {
            let depth = g.m.ram.u32(0x2139cdc);
            let states: Vec<String> = (0..6).map(|i| format!("{:x}", g.m.ram.u32(0x2139ce0 + 4 * i))).collect();
            let ovs: Vec<String> = (0..2).map(|i| format!("{:?}", lp_engine::runtime::natives::overlay_in_slot(&g.m, i))).collect();
            let req = 0x2152ea4;
            println!("frame {:>5}: states [{}] depth {}  overlays {}  screen run {} id {} result {}  ({} M instr, {} pending kinds, tick {})", f + 1, states.join(" "), depth, ovs.join(","), g.m.ram.u32(req), g.m.ram.i32(req + 0xc), g.m.ram.i32(req + 0x14), g.m.steps / 1_000_000, g.m.pending.len(), g.m.tick);
        }
        report(&g, &format!("frame {f}"));
    }
    let t = std::time::Instant::now();
    for _ in 0..60 {
        std::hint::black_box(g.m.screens(None));
    }
    println!("2D composition: {:.2} ms per frame", t.elapsed().as_secs_f64() * 1000.0 / 60.0);
    println!("card requests {:?}", g.m.card_requests);
    println!("sound commands: {}", g.m.snd.summary());
    {
        // registry entries routed to an engine subsystem: with none present every call ran as the game's own code
        let mut v: Vec<_> = g.m.pending.iter().filter(|(k, _)| matches!(lp_engine::runtime::ext::kind_of(k), Some(true))).collect();
        v.sort_by_key(|x| std::cmp::Reverse(*x.1));
        println!("subsystem-routed calls answered by the game's own code: {} functions, {} calls; {:?}", v.len(), v.iter().map(|x| *x.1).sum::<u64>(), v.iter().collect::<Vec<_>>());
    }
    if let Some(sh) = &g.m.shadow {
        for (k, (ok, bad, first)) in sh {
            println!("shadow {k:<36} agree {ok:>8} disagree {bad:>6} {first:x?}");
        }
    }
    if let Some(i) = a.iter().position(|x| x == "--save-out") {
        std::fs::write(&a[i + 1], &g.m.backup).expect("save out");
        println!("battery save ({} bytes, changed by the game: {}) -> {}", g.m.backup.len(), g.m.backup_dirty, a[i + 1]);
    }
    if a.iter().any(|x| x == "--check-sound") {
        // every bank the driver copied from RAM against the extracted SDAT: same instruments, same decoded waves
        use lp_engine::snd::Source;
        let sdat = lp_engine::sdat::Sdat::parse(std::fs::read(format!("{}/assets/sys/sound_data.sdat", a[1])).unwrap()).unwrap();
        let src = g.m.snd.src.clone();
        for (addr, bank, arcs) in src.banks_held() {
            let id = (0..sdat.banks.len()).find(|&i| sdat.bank(i).map_or(false, |b| b.instruments == bank.instruments));
            let Some(id) = id else {
                println!("bank {addr:#x}: no SDAT bank has these instruments");
                continue;
            };
            let (mut same, mut differ, mut absent) = (0, 0, 0);
            for inst in &bank.instruments {
                let regions: Vec<lp_engine::sdat::Region> = match inst {
                    lp_engine::sdat::Inst::Single(r) => vec![*r],
                    lp_engine::sdat::Inst::DrumSet { regions, .. } | lp_engine::sdat::Inst::KeySplit { regions, .. } => regions.clone(),
                    _ => vec![],
                };
                for r in regions.iter().filter(|r| r.kind == 1 || r.kind == 4) {
                    let k = r.wavearc as usize;
                    let ram = arcs.get(k).copied().flatten().and_then(|p| src.wave(p, r.wave as usize));
                    let ext = sdat.bank_wavearc(id, k).and_then(|w| sdat.wave(w, r.wave as usize));
                    match (ram, ext) {
                        (Some(x), Some(y)) if *x == *y => same += 1,
                        (None, _) => absent += 1,
                        _ => differ += 1,
                    }
                }
            }
            println!("bank {addr:#x} = SDAT bank {id}: waves same {same}, different {differ}, not loaded {absent}");
        }
        // the stream the ARM9 played on its own channels against the voice clips decoded from the archive
        let tap = g.m.snd.tap.take().unwrap_or_default();
        let first = tap.iter().position(|&v| v != 0).unwrap_or(tap.len());
        println!("stream: {} samples consumed, first sound at {first}", tap.len());
        if first + 64 <= tap.len() {
            let rd = |p: &str| std::fs::read(format!("{}/{p}", a[1])).unwrap();
            let (vtb, spp) = (rd("assets/sys/Siren14Voice.vtb"), rd("assets/sys/Siren14Voice.spp"));
            let t = lp_engine::siren::Tables::from_pack(&rd("engine/g7221.pack")).unwrap();
            let probe = &tap[first..first + 64];
            for clip in 0..vtb.len() / 8 {
                let e = &vtb[clip * 8..clip * 8 + 8];
                let (off, size) = (u32::from_le_bytes([e[0], e[1], e[2], e[3]]) as usize / 16, u32::from_le_bytes([e[4], e[5], e[6], e[7]]) as usize / 16);
                let Some(d) = spp.get(off..off + size.min(400)) else { continue };
                let (pcm, _) = lp_engine::siren::decode_clip(&t, d, 16000, 14000);
                if let Some(at) = pcm.windows(64).position(|w| w == probe) {
                    let start = first - at;
                    let (full, _) = lp_engine::siren::decode_clip(&t, &spp[off..off + size], 16000, 14000);
                    let n = full.len().min(tap.len() - start);
                    let same = (0..n).take_while(|&i| tap[start + i] == full[i]).count();
                    println!("stream = voice clip {clip} from sample {start}: {same} of {} clip samples identical in order ({} consumed)", full.len(), n);
                    let w = same.saturating_sub(4);
                    println!("  around the first difference: stream {:?}\n  clip   {:?}", &tap[start + w..(start + w + 12).min(tap.len())], &full[w..(w + 12).min(full.len())]);
                    let diffs = (0..n).filter(|&i| tap[start + i] != full[i]).count();
                    println!("  {diffs} samples differ in all");
                }
            }
        }
    }
    if let Some(path) = wav {
        audio.extend(sink.lock().unwrap().drain(..));
        let peak = audio.iter().fold(0f32, |p, v| p.max(v.abs()));
        let rate = g.m.snd.rate;
        let mut b = Vec::new();
        let data = (audio.len() * 2) as u32;
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&(36 + data).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&2u16.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate * 4).to_le_bytes());
        b.extend_from_slice(&4u16.to_le_bytes());
        b.extend_from_slice(&16u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data.to_le_bytes());
        for v in &audio {
            b.extend_from_slice(&((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
        }
        std::fs::write(&path, b).expect("wav");
        println!("audio: {:.1} s, peak {peak:.3} -> {path}", audio.len() as f64 / 2.0 / rate as f64);
    }
    println!("{frames} frames ok; {} instructions; {} functions interpreted; {} thread switches, {} interrupts, {} idle waits", g.m.steps, g.m.interpreted.len(), g.m.switches, g.m.irqs, g.m.idle_waits);
    let mut v: Vec<_> = g.m.interpreted.iter().collect();
    v.sort_by_key(|x| std::cmp::Reverse(*x.1));
    for (k, n) in v.iter().take(25) {
        println!("  {:>7}  {}", n, name(**k));
    }
}
