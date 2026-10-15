//! An ARM946E-S (ARMv5TE) interpreter for running the game's own code, loaded from the user's ROM image at run time:
//! integer Thumb in full and integer ARM (data processing, multiplies, branches and interworking, single, double and
//! multiple loads and stores, CLZ, status register moves), with coprocessor operations as no-ops and software
//! interrupts handed to the host.
//!
//! Memory is a `Bus`: `Flat` is one block of main RAM (self-contained routines such as the handwriting recogniser);
//! the runtime's bus is the game's whole memory map. A bus can mark addresses as traps: reaching one stops the run so
//! the host answers that function itself (`Stop::Trap`) and resumes at the caller (`resume_return`). A call returns
//! when the routine branches back to a sentinel return address. Anything unhandled is reported as an error rather than
//! guessed at.

pub const RAM_BASE: u32 = 0x200_0000;
pub const RAM_SIZE: usize = 0x40_0000;
/// return address handed to a called routine: reaching it ends the call
pub const RETURN: u32 = 0xffff_fff0;

#[derive(Debug)]
pub enum Fault {
    Memory { addr: u32, pc: u32 },
    Undefined { op: u32, thumb: bool, pc: u32 },
    Budget { pc: u32 },
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Fault::Memory { addr, pc } => write!(f, "memory access at {addr:#x} outside the memory map (pc {pc:#x})"),
            Fault::Undefined { op, thumb, pc } => {
                write!(f, "unhandled {} instruction {op:#x} at {pc:#x}", if *thumb { "Thumb" } else { "ARM" })
            }
            Fault::Budget { pc } => write!(f, "instruction budget exhausted (pc {pc:#x})"),
        }
    }
}

type R<T> = Result<T, Fault>;

/// memory as the CPU sees it (addresses are full 32-bit; accesses are aligned by the CPU before they get here)
pub trait Bus {
    fn r8(&mut self, a: u32) -> Option<u32>;
    fn r16(&mut self, a: u32) -> Option<u32>;
    fn r32(&mut self, a: u32) -> Option<u32>;
    fn w8(&mut self, a: u32, v: u32) -> Option<()>;
    fn w16(&mut self, a: u32, v: u32) -> Option<()>;
    fn w32(&mut self, a: u32, v: u32) -> Option<()>;
    /// the host answers the function at this address itself
    #[inline]
    fn is_trap(&self, _pc: u32) -> bool {
        false
    }
    /// an interrupt is pending (taken when the CPU's I bit is clear)
    #[inline]
    fn irq_pending(&self) -> bool {
        false
    }
}

impl<T: Bus> Bus for &mut T {
    #[inline]
    fn r8(&mut self, a: u32) -> Option<u32> {
        (**self).r8(a)
    }
    #[inline]
    fn r16(&mut self, a: u32) -> Option<u32> {
        (**self).r16(a)
    }
    #[inline]
    fn r32(&mut self, a: u32) -> Option<u32> {
        (**self).r32(a)
    }
    #[inline]
    fn w8(&mut self, a: u32, v: u32) -> Option<()> {
        (**self).w8(a, v)
    }
    #[inline]
    fn w16(&mut self, a: u32, v: u32) -> Option<()> {
        (**self).w16(a, v)
    }
    #[inline]
    fn w32(&mut self, a: u32, v: u32) -> Option<()> {
        (**self).w32(a, v)
    }
    #[inline]
    fn is_trap(&self, pc: u32) -> bool {
        (**self).is_trap(pc)
    }
    #[inline]
    fn irq_pending(&self) -> bool {
        (**self).irq_pending()
    }
}

/// one flat block of main RAM
pub struct Flat(pub Vec<u8>);

impl Flat {
    #[inline]
    fn off(&self, a: u32, n: u32) -> Option<usize> {
        let o = a.wrapping_sub(RAM_BASE);
        (o < RAM_SIZE as u32 && o as usize + n as usize <= RAM_SIZE).then_some(o as usize)
    }
}

impl Bus for Flat {
    #[inline]
    fn r8(&mut self, a: u32) -> Option<u32> {
        self.off(a, 1).map(|o| self.0[o] as u32)
    }
    #[inline]
    fn r16(&mut self, a: u32) -> Option<u32> {
        self.off(a, 2).map(|o| u16::from_le_bytes([self.0[o], self.0[o + 1]]) as u32)
    }
    #[inline]
    fn r32(&mut self, a: u32) -> Option<u32> {
        self.off(a, 4).map(|o| u32::from_le_bytes(self.0[o..o + 4].try_into().unwrap()))
    }
    #[inline]
    fn w8(&mut self, a: u32, v: u32) -> Option<()> {
        let o = self.off(a, 1)?;
        self.0[o] = v as u8;
        Some(())
    }
    #[inline]
    fn w16(&mut self, a: u32, v: u32) -> Option<()> {
        let o = self.off(a, 2)?;
        self.0[o..o + 2].copy_from_slice(&(v as u16).to_le_bytes());
        Some(())
    }
    #[inline]
    fn w32(&mut self, a: u32, v: u32) -> Option<()> {
        let o = self.off(a, 4)?;
        self.0[o..o + 4].copy_from_slice(&v.to_le_bytes());
        Some(())
    }
}

/// the CPU's registers and flags (kept apart from memory so a host can hold them across runs)
#[derive(Clone, Debug, Default)]
pub struct Regs {
    pub r: [u32; 16],
    pub n: bool,
    pub z: bool,
    pub c: bool,
    pub v: bool,
    pub thumb: bool,
    /// CPSR control bits other than T (I, F, mode): kept for MRS / MSR
    pub ctl: u32,
}

/// why a run stopped
#[derive(Debug, PartialEq)]
pub enum Stop {
    /// the routine returned to the sentinel (r0 holds its result)
    Returned,
    /// execution reached a trapped address (the function to answer); r0..r3 and the stack hold its arguments
    Trap(u32),
    /// a software interrupt (BIOS call) with its number; the PC is past the instruction
    Swi(u32),
    /// an interrupt is pending and enabled: the host runs its handler, then resumes here
    Irq,
}

