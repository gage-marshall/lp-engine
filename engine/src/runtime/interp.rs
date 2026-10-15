//! Running the game's own code for the functions the engine does not answer yet (registry kind `Pending`, or a
//! subsystem that declines a call): the ARM interpreter (`arm`) executes the function from the game image in the
//! game's memory. Where that code calls a function the registry answers (a native port, a display call, an engine
//! subsystem), execution traps out to the engine and returns to the game's code with the answer, so ported logic is
//! used wherever it exists and the game's code fills in the rest. BIOS calls (SWI) are answered here.
//!
//! This is the transitional tier: every function it runs is counted (`Machine::interpreted`), and each native port
//! replaces one with code checked against it.
use super::ext;
use super::{Machine, Subsystems};
use crate::arm::{Core, Regs, Stop};
use crate::flow::Arg;

/// the stacks crt0 (0x2000800) sets up in DTCM: supervisor at 0x27e3fc0, interrupt 0x40 below it (0x800 bytes),
/// the main thread's user stack below that; the engine runs the main thread's code on the user stack and interrupt
/// work on the interrupt stack
pub const STACK_TOP: u32 = 0x27e3778;
pub const IRQ_STACK_TOP: u32 = 0x27e3f7c & !7;
/// instructions one interpreted call may run before it is reported as stuck

/// OS_SaveContext / OS_LoadContext: the SDK's thread switch (setjmp / longjmp over an OSContext: +0 CPSR, +4 r0..r14,
/// +0x40 resume address + 4, +0x44 the supervisor stack), performed on the interpreter's registers so the SDK's own
/// scheduler runs its threads
/// ARM9 instructions per SDK tick (67 MHz CPU at about 1.5 cycles per instruction; a tick is 64 bus cycles)
pub const INSTR_PER_TICK: u64 = 85;

const OS_SAVE_CONTEXT: u32 = 0x2020c10;
const OS_LOAD_CONTEXT: u32 = 0x2020c5c;
/// OS_Halt: the CPU waits for an interrupt (only the idle thread does this once threads run)
const OS_HALT: u32 = 0x2022940;
const OSI_ARRANGE_TIMER: u32 = 0x2022098;
const OSI_RESCHEDULE_THREAD: u32 = 0x20203ac;

/// mark every static-code function the registry answers as a trap
pub fn install_traps(m: &mut Machine) {
    m.ram.clear_traps();
    for a in ext::trapped_addresses() {
        m.ram.set_trap(a);
    }
    m.ram.set_trap(OS_SAVE_CONTEXT);
    m.ram.set_trap(OS_LOAD_CONTEXT);
    m.ram.set_trap(OS_HALT);
}

fn cpsr_of(r: &Regs) -> u32 {
    (r.n as u32) << 31 | (r.z as u32) << 30 | (r.c as u32) << 29 | (r.v as u32) << 28 | (r.thumb as u32) << 5 | (r.ctl & 0xdf)
}

/// the thread switch on the interpreter's registers; true if handled
fn context_switch(m: &mut Machine, regs: &mut Regs, pc: u32) -> bool {
    match pc {
        OS_SAVE_CONTEXT => {
            let ctx = regs.r[0];
            let cpsr = cpsr_of(regs);
            m.ram.set_u32(ctx, cpsr);
            let mut saved = regs.r;
            saved[0] = 1; // resumed by OS_LoadContext, OS_SaveContext returns 1
            saved[1] = ctx + 4;
            saved[2] = cpsr;
            for i in 0..15 {
                m.ram.set_u32(ctx + 4 + 4 * i as u32, saved[i]);
            }
            m.ram.set_u32(ctx + 0x40, OS_SAVE_CONTEXT + 0x48); // as the game stores it: resume at its `bx lr`, + 4
            m.ram.set_u32(ctx + 0x44, regs.r[13]);
            regs.r[0] = 0;
            let lr = regs.r[14];
            regs.thumb = lr & 1 != 0;
            regs.r[15] = lr & !1;
            m.switches += 1;
            true
        }
        OS_LOAD_CONTEXT => {
            let ctx = regs.r[0];
            load_context(m, regs, ctx);
            if m.trap_depth > 0 {
                m.faults.push(format!("thread switch inside a nested call (depth {})", m.trap_depth));
            }
            m.switches += 1;
            true
        }
        OS_HALT => halt(m, regs),
        _ => false,
    }
}

