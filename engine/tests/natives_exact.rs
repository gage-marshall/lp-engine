//! Each pure native (memory routines, cache maintenance, the compiler's helpers, strncmp) against the game's own
//! function: the same memory and arguments, the game's code run by the interpreter and the native run by the
//! registry, then r0, r1 and every byte of the scratch area compared, over randomised cases (odd and unaligned
//! sizes, null pointers, NUL bytes, zero and extreme divisors). The time each native charges is compared with the
//! instructions the game's code ran. NNS G3D render commands too: the same render state and resources, the
//! memory, the envelope cache and the geometry engine's matrices compared. Needs ../content (skipped without it).
use lp_engine::flow::Arg;
use lp_engine::rom::Rom;
use lp_engine::runtime::{ext, interp, Game, Machine, NoSubsystems};
use std::path::Path;

const AREA: u32 = 0x23a0000;
const AREA_LEN: u32 = 0x4000;

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 33) as u32
    }
    fn below(&mut self, n: u32) -> u32 {
        self.next() % n.max(1)
    }
}

fn instructions(m: &Machine) -> u64 {
    m.tick * interp::INSTR_PER_TICK + m.instr_carry
}

/// run the game's function and the native on the same state; returns (game instructions, native instructions)
fn compare(m: &mut Machine, name: &str, addr: u32, args: &[i32], case: &str) -> (u64, u64) {
    compare_with(m, name, addr, args, case, &[])
}

/// `compare`, more memory compared too (address, length)
fn compare_with(m: &mut Machine, name: &str, addr: u32, args: &[i32], case: &str, more: &[(u32, usize)]) -> (u64, u64) {
    let extra: Vec<Vec<u8>> = more.iter().map(|&(a, n)| m.ram.bytes(a, n)).collect();
    let area: Vec<u8> = m.ram.bytes(AREA, AREA_LEN as usize);
    let mk = || args.iter().map(|&v| Arg::I(v)).collect::<Vec<Arg>>();
    // the game's time: its instructions and what the natives it calls charge
    let s0 = instructions(m);
    let r0 = interp::interpret(m, &mut NoSubsystems, addr, &mut mk());
    let game_steps = instructions(m) - s0;
    let r1 = m.last_r1;
    let after: Vec<u8> = m.ram.bytes(AREA, AREA_LEN as usize);
    let extra_after: Vec<Vec<u8>> = more.iter().map(|&(a, n)| m.ram.bytes(a, n)).collect();
    for (k, &(a, _)) in more.iter().enumerate() {
        m.ram.put(a, &extra[k]);
    }
    m.ram.put(AREA, &area);
    let t0 = instructions(m);
    let n0 = ext::call(m, &mut NoSubsystems, addr, &mut mk());
    let native_steps = instructions(m) - t0;
    let n1 = m.ret_hi.take().unwrap_or(r1);
    let native_after: Vec<u8> = m.ram.bytes(AREA, AREA_LEN as usize);
    assert!(m.faults.is_empty(), "{name} {case}: {:?}", m.faults);
    for (k, &(a, n)) in more.iter().enumerate() {
        let now = m.ram.bytes(a, n);
        if let Some(i) = (0..n).find(|&i| now[i] != extra_after[k][i]) {
            panic!("{name} {case}: memory differs at {:#x}: game {:#x}, native {:#x}", a + i as u32, extra_after[k][i], now[i]);
        }
    }
    if let Some(k) = (0..after.len()).find(|&k| after[k] != native_after[k]) {
        let w = |v: &[u8]| v[k.saturating_sub(4)..(k + 6).min(v.len())].to_vec();
        panic!("{name} {case}: memory differs first at {:#x}: before {:?}, game {:?}, native {:?}", AREA + k as u32, w(&area), w(&after), w(&native_after));
    }
    // results: r0 for functions that return a value, r1 too for the divisions (quotient and remainder / 64 bits);
    // a void function's registers are whatever it left there, which no caller reads
    let returns = ext::entry(addr).is_some_and(|e| e.ret == ext::Ret::Value);
    let two = matches!(name, "_s32_div_f" | "_ll_sdiv");
    if returns {
        assert_eq!(r0, n0, "{name} {case}: r0 differs (game vs native)");
    }
    if two {
        assert_eq!(r1, n1, "{name} {case}: r1 differs (game vs native)");
    }
    (game_steps, native_steps)
}