pub struct Core<B: Bus> {
    pub regs: Regs,
    pub bus: B,
    /// instructions executed since creation
    pub steps: u64,
    /// the last call targets (BL / BLX), most recent last, when tracing
    pub trace: Option<std::collections::VecDeque<(u32, u32)>>,
    /// every executed instruction's address and r0..r15 after it, when logging (diagnostics: lockstep comparison)
    pub pclog: Option<Vec<[u32; 17]>>,
    /// instructions run per halfword of code (`profile_index`), when profiling
    pub profile: Option<Vec<u32>>,
}

/// the flat-memory CPU for self-contained routines
pub type Cpu = Core<Flat>;

impl Cpu {
    pub fn new() -> Cpu {
        Core { regs: Regs { ctl: 0x1f, ..Default::default() }, bus: Flat(vec![0; RAM_SIZE]), steps: 0, trace: None, pclog: None, profile: None }
    }
}

impl Default for Cpu {
    fn default() -> Self {
        Cpu::new()
    }
}

#[inline]
fn add_flags(a: u32, b: u32, carry: bool) -> (u32, bool, bool) {
    let s = a as u64 + b as u64 + carry as u64;
    let r = s as u32;
    let v = ((a ^ r) & (b ^ r)) >> 31 != 0;
    (r, s >> 32 != 0, v)
}

impl<B: Bus> Core<B> {
    pub fn with_bus(bus: B, regs: Regs) -> Core<B> {
        Core { regs, bus, steps: 0, trace: None, pclog: None, profile: None }
    }

    // ---- memory -------------------------------------------------------------------------------------------------
    #[inline]
    fn fault(&self, addr: u32) -> Fault {
        Fault::Memory { addr, pc: self.regs.r[15] }
    }
    pub fn write(&mut self, addr: u32, data: &[u8]) -> R<()> {
        for (i, &b) in data.iter().enumerate() {
            self.w8(addr + i as u32, b as u32)?;
        }
        Ok(())
    }
    #[inline]
    pub fn r8(&mut self, a: u32) -> R<u32> {
        self.bus.r8(a).ok_or_else(|| self.fault(a))
    }
    #[inline]
    pub fn r16(&mut self, a: u32) -> R<u32> {
        let a = a & !1; // ARMv5: an unaligned halfword access reads the aligned halfword
        self.bus.r16(a).ok_or_else(|| self.fault(a))
    }
    #[inline]
    fn r32_aligned(&mut self, a: u32) -> R<u32> {
        self.bus.r32(a & !3).ok_or_else(|| self.fault(a))
    }
    /// LDR: an unaligned word load reads the aligned word rotated right by 8 * (addr & 3)
    #[inline]
    pub fn r32(&mut self, a: u32) -> R<u32> {
        Ok(self.r32_aligned(a)?.rotate_right(8 * (a & 3)))
    }
    #[inline]
    pub fn w8(&mut self, a: u32, v: u32) -> R<()> {
        self.bus.w8(a, v).ok_or_else(|| self.fault(a))
    }
    #[inline]
    pub fn w16(&mut self, a: u32, v: u32) -> R<()> {
        self.bus.w16(a & !1, v).ok_or_else(|| self.fault(a))
    }
    #[inline]
    pub fn w32(&mut self, a: u32, v: u32) -> R<()> {
        self.bus.w32(a & !3, v).ok_or_else(|| self.fault(a))
    }

    // ---- calls ----------------------------------------------------------------------------------------------------
    /// set up a call of the routine at `entry` (bit 0 set: Thumb) with the ARM procedure call standard: the first four
    /// arguments in r0..r3, the rest on the stack below `sp`; it returns to the sentinel
    pub fn begin_call(&mut self, entry: u32, args: &[u32], sp: u32) -> R<()> {
        let mut sp = sp;
        if args.len() > 4 {
            sp -= 4 * (args.len() - 4) as u32;
            for (i, &a) in args[4..].iter().enumerate() {
                self.w32(sp + 4 * i as u32, a)?;
            }
        }
        for (i, &a) in args.iter().take(4).enumerate() {
            self.regs.r[i] = a;
        }
        self.regs.r[13] = sp;
        self.regs.r[14] = RETURN;
        self.regs.thumb = entry & 1 != 0;
        self.regs.r[15] = entry & !1;
        Ok(())
    }

    /// run until the routine returns, a trap is reached (not at the first instruction) or a SWI executes
    pub fn run(&mut self, budget: u64) -> R<Stop> {
        let end = self.steps + budget;
        let mut first = true;
        loop {
            let pc = self.regs.r[15];
            if pc == RETURN & !1 {
                return Ok(Stop::Returned);
            }
            if !first && self.bus.is_trap(pc) {
                return Ok(Stop::Trap(pc));
            }
            if !first && self.regs.ctl & 0x80 == 0 && self.bus.irq_pending() {
                return Ok(Stop::Irq);
            }
            first = false;
            if self.steps >= end {
                return Err(Fault::Budget { pc });
            }
            self.steps += 1;
            if let Some(p) = self.profile.as_mut() {
                if let Some(i) = profile_index(pc) {
                    p[i] = p[i].saturating_add(1);
                }
            }
            let lr0 = self.regs.r[14];
            let swi = if self.regs.thumb { self.step_thumb()? } else { self.step_arm()? };
            if let Some(l) = self.pclog.as_mut().filter(|l| l.len() < 3_000_000) {
                let mut e = [0u32; 17];
                e[0] = pc;
                e[1..].copy_from_slice(&self.regs.r);
                l.push(e);
            }
            if let Some(t) = self.trace.as_mut() {
                let lr = self.regs.r[14];
                if lr != lr0 && (lr & !1) == pc + if self.regs.thumb || lr & 1 != 0 { 4 } else { 4 } || (lr & !1) == pc + 2 {
                    if t.len() >= 64 {
                        t.pop_front();
                    }
                    t.push_back((pc, self.regs.r[15]));
                }
            }
            if let Some(n) = swi {
                return Ok(Stop::Swi(n));
            }
        }
    }