fn load_context(m: &Machine, regs: &mut Regs, ctx: u32) {
    let cpsr = m.ram.u32(ctx);
    for i in 0..15 {
        regs.r[i] = m.ram.u32(ctx + 4 + 4 * i as u32);
    }
    regs.n = cpsr >> 31 != 0;
    regs.z = (cpsr >> 30) & 1 != 0;
    regs.c = (cpsr >> 29) & 1 != 0;
    regs.v = (cpsr >> 28) & 1 != 0;
    regs.thumb = (cpsr >> 5) & 1 != 0;
    regs.ctl = cpsr & 0xdf;
    regs.r[15] = m.ram.u32(ctx + 0x40).wrapping_sub(4) & if regs.thumb { !1 } else { !3 };
}

/// the SDK's thread information (OSi_ThreadInfo, OSi_RescheduleThread's pool word 0x2020468): +4 scheduler lock
/// count, +8 the address of the current-thread pointer (+0x20), +0x24 the thread list by priority; the
/// reschedule-needed flag is the halfword at 0x202046c's word
const THREAD_INFO: u32 = 0x2135990;

/// the end of an interrupt (OS_IrqHandler_ThreadSwitch): if a handler woke a thread that should run, the
/// interrupted thread's whole state is saved into its context (to resume exactly where it was interrupted) and the
/// selected thread's context loaded
fn irq_exit_reschedule(m: &mut Machine, regs: &mut Regs) {
    let flag = m.lit(0x202046c);
    if m.ram.u16(flag) == 0 || m.ram.u32(THREAD_INFO + 4) != 0 {
        return;
    }
    m.ram.set_u16(flag, 0);
    let cur = m.ram.u32(m.ram.u32(THREAD_INFO + 8));
    // OS_SelectThread: the first ready thread (state 1) in priority order
    let mut next = m.ram.u32(THREAD_INFO + 0x24);
    while next != 0 && m.ram.u32(next + 0x64) != 1 {
        next = m.ram.u32(next + 0x68);
    }
    if next == 0 || next == cur || cur == 0 {
        return;
    }
    let ctx = cur;
    m.ram.set_u32(ctx, cpsr_of(regs));
    for i in 0..15 {
        m.ram.set_u32(ctx + 4 + 4 * i as u32, regs.r[i]);
    }
    m.ram.set_u32(ctx + 0x40, regs.r[15].wrapping_add(4) | regs.thumb as u32);
    m.ram.set_u32(THREAD_INFO + 0x20, next);
    load_context(m, regs, next);
    m.switches += 1;
}

/// every thread waits: time passes to the next event, the next alarm or the next VBlank, and its interrupt is
/// delivered; then the interrupt's exit reschedules from the halted thread (OSi_RescheduleThread), as
/// OS_IrqHandler_ThreadSwitch does on the DS
fn halt(m: &mut Machine, regs: &mut Regs) -> bool {
    let q = m.lit(0x2022018);
    let head = m.ram.u32(q + 4);
    // the alarm's fire time is in OS ticks (OS_GetTick: since OS_InitTick)
    let base = m.tick_base.unwrap_or(0);
    let fire = (head != 0).then(|| (m.ram.u32(head + 0xc) as u64 | (m.ram.u32(head + 0x10) as u64) << 32) + base);
    m.idle_waits += 1;
    match fire {
        Some(f) if f <= m.vblank_tick => {
            m.tick = m.tick.max(f);
            let sp = std::mem::replace(&mut m.sp_top, IRQ_STACK_TOP);
            m.in_irq += 1;
            let mut none = super::NoSubsystems;
            interpret(m, &mut none, OSI_ARRANGE_TIMER, &mut []); // the timer interrupt's work
            m.in_irq -= 1;
            m.sp_top = sp;
        }
        _ => {
            m.tick = m.tick.max(m.vblank_tick);
            m.vblank_tick += super::TICKS_PER_FRAME;
            deliver_irq(m, 0);
        }
    }
    regs.thumb = false;
    regs.r[15] = OSI_RESCHEDULE_THREAD;
    true
}