/// NNSi_G3dFuncSbc_NODEMIX on a made-up render state: the render state at AREA (command pointer, render object,
/// the envelope-cached bits at +0xcc), the model with its envelope matrices, the command; random weights, joints
/// (some cached already), matrix stack slots and matrices. Returns (game instructions, native instructions,
/// joints, uncached joints).
fn nodemix_case(m: &mut Machine, rng: &mut Lcg, case: usize) -> (u64, u64, u32, u32) {
    const CACHE: (u32, usize) = (0x212d3a8, 32 * 0x64);
    let (rs, obj, mdl, evp, cmd) = (AREA, AREA + 0x200, AREA + 0x300, AREA + 0x400, AREA + 0x1000);
    let b: Vec<u8> = (0..AREA_LEN).map(|_| rng.next() as u8).collect();
    m.ram.put(AREA, &b);
    // matrices of modest size (as models have): 1.0 = 4096
    for k in 0..32 * 0x54 / 4 {
        m.ram.set_u32(evp + 4 * k, (rng.below(0x4000) as i32 - 0x2000) as u32);
    }
    for k in 0..CACHE.1 as u32 / 4 {
        m.ram.set_u32(CACHE.0 + 4 * k, (rng.below(0x4000) as i32 - 0x2000) as u32);
    }
    m.ram.set_u32(rs, cmd);
    m.ram.set_u32(rs + 4, obj);
    m.ram.set_u32(obj + 4, mdl);
    m.ram.set_u32(mdl + 0x10, evp - mdl);
    for k in 0..8 {
        m.ram.set_u32(rs + 0xcc + 4 * k, if case % 3 == 0 { 0 } else { rng.next() });
    }
    let n = 1 + rng.below(6);
    let mut c = vec![0x0b, rng.below(31) as u8, n as u8];
    let mut seen = std::collections::HashSet::new();
    let mut uncached = 0;
    for _ in 0..n {
        let j = rng.below(32);
        let cached = m.ram.u32(rs + 0xcc + 4 * (j >> 5)) & 1 << (j & 31) != 0;
        if !cached && seen.insert(j) {
            uncached += 1;
        }
        c.extend([rng.below(31) as u8, j as u8, rng.below(256) as u8]);
    }
    m.ram.put(cmd, &c);
    // the matrix stack holds something at every slot
    for k in 0..31u32 {
        let _ = m.ram.write(0x4000440, 2, 4);
        let _ = m.ram.write(0x4000454, 0, 4);
        let _ = m.ram.write(0x4000470, rng.below(0x8000), 4);
        let _ = m.ram.write(0x4000470, rng.below(0x8000), 4);
        let _ = m.ram.write(0x4000470, rng.below(0x8000), 4);
        let _ = m.ram.write(0x400044c, k, 4);
    }
    let area = m.ram.bytes(AREA, AREA_LEN as usize);
    let cache = m.ram.bytes(CACHE.0, CACHE.1);
    let gx = m.ram.gx.clone();
    let s0 = instructions(m);
    interp::interpret(m, &mut NoSubsystems, 0x2004dcc, &mut [Arg::I(rs as i32), Arg::I(0)]);
    let game_steps = instructions(m) - s0;
    let (g_area, g_cache, g_gx) = (m.ram.bytes(AREA, AREA_LEN as usize), m.ram.bytes(CACHE.0, CACHE.1), m.ram.gx.state());
    m.ram.put(AREA, &area);
    m.ram.put(CACHE.0, &cache);
    m.ram.gx = gx;
    let t0 = instructions(m);
    ext::call(m, &mut NoSubsystems, 0x2004dcc, &mut [Arg::I(rs as i32), Arg::I(0)]);
    let native_steps = instructions(m) - t0;
    assert!(m.faults.is_empty(), "NODEMIX case {case}: {:?}", m.faults);
    assert!(g_area == m.ram.bytes(AREA, AREA_LEN as usize), "NODEMIX case {case}: the render state or resources differ");
    assert!(g_cache == m.ram.bytes(CACHE.0, CACHE.1), "NODEMIX case {case}: the envelope cache differs");
    for ((name, a), (_, b)) in m.ram.gx.state().iter().zip(&g_gx) {
        // the vector matrix's translation row is loaded from stack garbage in the game (not used by the hardware)
        assert!(name.contains("translation") || a == b, "NODEMIX case {case}: geometry {name}: game {b}, native {a}");
    }
    (game_steps, native_steps, n, uncached)
}