    /// after the host answered a trapped function: return `r0` to its caller (as the function's `bx lr` would)
    pub fn resume_return(&mut self, r0: u32) {
        self.regs.r[0] = r0;
        let lr = self.regs.r[14];
        self.interwork(lr);
    }

    /// call a routine and run it to its return (traps and SWIs are errors here: for self-contained routines)
    pub fn call(&mut self, entry: u32, args: &[u32], sp: u32, budget: u64) -> R<u32> {
        self.begin_call(entry, args, sp)?;
        match self.run(budget)? {
            Stop::Returned => Ok(self.regs.r[0]),
            Stop::Trap(pc) | Stop::Swi(pc) => Err(Fault::Undefined { op: pc, thumb: self.regs.thumb, pc }),
            Stop::Irq => Err(Fault::Undefined { op: 0, thumb: self.regs.thumb, pc: self.regs.r[15] }),
        }
    }

    /// the CPSR as MRS reads it
    pub fn cpsr(&self) -> u32 {
        let g = &self.regs;
        (g.n as u32) << 31 | (g.z as u32) << 30 | (g.c as u32) << 29 | (g.v as u32) << 28 | (g.thumb as u32) << 5 | (g.ctl & 0xdf)
    }

    /// BX-style branch: bit 0 selects Thumb
    #[inline]
    fn interwork(&mut self, target: u32) {
        self.regs.thumb = target & 1 != 0;
        self.regs.r[15] = if self.regs.thumb { target & !1 } else { target & !3 };
    }

    #[inline]
    fn set_nz(&mut self, r: u32) {
        self.regs.n = r >> 31 != 0;
        self.regs.z = r == 0;
    }

    #[inline]
    fn add(&mut self, a: u32, b: u32, carry: bool, s: bool) -> u32 {
        let (r, c, v) = add_flags(a, b, carry);
        if s {
            self.set_nz(r);
            self.regs.c = c;
            self.regs.v = v;
        }
        r
    }
    /// a - b (- !carry): ARM's subtraction, carry = no borrow
    #[inline]
    fn sub(&mut self, a: u32, b: u32, carry: bool, s: bool) -> u32 {
        self.add(a, !b, carry, s)
    }

    fn cond(&self, c: u32) -> bool {
        match c {
            0 => self.regs.z,
            1 => !self.regs.z,
            2 => self.regs.c,
            3 => !self.regs.c,
            4 => self.regs.n,
            5 => !self.regs.n,
            6 => self.regs.v,
            7 => !self.regs.v,
            8 => self.regs.c && !self.regs.z,
            9 => !self.regs.c || self.regs.z,
            10 => self.regs.n == self.regs.v,
            11 => self.regs.n != self.regs.v,
            12 => !self.regs.z && self.regs.n == self.regs.v,
            13 => self.regs.z || self.regs.n != self.regs.v,
            _ => true,
        }
    }

    /// shift `v` by `kind` (0 LSL, 1 LSR, 2 ASR, 3 ROR) by a register amount (the low byte): (result, carry out)
    #[inline]
    fn shift_reg(&self, kind: u32, v: u32, amount: u32) -> (u32, bool) {
        let n = amount & 0xff;
        if n == 0 {
            return (v, self.regs.c);
        }
        match kind {
            0 => match n {
                1..=31 => (v << n, (v >> (32 - n)) & 1 != 0),
                32 => (0, v & 1 != 0),
                _ => (0, false),
            },
            1 => match n {
                1..=31 => (v >> n, (v >> (n - 1)) & 1 != 0),
                32 => (0, v >> 31 != 0),
                _ => (0, false),
            },
            2 => {
                if n < 32 {
                    (((v as i32) >> n) as u32, (v >> (n - 1)) & 1 != 0)
                } else {
                    (((v as i32) >> 31) as u32, v >> 31 != 0)
                }
            }
            _ => {
                let k = n & 31;
                if k == 0 {
                    (v, v >> 31 != 0)
                } else {
                    (v.rotate_right(k), (v >> (k - 1)) & 1 != 0)
                }
            }
        }
    }

    /// shift by an immediate field (0 encodes LSR/ASR #32 and RRX)
    #[inline]
    fn shift_imm(&self, kind: u32, v: u32, n: u32) -> (u32, bool) {
        match (kind, n) {
            (0, 0) => (v, self.regs.c),
            (1, 0) | (2, 0) => self.shift_reg(kind, v, 32),
            (3, 0) => ((v >> 1) | ((self.regs.c as u32) << 31), v & 1 != 0),
            _ => self.shift_reg(kind, v, n),
        }
    }

    // ---- Thumb --------------------------------------------------------------------------------------------------
    /// one Thumb instruction; Some(n): it was SWI n
    fn step_thumb(&mut self) -> R<Option<u32>> {
        let pc = self.regs.r[15];
        let op = self.r16(pc)?;
        if op >> 8 == 0xdf {
            self.regs.r[15] = pc + 2;
            return Ok(Some(op & 0xff));
        }
        self.thumb_op(pc, op)?;
        Ok(None)
    }