/// deliver interrupt `bit` as OS_IrqHandler (ITCM 0x1ff80d0) does: acknowledged in IF, the handler in OS_IRQTable
/// (its address is OS_IrqHandler's pool word 0x1ff8120) run in interrupt mode, then the threads waiting on
/// interrupts woken (OS_IrqHandler_ThreadSwitch: OS_WakeupThread on the queue at *0x1ff8278); a reschedule they
/// need is left marked for the caller (the SDK's deferred reschedule)
pub fn deliver_irq(m: &mut Machine, bit: u32) {
    let ime = m.ram.u32(0x4000208);
    let ie = m.ram.u32(0x4000210);
    let mask = 1u32 << bit;
    if ime & 1 == 0 || ie & mask == 0 {
        // the CPU takes no interrupt (master enable off, or this source not enabled): nothing runs
        return;
    }
    m.irqs += 1;
    *m.irq_counts.entry(bit).or_default() += 1;
    let mut none = super::NoSubsystems;
    let sp = std::mem::replace(&mut m.sp_top, IRQ_STACK_TOP);
    m.in_irq += 1;
    {
        let table = m.ram.u32(0x1ff8120);
        let handler = m.ram.u32(table + 4 * bit);
        if handler != 0 && ext::entry(handler).is_some() {
            ext::call(m, &mut none, handler, &mut []); // a handler the engine answers (MIi_FIFOCallback)
        } else if handler != 0 {
            interpret(m, &mut none, handler, &mut []);
        }
    }
    let queue = m.ram.u32(0x1ff8278);
    interpret(m, &mut none, 0x2020858, &mut [Arg::I(queue as i32)]); // OS_WakeupThread
    m.in_irq -= 1;
    m.sp_top = sp;
}

/// deliver the interrupts the engine's hardware raised (lowest bit first, as OS_IrqHandler picks them)
pub fn take_irqs(m: &mut Machine) {
    let mut n = 0;
    loop {
        let level = if m.ram.gx_fifo_irq && m.ram.u32(0x4000210) & (1 << 21) != 0 { 1 << 21 } else { 0 };
        let pending = m.ram.irq_raised | level;
        if pending == 0 {
            break;
        }
        n += 1;
        if n > 100_000 {
            m.faults.push(format!("interrupt {pending:#x} stays asserted"));
            break;
        }
        let bit = pending.trailing_zeros();
        m.ram.irq_raised &= !(1 << bit);
        deliver_irq(m, bit);
    }
}

/// the main thread lets the threads an interrupt woke run until they wait again (OSi_RescheduleThread from the
/// main thread's context: its OS_SaveContext captures this call, and the scheduler resumes it when they yield)
pub fn reschedule(m: &mut Machine) {
    let mut none = super::NoSubsystems;
    interpret(m, &mut none, OSI_RESCHEDULE_THREAD, &mut []);
}

/// run the game function at `target` with `args`; its r0
pub fn interpret(m: &mut Machine, s: &mut dyn Subsystems, target: u32, args: &mut [Arg]) -> i32 {
    *m.interpreted.entry(target).or_default() += 1;
    let budget = m.budget;
    m.chain.push(target);
    let r = interpret_inner(m, s, target, args, budget);
    m.chain.pop();
    r
}