#[test]
fn natives_match_the_games_functions() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    // the program as the game's own start-up code unpacks it (the card image is packed)
    let mut g = Game::new(rom, NoSubsystems);
    let Some(entry) = g.m.ram.boot_entry else { return };
    g.run_crt0(entry);
    let m = &mut g.m;
    interp::install_traps(m);
    let mut rng = Lcg(7);
    let fill = |m: &mut Machine, rng: &mut Lcg| {
        let b: Vec<u8> = (0..AREA_LEN).map(|_| rng.next() as u8).collect();
        m.ram.put(AREA, &b);
    };
    let mut cost: Vec<(&str, u64, u64)> = Vec::new();
    let mut run = |m: &mut Machine, name: &'static str, addr: u32, args: &[i32], case: String| {
        let (g, n) = compare(m, name, addr, args, &case);
        if std::env::var_os("LP_COST_LOG").is_some() { eprintln!("cost {name} {case} game {g} native {n}"); }
        cost.push((name, g, n));
    };
    for i in 0..200 {
        fill(m, &mut rng);
        let (src, dst) = (AREA + rng.below(0x1800), AREA + 0x2000 + rng.below(0x1800));
        let size = rng.below(if i % 10 == 0 { 3 } else { 400 });
        let c = format!("case {i}: src {src:#x} dst {dst:#x} size {size}");
        run(m, "MIi_CpuCopy16", 0x201e88c, &[src as i32, dst as i32, size as i32], c.clone());
        run(m, "MIi_CpuClear16", 0x201e874, &[rng.next() as i32, dst as i32, size as i32], c.clone());
        run(m, "MIi_CpuSend32", 0x201e8d4, &[(src & !3) as i32, (dst & !3) as i32, size as i32], c.clone());
        run(m, "MIi_CpuClearFast", 0x201e8ec, &[rng.next() as i32, (dst & !3) as i32, size as i32], c.clone());
        run(m, "MemCopyBytes", 0x202caf0, &[dst as i32, src as i32, size as i32], c.clone());
        let p = if i % 13 == 0 { 0 } else { dst };
        run(m, "Libc_Bzero", 0x20307b8, &[p as i32, size as i32], c.clone());
        run(m, "DC_FlushRange", 0x20211e8, &[src as i32, size as i32], c.clone());
        run(m, "DC_InvalidateRange", 0x20211b0, &[src as i32, size as i32], c.clone());
        // strings: mostly equal prefixes, NULs here and there
        let len = rng.below(24);
        let mut s1: Vec<u8> = (0..len).map(|_| b'a' + rng.below(3) as u8).collect();
        let mut s2 = s1.clone();
        if rng.below(2) == 0 && len > 0 {
            let k = rng.below(len) as usize;
            s2[k] = b'a' + rng.below(4) as u8;
        }
        s1.push(0);
        s2.push(if rng.below(4) == 0 { b'z' } else { 0 });
        m.ram.put(src, &s1);
        m.ram.put(dst, &s2);
        run(m, "strncmp", 0x202f3fc, &[src as i32, dst as i32, rng.below(30) as i32], format!("case {i}: {s1:?} {s2:?}"));
        // division: ordinary, negative, zero and extreme operands
        let pick = |rng: &mut Lcg| -> i32 {
            match rng.below(6) {
                0 => 0,
                1 => i32::MIN,
                2 => -1,
                3 => rng.below(100) as i32 - 50,
                _ => rng.next() as i32,
            }
        };
        let (x, y) = (pick(&mut rng), pick(&mut rng));
        run(m, "_s32_div_f", 0x20333c8, &[x, y], format!("case {i}: {x} / {y}"));
        let (xl, xh, yl, yh) = (rng.next() as i32, pick(&mut rng), pick(&mut rng), if rng.below(3) == 0 { 0 } else { pick(&mut rng) });
        run(m, "_ll_sdiv", 0x2033180, &[xl, xh, yl, yh], format!("case {i}: {xh:#x}:{xl:#x} / {yh:#x}:{yl:#x}"));
    }
    for i in 0..200 {
        // 4 x 4 matrices: small, large and extreme elements; the result apart, onto a or onto b
        for k in 0..32u32 {
            let v = match rng.below(4) { 0 => rng.next(), 1 => rng.below(0x20000).wrapping_sub(0x10000), _ => rng.below(0x2000).wrapping_sub(0x1000) };
            m.ram.set_u32(AREA + 4 * k, v);
        }
        let (pa, pb) = (AREA, AREA + 0x40);
        let ab = match i % 3 { 0 => AREA + 0x80, 1 => pa, _ => pb };
        run(m, "MTX_Concat44", 0x2019a10, &[pa as i32, pb as i32, ab as i32], format!("case {i}: into {ab:#x}"));
    }
    for i in 0..300 {
        // the G2D bit reader: any fill state, 0 to 8 bits
        fill(m, &mut rng);
        let reader = AREA + 0x40;
        m.ram.set_u32(reader, AREA + 0x800 + rng.below(64));
        m.ram.set_u8(reader + 4, rng.below(9) as u8);
        let n = rng.below(9) as i32;
        run(m, "NNSi_G2dBitReaderRead", 0x20135d0, &[reader as i32, n], format!("case {i}: avail {} bits {n}", m.ram.u8(reader + 4)));
        // a glyph's part in one character: LC_INFO at AREA, the character at +0x100, the glyph at +0x200
        fill(m, &mut rng);
        let dst_bpp = if rng.below(2) == 0 { 4 } else { 8 };
        let src_bpp = 1 + rng.below(if dst_bpp == 4 { 3 } else { 7 }) as i32;
        let cl = rng.below((1u32 << dst_bpp) - (1 << src_bpp) + 1);
        let (width, height) = (1 + rng.below(24) as i32, 1 + rng.below(24) as i32);
        let (ofs_x, ofs_y) = (rng.below(8 + width as u32 - 1) as i32 - width + 1, rng.below(8 + height as u32 - 1) as i32 - height + 1);
        let dsrc = src_bpp * width + rng.below(16) as i32;
        let info = [AREA + 0x100, AREA + 0x200, ofs_x as u32, ofs_y as u32, width as u32, height as u32, dsrc as u32, src_bpp as u32, dst_bpp, cl];
        for (k, v) in info.iter().enumerate() {
            m.ram.set_u32(AREA + 4 * k as u32, *v);
        }
        run(m, "LetterChar", 0x2012694, &[AREA as i32], format!("case {i}: {info:x?}"));
    }
    for i in 0..300 {
        // LZ8, standard and extended: a made-up stream at AREA + 0x100 (literals and copies of every length tier,
        // reaching before the output's start, the last copy sometimes past the size), the output at + 0x2000
        fill(m, &mut rng);
        let ext = i % 2 == 1;
        let size = if i % 25 == 0 { 0 } else { rng.below(0x1400) };
        let mut s: Vec<u8> = ((size << 8) | 0x10 | ext as u32).to_le_bytes().to_vec();
        let mut made = 0;
        while made < size {
            let at = s.len();
            s.push(0);
            for b in 0..8 {
                if made >= size {
                    break;
                }
                if rng.below(3) == 0 {
                    s.push(rng.next() as u8);
                    made += 1;
                    continue;
                }
                s[at] |= 0x80 >> b;
                let d = rng.below(0x1000);
                let left = size - made;
                let len = match (ext, rng.below(3)) {
                    (false, _) => 3 + rng.below(16),
                    (true, 0) => 3 + rng.below(14),
                    (true, 1) => 0x11 + rng.below(0x100),
                    _ => 0x111 + rng.below(0x200),
                };
                let len = if rng.below(8) == 0 { len } else { len.min(left.max(3)) };
                let len = if ext && len < 0x111 && len > 0x10 && rng.below(4) == 0 { 3 + rng.below(14) } else { len };
                match len {
                    _ if !ext => s.extend([((len - 3) << 4 | d >> 8) as u8, d as u8]),
                    3..=16 => s.extend([((len - 1) << 4 | d >> 8) as u8, d as u8]),
                    17..=272 => {
                        let v = len - 0x11;
                        s.extend([(v >> 4) as u8, ((v & 0xf) << 4 | d >> 8) as u8, d as u8]);
                    }
                    _ => {
                        let v = len - 0x111;
                        s.extend([(0x10 | v >> 12) as u8, (v >> 4) as u8, ((v & 0xf) << 4 | d >> 8) as u8, d as u8]);
                    }
                }
                made += len;
            }
        }
        m.ram.put(AREA + 0x100, &s);
        run(m, "MI_UncompressLZ8", 0x201ef8c, &[(AREA + 0x100) as i32, (AREA + 0x2000) as i32], format!("case {i}: size {size} ext {ext}"));
        // the backward form, in place: any bytes as the stream (each token is short), any header length and growth
        fill(m, &mut rng);
        let end = AREA + 0x2000;
        let (hl, sl, grow) = (8 + rng.below(4), if i % 20 == 0 { 0 } else { rng.below(0x300) }, rng.below(0x800));
        m.ram.set_u32(end - 8, hl << 24 | (hl + sl));
        m.ram.set_u32(end - 4, grow);
        let p = if i % 50 == 7 { 0 } else { end };
        run(m, "MIi_UncompressBackward", 0x2000950, &[p as i32], format!("case {i}: header {hl} stream {sl} growth {grow}"));
    }
    // the OAM manager: an instance at AREA over its buffers (2 x 0x540 bytes from the literal) and OAM
    let oam_buf = m.ram.u32(0x2011648);
    let regions = [(oam_buf, 2 * 0x540), (0x7000000, 0x800)];
    for i in 0..300 {
        let b: Vec<u8> = (0..2 * 0x540).map(|_| rng.next() as u8).collect();
        m.ram.put(oam_buf, &b);
        fill(m, &mut rng);
        let p = AREA;
        let ty = rng.below(2);
        let from = rng.below(128);
        let to = if i % 17 == 0 { from.saturating_sub(1) } else { (from + rng.below(40)).min(127) };
        let cur = from + rng.below(to + 3 - from);
        // affine areas: ordinary, empty (from = to + 1), to 0, and the game's 'none' (from 0x20, to 0, current 0x20)
        let mut af = rng.below(32);
        let at = match i % 7 {
            0 => 0,
            1 => af.saturating_sub(1),
            _ => (af + rng.below(8)).min(31),
        };
        let mut acur = af + rng.below(at.max(af) + 3 - af);
        if i % 11 == 0 {
            (af, acur) = (0x20, 0x20);
        }
        let at = if i % 11 == 0 { 0 } else { at };
        m.ram.set_u32(p, ty);
        for (k, v) in [from, to, cur, af, at, acur].iter().enumerate() {
            m.ram.set_u16(p + 4 + 2 * k as u32, *v as u16);
        }
        let fast = (i % 4 == 3) as u32;
        m.ram.set_u32(p + 0x14, fast);
        let c = format!("case {i}: type {ty} attrs {from}..{to} at {cur}, affine {af}..{at} at {acur} fast {fast}");
        let mut r = |m: &mut Machine, name: &'static str, addr: u32, args: &[i32]| {
            let (g, n) = compare_with(m, name, addr, args, &c, &regions);
            if std::env::var_os("LP_COST_LOG").is_some() {
                eprintln!("cost {name} {c} game {g} native {n}");
            }
            cost.push((name, g, n));
        };
        let (off, size) = (rng.below(0x3f8), rng.below(10));
        r(m, "CpuLoadOAMMain_", 0x201107c, &[(AREA + 0x100) as i32, off as i32, size as i32]);
        r(m, "CpuLoadOAMSub_", 0x2011068, &[(AREA + 0x100) as i32, off as i32, size as i32]);
        r(m, "NNS_G2dApplyOamManagerToHW", 0x2011454, &[p as i32]);
        r(m, "NNS_G2dResetOamManagerBuffer", 0x2011650, &[p as i32]);
        let num = 1 + rng.below(8);
        r(m, "NNS_G2dEntryOamManagerOam", 0x20112cc, &[p as i32, (AREA + 0x100) as i32, num as i32]);
        r(m, "NNS_G2dEntryOamManagerAffine", 0x2011398, &[p as i32, (AREA + 0x200) as i32]);
    }
    for i in 0..400 {
        // a sprite cell's OAMs: the cell at AREA (count, pointer to 6-byte entries at +0x100), OAMs out at +0x1000,
        // positions at +0x2000, the matrix at +0x40, the move at +0x60
        fill(m, &mut rng);
        let count = rng.below(24);
        m.ram.set_u16(AREA, count as u16);
        m.ram.set_u32(AREA + 4, AREA + 0x100);
        for k in 0..count {
            // positions spread around the screen, any shape, size, affine and flip bits
            let y = rng.below(256);
            let x = rng.below(512);
            let a0 = (y | (rng.next() & 0xff00)) & 0xffff;
            let a1 = (x | (rng.next() & 0xfe00)) & 0xffff;
            m.ram.set_u16(AREA + 0x100 + 6 * k, a0 as u16);
            m.ram.set_u16(AREA + 0x102 + 6 * k, a1 as u16);
        }
        let scale = |rng: &mut Lcg| (rng.below(0x3000) as i32 - 0x1800) as u32;
        let ident = i % 5 == 0;
        for k in 0..4u32 {
            m.ram.set_u32(AREA + 0x40 + 4 * k, if ident { if k == 0 || k == 3 { 0x1000 } else { 0 } } else { scale(&mut rng) });
        }
        m.ram.set_u32(AREA + 0x60, (rng.below(0x100000) as i32 - 0x40000) as u32);
        m.ram.set_u32(AREA + 0x64, (rng.below(0xc0000) as i32 - 0x30000) as u32);
        let mtx = if i % 3 == 0 { 0 } else { AREA + 0x40 };
        let tr = if i % 4 == 0 { 0 } else { AREA + 0x60 };
        let max = if i % 9 == 0 { rng.below(8) } else { 0x80 };
        let (aff, flags) = (rng.below(32), rng.below(4));
        let c = format!("case {i}: {count} OAMs, max {max}, matrix {mtx:#x} move {tr:#x} flags {flags}");
        let args = [(AREA + 0x1000) as i32, (AREA + 0x2000) as i32, max as i32, AREA as i32, mtx as i32, tr as i32, aff as i32, flags as i32];
        let (g, n) = compare(m, "Sprite_CopyCellOamsWithFlip", 0x204948c, &args, &c);
        if std::env::var_os("LP_COST_LOG").is_some() {
            eprintln!("cost Sprite_CopyCellOamsWithFlip {c} game {g} native {n}");
        }
        cost.push(("Sprite_CopyCellOamsWithFlip", g, n));
    }
    // the sprite slots (256 x 0x38) at AREA through the slot-array pointer, the two priority lists compared too
    let (slots_ptr, list2d, list3d) = (m.ram.u32(0x2048670), m.ram.u32(0x2048668), m.ram.u32(0x204866c));
    let saved = m.ram.u32(slots_ptr);
    for i in 0..100 {
        fill(m, &mut rng);
        m.ram.set_u32(slots_ptr, AREA);
        for k in 0..256u32 {
            let s = AREA + k * 0x38;
            m.ram.set_u8(s + 0x1b, (rng.below(3) == 0) as u8 * rng.below(256) as u8);
            m.ram.set_u8(s + 0x24, rng.below(4) as u8);
            m.ram.set_u8(s + 0x19, rng.below(if i % 2 == 0 { 64 } else { 256 }) as u8);
        }
        let c = format!("case {i}");
        let (g, n) = compare_with(m, "Sprite_BuildPriorityLists", 0x20485d0, &[], &c, &[(list2d, 0x100), (list3d, 0x100)]);
        if std::env::var_os("LP_COST_LOG").is_some() {
            eprintln!("cost Sprite_BuildPriorityLists {c} game {g} native {n}");
        }
        cost.push(("Sprite_BuildPriorityLists", g, n));
    }
    m.ram.set_u32(slots_ptr, saved);
    for i in 0..300 {
        let (g, n, joints, uncached) = nodemix_case(m, &mut rng, i);
        if std::env::var_os("LP_NODEMIX_LOG").is_some() {
            eprintln!("nodemix joints {joints} uncached {uncached} game {g} native {n}");
        }
        cost.push(("NNSi_G3dFuncSbc_NODEMIX", g, n));
    }
    // the time charged against the instructions run, per function
    let mut names: Vec<&str> = cost.iter().map(|c| c.0).collect();
    names.dedup();
    names.sort();
    names.dedup();
    for n in names {
        let (g, c): (u64, u64) = cost.iter().filter(|x| x.0 == n).fold((0, 0), |a, x| (a.0 + x.1, a.1 + x.2));
        let off = 100.0 * (c as f64 - g as f64) / g.max(1) as f64;
        eprintln!("{n:20} game {g:>9} instructions, native charges {c:>9} ({off:+.1}%)");
        assert!(off.abs() <= 10.0, "{n}: the time charged is {off:+.1}% off the game's");
    }
}