    fn thumb_op(&mut self, pc: u32, op: u32) -> R<()> {
        self.regs.r[15] = pc + 2;
        let pcv = pc + 4; // the value of PC as an operand
        let rd = (op & 7) as usize;
        let rs = ((op >> 3) & 7) as usize;
        match op >> 11 {
            // shift by immediate
            0..=2 => {
                let kind = op >> 11;
                let (r, c) = self.shift_imm(kind, self.regs.r[rs], (op >> 6) & 31);
                self.regs.r[rd] = r;
                self.set_nz(r);
                self.regs.c = c;
            }
            // add / subtract register or 3-bit immediate
            3 => {
                let b = if op & 0x400 != 0 { (op >> 6) & 7 } else { self.regs.r[((op >> 6) & 7) as usize] };
                let a = self.regs.r[rs];
                self.regs.r[rd] = if op & 0x200 != 0 { self.sub(a, b, true, true) } else { self.add(a, b, false, true) };
            }
            // MOV / CMP / ADD / SUB with an 8-bit immediate
            4..=7 => {
                let d = ((op >> 8) & 7) as usize;
                let imm = op & 0xff;
                match (op >> 11) & 3 {
                    0 => {
                        self.regs.r[d] = imm;
                        self.set_nz(imm);
                    }
                    1 => {
                        self.sub(self.regs.r[d], imm, true, true);
                    }
                    2 => self.regs.r[d] = self.add(self.regs.r[d], imm, false, true),
                    _ => self.regs.r[d] = self.sub(self.regs.r[d], imm, true, true),
                }
            }
            8 => {
                if op & 0x400 == 0 {
                    self.thumb_alu(op, rd, rs);
                } else {
                    // high register operations and branch exchange
                    let d = rd | ((op >> 4) & 8) as usize;
                    let m = ((op >> 3) & 15) as usize;
                    let mv = if m == 15 { pcv } else { self.regs.r[m] };
                    match (op >> 8) & 3 {
                        0 => {
                            let dv = if d == 15 { pcv } else { self.regs.r[d] };
                            let r = dv.wrapping_add(mv);
                            if d == 15 {
                                self.regs.r[15] = r & !1;
                            } else {
                                self.regs.r[d] = r;
                            }
                        }
                        1 => {
                            let dv = if d == 15 { pcv } else { self.regs.r[d] };
                            self.sub(dv, mv, true, true);
                        }
                        2 => {
                            if d == 15 {
                                self.regs.r[15] = mv & !1;
                            } else {
                                self.regs.r[d] = mv;
                            }
                        }
                        _ => {
                            if op & 0x80 != 0 {
                                self.regs.r[14] = (pc + 2) | 1; // BLX register
                            }
                            self.interwork(mv);
                        }
                    }
                }
            }
            // LDR Rd, [PC, #imm8 * 4]
            9 => {
                let d = ((op >> 8) & 7) as usize;
                self.regs.r[d] = self.r32((pcv & !3) + (op & 0xff) * 4)?;
            }
            // load / store with register offset
            10 | 11 => {
                let a = self.regs.r[rs].wrapping_add(self.regs.r[((op >> 6) & 7) as usize]);
                match (op >> 9) & 7 {
                    0 => self.w32(a, self.regs.r[rd])?,
                    1 => self.w16(a, self.regs.r[rd])?,
                    2 => self.w8(a, self.regs.r[rd])?,
                    3 => self.regs.r[rd] = self.r8(a)? as i8 as i32 as u32,
                    4 => self.regs.r[rd] = self.r32(a)?,
                    5 => self.regs.r[rd] = self.r16(a)?,
                    6 => self.regs.r[rd] = self.r8(a)?,
                    _ => self.regs.r[rd] = self.r16(a)? as i16 as i32 as u32,
                }
            }
            // load / store with a 5-bit immediate offset
            12 => self.w32(self.regs.r[rs].wrapping_add(((op >> 6) & 31) * 4), self.regs.r[rd])?,
            13 => self.regs.r[rd] = self.r32(self.regs.r[rs].wrapping_add(((op >> 6) & 31) * 4))?,
            14 => self.w8(self.regs.r[rs].wrapping_add((op >> 6) & 31), self.regs.r[rd])?,
            15 => self.regs.r[rd] = self.r8(self.regs.r[rs].wrapping_add((op >> 6) & 31))?,
            16 => self.w16(self.regs.r[rs].wrapping_add(((op >> 6) & 31) * 2), self.regs.r[rd])?,
            17 => self.regs.r[rd] = self.r16(self.regs.r[rs].wrapping_add(((op >> 6) & 31) * 2))?,
            // SP-relative load / store
            18 | 19 => {
                let d = ((op >> 8) & 7) as usize;
                let a = self.regs.r[13].wrapping_add((op & 0xff) * 4);
                if op & 0x800 != 0 {
                    self.regs.r[d] = self.r32(a)?;
                } else {
                    self.w32(a, self.regs.r[d])?;
                }
            }
            // ADD Rd, PC / SP, #imm8 * 4
            20 | 21 => {
                let d = ((op >> 8) & 7) as usize;
                let base = if op & 0x800 != 0 { self.regs.r[13] } else { pcv & !3 };
                self.regs.r[d] = base.wrapping_add((op & 0xff) * 4);
            }
            22 | 23 => self.thumb_misc(op, pc)?,
            // LDMIA / STMIA Rb!, {rlist}
            24 | 25 => {
                let b = ((op >> 8) & 7) as usize;
                let list = op & 0xff;
                let mut a = self.regs.r[b];
                if op & 0x800 != 0 {
                    for i in 0..8 {
                        if list & (1 << i) != 0 {
                            self.regs.r[i] = self.r32_aligned(a)?;
                            a = a.wrapping_add(4);
                        }
                    }
                    if list & (1 << b) == 0 {
                        self.regs.r[b] = a;
                    }
                } else {
                    for i in 0..8 {
                        if list & (1 << i) != 0 {
                            self.w32(a, self.regs.r[i])?;
                            a = a.wrapping_add(4);
                        }
                    }
                    self.regs.r[b] = a;
                }
            }
            // conditional branch / SWI
            26 | 27 => {
                let c = (op >> 8) & 15;
                if c >= 14 {
                    return Err(Fault::Undefined { op, thumb: true, pc });
                }
                if self.cond(c) {
                    self.regs.r[15] = pcv.wrapping_add(((op & 0xff) as u8 as i8 as i32 * 2) as u32);
                }
            }
            // unconditional branch
            28 => self.regs.r[15] = pcv.wrapping_add((((op & 0x7ff) << 21) as i32 >> 20) as u32),
            // BLX suffix (to ARM)
            29 => {
                if op & 1 != 0 {
                    return Err(Fault::Undefined { op, thumb: true, pc });
                }
                let t = self.regs.r[14].wrapping_add((op & 0x7ff) << 1) & !3;
                self.regs.r[14] = (pc + 2) | 1;
                self.regs.thumb = false;
                self.regs.r[15] = t;
            }
            // BL / BLX prefix: LR = PC + (offset << 12)
            30 => self.regs.r[14] = pcv.wrapping_add((((op & 0x7ff) << 21) as i32 >> 9) as u32),
            // BL suffix
            _ => {
                let t = self.regs.r[14].wrapping_add((op & 0x7ff) << 1);
                self.regs.r[14] = (pc + 2) | 1;
                self.regs.r[15] = t;
            }
        }
        Ok(())
    }