fn interpret_inner(m: &mut Machine, s: &mut dyn Subsystems, target: u32, args: &mut [Arg], budget: u64) -> i32 {
    let sp_top = m.sp_top;
    // Out buffers become stack memory the callee can write; read back after the call
    let mut sp = sp_top;
    let mut outs = Vec::new();
    let mut words = Vec::with_capacity(args.len());
    for (i, a) in args.iter().enumerate() {
        match a {
            Arg::I(v) => words.push(*v as u32),
            Arg::Any => words.push(0),
            Arg::Out(buf) => {
                sp -= 4 * (buf.len().max(4) as u32);
                for (k, w) in buf.iter().enumerate() {
                    m.ram.set_u32(sp + 4 * k as u32, w.unwrap_or(0) as u32);
                }
                outs.push((i, sp));
                words.push(sp);
            }
        }
    }
    sp &= !7;
    if let Some((f, t, dir)) = m.snapshot_at.clone() {
        if f == m.frame && t == target && m.pclog.is_none() {
            snapshot(m, &dir, target, &words, sp);
            m.pclog = Some(Vec::new());
            m.pclog_owner = Some(target);
        }
    }
    let mut regs = Regs { ctl: if m.in_irq > 0 { 0x92 } else { 0x1f }, ..Default::default() };
    let r0;
    let mut steps = 0;
    {
        let mut core = Core::with_bus(&mut m.ram, regs.clone());
        if let Err(e) = core.begin_call(target, &words, sp) {
            m.faults.push(format!("{target:#x}: {e}"));
            return 0;
        }
        regs = core.regs.clone();
    }
    loop {
        let stop = {
            let mut core = Core::with_bus(&mut m.ram, std::mem::take(&mut regs));
            core.steps = steps;
            core.trace = m.trace.take();
            core.pclog = m.pclog.take();
            core.pclog_window = m.pclog_window.as_ref().map(|w| (w.0, w.1));
            core.pclog_done = m.pclog_done;
            core.profile = m.profile.take();
            // time passes with the instructions run (INSTR_PER_TICK each SDK tick): the slice ends when the next
            // VBlank is due, so its interrupt can preempt busy code as on the DS
            let until_vblank = m.vblank_tick.saturating_sub(m.tick).saturating_mul(INSTR_PER_TICK).max(1);
            let slice = if m.in_irq == 0 { until_vblank.min(budget.saturating_sub(steps)) } else { budget.saturating_sub(steps) };
            let before = core.steps;
            let r = core.run(slice);
            m.instr_carry += core.steps - before;
            m.tick += m.instr_carry / INSTR_PER_TICK;
            m.instr_carry %= INSTR_PER_TICK;
            if m.in_irq == 0 && m.tick >= m.vblank_tick {
                m.vblank_tick += super::TICKS_PER_FRAME;
            }
            steps = core.steps;
            regs = core.regs;
            m.trace = core.trace.take();
            m.pclog = core.pclog.take();
            if m.pclog_window.is_some() {
                m.pclog_done = core.pclog_done;
                if m.pclog_done {
                    if let (Some(log), Some((_, _, path))) = (m.pclog.take(), m.pclog_window.take()) {
                        let t: String = log.iter().map(|e| e.iter().map(|v| format!("{v:x}")).collect::<Vec<_>>().join(" ") + "\n").collect();
                        let _ = std::fs::write(&path, t);
                        eprintln!("pclog: {} instructions -> {path}", log.len());
                    }
                }
            }
            m.profile = core.profile.take();
            r
        };
        if std::mem::take(&mut m.ram.tm0_started) && m.tick_base.is_none() {
            m.tick_base = Some(m.tick);
        }
        match stop {
            Ok(Stop::Returned) => {
                r0 = regs.r[0];
                m.last_r1 = regs.r[1];
                break;
            }
            Ok(Stop::Trap(pc)) if context_switch(m, &mut regs, pc) => {}
            Ok(Stop::Trap(pc)) if m.stop_at == Some(pc) => {
                r0 = regs.r[0];
                break;
            }
            Ok(Stop::Trap(pc)) if m.breakpoints.contains(&pc) => {
                let t = m.trace.as_ref().map(|t| t.iter().rev().take(24).map(|(f, to)| format!("{f:#x}->{to:#x}")).collect::<Vec<_>>().join(" ")).unwrap_or_default();
                let chain: Vec<String> = m.chain.iter().map(|&a| format!("{a:#x}")).collect();
                m.faults.push(format!("breakpoint {pc:#x} [r0-r3 {:#x} {:#x} {:#x} {:#x}; lr {:#x}; chain {} ; recent calls {t}]", regs.r[0], regs.r[1], regs.r[2], regs.r[3], regs.r[14], chain.join(" > ")));
                r0 = 0;
                break;
            }
            Ok(Stop::Trap(pc)) => {
                // the engine answers this function; arguments r0..r3, then the caller's stack
                let sp = regs.r[13];
                let mut a: Vec<Arg> = (0..4).map(|i| Arg::I(regs.r[i] as i32)).collect();
                a.extend((0..4).map(|i| Arg::I(m.ram.u32(sp + 4 * i) as i32)));
                m.sp_top = sp.wrapping_sub(0x40) & !7;
                m.trap_depth += 1;
                let v = ext::call_answered(m, s, pc, &mut a);
                m.trap_depth -= 1;
                m.sp_top = sp_top;
                let mut core = Core::with_bus(&mut m.ram, std::mem::take(&mut regs));
                core.regs.r[1] = m.ret_hi.take().unwrap_or(core.regs.r[1]);
                core.resume_return(v as u32);
                regs = core.regs;
            }
            Ok(Stop::Irq) => {
                // the CPU takes the interrupt between two instructions: the handler runs on the interrupt stack
                // (as the game's code), then the interrupt's exit switches threads if a woken one should run
                take_irqs(m);
                if m.trap_depth == 0 {
                    irq_exit_reschedule(m, &mut regs);
                }
            }
            Err(crate::arm::Fault::Budget { .. }) if steps < budget => {
                // a time slice ended: the next VBlank is due
                m.ram.irq_raised |= 1;
            }
            Ok(Stop::Swi(n)) => {
                if let Err(e) = bios(m, &mut regs, n) {
                    m.faults.push(format!("{target:#x}: SWI {n:#x}: {e}"));
                    r0 = 0;
                    break;
                }
            }
            Err(e) => {
                let mut chain: Vec<String> = m.chain.iter().map(|&a| format!("{a:#x}")).collect();
                if let Some(t) = &m.trace {
                    chain.push(format!("; recent calls {}", t.iter().rev().take(16).map(|(f, to)| format!("{f:#x}->{to:#x}")).collect::<Vec<_>>().join(" ")));
                }
                m.faults.push(format!("{e} [lr {:#x}; chain {}]", regs.r[14], chain.join(" > ")));
                r0 = 0;
                break;
            }
        }
    }
    m.steps += steps;
    if m.pclog_owner == Some(target) {
        if let (Some(log), Some((_, _, dir))) = (m.pclog.take(), m.snapshot_at.take()) {
            let mut b = Vec::with_capacity(log.len() * 68);
            for e in &log {
                for w in e {
                    b.extend_from_slice(&w.to_le_bytes());
                }
            }
            let _ = std::fs::write(format!("{dir}/pclog.bin"), b);
        }
        m.pclog_owner = None;
    }
    for (i, at) in outs {
        if let Some(Arg::Out(buf)) = args.get_mut(i) {
            for k in 0..buf.len() {
                buf[k] = Some(m.ram.u32(at + 4 * k as u32) as i32);
            }
        }
    }
    r0 as i32
}