    fn thumb_alu(&mut self, op: u32, d: usize, s: usize) {
        let a = self.regs.r[d];
        let b = self.regs.r[s];
        let r = match (op >> 6) & 15 {
            0 => a & b,
            1 => a ^ b,
            2 | 3 | 4 | 7 => {
                let kind = match (op >> 6) & 15 {
                    2 => 0,
                    3 => 1,
                    4 => 2,
                    _ => 3,
                };
                let (r, c) = self.shift_reg(kind, a, b);
                self.regs.c = c;
                r
            }
            5 => self.add(a, b, self.regs.c, true),
            6 => self.sub(a, b, self.regs.c, true),
            8 => {
                let r = a & b;
                self.set_nz(r);
                return; // TST
            }
            9 => self.sub(0, b, true, true), // NEG
            10 => {
                self.sub(a, b, true, true); // CMP
                return;
            }
            11 => {
                self.add(a, b, false, true); // CMN
                return;
            }
            12 => a | b,
            13 => a.wrapping_mul(b), // MUL: ARMv5 leaves C unchanged
            14 => a & !b,
            _ => !b,
        };
        self.regs.r[d] = r;
        self.set_nz(r);
    }

    fn thumb_misc(&mut self, op: u32, pc: u32) -> R<()> {
        match (op >> 8) & 15 {
            0 => {
                let imm = (op & 0x7f) * 4;
                self.regs.r[13] = if op & 0x80 != 0 { self.regs.r[13].wrapping_sub(imm) } else { self.regs.r[13].wrapping_add(imm) };
            }
            4 | 5 => {
                // PUSH {rlist, LR}
                let list = (op & 0xff) | if op & 0x100 != 0 { 1 << 14 } else { 0 };
                let mut a = self.regs.r[13].wrapping_sub(4 * list.count_ones());
                self.regs.r[13] = a;
                for i in 0..15 {
                    if list & (1 << i) != 0 {
                        self.w32(a, self.regs.r[i])?;
                        a += 4;
                    }
                }
            }
            12 | 13 => {
                // POP {rlist, PC}: loading PC interworks (ARMv5)
                let mut a = self.regs.r[13];
                for i in 0..8 {
                    if op & (1 << i) != 0 {
                        self.regs.r[i] = self.r32_aligned(a)?;
                        a += 4;
                    }
                }
                if op & 0x100 != 0 {
                    let t = self.r32_aligned(a)?;
                    a += 4;
                    self.regs.r[13] = a;
                    self.interwork(t);
                } else {
                    self.regs.r[13] = a;
                }
            }
            _ => return Err(Fault::Undefined { op, thumb: true, pc }),
        }
        Ok(())
    }

    // ---- ARM ----------------------------------------------------------------------------------------------------
    /// one ARM instruction; Some(n): it was SWI n (the BIOS takes the number from bits 16..23)
    fn step_arm(&mut self) -> R<Option<u32>> {
        let pc = self.regs.r[15];
        let op = self.r32_aligned(pc)?;
        if op >> 28 != 15 && (op >> 24) & 15 == 15 {
            self.regs.r[15] = pc + 4;
            return Ok(self.cond(op >> 28).then_some((op >> 16) & 0xff));
        }
        self.arm_op(pc, op)?;
        Ok(None)
    }

    fn arm_op(&mut self, pc: u32, op: u32) -> R<()> {
        self.regs.r[15] = pc + 4;
        let pcv = pc + 8;
        let cond = op >> 28;
        if cond == 15 {
            // BLX immediate
            if (op >> 25) & 7 == 5 {
                let off = ((op << 8) as i32 >> 6) as u32 | ((op >> 23) & 2);
                self.regs.r[14] = pc + 4;
                self.regs.thumb = true;
                self.regs.r[15] = pcv.wrapping_add(off);
                return Ok(());
            }
            return Err(Fault::Undefined { op, thumb: false, pc });
        }
        if !self.cond(cond) {
            return Ok(());
        }
        let reg = |s: &Self, i: u32| if i == 15 { pcv } else { s.regs.r[i as usize] };
        match (op >> 25) & 7 {
            0 | 1 => {
                if op & 0x0ff0_00f0 == 0x0120_0010 || op & 0x0ff0_00f0 == 0x0120_0030 {
                    // BX / BLX register
                    let t = reg(self, op & 15);
                    if op & 0x20 != 0 {
                        self.regs.r[14] = pc + 4;
                    }
                    self.interwork(t);
                    return Ok(());
                }
                if op & 0x0fff_0ff0 == 0x016f_0f10 {
                    // CLZ
                    self.regs.r[((op >> 12) & 15) as usize] = reg(self, op & 15).leading_zeros();
                    return Ok(());
                }
                if op & 0x0e00_0090 == 0x0000_0090 && (op >> 25) & 1 == 0 {
                    return self.arm_mul_or_extra(op, pc, pcv);
                }
                if op & 0x0f90_0090 == 0x0100_0080 {
                    // SMLAxy / SMLAWy / SMULWy / SMLALxy / SMULxy (ARMv5TE)
                    return self.arm_dsp_mul(op);
                }
                if op & 0x0f90_00f0 == 0x0100_0050 {
                    // QADD / QSUB / QDADD / QDSUB
                    return self.arm_qarith(op);
                }
                if op & 0x0d90_0000 == 0x0100_0000 {
                    // TST/TEQ/CMP/CMN without S: MRS / MSR
                    return self.arm_psr(op, pc, pcv);
                }
                self.arm_dp(op, pc, pcv)
            }
            2 | 3 => {
                if (op >> 25) & 1 != 0 && op & 0x10 != 0 {
                    return Err(Fault::Undefined { op, thumb: false, pc });
                }
                // LDR / STR / LDRB / STRB
                let n = (op >> 16) & 15;
                let d = ((op >> 12) & 15) as usize;
                let off = if (op >> 25) & 1 == 0 {
                    op & 0xfff
                } else {
                    self.shift_imm((op >> 5) & 3, reg(self, op & 15), (op >> 7) & 31).0
                };
                let base = reg(self, n);
                let up = op & (1 << 23) != 0;
                let target = if up { base.wrapping_add(off) } else { base.wrapping_sub(off) };
                let pre = op & (1 << 24) != 0;
                let a = if pre { target } else { base };
                let byte = op & (1 << 22) != 0;
                let load = op & (1 << 20) != 0;
                if load {
                    let v = if byte { self.r8(a)? } else { self.r32(a)? };
                    if !pre || op & (1 << 21) != 0 {
                        self.regs.r[n as usize] = target;
                    }
                    if d == 15 {
                        self.interwork(v);
                    } else {
                        self.regs.r[d] = v;
                    }
                } else {
                    let v = if d == 15 { pc + 12 } else { self.regs.r[d] };
                    if byte {
                        self.w8(a, v)?;
                    } else {
                        self.w32(a, v)?;
                    }
                    if !pre || op & (1 << 21) != 0 {
                        self.regs.r[n as usize] = target;
                    }
                }
                Ok(())
            }
            4 => {
                // LDM / STM (user-bank and SPSR forms unsupported)
                if op & (1 << 22) != 0 {
                    return Err(Fault::Undefined { op, thumb: false, pc });
                }
                let n = ((op >> 16) & 15) as usize;
                let list = op & 0xffff;
                let cnt = list.count_ones();
                let base = self.regs.r[n];
                let up = op & (1 << 23) != 0;
                let pre = op & (1 << 24) != 0;
                let mut a = match (up, pre) {
                    (true, false) => base,
                    (true, true) => base + 4,
                    (false, false) => base.wrapping_sub(4 * cnt) + 4,
                    (false, true) => base.wrapping_sub(4 * cnt),
                };
                let new_base = if up { base.wrapping_add(4 * cnt) } else { base.wrapping_sub(4 * cnt) };
                let wb = op & (1 << 21) != 0;
                if op & (1 << 20) != 0 {
                    if wb {
                        self.regs.r[n] = new_base;
                    }
                    for i in 0..16 {
                        if list & (1 << i) != 0 {
                            let v = self.r32_aligned(a)?;
                            a += 4;
                            if i == 15 {
                                self.interwork(v);
                            } else {
                                self.regs.r[i] = v;
                            }
                        }
                    }
                } else {
                    for i in 0..16 {
                        if list & (1 << i) != 0 {
                            let v = if i == 15 { pc + 12 } else { self.regs.r[i] };
                            self.w32(a, v)?;
                            a += 4;
                        }
                    }
                    if wb {
                        self.regs.r[n] = new_base;
                    }
                }
                Ok(())
            }
            5 => {
                // B / BL
                if op & (1 << 24) != 0 {
                    self.regs.r[14] = pc + 4;
                }
                self.regs.r[15] = pcv.wrapping_add(((op << 8) as i32 >> 6) as u32);
                Ok(())
            }
            7 if op & (1 << 24) == 0 => {
                // CDP / MCR / MRC: the system control coprocessor (caches, protection unit) has no effect here; MRC
                // reads 0
                if op & 0x10 != 0 && op & (1 << 20) != 0 {
                    let d = ((op >> 12) & 15) as usize;
                    if d != 15 {
                        self.regs.r[d] = 0;
                    }
                }
                Ok(())
            }
            _ => Err(Fault::Undefined { op, thumb: false, pc }),
        }
    }

    /// the ARMv5TE signed halfword multiplies (the sticky Q flag is not kept)
    fn arm_dsp_mul(&mut self, op: u32) -> R<()> {
        let d = ((op >> 16) & 15) as usize;
        let acc = self.regs.r[((op >> 12) & 15) as usize];
        let rm = self.regs.r[(op & 15) as usize];
        let rs = self.regs.r[((op >> 8) & 15) as usize];
        let half = |v: u32, top: bool| if top { (v >> 16) as i16 as i32 } else { v as i16 as i32 };
        let (x, y) = ((op >> 5) & 1 != 0, (op >> 6) & 1 != 0);
        match (op >> 21) & 3 {
            0 => {
                // SMLAxy
                let p = half(rm, x).wrapping_mul(half(rs, y));
                self.regs.r[d] = (p as u32).wrapping_add(acc);
            }
            1 => {
                // SMLAWy (x = 0) / SMULWy (x = 1)
                let p = ((rm as i32 as i64 * half(rs, y) as i64) >> 16) as u32;
                self.regs.r[d] = if x { p } else { p.wrapping_add(acc) };
            }
            2 => {
                // SMLALxy: RdHi:RdLo += product
                let lo = ((op >> 12) & 15) as usize;
                let p = half(rm, x) as i64 * half(rs, y) as i64;
                let v = (((self.regs.r[d] as u64) << 32) | self.regs.r[lo] as u64).wrapping_add(p as u64);
                self.regs.r[lo] = v as u32;
                self.regs.r[d] = (v >> 32) as u32;
            }
            _ => {
                // SMULxy
                self.regs.r[d] = half(rm, x).wrapping_mul(half(rs, y)) as u32;
            }
        }
        Ok(())
    }