/// the BIOS calls the game's code makes (ARM9 BIOS numbering)
fn bios(m: &mut Machine, regs: &mut Regs, n: u32) -> Result<(), String> {
    let r = &mut regs.r;
    match n {
        // WaitByLoop(count), Halt, VBlankIntrWait, IntrWait: time passes outside the interpreter
        0x03 | 0x06 | 0x05 | 0x04 => {}
        // Div: r0 / r1 -> r0 quotient, r1 remainder, r3 |quotient|
        0x09 => {
            let (a, b) = (r[0] as i32, r[1] as i32);
            if b == 0 {
                return Err("division by zero".into());
            }
            let q = a.wrapping_div(b);
            r[1] = a.wrapping_rem(b) as u32;
            r[0] = q as u32;
            r[3] = q.unsigned_abs();
        }
        // CpuSet(src, dst, ctl): ctl bits 0..20 count, bit 24 fill, bit 26 words
        0x0b => {
            let (src, dst, ctl) = (r[0], r[1], r[2]);
            let cnt = ctl & 0x1fffff;
            let (fill, unit) = (ctl & (1 << 24) != 0, if ctl & (1 << 26) != 0 { 4 } else { 2 });
            for i in 0..cnt {
                let s = if fill { src } else { src + i * unit };
                let v = m.ram.read(s, unit);
                m.ram.write(dst + i * unit, v, unit);
            }
        }
        // CpuFastSet(src, dst, ctl): words, count rounded up to 8
        0x0c => {
            let (src, dst, ctl) = (r[0], r[1], r[2]);
            let cnt = ((ctl & 0x1fffff) + 7) & !7;
            let fill = ctl & (1 << 24) != 0;
            for i in 0..cnt {
                let v = m.ram.u32(if fill { src } else { src + 4 * i });
                m.ram.set_u32(dst + 4 * i, v);
            }
        }
        // Sqrt(u32)
        0x0d => r[0] = (r[0] as f64).sqrt() as u32,
        // LZ77UncompReadNormalWrite8bit / ..16bit (src, dst)
        0x11 | 0x12 => {
            let src = r[0];
            let hdr = m.ram.u32(src);
            let len = (hdr >> 8) as usize;
            let packed = m.ram.bytes(src, len * 2 + 16);
            let out = crate::save::lz10(&packed).ok_or("bad LZ77 data")?;
            m.ram.put(r[1], &out[..len.min(out.len())]);
        }
        _ => return Err(format!("unhandled BIOS call {n:#x}")),
    }
    Ok(())
}

/// write the machine's memory and a call's entry state (diagnostics: lockstep comparison with another emulator,
/// sandbox/flow/tools/lockstep.py); the directory must stay out of the repository
fn snapshot(m: &Machine, dir: &str, target: u32, args: &[u32], sp: u32) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(format!("{dir}/main.bin"), m.ram.bytes(super::ram::MAIN, super::ram::MAIN_SIZE as usize));
    let _ = std::fs::write(format!("{dir}/dtcm.bin"), m.ram.bytes(super::ram::DTCM, super::ram::DTCM_SIZE as usize));
    let _ = std::fs::write(format!("{dir}/itcm.bin"), m.ram.bytes(super::ram::ITCM, super::ram::ITCM_SIZE as usize));
    let traps: Vec<String> = super::ext::trapped_addresses().iter().map(|a| format!("{a:x}")).collect();
    let entry = serde_json::json!({ "target": target, "args": args, "sp": sp, "in_irq": m.in_irq, "traps": traps });
    let _ = std::fs::write(format!("{dir}/entry.json"), entry.to_string());
}