    /// QADD / QSUB / QDADD / QDSUB: saturating
    fn arm_qarith(&mut self, op: u32) -> R<()> {
        let d = ((op >> 12) & 15) as usize;
        let m = self.regs.r[(op & 15) as usize] as i32;
        let n = self.regs.r[((op >> 16) & 15) as usize] as i32;
        let nn = if op & (1 << 22) != 0 { n.saturating_mul(2) } else { n };
        self.regs.r[d] = if op & (1 << 21) != 0 { m.saturating_sub(nn) } else { m.saturating_add(nn) } as u32;
        Ok(())
    }

    /// MRS / MSR (the status register is kept as flags + control bits; SPSR reads 0, writes are dropped)
    fn arm_psr(&mut self, op: u32, pc: u32, _pcv: u32) -> R<()> {
        let spsr = op & (1 << 22) != 0;
        if op & 0x0fbf_0fff == 0x010f_0000 {
            // MRS
            let d = ((op >> 12) & 15) as usize;
            self.regs.r[d] = if spsr { 0 } else { self.cpsr() };
            return Ok(());
        }
        if op & 0x0fb0_f000 == 0x0320_f000 || op & 0x0fb0_fff0 == 0x0120_f000 {
            // MSR (immediate / register)
            let v = if op & (1 << 25) != 0 { (op & 0xff).rotate_right(((op >> 8) & 15) * 2) } else { self.regs.r[(op & 15) as usize] };
            if spsr {
                return Ok(());
            }
            let mask = (op >> 16) & 15;
            if mask & 8 != 0 {
                self.regs.n = v >> 31 != 0;
                self.regs.z = (v >> 30) & 1 != 0;
                self.regs.c = (v >> 29) & 1 != 0;
                self.regs.v = (v >> 28) & 1 != 0;
            }
            if mask & 1 != 0 {
                self.regs.ctl = v & 0xdf;
            }
            return Ok(());
        }
        Err(Fault::Undefined { op, thumb: false, pc })
    }

    fn arm_dp(&mut self, op: u32, pc: u32, pcv: u32) -> R<()> {
        let s = op & (1 << 20) != 0;
        let (b, sc) = if op & (1 << 25) != 0 {
            let rot = ((op >> 8) & 15) * 2;
            let v = (op & 0xff).rotate_right(rot);
            (v, if rot == 0 { self.regs.c } else { v >> 31 != 0 })
        } else {
            let m = op & 15;
            let kind = (op >> 5) & 3;
            if op & 0x10 != 0 {
                let mv = if m == 15 { pcv + 4 } else { self.regs.r[m as usize] };
                self.shift_reg(kind, mv, self.regs.r[((op >> 8) & 15) as usize])
            } else {
                let mv = if m == 15 { pcv } else { self.regs.r[m as usize] };
                self.shift_imm(kind, mv, (op >> 7) & 31)
            }
        };
        let n = (op >> 16) & 15;
        let a = if n == 15 { if op & 0x0200_0010 == 0x10 { pcv + 4 } else { pcv } } else { self.regs.r[n as usize] };
        let d = ((op >> 12) & 15) as usize;
        if s && d == 15 {
            return Err(Fault::Undefined { op, thumb: false, pc });
        }
        let opc = (op >> 21) & 15;
        let logical = |cpu: &mut Self, r: u32| {
            if s {
                cpu.set_nz(r);
                cpu.regs.c = sc;
            }
            r
        };
        let r = match opc {
            0 => logical(self, a & b),
            1 => logical(self, a ^ b),
            2 => self.sub(a, b, true, s),
            3 => self.sub(b, a, true, s),
            4 => self.add(a, b, false, s),
            5 => self.add(a, b, self.regs.c, s),
            6 => self.sub(a, b, self.regs.c, s),
            7 => self.sub(b, a, self.regs.c, s),
            8 => {
                logical(self, a & b);
                return Ok(());
            }
            9 => {
                logical(self, a ^ b);
                return Ok(());
            }
            10 => {
                self.sub(a, b, true, true);
                return Ok(());
            }
            11 => {
                self.add(a, b, false, true);
                return Ok(());
            }
            12 => logical(self, a | b),
            13 => logical(self, b),
            14 => logical(self, a & !b),
            _ => logical(self, !b),
        };
        if d == 15 {
            self.regs.r[15] = r & !3;
        } else {
            self.regs.r[d] = r;
        }
        Ok(())
    }

    /// multiplies and the halfword / signed byte transfers
    fn arm_mul_or_extra(&mut self, op: u32, pc: u32, pcv: u32) -> R<()> {
        if op & 0x0fb0_0ff0 == 0x0100_0090 {
            // SWP / SWPB: Rd = [Rn]; [Rn] = Rm
            let (n, d, mr) = (((op >> 16) & 15) as usize, ((op >> 12) & 15) as usize, (op & 15) as usize);
            let a = self.regs.r[n];
            let v = self.regs.r[mr];
            if op & (1 << 22) != 0 {
                let t = self.r8(a)?;
                self.w8(a, v)?;
                self.regs.r[d] = t;
            } else {
                let t = self.r32(a)?;
                self.w32(a, v)?;
                self.regs.r[d] = t;
            }
            return Ok(());
        }
        let sh = (op >> 5) & 3;
        let s = op & (1 << 20) != 0;
        if sh == 0 {
            let m = self.regs.r[(op & 15) as usize];
            let rs = self.regs.r[((op >> 8) & 15) as usize];
            let hi = ((op >> 16) & 15) as usize;
            let lo = ((op >> 12) & 15) as usize;
            match (op >> 21) & 15 {
                0 | 1 => {
                    let mut r = m.wrapping_mul(rs);
                    if (op >> 21) & 1 != 0 {
                        r = r.wrapping_add(self.regs.r[lo]);
                    }
                    self.regs.r[hi] = r;
                    if s {
                        self.set_nz(r);
                    }
                }
                4..=7 => {
                    let signed = (op >> 22) & 1 != 0;
                    let mut r = if signed {
                        (m as i32 as i64).wrapping_mul(rs as i32 as i64) as u64
                    } else {
                        (m as u64) * (rs as u64)
                    };
                    if (op >> 21) & 1 != 0 {
                        r = r.wrapping_add(((self.regs.r[hi] as u64) << 32) | self.regs.r[lo] as u64);
                    }
                    self.regs.r[lo] = r as u32;
                    self.regs.r[hi] = (r >> 32) as u32;
                    if s {
                        self.regs.n = r >> 63 != 0;
                        self.regs.z = r == 0;
                    }
                }
                _ => return Err(Fault::Undefined { op, thumb: false, pc }),
            }
            return Ok(());
        }
        // LDRH / STRH / LDRSB / LDRSH (LDRD / STRD unsupported)
        let n = ((op >> 16) & 15) as usize;
        let d = ((op >> 12) & 15) as usize;
        let load = op & (1 << 20) != 0;
        if !load && sh != 1 {
            // LDRD (sh 2) / STRD (sh 3): a register pair Rd, Rd+1
            let off = if op & (1 << 22) != 0 { ((op >> 4) & 0xf0) | (op & 15) } else { self.regs.r[(op & 15) as usize] };
            let base = if n == 15 { pcv } else { self.regs.r[n] };
            let target = if op & (1 << 23) != 0 { base.wrapping_add(off) } else { base.wrapping_sub(off) };
            let pre = op & (1 << 24) != 0;
            let a = if pre { target } else { base };
            if d & 1 != 0 || d == 14 {
                return Err(Fault::Undefined { op, thumb: false, pc });
            }
            if sh == 2 {
                let (lo, hi) = (self.r32_aligned(a)?, self.r32_aligned(a + 4)?);
                self.regs.r[d] = lo;
                self.regs.r[d + 1] = hi;
            } else {
                self.w32(a, self.regs.r[d])?;
                self.w32(a + 4, self.regs.r[d + 1])?;
            }
            if !pre || op & (1 << 21) != 0 {
                self.regs.r[n] = target;
            }
            return Ok(());
        }
        let off = if op & (1 << 22) != 0 { ((op >> 4) & 0xf0) | (op & 15) } else { self.regs.r[(op & 15) as usize] };
        let base = if n == 15 { pcv } else { self.regs.r[n] };
        let target = if op & (1 << 23) != 0 { base.wrapping_add(off) } else { base.wrapping_sub(off) };
        let pre = op & (1 << 24) != 0;
        let a = if pre { target } else { base };
        if load {
            let v = match sh {
                1 => self.r16(a)?,
                2 => self.r8(a)? as i8 as i32 as u32,
                _ => self.r16(a)? as i16 as i32 as u32,
            };
            if !pre || op & (1 << 21) != 0 {
                self.regs.r[n] = target;
            }
            self.regs.r[d] = v;
        } else {
            self.w16(a, self.regs.r[d])?;
            if !pre || op & (1 << 21) != 0 {
                self.regs.r[n] = target;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_thumb(code: &[u16], setup: impl FnOnce(&mut Cpu)) -> Cpu {
        let mut c = Cpu::new();
        let base = RAM_BASE + 0x1000;
        for (i, &h) in code.iter().enumerate() {
            c.w16(base + 2 * i as u32, h as u32).unwrap();
        }
        c.w16(base + 2 * code.len() as u32, 0x4770).unwrap(); // bx lr
        setup(&mut c);
        let (r0, r1, r2, r3) = (c.regs.r[0], c.regs.r[1], c.regs.r[2], c.regs.r[3]);
        c.call(base | 1, &[r0, r1, r2, r3], RAM_BASE + 0x8000, 1000).unwrap();
        c
    }

    #[test]
    fn thumb_arithmetic_and_flags() {
        // subs r0, r0, r1 ; 1 - 2 = -1, borrow -> C clear, N set
        let c = run_thumb(&[0x1a40], |c| {
            c.regs.r[0] = 1;
            c.regs.r[1] = 2;
        });
        assert_eq!(c.regs.r[0], 0xffff_ffff);
        assert!(c.regs.n && !c.regs.c && !c.regs.z);
        // adds r0, r0, r1 ; 0x7fffffff + 1 overflows
        let c = run_thumb(&[0x1840], |c| {
            c.regs.r[0] = 0x7fff_ffff;
            c.regs.r[1] = 1;
        });
        assert!(c.regs.v && c.regs.n);
        // asrs r0, r1, #0 encodes ASR #32
        let c = run_thumb(&[0x1008], |c| c.regs.r[1] = 0x8000_0000);
        assert_eq!(c.regs.r[0], 0xffff_ffff);
        assert!(c.regs.c);
    }

    #[test]
    fn thumb_bl_and_push_pop() {
        // push {lr}; bl +2 (to the movs); b +2 (skip); movs r0,#7; bx lr(=back) ... laid out as a call and a leaf
        // 0: push {lr}       b500
        // 2: bl leaf (at 10)  f000 f803
        // 6: pop {pc}        bd00
        // 8: nop             46c0
        // 10: movs r0, #7    2007
        // 12: bx lr          4770
        let c = run_thumb(&[0xb500, 0xf000, 0xf802, 0xbd00, 0x46c0, 0x2007, 0x4770], |_| {});
        assert_eq!(c.regs.r[0], 7);
    }
}

/// where an instruction at `pc` is counted when profiling: main RAM (0x2000000..0x2400000) and ITCM
/// (0x1ff8000..0x2000000), a halfword each
pub fn profile_index(pc: u32) -> Option<usize> {
    match pc {
        0x2000000..=0x23fffff => Some(((pc - 0x2000000) / 2) as usize),
        0x1ff8000..=0x1ffffff => Some(0x200000 + ((pc - 0x1ff8000) / 2) as usize),
        _ => None,
    }
}

/// the size of a profile (`profile_index`)
pub const PROFILE_LEN: usize = 0x200000 + 0x4000;

/// the address a profile index stands for
pub fn profile_pc(i: usize) -> u32 {
    if i < 0x200000 { 0x2000000 + 2 * i as u32 } else { 0x1ff8000 + 2 * (i - 0x200000) as u32 }
}
