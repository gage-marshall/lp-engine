//! Game functions ported from the code, over the game's memory. Each is the function at the address its comment names;
//! the globals it touches are found the way the code finds them, through its literal pool (`m.lit(pool address)`).
use super::ext;
use super::{Machine, Subsystems};
use crate::flow::Arg;

type S<'a> = &'a mut dyn Subsystems;

fn arg(a: &[Arg], i: usize) -> i32 {
    a.get(i).and_then(|x| x.int()).unwrap_or(0)
}

/// write words to an output argument: a buffer of the ported caller, or a pointer from the game's code
fn put_out(m: &mut Machine, a: &mut [Arg], i: usize, words: &[i32]) {
    match a.get_mut(i) {
        Some(Arg::Out(buf)) => {
            for (k, &w) in words.iter().enumerate().take(buf.len()) {
                buf[k] = Some(w);
            }
        }
        Some(Arg::I(p)) if *p != 0 => {
            let p = *p as u32;
            for (k, &w) in words.iter().enumerate() {
                m.ram.set_u32(p + 4 * k as u32, w as u32);
            }
        }
        _ => {}
    }
}

fn call(m: &mut Machine, s: S, target: u32, args: &mut [Arg]) -> i32 {
    ext::call(m, s, target, args)
}

/// the function a literal pool word names (the game's veneers: `(*DAT_x)(...)`), called with integer arguments
fn call_ptr(m: &mut Machine, s: S, pool_value: u32, args: &[i32]) -> i32 {
    let target = pool_value & !1;
    if target == 0 {
        return 0;
    }
    let mut a: Vec<Arg> = args.iter().map(|&v| Arg::I(v)).collect();
    ext::call(m, s, target, &mut a)
}

// ---- the state stack: Main_IncStateDepth 0x203dba4 .. Main_AdvanceStateSequence 0x203dc58 ----------------------------

pub fn main_get_state(m: &Machine) -> i32 {
    // 0x203dbd4
    let (depth, states) = (m.lit(0x203dbe8), m.lit(0x203dbec));
    m.ram.i32(states.wrapping_add(m.ram.u32(depth).wrapping_mul(4)))
}

/// 0x203dbf0 (labelled Main_FreeStateBuffer): sets the state at the current depth; deeper levels restart at 0 and drop
/// their buffers
pub fn main_set_state(m: &mut Machine, sub: S, s: i32) {
    let (depth_p, states, base) = (m.lit(0x203dc4c), m.lit(0x203dc50), m.lit(0x203dc54));
    let depth = m.ram.i32(depth_p);
    m.ram.set_u32(states.wrapping_add((depth * 4) as u32), s as u32);
    let mut i = depth + 1;
    while i < 0xf {
        let e = base.wrapping_add((i * 4) as u32);
        m.ram.set_u32(e + 4, 0);
        let buf = m.ram.u32(e + 0x40);
        if buf != 0 {
            call(m, sub, 0x203fd58, &mut [Arg::I(buf as i32)]); // GameHeap_FreeExp
            m.ram.set_u32(e + 0x40, 0);
        }
        i += 1;
    }
}

pub fn main_inc_depth(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x203dbb8);
    let v = m.ram.u32(p).wrapping_add(1);
    m.ram.set_u32(p, v);
    0
}
pub fn main_dec_depth(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x203dbd0);
    let v = m.ram.u32(p).wrapping_sub(1);
    m.ram.set_u32(p, v);
    0
}
pub fn main_get_state_fn(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    main_get_state(m)
}
pub fn main_set_state_fn(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    main_set_state(m, s, arg(a, 0));
    0
}
pub fn main_advance(m: &mut Machine, sub: S, _: &mut [Arg]) -> i32 {
    let s = main_get_state(m);
    main_set_state(m, sub, s + 1);
    0
}

/// 0x202cb10 memset(dst, value, n): returns dst
pub fn memset(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (d, v, n) = (arg(a, 0) as u32, arg(a, 1) as u8, arg(a, 2) as u32);
    m.ram.fill(d, v, n);
    d as i32
}

// ---- overlays: Overlay_GetSlot 0x20ef8fc, Overlay_Load 0x20ef978, Overlay_Unload 0x20ef9ec, Overlay_IsLoaded 0x20efa50

fn overlay_slot(m: &Machine, id: u32) -> Option<u32> {
    let lists = m.lit(0x20ef948);
    for slot in 0..2 {
        let mut p = m.ram.u32(lists + 4 * slot);
        loop {
            let v = m.ram.u32(p);
            if v == u32::MAX {
                break;
            }
            if v == id {
                return Some(slot);
            }
            p += 4;
        }
    }
    None
}

pub fn overlay_load(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let mut id = arg(a, 0) as u32;
    if id == u32::MAX {
        id = m.ram.u32(m.lit(0x20ef9e4) + 4);
    }
    let Some(slot) = overlay_slot(m, id) else { return 0 };
    let table = m.lit(0x20ef9e8);
    if m.ram.u32(table + 4 * slot) != u32::MAX {
        overlay_unload(m, s, &mut [Arg::I(-1)]);
    }
    if id != u32::MAX {
        // FS_LoadOverlay: the overlay's code and data at its address, its bss cleared, then (FS_StartOverlay) its
        // static initialisers run, as the game's code
        if let Some((base, img)) = m.rom.overlay_image(id) {
            let img = img.to_vec();
            // (profiling: the counts so far belong to the overlays that were there)
            m.profile_flush();
            let end = base + img.len() as u32 + m.rom.overlay_info(id).map_or(0, |i| i.0);
            m.overlays_loaded.retain(|&(_, b, e)| e <= base || b >= end);
            m.overlays_loaded.push((id, base, end));
            m.ram.put(base, &img);
            if let Some((bss, (lo, hi))) = m.rom.overlay_info(id) {
                m.ram.fill(base + img.len() as u32, 0, bss);
                if std::env::var("LP_DEBUG_OVL").is_ok() {
                    eprintln!("frame {}: overlay {id} loaded at {base:#x} ({} bytes, bss {bss:#x}), sinit {lo:#x}..{hi:#x}", m.frame, img.len());
                }
                let mut p = lo;
                while p < hi {
                    let f = m.ram.u32(p);
                    if f != 0 {
                        super::interp::interpret(m, s, f, &mut []);
                    }
                    p += 4;
                }
                // analysis: the overlay as the game left it after its initialisers (LP_DUMP_OVL=dir; keep the
                // directory out of the repository: decrypted code is never committed)
                if let Ok(dir) = std::env::var("LP_DUMP_OVL") {
                    let d = m.ram.bytes(base, img.len() + bss as usize);
                    let _ = std::fs::write(format!("{dir}/ov{id:02}_ram.bin"), d);
                }
            }
        }
        m.ram.set_u32(table + 4 * slot, id);
    }
    0
}

pub fn overlay_unload(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let mut id = arg(a, 0) as u32;
    if id == u32::MAX {
        id = m.ram.u32(m.lit(0x20efa48) + 4);
    }
    if let Some(slot) = overlay_slot(m, id) {
        let table = m.lit(0x20efa4c);
        if m.ram.u32(table + 4 * slot) == id {
            m.ram.set_u32(table + 4 * slot, u32::MAX);
            m.profile_flush();
            m.overlays_loaded.retain(|o| o.0 != id);
        }
    }
    0
}

pub fn overlay_is_loaded(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let id = arg(a, 0) as u32;
    let table = m.lit(0x20efa7c);
    (0..2).any(|i| m.ram.u32(table + 4 * i) == id) as i32
}

/// the overlay loaded in a slot (None: empty)
pub fn overlay_in_slot(m: &Machine, slot: u32) -> Option<u32> {
    let v = m.ram.u32(m.lit(0x20ef9e8) + 4 * slot);
    (v != u32::MAX).then_some(v)
}

// ---- display registers (kept in RAM for the renderer) --------------------------------------------------------------

/// 0x201a73c GX_DispOn: DISPCNT display mode on (the saved mode, or 1)
pub fn gx_disp_on(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let saved = m.ram.u16(m.lit(0x201a778)) as u32;
    m.ram.set_u16(m.lit(0x201a77c), 1);
    let v = m.ram.u32(0x4000000);
    m.ram.set_u32(0x4000000, if saved == 0 { v | 0x10000 } else { (v & 0xfffcffff) | saved << 16 });
    0
}

/// 0x201a950 G2x_SetBlendAlpha_(reg, plane1, plane2, ev1, ev2)
pub fn g2x_set_blend_alpha(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (reg, p1, p2) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let (ev1, ev2) = (arg(a, 3) as u32, arg(a, 4) as u32);
    m.ram.set_u32(reg, p1 | 0x40 | p2 << 8 | (ev1 | ev2 << 8) << 16);
    0
}

/// 0x20414bc: POWCNT's screen swap bit (1: the main engine on the top screen)
pub fn display_set_lcd_swap(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let r = m.lit(0x20414e0);
    let v = m.ram.u16(r);
    m.ram.set_u16(r, if arg(a, 0) == 1 { v | 0x8000 } else { v & 0x7fff });
    0
}

/// 0x20410e0
pub fn ui_set_flags_4_0_1(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let (a, b) = (m.lit(0x20410fc), m.lit(0x2041100));
    m.ram.set_u32(a + 4, 0);
    m.ram.set_u32(b + 4, 1);
    0
}

/// 0x2043084 Gfx_SetBgVisibleFlag(value, engine, bg)
pub fn gfx_set_bg_visible_flag(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (v, eng, bg) = (arg(a, 0) as u32, arg(a, 1), arg(a, 2));
    let base = match eng {
        0 => m.lit(0x20430d8),
        1 => m.lit(0x20430dc),
        _ => return 0,
    };
    if (0..4).contains(&bg) {
        m.ram.set_u32(base + 4 + 4 * bg as u32, v);
    }
    0
}

/// 0x2052738
pub fn projection_set_callback(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x205274c);
    m.ram.set_u8(p + 0x34, arg(a, 0) as u8);
    m.ram.set_u16(p + 0x36, 0);
    0
}

// ---- hooks ------------------------------------------------------------------------------------------------------------

/// the incoming integer arguments: a veneer (`(*DAT_x)()`) leaves r0..r3 as its caller set them
fn passed(a: &[Arg]) -> Vec<i32> {
    a.iter().take(4).map(|x| x.int().unwrap_or(0)).collect()
}

pub fn hook_gfxres(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x2047d48);
    call_ptr(m, s, p, &passed(a))
}
pub fn hook_bgload(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x2047d54);
    call_ptr(m, s, p, &passed(a))
}
pub fn hook_clock0(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x205f0e8);
    call_ptr(m, s, p, &[0])
}
pub fn hook_clock1(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x205f0d8);
    call_ptr(m, s, p, &[1])
}
pub fn hook_flags_const(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let (p, k) = (m.lit(0x2057a34), m.lit(0x2057a30));
    call_ptr(m, s, p, &[k as i32])
}
pub fn hook_flags_bitrange(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x20536d0);
    call_ptr(m, s, p, &[0x100000, 0x100])
}
pub fn hook_uiflow_tertiary(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20b7a70);
    call_ptr(m, s, p, &passed(a))
}
pub fn hook_fadestate_a(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x2091778);
    call_ptr(m, s, p, &[0x20])
}
pub fn hook_sceneloop_a(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x209f508);
    call_ptr(m, s, p, &passed(a))
}
pub fn hook_sceneloop_b(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x209f654);
    call_ptr(m, s, p, &passed(a))
}
pub fn hook_randevent_d9fc8(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20d9fd0);
    call_ptr(m, s, p, &passed(a))
}
/// 0x2094348: the save block's hook called with its field 0x1ec
pub fn hook_savedata_1ec(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let (blk, p) = (m.lit(0x209435c), m.lit(0x2094360));
    let v = m.ram.u32(m.ram.u32(blk + 4) + 0x1ec);
    call_ptr(m, s, p, &[v as i32])
}

// ---- ScreenFade 0x213e4cc (two 0x14-byte channels: +0 level 16.16, +4 step, +8 state, +0xc counter, +0x10 delay) --

fn fade_channel(which: i32) -> u32 {
    if which != 1 { 0x14 } else { 0 }
}

/// 0x20451f8 ScreenFade_ToBlack(which, frames)
pub fn fade_to_black(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let o = fade_channel(arg(a, 0));
    let n = arg(a, 1).max(1);
    let (st, cur, step) = (m.lit(0x2045244), m.lit(0x2045248), m.lit(0x204524c));
    m.ram.set_u8(st + o, 0);
    let v = (-0x10000 - m.ram.i32(cur + o)) / n;
    m.ram.set_u32(step + o, v as u32);
    0
}

/// 0x2045250 ScreenFade_ToWhite(which, frames)
pub fn fade_to_white(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let o = fade_channel(arg(a, 0));
    let n = arg(a, 1).max(1);
    let (st, cur, step) = (m.lit(0x2045298), m.lit(0x204529c), m.lit(0x20452a0));
    m.ram.set_u8(st + o, 1);
    let v = (0x10000 - m.ram.i32(cur + o)) / n;
    m.ram.set_u32(step + o, v as u32);
    0
}

/// 0x20452a4 ScreenFade_ToClear(which, frames, delay)
pub fn fade_to_clear(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let o = fade_channel(arg(a, 0));
    let n = arg(a, 1).max(1);
    let (st, cur, step, ctr, delay) = (m.lit(0x2045304), m.lit(0x2045308), m.lit(0x204530c), m.lit(0x2045310), m.lit(0x2045314));
    m.ram.set_u8(st + o, 2);
    let v = -m.ram.i32(cur + o) / n;
    m.ram.set_u32(step + o, v as u32);
    m.ram.set_u32(ctr + o, 0);
    m.ram.set_u32(delay + o, arg(a, 2) as u32);
    0
}

/// 0x2045318
pub fn fade_is_active(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let o = fade_channel(arg(a, 0));
    (m.ram.u8(m.lit(0x2045344) + o) != 3) as i32
}

/// 0x2045390 ScreenFade_Process (every frame): steps both channels and sets the master brightness registers
pub fn screen_fade_process(m: &mut Machine) {
    let (reg_b, base) = (m.lit(0x2045500), m.lit(0x2045504));
    for ch in 0..2u32 {
        let p = base + ch * 0x14;
        let state = m.ram.u8(p + 8);
        let level = |m: &Machine| {
            let v = m.ram.i32(p);
            v + (((v >> 0xb) as u32) >> 0x14) as i32
        };
        let set = match state {
            0 => {
                let v = m.ram.i32(p).wrapping_add(m.ram.i32(p + 4));
                m.ram.set_u32(p, v as u32);
                if v < -0xffff {
                    m.ram.set_u32(p, (-0x10000i32) as u32);
                    m.ram.set_u8(p + 8, 3);
                }
                true
            }
            1 => {
                let v = m.ram.i32(p).wrapping_add(m.ram.i32(p + 4));
                m.ram.set_u32(p, v as u32);
                if v > 0xffff {
                    m.ram.set_u32(p, 0x10000);
                    m.ram.set_u8(p + 8, 3);
                }
                true
            }
            2 => {
                let c = m.ram.u32(p + 0xc).wrapping_add(1);
                m.ram.set_u32(p + 0xc, c);
                if m.ram.u32(p + 0x10) <= c {
                    let step = m.ram.i32(p + 4);
                    let v = m.ram.i32(p).wrapping_add(step);
                    m.ram.set_u32(p, v as u32);
                    let done = if step < 0 { v < 1 } else { v > -1 };
                    if done {
                        m.ram.set_u32(p, 0);
                        m.ram.set_u8(p + 8, 3);
                    }
                    true
                } else {
                    false
                }
            }
            _ => false,
        };
        if set {
            let reg = if ch == 0 { reg_b.wrapping_sub(0x1000) } else { reg_b }; // vu16 pointer - 0x800 halfwords
            let b = level(m) >> 0xc;
            // GXx_SetMasterBrightness_: positive brightens (mode 1), negative darkens (mode 2)
            let v = if b == 0 { 0 } else if b > 0 { 0x4000 | b as u32 } else { 0x8000 | (-b) as u32 };
            m.ram.set_u16(reg, v as u16);
        }
    }
}

/// the master brightness of a screen (0 main, 1 sub) as -16..16 (negative darker, positive whiter)
pub fn master_brightness(m: &Machine, engine: u32) -> i32 {
    let reg = if engine == 0 { 0x400006c } else { 0x400106c };
    let v = m.ram.u16(reg) as i32;
    match v >> 14 {
        1 => v & 0x1f,
        2 => -(v & 0x1f),
        _ => 0,
    }
}

// ---- GameFlags (the heap block whose address is at 0x2141908) --------------------------------------------------------

fn flags_block(m: &Machine, pool: u32) -> u32 {
    m.ram.u32(m.lit(pool) + 4)
}

/// 0x2053620 GameFlags_Assign(mask0, mask1, on)
pub fn flags_assign(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x205367c);
    let (a0, a1, on) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2));
    let (w0, w1) = (m.ram.u32(p + 0x14), m.ram.u32(p + 0x18));
    if on != 0 {
        m.ram.set_u32(p + 0x14, w0 | a0);
        m.ram.set_u32(p + 0x18, w1 | a1);
    } else {
        m.ram.set_u32(p + 0x14, w0 & !a0);
        m.ram.set_u32(p + 0x18, w1 & !a1);
    }
    0
}

pub fn flags_test_raw(m: &Machine, a0: u32, a1: u32) -> bool {
    let p = flags_block(m, 0x20536bc);
    p != 0 && ((m.ram.u32(p + 0x18) & a1) != 0 || (m.ram.u32(p + 0x14) & a0) != 0)
}

/// 0x2053680 GameFlags_Test(mask0, mask1)
pub fn flags_test(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    flags_test_raw(m, arg(a, 0) as u32, arg(a, 1) as u32) as i32
}
/// 0x2053e68
pub fn flags_set_0(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x2053e78);
    m.ram.set_u32(p, arg(a, 0) as u32);
    0
}
/// 0x2053e7c
pub fn flags_get_0(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x2053e8c);
    m.ram.i32(p)
}
/// 0x2053e90: +8 = +4, +4 = value
pub fn flags_push_4(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x2053eac);
    let v = m.ram.u32(p + 4);
    m.ram.set_u32(p + 8, v);
    m.ram.set_u32(p + 4, arg(a, 0) as u32);
    0
}
/// 0x2053f88
pub fn flags_set_98(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x2053f98);
    m.ram.set_u32(p + 0x98, arg(a, 0) as u32);
    0
}
/// 0x2057918 GameFlags_ComputeField0x104(y, m, d, wd, which): stores a date at +0xc8 (which != 0) or +0xb8
pub fn flags_compute_104(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x2057980);
    let o = if arg(a, 4) as u8 != 0 { 200 } else { 0xb8 };
    for i in 0..4 {
        m.ram.set_u32(p + o + 4 * i as u32, arg(a, i) as u32);
    }
    0
}
/// 0x2057984 GameFlags_CopyVec4_Ofs0xb8_0xc8(out, which)
pub fn flags_copy_vec4(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x20579b0);
    let o = if arg(a, 1) == 0 { 0xb8 } else { 200 };
    let w: Vec<i32> = (0..4).map(|i| m.ram.i32(p + o + 4 * i)).collect();
    put_out(m, a, 0, &w);
    0
}
/// 0x20537f0
pub fn flags_get_e0(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = flags_block(m, 0x2053800);
    m.ram.u8(p + 0xe0) as i32
}

// ---- GameState: the static block whose address is at 0x21246ac ----------------------------------------------------

fn gs(m: &Machine, pool: u32) -> u32 {
    m.ram.u32(m.lit(pool))
}
/// 0x20639e0
pub fn gs_get_78(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.u8(gs(m, 0x20639f0) + 0x78) as i32
}
/// 0x2063a80
pub fn gs_get_7a(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.u8(gs(m, 0x2063a90) + 0x7a) as i32
}
/// 0x2063cc4
pub fn gs_get_98(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.i32(gs(m, 0x2063cd4) + 0x98)
}
/// 0x2063514 GameState_SetPointer(p): stores the pointer, then tail-calls the hook in its pool
pub fn gs_set_pointer(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let (blk, hook) = (m.lit(0x2063524), m.lit(0x2063528));
    let v = arg(a, 0);
    m.ram.set_u32(blk + 4, v as u32);
    let mut args = [Arg::I(v)];
    call(m, s, hook & !1, &mut args)
}

/// 0x206325c GameState_GetCurrentSlot: a constant in its pool
pub fn gs_current_slot(m: &Machine) -> i32 {
    m.lit(0x2063264) as i32
}

/// 0x20b85e0 UiInput_Reset: the opening's map mode and character, and GameState+0x7a = 1
pub fn ui_input_reset(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    call(m, s, crate::flow::addr::MapArc_SetCurrentMode, &mut [Arg::I(10)]);
    call(m, s, crate::flow::addr::MapArc_SetCurrentCharId, &mut [Arg::I(0x80000000u32 as i32)]);
    let p = gs(m, 0x2063a7c); // GameState_SetField0x7a 0x2063a6c
    m.ram.set_u8(p + 0x7a, 1);
    0
}

// ---- MapArc (0x214695c) ---------------------------------------------------------------------------------------------

/// 0x207025c
pub fn maparc_get_char(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.i32(m.lit(0x2070268) + 0x1c)
}
/// 0x207026c
pub fn maparc_clear_char(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x207027c);
    m.ram.set_u32(p + 0x1c, 0);
    0
}
/// 0x2070280
pub fn maparc_set_mode(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x207028c);
    m.ram.set_u32(p + 0x20, arg(a, 0) as u32);
    0
}
pub fn maparc_mode(m: &Machine) -> i32 {
    // 0x2070290: 1 while the flag (1, 0) is set
    if flags_test_raw(m, 1, 0) { 1 } else { m.ram.i32(m.lit(0x20702b8) + 0x20) }
}
pub fn maparc_get_mode(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    maparc_mode(m)
}
/// 0x2070390
pub fn maparc_set_34(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x207039c);
    m.ram.set_u32(p + 0x34, arg(a, 0) as u32);
    0
}
/// 0x20703b0
pub fn maparc_set_bgm(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20703bc);
    m.ram.set_u32(p + 0x38, arg(a, 0) as u32);
    0
}
/// 0x20703c0
pub fn maparc_get_bgm(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.i32(m.lit(0x20703cc) + 0x38)
}

/// 0x206fcbc CharId_DecodeSlotFromHighBits: the heroine slot of a character id
pub fn char_slot(m: &Machine, id: u32) -> i32 {
    let b = (id >> 0x1b) as u8;
    if b < 0xc {
        if b < 0xb {
            return if b == 1 { 2 } else { 0 };
        }
        return 1;
    }
    if b > 0x14 || b != 0x10 {
        return 0;
    }
    gs_current_slot(m)
}

/// 0x20703d0 MapArc_GetActiveCharIdAndMode(which, &kind, &slot)
pub fn maparc_get_active(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let which = arg(a, 0);
    let global = global_char_id(m); // CharId_GetGlobalValue 0x206fc6c
    let mut out = (0i32, 0i32);
    let mut id = 0i32;
    let mut done = false;
    if global != 0 {
        out = (maparc_mode(m), char_slot(m, global as u32));
        id = global;
        done = true;
    } else if which == 0 {
        if flags_test_raw(m, 1, 0) {
            id = call(m, s, crate::flow::addr::SceneRoute_SelectTextRoute, &mut []);
            out = (1, char_slot(m, id as u32));
        } else if flags_test_raw(m, 0x100000, 0) {
            id = call(m, s, crate::flow::addr::StateLogic_UpdateData_dd26c, &mut []);
            out = (0x10, char_slot(m, id as u32));
        } else if flags_test_raw(m, 0, 0x100) {
            id = call(m, s, crate::flow::addr::ItemDb_GetSelectedQueueValue, &mut []);
            out = (0x10, char_slot(m, id as u32));
        } else {
            let (p, q) = (m.lit(0x2070504), m.lit(0x2070508));
            id = m.ram.i32(p + 0x1c);
            out = (m.ram.i32(p + 0x20), m.ram.i32(q + 0x18));
        }
    } else if which == 4 {
        let w = crate::flow::addr::MapEvent_CheckWorkState;
        id = call(m, s, w, &mut [Arg::I(0)]);
        if id == 0 {
            id = call(m, s, w, &mut [Arg::I(1)]);
        }
        out = (8, char_slot(m, id as u32));
    }
    if !done && id == 0 {
        out = (0, -1);
    }
    put_out(m, a, 1, &[out.0]);
    put_out(m, a, 2, &[out.1]);
    id
}

// ---- the clock: GameClock (the block at *0x2142c6c: +0 mode, +0x3c.. the manual date) and the RTC copy (*0x21418f4)

/// GameClock's block `c` (*0x2142c6c). Every getter (hour 0x205f2ac, year 0x205f4b0, month 0x205f53c, day 0x205f5c8,
/// weekday 0x205f678) picks one of three sources: the manual date (c[0] != 0: +0x3c hour, +0x48 year, +0x4c month,
/// +0x50 day, +0x54 weekday); the block's own clock when c+0xc is set, or when flag 0x8000000 is set and
/// GameFlags field 0xe0 is 0 (+0x20 hour, +0x2c year, +0x30 month, +0x34 day, +0x38 weekday); otherwise the RTC copy
/// at *(0x21418f0 + 4) (+0x20 hour .. +0x38 weekday), the hour shifted by the offset in seconds at c+8 and the
/// weekday by c+0x5c
enum Clock {
    Manual(u32),
    Own(u32),
    Rtc(u32, u32),
}
fn clock_source(m: &Machine) -> Clock {
    let c = m.ram.u32(m.lit(0x205f2d4));
    if m.ram.u32(c) != 0 {
        Clock::Manual(c)
    } else if m.ram.u32(c + 0xc) != 0 || (flags_test_raw(m, 0x8000000, 0) && m.ram.u8(flags_block(m, 0x2053800) + 0xe0) == 0) {
        Clock::Own(c)
    } else {
        Clock::Rtc(c, m.ram.u32(m.lit(0x2053014) + 4))
    }
}
/// the date and time at `p` (+0 year 0-99, month, day, weekday; `t`: hour, minute, second) as seconds since
/// 2000-01-01 (RTC_ConvertDateTimeToSecond 0x20238f8; -1 when out of range)
fn rtc_seconds(m: &Machine, p: u32, t: u32) -> i64 {
    let v = |a: u32| m.ram.u32(a) as i64;
    let (y, mo, d, h, mi, se) = (v(p), v(p + 4), v(p + 8), v(t), v(t + 4), v(t + 8));
    if y > 99 || !(1..=12).contains(&mo) || d < 1 || d > 31 || h > 23 || mi > 59 || se > 59 {
        return -1;
    }
    let days = (super::sdk::local_seconds(2000 + y, mo, d, 0, 0, 0) - super::sdk::local_seconds(2000, 1, 1, 0, 0, 0)) / 86400;
    days * 86400 + h * 3600 + mi * 60 + se
}
/// 0x205f2ac
pub fn clock_hour_raw(m: &Machine) -> i32 {
    match clock_source(m) {
        Clock::Manual(c) => m.ram.i32(c + 0x3c),
        Clock::Own(c) => m.ram.i32(c + 0x20),
        Clock::Rtc(c, r) => {
            // the hour of (RTC date-time + offset), through RTC_ConvertSecondToDateTime 0x2023ad4 (clamped to
            // 2000-01-01 .. 2099-12-31 23:59:59)
            let t = (rtc_seconds(m, r + 0x2c, r + 0x20) + m.ram.i32(c + 8) as i64).clamp(0, 3_155_759_999);
            ((t % 86400) / 3600) as i32
        }
    }
}
pub fn clock_hour(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    clock_hour_raw(m)
}
fn clock_field(m: &Machine, manual: u32, own: u32) -> i32 {
    match clock_source(m) {
        Clock::Manual(c) => m.ram.i32(c + manual),
        Clock::Own(c) => m.ram.i32(c + own),
        Clock::Rtc(_, r) => m.ram.i32(r + own),
    }
}
/// 0x205f4b0
pub fn clock_year_raw(m: &Machine) -> i32 {
    clock_field(m, 0x48, 0x2c)
}
pub fn clock_year(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    clock_year_raw(m)
}
/// 0x205f53c
pub fn clock_month_raw(m: &Machine) -> i32 {
    clock_field(m, 0x4c, 0x30)
}
pub fn clock_month(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    clock_month_raw(m)
}
/// 0x205f5c8
pub fn clock_day_raw(m: &Machine) -> i32 {
    clock_field(m, 0x50, 0x34)
}
pub fn clock_day(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    clock_day_raw(m)
}
/// 0x205f678
pub fn clock_weekday_raw(m: &Machine) -> i32 {
    match clock_source(m) {
        Clock::Manual(c) => m.ram.i32(c + 0x54),
        Clock::Own(c) => m.ram.i32(c + 0x38),
        Clock::Rtc(c, r) => {
            let mut w = m.ram.i32(r + 0x38) + m.ram.i32(c + 0x5c);
            while w >= 7 {
                w -= 7;
            }
            w
        }
    }
}

/// 0x205ebf4 TimePeriod_FromHour: the period boundaries are bytes in the game's data
pub fn time_period_raw(m: &Machine, hour: i32) -> i32 {
    let (a, b) = (m.lit(0x205ec34), m.lit(0x205ec38));
    if hour < m.rom.u8(a + 0x51) as i32 {
        return 5;
    }
    (0..5).find(|&i| hour < m.rom.u8(b + i) as i32).map_or(5, |i| i as i32)
}
pub fn time_period_from_hour(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    time_period_raw(m, arg(a, 0))
}
/// 0x205f728
pub fn clock_time_period(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let h = clock_hour_raw(m);
    time_period_raw(m, h)
}
/// 0x205f97c GameClock_GetDate(out[4]): year, month, day, weekday
pub fn clock_get_date(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let d = [clock_year_raw(m), clock_month_raw(m), clock_day_raw(m), clock_weekday_raw(m)];
    put_out(m, a, 0, &d);
    0
}

// ---- the screen manager's screen-side block (0x214fde0) and the persistent block (0x2152ed8) -----------------------

/// 0x20885e4
pub fn screen_get_next(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.i32(m.lit(0x20885f0) + 4)
}
/// 0x2088614
pub fn screen_get_result(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.i32(m.lit(0x2088620) + 0x14)
}
/// 0x208861c.. Screen_GetAltResult (labelled Sprite_GetField1c)
pub fn screen_get_alt(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let a = crate::flow::addr::Screen_GetAltResult;
    let pool = super::super::flow::code::first_literal(&m.rom, a).unwrap_or(0x214fde0);
    m.ram.i32(pool + 0x1c)
}
/// 0x20885f4
pub fn screen_get_10(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let pool = crate::flow::code::first_literal(&m.rom, 0x20885f4).unwrap_or(0x214fde0);
    m.ram.i32(pool + 0x10)
}
/// 0x208a230 Screen_GetSaved: the persistent block's address
pub fn screen_get_saved(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.lit(0x208a238) as i32
}
/// 0x208a288
pub fn screen_get_saved_size(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.lit(0x208a290) as i32
}
/// 0x208a1f0: the running screen's entry argument (REQ + 0x1c)
pub fn screen_get_entry_arg(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.ram.i32(m.lit(0x208a1fc) + 0x1c)
}
/// 0x208a200
pub fn screen_set_entry_arg(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x208a20c);
    m.ram.set_u32(p + 0x1c, arg(a, 0) as u32);
    0
}
/// 0x20886b4
pub fn screen_set_8(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20886c0);
    m.ram.set_u32(p + 8, arg(a, 0) as u32);
    0
}
/// 0x20886c4
pub fn screen_set_c(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20886d0);
    m.ram.set_u32(p + 0xc, arg(a, 0) as u32);
    0
}
/// 0x20886e8
pub fn screen_set_20(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20886f4);
    m.ram.set_u32(p + 0x20, arg(a, 0) as u32);
    0
}
/// 0x208a23c (labelled Sprite_ClearContext): clears the persistent block (0xaf * 32 bytes) and three words after it
pub fn screen_clear_saved(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let (p, q) = (m.lit(0x208a280), m.lit(0x208a284));
    m.ram.fill(p, 0, 0xaf * 32);
    for o in [0x28, 0x2c, 0x30] {
        m.ram.set_u32(q + o, 0);
    }
    0
}
/// 0x2089ce0 (labelled Sprite_ClearAnimation): clears the request and screen blocks; the manager restarts idle
pub fn screen_clear_all(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x2089d3c);
    m.ram.fill(p, 0, 0xb0 * 32 + 5 * 4);
    let q = m.lit(0x2089d40);
    m.ram.set_u32(q, 1);
    m.ram.set_u32(q + 12, u32::MAX);
    m.ram.set_u32(q + 16, u32::MAX);
    m.ram.set_u32(q + 20, u32::MAX);
    call(m, s, crate::flow::addr::SpriteSystem_Init, &mut []);
    call(m, s, crate::flow::addr::Bg_ClearState, &mut []);
    0
}
/// 0x2087538: the screen's procedure table from the game's screen list (21 pairs: id, table)
pub fn screen_procedure_table(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let id = arg(a, 0);
    let (ids, tables) = (m.lit(0x2087564), m.lit(0x2087568));
    let mut i = 0;
    while i < 0x15 && m.ram.i32(ids + i * 8) != id {
        i += 1;
    }
    m.ram.i32(tables + i * 8)
}
/// 0x208757c: the screen object cleared through the pool's memset-like hook
pub fn screen_clear_object(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x208758c);
    let obj = arg(a, 0);
    call_ptr(m, s, p, &[obj, 0, 0x29c])
}
/// 0x2088298: a screen starts: its id and the running state in the screen block
pub fn screen_begin(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let id = arg(a, 0);
    if id == -1 {
        return 0;
    }
    call(m, s, crate::flow::addr::Sprite_DispatchDrawState, &mut [Arg::I(id)]); // the id, still in r0
    let p = m.lit(0x20882c4);
    m.ram.set_u32(p, id as u32);
    m.ram.set_u32(p + 8, 1);
    m.ram.set_u32(p + 12, 1);
    0
}

/// 0x2077370
pub fn save_select_tick_wait(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = m.lit(0x2077390);
    let v = m.ram.i32(p + 0xc) + 1;
    m.ram.set_u32(p + 0xc, v as u32);
    if v > 99 {
        m.ram.set_u32(p + 0xc, 2);
    }
    0
}

/// 0x2049a4c: the id is in one of two 9-entry tables
pub fn scene_id_in_tables(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let id = arg(a, 0);
    let (t1, t2) = (m.lit(0x2049a9c), m.lit(0x2049aa0));
    ((0..9).any(|i| m.ram.i32(t1 + 4 * i) == id) || (0..9).any(|i| m.ram.i32(t2 + 4 * i) == id)) as i32
}

/// 0x20920e4
pub fn gameflow_get_4(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = m.ram.u32(m.lit(0x20920f4));
    m.ram.i32(p + 4)
}
/// 0x20b96cc
pub fn ui_selection_set(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = m.lit(0x20b96d8);
    m.ram.set_u8(p, arg(a, 0) as u8);
    0
}
/// 0x209410c
pub fn savedata_record0_used(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let p = m.ram.u32(m.lit(0x209412c) + 4);
    (m.ram.u8(p + 0x168) as i8 != 0) as i32
}

/// 0x203fac0 GameHeap_StartGameLoad (depth-1 state 0): the boot-time save load; then the flow (state 5)
pub fn game_heap_start_game_load(m: &mut Machine, s: S) -> i32 {
    let ov = m.lit(0x203fb04);
    call(m, s, 0x203ff24, &mut [Arg::I(ov as i32)]);
    fade_to_white(m, s, &mut [Arg::I(1), Arg::I(1)]);
    fade_to_white(m, s, &mut [Arg::I(2), Arg::I(1)]);
    use crate::flow::addr::*;
    for a in [SaveData_InitializeRecord, GameState_RunSequence, SaveData_AllocHeaderBlock, SaveData_ValidateRecord, SlotWorkflow_ClearAction] {
        call(m, s, a, &mut []);
    }
    5
}

// ---- the voice decoder (G.722.1) ----------------------------------------------------------------------------------
// The game's decoder state (0xfb0 bytes, G7221_InitDecoder): +0xf80 the bit callback's argument, +0xf90 the
// callback (returns the next frame's 16-bit words), +0xfa0 bits per frame, +0xfa8 samples per frame (320 / 640);
// per channel (0, 1) the coefficient history at +ch*0x500, the overlap samples at +0xa00 + ch*0x280, the noise
// generator at +0xf20 + ch*0x10 and the error state at +0xf00 + ch*4. The engine's decoder (`siren::Decoder`, one per
// state and channel) keeps the history itself; nothing else reads the game's copy.

/// the game decoder's instructions per 640-sample frame (title: 21,467,989 for 125 frames)
const G7221_FRAME_INSTRUCTIONS: u64 = 171_744;

fn g7221_tables(m: &mut Machine) -> Option<&'static crate::siren::Tables> {
    if m.g7221_tables.is_none() {
        let raw = std::fs::read(m.rom.content_dir().join("engine/g7221.pack")).ok()?;
        m.g7221_tables = Some(Box::leak(Box::new(crate::siren::Tables::from_pack(&raw).ok()?)));
    }
    m.g7221_tables
}

/// 0x2001a58 G7221_DecodeFrame(state, out, channel): the next frame's bits from the game's callback, decoded into
/// `out` (16-bit PCM); returns the samples per frame
pub fn g7221_decode_frame(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let (state, out, ch) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let (cb, cb_arg) = (m.ram.u32(state + 0xf90), m.ram.u32(state + 0xf80));
    let bits = m.ram.u32(state + 0xfa0) as i16;
    let n = m.ram.u32(state + 0xfa8);
    let words_at = super::ext::call(m, s, cb, &mut [Arg::I(cb_arg as i32)]) as u32;
    let frame: Vec<u16> = (0..(bits as u32).div_ceil(16)).map(|i| m.ram.u16(words_at + 2 * i)).collect();
    let Some(t) = g7221_tables(m) else { return n as i32 };
    let dec = m.g7221.entry((state, ch)).or_insert_with(|| crate::siren::Decoder::new(t, if n == 320 { 7000 } else { 14000 }));
    let (pcm, _) = dec.decode_frame(&frame, bits);
    m.g7221_frames += 1;
    // the time the game's decoder takes (measured: 21.47 M instructions for 125 frames of 640 samples on the
    // title), so the stream thread keeps its pace against the others
    m.spend(G7221_FRAME_INSTRUCTIONS * n as u64 / 640);
    for (i, v) in pcm.iter().enumerate() {
        m.ram.set_u16(out + 2 * i as u32, *v as u16);
    }
    n as i32
}

/// 0x20019e4 G7221_ResetDecoderBuffers(state): both channels' history cleared, as the game does in its state, and the
/// engine's decoders for it started afresh
pub fn g7221_reset(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let state = arg(a, 0) as u32;
    let n = m.ram.u32(state + 0xfa8);
    for ch in 0..2u32 {
        m.ram.fill(state + ch * 0x500, 0, 2 * n);
        m.ram.fill(state + 0xa00 + ch * 0x280, 0, n & 0x7fff_ffff);
        for k in 0..4 {
            m.ram.set_u32(state + ch * 0x10 + 0xf20 + 4 * k, 1);
        }
        m.ram.set_u32(state + 0xf00 + 4 * ch, 0);
        m.g7221.remove(&(state, ch));
    }
    0
}

// ---- memory routines, cache maintenance and the compiler's helpers ------------------------------------------------
// Pure functions of their arguments and memory, done through the same bus the game's code uses (halfwords aligned
// down, writes to I/O with their effects, null-page writes refused). Each charges the instructions the game's loop
// would have run (from its disassembly), so time passes as it would. Checked against the game's own functions on
// the same memory and arguments (tests/natives_exact.rs).
use crate::arm::Bus;

/// 0x201e88c MIi_CpuCopy16(src, dst, size): halfwords while n < size (an odd size copies one more); 5 instructions a halfword
pub fn mi_cpu_copy16(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (src, dst, size) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let mut n = 0;
    while n < size {
        let v = m.ram.r16(src.wrapping_add(n) & !1).unwrap_or(0);
        let _ = m.ram.w16(dst.wrapping_add(n) & !1, v);
        n += 2;
    }
    m.spend(3 + 5 * size.div_ceil(2) as u64);
    0
}

/// 0x201e874 MIi_CpuClear16(data, dst, size): the halfword stored while n < size; 4 instructions a halfword
pub fn mi_cpu_clear16(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (data, dst, size) = (arg(a, 0) as u32 & 0xffff, arg(a, 1) as u32, arg(a, 2) as u32);
    let mut n = 0;
    while n < size {
        let _ = m.ram.w16(dst.wrapping_add(n) & !1, data);
        n += 2;
    }
    m.spend(3 + 4 * size.div_ceil(2) as u64);
    0
}

/// 0x201e8d4 MIi_CpuSend32(src, dst, size): each source word stored to the same address (a port); 4 a word
pub fn mi_cpu_send32(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (src, dst, size) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let mut p = src;
    let end = src.wrapping_add(size);
    while p < end {
        let v = m.ram.r32(p & !3).unwrap_or(0);
        let _ = m.ram.w32(dst & !3, v);
        p += 4;
    }
    m.spend(3 + 4 * size.div_ceil(4) as u64);
    0
}

/// 0x201e8ec MIi_CpuClearFast(data, dst, size): the word stored to the end (a size not a multiple of 4 stores the
/// last word whole); 32-byte blocks take 3 instructions, single words 3
pub fn mi_cpu_clear_fast(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (data, dst, size) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let end = dst.wrapping_add(size);
    let mut p = dst;
    while p < end {
        let _ = m.ram.w32(p & !3, data);
        p += 4;
    }
    m.spend(15 + 3 * (size / 32) as u64 + 3 * (size % 32).div_ceil(4) as u64);
    0
}

/// 0x202caf0 MemCopyBytes(dst, src, n): bytes forward; returns dst; 4 instructions a byte
pub fn mem_copy_bytes(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (dst, src, n) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    for i in 0..n {
        let v = m.ram.r8(src.wrapping_add(i)).unwrap_or(0);
        let _ = m.ram.w8(dst.wrapping_add(i), v);
    }
    m.spend(3 + 4 * n as u64);
    dst as i32
}

/// 0x20307b8 Libc_Bzero(p, n): n bytes cleared (nothing for a null pointer or zero size); 3 instructions a byte
pub fn libc_bzero(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (p, n) = (arg(a, 0) as u32, arg(a, 1) as u32);
    if p != 0 && n != 0 {
        for i in 0..n {
            let _ = m.ram.w8(p.wrapping_add(i), 0);
        }
        m.spend(6 + 3 * n as u64);
    } else {
        m.spend(4);
    }
    p as i32
}

/// 0x2000950 MIi_UncompressBackward(end): the packed program and overlays, decoded in place from the end down. The
/// footer below `end`: a word (header length << 24 | packed length) and the growth; the stream is read downward from
/// end - header length to end - packed length, the output written downward from end + growth. A flag byte, then 8
/// tokens (top bit first): a byte, or two (high first) for a copy of (high >> 4) + 3 bytes from (12 bits) + 2 above
/// the output. The bounds compare signed, as the game's code does; the data cache cleaned and invalidated by line
/// over the output after (no cache here, time only). Instructions: 9 on entry, 2 a bound check, 2 a flag byte and
/// 2 a bit, 5 + 3 a byte, 11 + 4 a copied byte for a copy, 4 + 6 a cache line; 3 for a null `end`.
pub fn mi_uncompress_backward(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let end = arg(a, 0) as u32;
    if end == 0 {
        m.spend(3);
        return 0;
    }
    let (w1, w2) = (m.ram.u32(end.wrapping_sub(8)), m.ram.u32(end.wrapping_sub(4)));
    let mut dst = end.wrapping_add(w2);
    let top = dst;
    let mut src = end.wrapping_sub(w1 >> 24);
    let low = end.wrapping_sub(w1 & 0xff_ffff);
    let mut c = 9u64;
    'outer: loop {
        c += 2;
        if src as i32 <= low as i32 {
            break;
        }
        src = src.wrapping_sub(1);
        let mut flags = m.ram.u8(src) as u32;
        c += 2;
        for _ in 0..8 {
            c += 2;
            if flags & 0x80 == 0 {
                src = src.wrapping_sub(1);
                dst = dst.wrapping_sub(1);
                let v = m.ram.u8(src);
                m.ram.set_u8(dst, v);
                c += 5;
            } else {
                let hi = m.ram.u8(src.wrapping_sub(1)) as u32;
                let lo = m.ram.u8(src.wrapping_sub(2)) as u32;
                src = src.wrapping_sub(2);
                let disp = ((hi << 8 | lo) & 0xfff) + 2;
                let n = (hi >> 4) + 3;
                for _ in 0..n {
                    let v = m.ram.u8(dst.wrapping_add(disp));
                    dst = dst.wrapping_sub(1);
                    m.ram.set_u8(dst, v);
                }
                c += 8 + 4 * n as u64;
            }
            c += 3;
            flags <<= 1;
            if src as i32 <= low as i32 {
                break 'outer;
            }
        }
        c += 2; // the ninth bit test ends the flag byte
    }
    // the cache by line from the stream's start to the output's end, at least one line
    let mut line = low & !0x1f;
    loop {
        c += 6;
        line = line.wrapping_add(32);
        if line as i32 >= top as i32 {
            break;
        }
    }
    m.spend(c + 4);
    0
}

/// 0x201ef8c MI_UncompressLZ8(src, dst): LZ77 with byte writes. Header: size << 8, type; a nonzero low nibble is the
/// extended form, whose copy lengths take one (3-16), two (17-272) or three (273-65808) nibbles' worth of bytes by
/// the first nibble. A flag byte, then up to 8 tokens (top bit first): a byte, or a copy from (12 bits) + 1 back;
/// a copy is done whole even past the size. Instructions: 6 on entry, 2 a size check (+2 leaving), 2 a flag byte
/// and 2 a bit, 7 a byte, a copy 14 (standard) / 17 (one nibble) / 24 / 28 + 5 a copied byte, 3 after a token and
/// 1 more when the size ran out.
pub fn mi_uncompress_lz8(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (mut src, mut dst) = (arg(a, 0) as u32, arg(a, 1) as u32);
    let head = m.ram.u32(src);
    src = src.wrapping_add(4);
    let mut left = (head >> 8) as i32;
    let ext = head & 0xf != 0;
    let rd = |m: &Machine, src: &mut u32| {
        let v = m.ram.u8(*src) as u32;
        *src = src.wrapping_add(1);
        v
    };
    let mut c = 6u64;
    'outer: loop {
        c += 2;
        if left <= 0 {
            break;
        }
        let mut flags = rd(m, &mut src);
        c += 2;
        for _ in 0..8 {
            c += 2;
            if flags & 0x80 == 0 {
                let v = rd(m, &mut src) as u8;
                m.ram.set_u8(dst, v);
                dst = dst.wrapping_add(1);
                left -= 1;
                c += 7;
            } else {
                let b0 = m.ram.u8(src) as u32;
                c += 6;
                let (base, b) = if !ext {
                    (3, b0)
                } else if b0 & 0xe0 != 0 {
                    c += 3;
                    (1, b0)
                } else {
                    c += 3 + 7;
                    src = src.wrapping_add(1);
                    let mut base = (b0 & 0xf) << 4;
                    if b0 & 0x10 != 0 {
                        c += 4;
                        base = (base << 8) + (rd(m, &mut src) << 4) + 0x100;
                    }
                    (base + 0x11, m.ram.u8(src) as u32)
                };
                let n = base + (b >> 4);
                src = src.wrapping_add(1);
                let disp = ((b & 0xf) << 8 | rd(m, &mut src)) + 1;
                left -= n as i32;
                for _ in 0..n {
                    let v = m.ram.u8(dst.wrapping_sub(disp));
                    m.ram.set_u8(dst, v);
                    dst = dst.wrapping_add(1);
                }
                c += 8 + 5 * n as u64;
            }
            c += 3;
            flags <<= 1;
            if left <= 0 {
                c += 1;
                continue 'outer;
            }
        }
        c += 2; // the ninth bit test ends the flag byte
    }
    m.spend(c + 2);
    0
}

/// 0x206fc6c CharId_GetGlobalValue: the word at 0x214695c (the game's debug override of the scene's script id; 0 in
/// play), or the id `Machine::event_override` asks for. 3 instructions.
pub fn char_id_get_global(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    m.spend(3);
    global_char_id(m)
}
/// the debug override's word, or the requested event until its script starts (`ext::call` drops it when the script
/// start 0x205e7c4 receives that id: the scene machine reads the word in setup, at the start and when re-entering)
fn global_char_id(m: &mut Machine) -> i32 {
    match m.event_override {
        Some(id) => id as i32,
        None => m.ram.i32(m.lit(0x206fc78)),
    }
}

/// the proportional text of the English patch: on when `patch/text.json` (beside the content) says
/// {"proportional": true} (LP_NO_PATCH=1 turns the whole patch overlay off: the game as it is). Per font (LC12, LC10: content/assets/test_font) and ASCII character 20h..7eh, the columns
/// its ink spans in reading order, from the pen position (the glyph's left bearing plus its first / last inked
/// column); a space is 4 (LC12) / 3 (LC10) pixels wide. None: the game's own fixed widths.
pub fn proportional_text_ink(rom: &crate::rom::Rom) -> Option<Box<[[(i8, i8); 95]; 2]>> {
    if std::env::var_os("LP_NO_PATCH").is_some() {
        return None;
    }
    let dir = rom.content_dir();
    let cfg: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.parent()?.join("patch/text.json")).ok()?).ok()?;
    if cfg.get("proportional").and_then(|v| v.as_bool()) != Some(true) {
        return None;
    }
    let mut ink = Box::new([[(0i8, 0i8); 95]; 2]);
    for (k, (name, space)) in [("LC12_UTF8_R90.NFTR", 4i8), ("LC10_UTF8_R90.NFTR", 3)].iter().enumerate() {
        let f = crate::font::Font::parse(&std::fs::read(dir.join("assets/test_font").join(name)).ok()?).ok()?;
        for c in 0x20u8..0x7f {
            let gi = f.glyph_index(c as char);
            let left = f.metrics(gi).0 as i8;
            let (w, _, rows) = f.glyph_bitmap(gi);
            let cols: Vec<usize> = (0..w).filter(|&x| rows.iter().any(|r| r[x] != 0)).collect();
            ink[k][(c - 0x20) as usize] = match (cols.first(), cols.last()) {
                (Some(&a), Some(&b)) if c != b' ' => (left + a as i8, left + b as i8),
                _ => (0, space - 2),
            };
        }
    }
    Some(ink)
}

/// 0x2084a94 Text_GetGlyphWidth(&p, &width): the character at *p (UTF-8, 1 to 3 bytes) and the pen advance: 0 for
/// the end and for '\\' (which toggles the colour; the pointer moves on), 6 / 12 (LC12, font mode 1 at
/// [0x214baf4 + 8]) or 5 / 10 (LC10) for one-byte / longer characters. With proportional text (Machine::text_ink)
/// a one-byte character advances to one pixel past its ink, measured against where the next character's ink
/// starts. Instructions: 17 at the end, 25 for '\\' and one-byte characters, 39 / 42 for two / three bytes.
pub fn text_get_glyph_width(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (pp, wp) = (arg(a, 0) as u32, arg(a, 1) as u32);
    let lc12 = m.ram.u32(m.lit(0x2084b78) + 8) == 1;
    let (half, full) = if lc12 { (6, 12) } else { (5, 10) };
    let p = m.ram.u32(pp);
    let b0 = m.ram.u8(p) as u32;
    let (code, width, len, cost) = match b0 {
        0 => (0, 0, 0, 17),
        0x5c => (0, 0, 1, 25),
        _ if b0 & 0x80 == 0 => {
            let w = match &m.text_ink {
                Some(ink) => proportional_advance(m, ink, p),
                None => half,
            };
            (b0, w as u32, 1, 25)
        }
        _ if b0 & 0xe0 == 0xc0 => (((b0 & 0x1f) << 6 | (m.ram.u8(p + 1) as u32 & 0x3f)) & 0xffff, full as u32, 2, 39),
        _ => (((b0 & 0x1f) << 12 | (m.ram.u8(p + 1) as u32 & 0x3f) << 6 | (m.ram.u8(p + 2) as u32 & 0x3f)) & 0xffff, full as u32, 3, 42),
    };
    m.ram.set_u32(pp, p.wrapping_add(len));
    m.ram.set_u32(wp, width);
    m.spend(cost);
    code as i32
}

/// the pen advance of the one-byte character at `p` with proportional text (the font in mode [0x214baf4 + 8]): to
/// one pixel past its ink, against where the next character's ink starts; '\\' draws nothing
fn proportional_advance(m: &Machine, ink: &[[(i8, i8); 95]; 2], p: u32) -> i32 {
    let lc12 = m.ram.u32(m.lit(0x2084b78) + 8) == 1;
    let t = &ink[if lc12 { 0 } else { 1 }];
    let (b0, next) = (m.ram.u8(p), m.ram.u8(p + 1));
    if b0 == 0x5c {
        return 0;
    }
    let start = if (0x20..0x7f).contains(&next) { t[(next - 0x20) as usize].0 as i32 } else { 0 };
    let end = if (0x20..0x7f).contains(&b0) { t[(b0 - 0x20) as usize].1 as i32 } else { if lc12 { 4 } else { 3 } };
    (end + 2 - start).max(1)
}

/// 0x208ba18 TextCursor_Initialize(src, slot, units): one line of a script message for the message window: characters
/// (lengths from TextCursor_ProcessInput 0x208bc14) are taken while the width so far is below units * 12 px (one-byte
/// characters 6, longer ones 12); a newline ends the line. Without a newline, TextCursor_InitializeSecondary
/// 0x208bfb8 (next character, the last one's class 1 narrow / 3 wide, &newline flag) applies the line-start rule and
/// its result adjusts the bytes copied (when the flag stays clear) and taken. The line goes to `slot` with a 0 one
/// byte past it (as the game writes it). Returns the bytes taken. With proportional text one-byte characters take
/// their proportional advance and a character is taken only if it fits. Instructions: 15 + 6 a step (+2 for the
/// bound) + 4 / 9 (one-byte or two) + 9 and 1 a character; 18 at the end, 5 more with the line-start rule.
pub fn text_cursor_initialize(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let (src, slot, units) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2));
    let limit = units.wrapping_mul(12);
    let ink = m.text_ink.clone();
    let (mut taken, mut copy, mut width, mut p, mut class, mut newline) = (0u32, 0u32, 0i32, src, 0i32, 0i32);
    let mut c = 15u64;
    loop {
        c += 4;
        let n = call(m, s, 0x208bc14, &mut [Arg::I(p as i32)]) as u32;
        if n == 0 {
            break;
        }
        c += 2;
        if width >= limit {
            break;
        }
        let first = m.ram.u8(p);
        let w = match &ink {
            Some(ink) if n == 1 => proportional_advance(m, ink, p),
            _ if n >= 2 => 12,
            _ => 6,
        };
        if ink.is_some() && width + w > limit && first != 0xa {
            break;
        }
        c += if n == 3 { 4 } else { 9 };
        class = if n == 1 { 1 } else { 3 };
        width += w;
        taken += n;
        copy = taken;
        c += 9;
        if first == 0xa {
            newline = 1;
            copy = taken - n;
            p = p.wrapping_add(n);
            break;
        }
        p = p.wrapping_add(n);
        c += 1;
    }
    c += 4;
    let adjust = if newline == 1 {
        0
    } else {
        // the line-start rule may move the last character down, and may set the flag
        let mut flag = [Some(newline)];
        let r = call(m, s, 0x208bfb8, &mut [Arg::I(p as i32), Arg::I(class), Arg::Out(&mut flag)]);
        newline = flag[0].unwrap_or(newline);
        c += 5;
        r
    };
    if newline == 0 {
        copy = copy.wrapping_add(adjust as u32);
    }
    call(m, s, 0x202caf0, &mut [Arg::I(slot as i32), Arg::I(src as i32), Arg::I(copy as i32)]);
    m.ram.set_u8(slot.wrapping_add(copy).wrapping_add(1), 0);
    c += 14;
    m.spend(c);
    taken.wrapping_add(adjust as u32) as i32
}

/// 0x208bb14 TextCursor_Update(src, layout, line, units): one line of a message for the message window: characters
/// (lengths from TextCursor_ProcessInput 0x208bc14) are taken while the width so far is below units * 12 px (3-byte
/// characters 12, others 6); a newline ends the line (counted in layout +0xa); the line's bytes are copied to the
/// layout's 64-byte slot `line` (+0xc) with a 0 one byte past them (as the game writes it), the line count of
/// paragraph layout[+0xa] (+5) raised if it took any; a newline right after a full line is taken too. Returns the
/// bytes taken. With proportional text one-byte characters take their proportional advance and a character is
/// taken only if it fits. Instructions: 13 + 6 a step (+2 for the bound) + 8 and 1 a character, 6 for a newline;
/// 18 at the end, 6 more for a newline after a full line.
pub fn text_cursor_update(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let (src, sl, line, units) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32, arg(a, 3));
    let limit = units.wrapping_mul(12);
    let ink = m.text_ink.clone();
    let fp = sl + 0xc + (line << 6);
    let r7 = m.ram.u8(sl + 0xa) as i8 as i32;
    let (mut taken, mut copy, mut width, mut p) = (0u32, 0u32, 0i32, src);
    let mut c = 13u64;
    loop {
        c += 4;
        let n = call(m, s, 0x208bc14, &mut [Arg::I(p as i32)]) as u32;
        if n == 0 {
            break;
        }
        c += 2;
        if width >= limit {
            break;
        }
        let first = m.ram.u8(p) as i8;
        let w = match &ink {
            Some(ink) if n == 1 => proportional_advance(m, ink, p),
            _ if n == 3 => 12,
            _ => 6,
        };
        if ink.is_some() && width + w > limit && first != 0xa {
            break;
        }
        c += 8;
        width += w;
        taken += n;
        copy = taken;
        if first == 0xa {
            copy = taken - n;
            p = p.wrapping_add(n);
            let v = m.ram.u8(sl + 0xa);
            m.ram.set_u8(sl + 0xa, v.wrapping_add(1));
            c += 6;
            break;
        }
        p = p.wrapping_add(n);
        c += 1;
    }
    if copy != 0 {
        let at = (sl + 5).wrapping_add(r7 as u32);
        let v = m.ram.u8(at);
        m.ram.set_u8(at, v.wrapping_add(1));
    }
    call(m, s, 0x202caf0, &mut [Arg::I(fp as i32), Arg::I(src as i32), Arg::I(copy as i32)]);
    m.ram.set_u8(fp + copy + 1, 0);
    c += 18;
    if width == limit && m.ram.u8(p) == 0xa {
        let n = call(m, s, 0x208bc14, &mut [Arg::I(p as i32)]) as u32;
        taken += n;
        let v = m.ram.u8(sl + 0xa);
        m.ram.set_u8(sl + 0xa, v.wrapping_add(1));
        c += 6;
    }
    m.spend(c);
    taken as i32
}

/// IE, IF
const REG_IE: u32 = 0x4000210;
const REG_IF: u32 = 0x4000214;

/// 0x1ff8000 MIi_DmaSetParameters(ch, src, dst, ctrl, flags) (ITCM): a DMA channel's source, destination and control
/// written in that order (the control word starts the engine's transfer), between OS_DisableInterrupts and
/// OS_RestoreInterrupts unless flags bit 0. Flags 0x10 / 0x20: the source is a word / halfword of the channel's
/// fill register (0x40000e0 + 4 ch), written first. Flags 2: channel 0 without flags 4 is then cleared (0, 0 and the
/// pool's control word) and the result is DMA 0's source register read; else the restore's result (interrupts were
/// off: 0x80), or ch with flags bit 0. Instructions: 32, 13 for the interrupt pair, 10 for flags 2.
pub fn mi_dma_set_parameters(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (ch, src, dst, ctrl, flags) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32, arg(a, 3) as u32, arg(a, 4) as u32);
    dma_set_parameters(m, ch, src, dst, ctrl, flags)
}
fn dma_set_parameters(m: &mut Machine, ch: u32, mut src: u32, dst: u32, ctrl: u32, flags: u32) -> i32 {
    let reg = 0x40000b0 + 12 * ch;
    let fill = 0x40000e0 + 4 * ch;
    if flags & 0x10 != 0 {
        m.ram.write(fill, src, 4);
        src = fill;
    } else if flags & 0x20 != 0 {
        m.ram.write(fill, src & 0xffff, 2);
        src = fill;
    }
    m.ram.write(reg, src, 4);
    m.ram.write(reg + 4, dst, 4);
    m.ram.write(reg + 8, ctrl, 4);
    let mut c = 32;
    if flags & 1 == 0 {
        c += 13;
    }
    let r = if flags & 2 != 0 {
        c += 10;
        if flags & 4 == 0 && ch == 0 {
            let cnt = m.ram.u32(0x1ff80cc);
            m.ram.write(reg, 0, 4);
            m.ram.write(reg + 4, 0, 4);
            m.ram.write(reg + 8, cnt, 4);
        }
        m.ram.read(m.ram.u32(0x1ff80c8), 4) as i32
    } else if flags & 1 == 0 {
        0x80
    } else {
        ch as i32
    };
    m.spend(c);
    r
}

/// 0x201fd44 OS_ResetRequestIrqMask(mask): IF read and the mask written to it (acknowledged), IME held 0 meanwhile;
/// returns IF as it was. 10 instructions.
pub fn os_reset_request_irq_mask(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let r = reset_request_irq_mask(m, arg(a, 0) as u32);
    m.spend(10);
    r as i32
}
fn reset_request_irq_mask(m: &mut Machine, mask: u32) -> u32 {
    let old = m.ram.read(REG_IF, 4);
    m.ram.write(REG_IF, mask, 4);
    old
}

/// 0x201fce4 OS_EnableIrqMask(mask): IE |= mask, IME held 0 meanwhile; returns IE as it was. 11 instructions.
pub fn os_enable_irq_mask(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let r = enable_irq_mask(m, arg(a, 0) as u32);
    m.spend(11);
    r as i32
}
fn enable_irq_mask(m: &mut Machine, mask: u32) -> u32 {
    let old = m.ram.read(REG_IE, 4);
    m.ram.write(REG_IE, old | mask, 4);
    old
}

/// 0x201fc2c OSi_EnterDmaCallback(dma, callback, arg): the DMA channel's completion callback and argument in the
/// OS's table (pool words), its interrupt enabled, and whether it was enabled before kept. 15 + 11 instructions.
pub fn osi_enter_dma_callback(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    enter_dma_callback(m, arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    m.spend(26);
    0
}
fn enter_dma_callback(m: &mut Machine, dma: u32, cb: u32, cb_arg: u32) {
    let k = 12 * dma;
    let (t_cb, t_arg, t_was) = (m.ram.u32(0x201fc68), m.ram.u32(0x201fc6c), m.ram.u32(0x201fc70));
    m.ram.set_u32(t_cb.wrapping_add(k), cb);
    m.ram.set_u32(t_arg.wrapping_add(k), cb_arg);
    let bit = 1u32.wrapping_shl(dma + 8);
    let old = enable_irq_mask(m, bit);
    m.ram.set_u32(t_was.wrapping_add(k), old & bit);
}

/// 0x201e678 MIi_FIFOCallback: the geometry FIFO interrupt's handler while MI_SendGXCommandAsync sends: the next
/// chunk (at most 0x1d8 bytes) of the command list by DMA into the FIFO (the state at the pool's address: +4 the
/// channel, +8 the source, +0xc the bytes left); for the last chunk the channel's completion callback (pool word) is
/// entered first and its control word asks for the completion interrupt; the FIFO interrupt acknowledged.
/// Instructions: 5 with nothing left; else 81, the last chunk 111 (the calls included).
pub fn mi_fifo_callback(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let st = m.ram.u32(0x201e71c);
    let left = m.ram.u32(st + 0xc);
    if left == 0 {
        m.spend(5);
        return 0;
    }
    let n = left.min(0x1d8);
    let src = m.ram.u32(st + 8);
    m.ram.set_u32(st + 0xc, left - n);
    m.ram.set_u32(st + 8, src.wrapping_add(n));
    let ch = m.ram.u32(st + 4);
    let dst = m.ram.u32(0x201e724);
    let last = left == n;
    let ctrl = if last {
        let cb = m.ram.u32(0x201e720);
        enter_dma_callback(m, ch, cb, 0);
        0xc440_0000 | n >> 2
    } else {
        m.ram.u32(0x201e728) | n >> 2
    };
    dma_set_parameters(m, ch, src, dst, ctrl, 0);
    reset_request_irq_mask(m, 0x200000);
    // dma_set_parameters charged its 45
    m.spend(if last { 66 } else { 36 });
    0
}

/// the OS's interrupt handler tables: 0x201fb98 / 0x201fc24 the handlers by bit, 0x201fb9c / 0x201fc28 the
/// DMA (bits 8-11) and timer (bits 3-6) entries of 12 bytes (handler, enabled flag, argument)
fn irq_entry(k: u32, table: u32, special: u32) -> u32 {
    match k {
        8..=11 => special + (k - 8) * 12,
        3..=6 => special + (k + 1) * 12,
        _ => table + 4 * k,
    }
}

/// 0x201fb18 OS_SetIrqFunction(mask, fn): every bit 0-21 of mask: the plain table's word, or the DMA / timer entry
/// as (fn, 1, 0). Instructions: 10 + 6 a bit clear; a bit set 14 (bits 0-2), 17 (3-6), 18 (7), 15 (8-11), 22 (12-21).
pub fn os_set_irq_function(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (mut mask, f) = (arg(a, 0) as u32, arg(a, 1) as u32);
    let (table, special) = (m.ram.u32(0x201fb98), m.ram.u32(0x201fb9c));
    let mut c = 10u64;
    for k in 0..22 {
        if mask & 1 == 0 {
            c += 6;
        } else {
            let e = irq_entry(k, table, special);
            if matches!(k, 3..=6 | 8..=11) {
                m.ram.set_u32(e, f);
                m.ram.set_u32(e + 4, 1);
                m.ram.set_u32(e + 8, 0);
            } else {
                m.ram.set_u32(e, f);
            }
            c += match k {
                0..=2 => 14,
                3..=6 => 17,
                7 => 18,
                8..=11 => 15,
                _ => 22,
            };
        }
        mask >>= 1;
    }
    m.spend(c);
    0
}

/// 0x201fba0 OS_GetIrqFunction(mask): the handler of the lowest bit 0-21 of mask, 0 without one. Instructions: 2 + 7
/// a bit below it; then 8 (bits 0-2), 14 (3-6), 10 (7), 12 (8-21); 158 without a bit.
pub fn os_get_irq_function(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let mask = arg(a, 0) as u32;
    let k = mask.trailing_zeros();
    if k >= 22 {
        m.spend(158);
        return 0;
    }
    let (table, special) = (m.ram.u32(0x201fc24), m.ram.u32(0x201fc28));
    let f = m.ram.u32(irq_entry(k, table, special));
    m.spend(2 + 7 * k as u64 + match k {
        0..=2 => 8,
        3..=6 => 14,
        7 => 10,
        _ => 12,
    });
    f as i32
}

/// 0x20211e8 DC_FlushRange / 0x20211b0 DC_InvalidateRange(addr, size): data cache maintenance by 32-byte line;
/// the engine has no cache, so only the time passes (5 / 4 instructions a line)
pub fn dc_flush_range(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (p, size) = (arg(a, 0) as u32, arg(a, 1) as u32);
    m.spend(4 + 5 * ((p & 0x1f) + size).div_ceil(32) as u64);
    0
}
pub fn dc_invalidate_range(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (p, size) = (arg(a, 0) as u32, arg(a, 1) as u32);
    m.spend(3 + 4 * ((p & 0x1f) + size).div_ceil(32) as u64);
    0
}

/// 0x202f3fc strncmp(a, b, n): the difference of the first differing bytes, 0 at a NUL or after n. The game's loop
/// runs 9 instructions a matching byte, 5 for a differing one, 2 + 2 on entry and exit
pub fn strncmp(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (mut p, mut q, mut n) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let mut steps = 2u64;
    let r = loop {
        if n == 0 {
            steps += 2;
            break 0;
        }
        let (c2, c1) = (m.ram.r8(q).unwrap_or(0) as i32, m.ram.r8(p).unwrap_or(0) as i32);
        q = q.wrapping_add(1);
        p = p.wrapping_add(1);
        if c1 != c2 {
            steps += 5;
            break c1 - c2;
        }
        if c1 == 0 {
            steps += 9;
            break 0;
        }
        steps += 9;
        n -= 1;
    };
    m.spend(steps);
    r
}

/// 0x20333c8 _s32_div_f(a, b): the quotient in r0 and the remainder in r1, truncating (the remainder takes the
/// dividend's sign); dividing by zero as `s32_div_by_zero` says
pub fn s32_div_f(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (x, y) = (arg(a, 0), arg(a, 1));
    let (q, r) = if y == 0 { s32_div_by_zero(x) } else { (x.wrapping_div(y), x.wrapping_rem(y)) };
    m.ret_hi = Some(r as u32);
    m.spend(S32_DIV_INSTRUCTIONS);
    q
}

/// what the game's _s32_div_f returns for a zero divisor: it divides the magnitudes and gives the signs back at its
/// end, which for a zero divisor leaves the dividend as the quotient and 0 as the remainder (checked in
/// tests/natives_exact.rs)
fn s32_div_by_zero(x: i32) -> (i32, i32) {
    (x, 0)
}
const S32_DIV_INSTRUCTIONS: u64 = 40;

/// 0x2033180 _ll_sdiv(a, b): 64-bit signed division (a in r0:r1, b in r2:r3), the quotient in r0:r1
pub fn ll_sdiv(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let x = (arg(a, 0) as u32 as u64 | (arg(a, 1) as u32 as u64) << 32) as i64;
    let y = (arg(a, 2) as u32 as u64 | (arg(a, 3) as u32 as u64) << 32) as i64;
    let q = if y == 0 { ll_sdiv_by_zero(x) } else { x.wrapping_div(y) };
    m.ret_hi = Some((q as u64 >> 32) as u32);
    m.spend(LL_SDIV_INSTRUCTIONS);
    q as u64 as u32 as i32
}

/// what the game's _ll_sdiv returns for a zero divisor: it returns at once, the dividend still in r0:r1 (checked in
/// tests/natives_exact.rs)
fn ll_sdiv_by_zero(x: i64) -> i64 {
    x
}
const LL_SDIV_INSTRUCTIONS: u64 = 320;

// ---- NitroSystem G3D: the model's render commands (SBC) -------------------------------------------------------------
// The game's NNS G3D drawing as its code does it, register for register: the geometry engine is driven through the
// same command ports, so the matrices it ends with are the game's; checked against the game's function while it runs
// (LP_CHECK_NATIVE, `ext::check_native`).

/// the envelope matrix cache in NNS_G3dRSOnGlb: per joint the clip matrix (4 x 4) and the vector matrix (3 x 3)
const G3D_EVP_CACHE: u32 = 0x212d3a8;
const G3D_EVP_STRIDE: u32 = 0x64;
const GE_FIFO: u32 = 0x4000400;
const GE_MTX_MODE: u32 = 0x4000440;
const GE_MTX_STORE: u32 = 0x400044c;
const GE_MTX_RESTORE: u32 = 0x4000450;
const GE_MTX_IDENTITY: u32 = 0x4000454;

/// a matrix command and its words through the geometry FIFO (G3_MultMtx43 / G3_MultMtx33 / G3_LoadMtx43)
fn ge_matrix(m: &mut Machine, cmd: u32, words: &[u32]) {
    let _ = m.ram.w32(GE_FIFO, cmd);
    for &w in words {
        let _ = m.ram.w32(GE_FIFO, w);
    }
}

/// 0x2004dcc NNSi_G3dFuncSbc_NODEMIX(rs, opt), the envelope cache build: `idxMtxDest, numMtx, (idxStack, idxJnt,
/// ratio) * numMtx`. The weighted sum of each joint's (animation x inverse bind) matrix, the products cached per
/// joint for the frame (bit set in rs->isEvpCached), stored to the matrix stack at idxMtxDest. The vector matrix is
/// loaded from the 9 words of the normal sum and 3 words beyond them, stack garbage in the game (the caller's
/// saved registers); the native sends 0: that row is the vector matrix's translation, which the hardware never uses.
pub fn g3d_sbc_nodemix(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    let rs = arg(a, 0) as u32;
    let c = m.ram.u32(rs);
    let mdl = m.ram.u32(m.ram.u32(rs + 4) + 4);
    let evp = mdl.wrapping_add(m.ram.u32(mdl + 0x10));
    let num = m.ram.u8(c + 2) as u32;
    let mut sum_m = [0i32; 12];
    let mut sum_n = [0i32; 9];
    m.spend(30); // entry, the sum cleared (MI_CpuClearFast of 84 bytes)
    ext::call(m, s, 0x2005d6c, &mut [Arg::I(0)]); // NNS_G3dGeFlushBuffer
    let _ = m.ram.w32(GE_MTX_MODE, 0);
    let _ = m.ram.w32(GE_MTX_STORE, 1);
    let _ = m.ram.w32(GE_MTX_IDENTITY, 0);
    let _ = m.ram.w32(GE_MTX_MODE, 2);
    let (mut w, mut py, mut p) = (0i64, 0u32, c + 3);
    let mut uncached = 0u64;
    let acc = |sum: &mut [i32], w: i64, m: &Machine, base: u32, idx: &[u32]| {
        for (k, &i) in idx.iter().enumerate() {
            sum[k] = sum[k].wrapping_add(((w * m.ram.i32(base + 4 * i) as i64) >> 12) as i32);
        }
    };
    const N9: [u32; 9] = [0, 1, 2, 3, 4, 5, 6, 7, 8];
    const M12: [u32; 12] = [0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14];
    for i in 0..num {
        let jnt = m.ram.u8(p + 1) as u32;
        let bits = rs + 0xcc + (jnt >> 5) * 4;
        let cached = m.ram.u32(bits) & 1 << (jnt & 31) != 0;
        let px = G3D_EVP_CACHE + jnt * G3D_EVP_STRIDE;
        if !cached {
            uncached += 1;
            let v = m.ram.u32(bits) | 1 << (jnt & 31);
            m.ram.set_u32(bits, v);
            let _ = m.ram.w32(GE_MTX_RESTORE, m.ram.u8(p) as u32);
            let _ = m.ram.w32(GE_MTX_MODE, 1);
            let inv_m: Vec<u32> = (0..12).map(|k| m.ram.u32(evp + jnt * 0x54 + 4 * k)).collect();
            ge_matrix(m, 0x19, &inv_m);
        }
        if i != 0 {
            acc(&mut sum_n, w, m, py, &N9);
        }
        if !cached {
            for k in 0..16 {
                let v = m.ram.read(0x4000640 + 4 * k, 4);
                m.ram.set_u32(px + 4 * k, v);
            }
            let _ = m.ram.w32(GE_MTX_MODE, 2);
            let inv_n: Vec<u32> = (0..9).map(|k| m.ram.u32(evp + jnt * 0x54 + 0x30 + 4 * k)).collect();
            ge_matrix(m, 0x1a, &inv_n);
        }
        w = (m.ram.u8(p + 2) as i64) << 4;
        acc(&mut sum_m, w, m, px, &M12);
        p += 3;
        py = px + 0x40;
        if !cached {
            for k in 0..9 {
                let v = m.ram.read(0x4000680 + 4 * k, 4);
                m.ram.set_u32(py + 4 * k, v);
            }
        }
    }
    acc(&mut sum_n, w, m, py, &N9);
    let mut n12: Vec<u32> = sum_n.iter().map(|&v| v as u32).collect();
    n12.extend([0, 0, 0]);
    ge_matrix(m, 0x17, &n12);
    let _ = m.ram.w32(GE_MTX_MODE, 1);
    let mm: Vec<u32> = sum_m.iter().map(|&v| v as u32).collect();
    ge_matrix(m, 0x17, &mm);
    let _ = m.ram.w32(GE_MTX_MODE, 0);
    let _ = m.ram.w32(GE_MTX_RESTORE, 1);
    let _ = m.ram.w32(GE_MTX_MODE, 2);
    let _ = m.ram.w32(GE_MTX_STORE, m.ram.u8(c + 1) as u32);
    m.ram.set_u32(rs, c + 3 + 3 * num);
    // the game's code: 83 + 256 a joint + 93 a joint not cached yet, NNS_G3dGeFlushBuffer's 11 (empty buffer) among them
    m.spend(42 + 256 * num as u64 + 93 * uncached);
    0
}

/// 0x2019a10 MTX_Concat44(a, b, ab): ab = a x b (4 x 4, fx32), each element the 64-bit sum of products >> 12, in the
/// SDK's order of reads and writes (row by row, so ab may be a); into a temporary first when ab is b
pub fn mtx_concat44(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (pa, pb, ab) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    let mut tmp = [0i32; 16];
    let to_tmp = ab == pb;
    for r in 0..4u32 {
        let x: Vec<i64> = (0..4).map(|k| m.ram.i32(pa + 16 * r + 4 * k) as i64).collect();
        // the SDK's element order per row (which of b's columns it keeps in registers)
        let cols: [u32; 4] = match r {
            0 => [0, 1, 3, 2],
            1 => [2, 1, 3, 0],
            2 => [0, 1, 3, 2],
            _ => [2, 1, 0, 3],
        };
        for c in cols {
            let s: i64 = (0..4).map(|k| x[k as usize] * m.ram.i32(pb + 16 * k + 4 * c) as i64).sum();
            let v = (s >> 12) as i32;
            if to_tmp {
                tmp[(4 * r + c) as usize] = v;
            } else {
                m.ram.set_u32(ab + 16 * r + 4 * c, v as u32);
            }
        }
    }
    if to_tmp {
        for (k, v) in tmp.iter().enumerate() {
            m.ram.set_u32(ab + 4 * k as u32, *v as u32);
        }
    }
    // the game's code: 216 instructions, 23 more for the copy out of the temporary
    m.spend(if to_tmp { 239 } else { 216 });
    0
}

// ---- NitroSystem G2D: text into characters (CharCanvas) --------------------------------------------------------------

/// the bit reader (NNSiG2dBitReader: src pointer, availableBits s8 at +4, the byte cache at +5) as the game's code
/// reads it; returns the value and the cost: the game's instructions for this read, the reloads among them
struct BitReader {
    src: u32,
    avail: i32,
    bits: u32,
    cost: u64,
}

impl BitReader {
    /// NNSi_G2dBitReaderRead: nBits (at most 8) from the byte cache, a byte reloaded when it runs short
    fn read(&mut self, m: &Machine, n: i32) -> u32 {
        let mut val = self.bits;
        let avl = self.avail;
        if avl < n {
            let lack = n - avl;
            val = val.wrapping_shl(lack as u32);
            self.bits = m.ram.u8(self.src) as u32;
            self.src = self.src.wrapping_add(1);
            self.avail = 8;
            self.cost += BITREAD_RELOAD;
            val |= self.read(m, lack);
        } else {
            val = val.wrapping_shr((avl - n) as u32);
            self.avail = avl - n;
            self.cost += BITREAD;
        }
        val & 0xff_u32.wrapping_shr((8 - n) as u32)
    }
}
/// the game's instructions for one NNSi_G2dBitReaderRead call (call and return included): 13 with the bits at hand,
/// 24 for one that reloads (the recursive call on top)
const BITREAD: u64 = 13;
const BITREAD_RELOAD: u64 = 24;

/// 0x20135d0 NNSi_G2dBitReaderRead(reader, nBits)
pub fn g2d_bit_reader_read(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = arg(a, 0) as u32;
    let mut r = BitReader { src: m.ram.u32(p), avail: m.ram.u8(p + 4) as i8 as i32, bits: m.ram.u8(p + 5) as u32, cost: 0 };
    let v = r.read(m, arg(a, 1));
    m.ram.set_u32(p, r.src);
    m.ram.set_u8(p + 4, r.avail as u8);
    m.ram.set_u8(p + 5, r.bits as u8);
    m.spend(r.cost);
    v as i32
}

/// 0x2012694 LetterChar(LC_INFO*): one character's part of a glyph drawn into a 4 or 8 bpp character (8 x 8): the
/// glyph's dots from bit offsets, colour cl + dot where the dot is not 0. LC_INFO: dst, src, ofs_x, ofs_y, width,
/// height, dsrc (bits a glyph line), srcBpp, dstBpp, cl.
pub fn g2d_letter_char(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let p = arg(a, 0) as u32;
    let f = |k: u32| m.ram.i32(p + 4 * k);
    let (dst, src, ofs_x, ofs_y, width, height, dsrc, src_bpp, dst_bpp, cl) = (f(0) as u32, f(1) as u32, f(2), f(3), f(4), f(5), f(6), f(7), f(8), f(9) as u32);
    let mut x_st = ofs_x.max(0) as u32;
    let y_st = ofs_y.max(0) as u32;
    let mut x_ed = 8.min(ofs_x + width) as u32;
    let y_ed = 8.min(ofs_y + height) as u32;
    let bit_y_begin = (-ofs_y.min(0)) as u32;
    let mut offset = ((-ofs_x.min(0)) * src_bpp) as u32 + bit_y_begin.wrapping_mul(dsrc as u32);
    x_st *= dst_bpp as u32;
    x_ed *= dst_bpp as u32;
    let (mut rows, mut pixels, mut set, mut reads) = (0u64, 0u64, 0u64, 0u64);
    let row = |m: &mut Machine, offset: u32| -> BitReader {
        let mut r = BitReader { src: src.wrapping_add(offset / 8), avail: 0, bits: 0, cost: 0 };
        r.read(m, (offset % 8) as i32);
        r
    };
    if dst_bpp == 4 {
        for y in y_st..y_ed {
            let pd = dst + 4 * y;
            let mut out = m.ram.u32(pd);
            let mut r = row(m, offset);
            let mut x = x_st;
            while x < x_ed {
                let bits = r.read(m, src_bpp);
                if bits != 0 {
                    out = (out & !(0xf_u32 << x)) | (cl.wrapping_add(bits) << x);
                    set += 1;
                }
                pixels += 1;
                x += 4;
            }
            m.ram.set_u32(pd, out);
            offset = offset.wrapping_add(dsrc as u32);
            rows += 1;
            reads += r.cost;
        }
    } else {
        for y in y_st..y_ed {
            let pd = dst + 8 * y;
            let (mut o0, mut o1) = (m.ram.u32(pd), m.ram.u32(pd + 4));
            let mut r = row(m, offset);
            let mut x = x_st;
            while x < x_ed {
                let bits = r.read(m, src_bpp);
                if bits != 0 {
                    if x < 32 {
                        o0 = (o0 & !(0xff_u32 << x)) | (cl.wrapping_add(bits) << x);
                    } else {
                        o1 = (o1 & !(0xff_u32 << (x - 32))) | (cl.wrapping_add(bits) << (x - 32));
                    }
                    set += 1;
                }
                pixels += 1;
                x += 8;
            }
            m.ram.set_u32(pd, o0);
            m.ram.set_u32(pd + 4, o1);
            offset = offset.wrapping_add(dsrc as u32);
            rows += 1;
            reads += r.cost;
        }
    }
    // the game's code besides the reads: 55 + 25 a line + 11 a dot (4 bpp); 51 + 23 a line + 8 a dot + 10 a dot set (8 bpp)
    m.spend(reads + if dst_bpp == 4 { 55 + 25 * rows + 11 * pixels } else { 51 + 23 * rows + 8 * pixels + 10 * set });
    0
}

// ---- NitroSystem G2D: the OAM manager --------------------------------------------------------------------------------
// NNSG2dOamManagerInstance: type (0 main, 1 sub) at +0; the attribute area (from, to, current: u16) at +4, the affine
// area at +0xa; bFastTransferEnable at +0x14. The buffers: per type 0x540 bytes from the address the code's literal
// pool holds, the OAM image at +0x100 (attributes 8 bytes apart, the affine parameters in their 4th halfwords).

fn oam_buffer(m: &Machine, pman: u32, lit: u32) -> u32 {
    m.ram.u32(pman).wrapping_mul(0x540).wrapping_add(m.ram.u32(lit)).wrapping_add(0x100)
}

/// whether a manage area holds anything (current within to + 1, from not past to)
fn oam_area_valid(m: &Machine, area: u32) -> bool {
    let (from, to, cur) = (m.ram.u16(area) as u32, m.ram.u16(area + 2) as u32, m.ram.u16(area + 4) as u32);
    cur <= to + 1 && from <= to
}

/// a call through the manager's load function table (CpuLoadOAMMain_ / CpuLoadOAMSub_: MI_CpuCopy16 into OAM at
/// the offset), done here when the table holds those, else the game's function
fn oam_load(m: &mut Machine, s: S, func: u32, src: u32, offset: u32, size: u32) {
    let base = match func {
        0x201107c => 0x7000000,
        0x2011068 => 0x7000400,
        _ => {
            ext::call(m, s, func, &mut [Arg::I(src as i32), Arg::I(offset as i32), Arg::I(size as i32)]);
            return;
        }
    };
    ext::call(m, s, 0x201e88c, &mut [Arg::I(src as i32), Arg::I(base + offset as i32), Arg::I(size as i32)]); // MIi_CpuCopy16
    m.spend(if func == 0x201107c { OAM_LOAD_MAIN } else { OAM_LOAD_SUB });
}
/// the load functions' own instructions (the copy charged by MIi_CpuCopy16)
const OAM_LOAD_MAIN: u64 = 3;
const OAM_LOAD_SUB: u64 = 4;

/// 0x201107c CpuLoadOAMMain_(src, offset, size) and 0x2011068 CpuLoadOAMSub_
pub fn g2d_cpu_load_oam_main(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    oam_load(m, s, 0x201107c, arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    0
}
pub fn g2d_cpu_load_oam_sub(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    oam_load(m, s, 0x2011068, arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32);
    0
}

/// 0x2011454 NNS_G2dApplyOamManagerToHW(pMan): the managed attributes (6 bytes each) and, when the affine area
/// holds anything, its parameters (the 4 halfwords) loaded into OAM one by one through the load function; with fast
/// transfer the attribute range flushed and loaded in one go (GX_LoadOAM / GXS_LoadOAM)
pub fn g2d_apply_oam_manager(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    const LIT_BUF: u32 = 0x2011648;
    const LIT_FUNCS: u32 = 0x201164c;
    let p = arg(a, 0) as u32;
    let ty = m.ram.u32(p);
    let buf = oam_buffer(m, p, LIT_BUF);
    let (from, to) = (m.ram.u16(p + 4) as u32, m.ram.u16(p + 6) as u32);
    if m.ram.u32(p + 0x14) != 0 {
        let n = ((to.wrapping_sub(from) + 1) & 0x1fff) << 3;
        let src = buf + from * 8;
        m.spend(if ty == 0 { 29 } else { 31 }); // the game's code
        ext::call(m, s, 0x20211e8, &mut [Arg::I(src as i32), Arg::I(n as i32)]); // DC_FlushRange
        let f = match ty {
            0 => 0x201b63c, // GX_LoadOAM
            1 => 0x201b6a0, // GXS_LoadOAM
            _ => return 0,
        };
        ext::call(m, s, f, &mut [Arg::I(src as i32), Arg::I(((from & 0x1fff) << 3) as i32), Arg::I(n as i32)]);
        return 0;
    }
    let func = m.ram.u32(m.ram.u32(LIT_FUNCS).wrapping_add(4 * ty));
    let n = (to.wrapping_sub(from) + 1) & 0xffff;
    let (mut src, mut off) = (buf + from * 8, (from & 0x1fff) << 3);
    for _ in 0..n {
        oam_load(m, s, func, src, off, 6);
        off = (off + 8) & 0xffff;
        src += 8;
    }
    let mut n_aff = 0;
    if oam_area_valid(m, p + 0xa) {
        let (af, at) = (m.ram.u16(p + 0xa) as u32, m.ram.u16(p + 0xc) as u32);
        let func = m.ram.u32(m.ram.u32(LIT_FUNCS).wrapping_add(4 * ty));
        n_aff = (at.wrapping_sub(af) + 1) & 0xffff;
        let (mut src, mut off) = (buf + af * 0x20, (af & 0x7ff) << 5);
        for _ in 0..n_aff {
            for k in [6, 0xe, 0x16, 0x1e] {
                oam_load(m, s, func, src + k, off + k, 2);
            }
            off = (off + 0x20) & 0xffff;
            src += 0x20;
        }
    }
    // the game's code: 32 + 13 an attribute (31 with none), 23 + 25 an affine parameter set when there are any
    // (3 more for an empty affine area, from = to + 1, with current within to + 1: the way the check is compiled)
    let (af, at, ac) = (m.ram.u16(p + 0xa) as u32, m.ram.u16(p + 0xc) as u32, m.ram.u16(p + 0xe) as u32);
    let empty = af == at + 1 && ac <= at + 1;
    m.spend(32 + 13 * n as u64 + if n_aff > 0 { 23 + 25 * n_aff as u64 } else { 0 } - (n == 0) as u64 + 3 * empty as u64);
    0
}

/// 0x2011650 NNS_G2dResetOamManagerBuffer(pMan): the managed attributes made invisible (attribute 0 = 0xc0) - one
/// halfword each, or with fast transfer the whole range filled with 0xc0 words (DMA, or the CPU when no DMA channel
/// is set); both current indices back to their area's start (the affine one to the affine area's start as the game
/// does: `from` of the affine area)
pub fn g2d_reset_oam_manager(m: &mut Machine, s: S, a: &mut [Arg]) -> i32 {
    const LIT_BUF: u32 = 0x2011788;
    const LIT_DMA: u32 = 0x201178c;
    let p = arg(a, 0) as u32;
    let buf = oam_buffer(m, p, LIT_BUF);
    let (from, to) = (m.ram.u16(p + 4) as u32, m.ram.u16(p + 6) as u32);
    if m.ram.u32(p + 0x14) == 0 {
        let n = (to.wrapping_sub(from) + 1) & 0xffff;
        for i in 0..n {
            m.ram.set_u16(buf + from * 8 + 8 * i, 0xc0);
        }
        m.spend(26 + 6 * n as u64 + (n > 0) as u64); // the game's code
    } else {
        let valid = oam_area_valid(m, p + 4);
        let bytes = if valid { ((to - from + 1) & 0xffff) << 3 } else { 0 };
        let start = buf + from * 8;
        // the game's code: 52, 44 for an area holding nothing (47 when only from is past to)
        let cur = m.ram.u16(p + 8) as u32;
        m.spend(if valid { 52 } else { 44 + 3 * (cur <= to + 1) as u64 });
        ext::call(m, s, 0x20211b0, &mut [Arg::I(start as i32), Arg::I(bytes as i32)]); // DC_InvalidateRange
        let dma = m.ram.u32(m.ram.u32(LIT_DMA));
        if dma > 3 {
            ext::call(m, s, 0x201e8a8, &mut [Arg::I(0xc0), Arg::I(start as i32), Arg::I(bytes as i32)]); // MIi_CpuClear32
        } else {
            ext::call(m, s, 0x201dfd0, &mut [Arg::I(dma as i32), Arg::I(start as i32), Arg::I(0xc0), Arg::I(bytes as i32), Arg::I(1)]); // MIi_DmaFill32
        }
    }
    let af = m.ram.u16(p + 0xa);
    let f = m.ram.u16(p + 4);
    m.ram.set_u16(p + 8, f);
    m.ram.set_u16(p + 0xe, af);
    0
}

/// 0x20112cc NNS_G2dEntryOamManagerOam(pMan, pOam, num): when the attribute area has room for num, the attributes'
/// 3 halfwords copied in at the current index, which advances; TRUE / FALSE
pub fn g2d_entry_oam_manager_oam(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    const LIT_BUF: u32 = 0x2011394;
    let (p, mut src, num) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32 & 0xffff);
    let (to, cur) = (m.ram.u16(p + 6) as u32, m.ram.u16(p + 8) as u32);
    let room = if oam_area_valid(m, p + 4) { (to + 1).wrapping_sub(cur) & 0xffff } else { 0 };
    if room < num {
        m.spend(OAM_ENTRY_FULL);
        return 0;
    }
    let dst = oam_buffer(m, p, LIT_BUF) + cur * 8;
    for i in 0..num {
        for k in [0, 2, 4] {
            let v = m.ram.u16(src + k);
            m.ram.set_u16(dst + 8 * i + k, v);
        }
        let c = m.ram.u16(p + 8).wrapping_add(1);
        m.ram.set_u16(p + 8, c);
        src += 6 + 2; // GXOamAttr: 8 bytes
    }
    m.spend(OAM_ENTRY + OAM_ENTRY_EACH * num as u64);
    1
}
/// the game's instructions: 23 when full, else 33 + 15 an attribute
const OAM_ENTRY_FULL: u64 = 23;
const OAM_ENTRY: u64 = 33;
const OAM_ENTRY_EACH: u64 = 15;

/// 0x2011398 NNS_G2dEntryOamManagerAffine(pMan, mtx): with room in the affine area, the 2 x 2 matrix (fx32 >> 4)
/// into the parameters at the current index, which advances; the index, or 0xfffe (none)
pub fn g2d_entry_oam_manager_affine(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    const LIT_NONE: u32 = 0x201144c;
    const LIT_BUF: u32 = 0x2011450;
    let (p, mtx) = (arg(a, 0) as u32, arg(a, 1) as u32);
    let (to, cur) = (m.ram.u16(p + 0xc) as u32, m.ram.u16(p + 0xe) as u32);
    let room = if oam_area_valid(m, p + 0xa) { (to + 1).wrapping_sub(cur) & 0xffff } else { 0 };
    if room == 0 {
        // 3 more for an empty area (from = to + 1)
        let empty = m.ram.u16(p + 0xa) as u32 == to + 1;
        m.spend(OAM_AFFINE_FULL + 3 * empty as u64);
        return (m.ram.u32(LIT_NONE) & 0xffff) as i32;
    }
    let dst = oam_buffer(m, p, LIT_BUF) + cur * 0x20;
    for (k, o) in [6u32, 0xe, 0x16, 0x1e].iter().enumerate() {
        let v = m.ram.i32(mtx + 4 * k as u32) >> 4;
        m.ram.set_u16(dst + o, v as u16);
    }
    m.ram.set_u16(p + 0xe, (cur + 1) as u16);
    m.spend(OAM_AFFINE);
    cur as i32
}
/// the game's instructions: 19 when full, else 45
const OAM_AFFINE_FULL: u64 = 19;
const OAM_AFFINE: u64 = 45;

// ---- the game's sprite layer over G2D --------------------------------------------------------------------------------

/// 0x204948c Sprite_CopyCellOamsWithFlip(oams, pos, max, cell, mtx, trans, affineIdx, flags): a cell's OAM
/// attributes (count u16 at +0, 6-byte entries from the pointer at +4, at most `max`) into `oams` (8 bytes apart),
/// each with its position in fx32 into `pos` (x, y): flipped (flags bit 0 horizontally, bit 1 vertically), through
/// the 2 x 2 matrix `mtx` as an affine OBJ (double size centred, the affine index set, the flip bits kept when
/// flipped) and moved by `trans`; with neither matrix nor move kept as they are, otherwise kept only when on screen
/// (256 x 192) with the position written back. Returns how many were kept.
pub fn sprite_copy_cell_oams(m: &mut Machine, _: S, a: &mut [Arg]) -> i32 {
    let (oams, pos, max, cell) = (arg(a, 0) as u32, arg(a, 1) as u32, arg(a, 2) as u32, arg(a, 3) as u32);
    let (mtx, trans, aff_idx, flags) = (arg(a, 4) as u32, arg(a, 5) as u32, arg(a, 6) as u32 & 0xffff, arg(a, 7) as u32);
    let lit = |k: u32| m.ram.u32(0x204997c + 4 * k);
    let (size_mask, tbl_w, tbl_h, x_mask, affine_mask, pos_mask) = (lit(0), lit(1), lit(2), lit(3), lit(4), lit(5));
    let n = (m.ram.u16(cell) as u32).min(max);
    let src = m.ram.u32(cell + 4);
    let (flip_h, flip_v) = (flags & 1 != 0, flags & 2 != 0);
    // OBJ width and height (pixels) by shape and size
    let dims = |m: &Machine, attr: u32| -> (u32, u32) {
        let shape = (attr & size_mask & 0xc000) >> 14;
        let size = ((attr & size_mask) >> 30) * 2;
        (m.ram.u16(tbl_w + size + shape * 8) as u32, m.ram.u16(tbl_h + size + shape * 8) as u32)
    };
    // the game's instructions: 28, 4 more with any entries, 34 an entry; one moved or transformed: 73 kept, 48 / 50 /
    // 53 / 55 dropped by the 1st / 2nd / 3rd / 4th screen test, 14 the move, 117 the matrix (136 double size) + 10 flipped
    let mut cost = 28 + 4 * (n > 0) as u64 + 34 * n as u64;
    let mut kept = 0u32;
    for i in 0..n {
        let d = oams + kept * 8;
        let p = pos + kept * 8;
        for k in [0, 2, 4] {
            let v = m.ram.u16(src + i * 6 + k);
            m.ram.set_u16(d + k, v);
        }
        if mtx == 0 && trans == 0 {
            kept += 1;
            continue;
        }
        let attr = m.ram.u32(d);
        let (w, h) = dims(m, attr);
        let (w, h) = ((w << 12) as i32, (h << 12) as i32);
        let mut x = ((((attr & x_mask) as i32 >> 16) as u32 | if (attr & x_mask) >> 16 > 0xff { 0xffff_ff00 } else { 0 }) << 12) as i32;
        let y = attr & 0xff;
        let mut y = ((y | if y > 0x7f { 0xffff_ff00 } else { 0 }) << 12) as i32;
        if flip_h {
            x = x.wrapping_add(w).wrapping_neg();
        }
        if flip_v {
            y = y.wrapping_add(h).wrapping_neg();
        }
        let (mut px, mut py) = (x, y);
        if mtx != 0 {
            let mm: Vec<i64> = (0..4).map(|k| m.ram.i32(mtx + 4 * k) as i64).collect();
            let orig = m.ram.u32(d);
            let mut t = orig & 0x300;
            if t != 0x100 && t != 0x300 {
                t = orig & 0x3000_0300;
            }
            cost += if t == 0x300 { 136 } else { 117 } + 10 * (flip_h || flip_v) as u64;
            if t == 0x300 {
                let (dw, dh) = dims(m, orig);
                px = px.wrapping_add((dw * 0x800) as i32);
                py = py.wrapping_add((dh * 0x800) as i32);
            }
            let l6 = px as i64 * mm[0] + mm[2] * py as i64 + 0x1000;
            let l7 = px as i64 * mm[1] + mm[3] * py as i64 + 0x1000;
            px = (l6 >> 12) as i32;
            py = (l7 >> 12) as i32;
            let mut v = orig & affine_mask | 0x100 | aff_idx << 25;
            if flip_h || flip_v {
                v = v & 0xcfff_ffff | orig & 0x3000_0000;
            }
            m.ram.set_u32(d, v);
            let (w2, h2) = dims(m, v);
            let (w2, h2) = ((w2 >> 1) as i32, (h2 >> 1) as i32);
            let mi: Vec<i32> = mm.iter().map(|&v| v as i32).collect();
            px = px.wrapping_add(mi[2].wrapping_mul(h2)).wrapping_add(mi[0].wrapping_mul(w2)).wrapping_add(w2.wrapping_mul(-0x1000));
            py = py.wrapping_add(mi[3].wrapping_mul(h2)).wrapping_add(mi[1].wrapping_mul(w2)).wrapping_add(h2.wrapping_mul(-0x1000));
        }
        if trans != 0 {
            cost += 14;
            let (tx, ty) = (m.ram.i32(trans), m.ram.i32(trans + 4));
            px = px.wrapping_add(tx);
            py = py.wrapping_add(ty);
            x = x.wrapping_add(tx);
            y = y.wrapping_add(ty);
        }
        m.ram.set_u32(p, px as u32);
        m.ram.set_u32(p + 4, py as u32);
        let stage = if x.wrapping_add(w) < 0 { 1 } else if x > 0x100000 { 2 } else if y.wrapping_add(h) < 0 { 3 } else if y > 0xc0000 { 4 } else { 0 };
        cost += [73, 48, 50, 53, 55][stage];
        if stage == 0 {
            let v = m.ram.u32(d) & pos_mask | ((py.wrapping_add(0x800) >> 12) as u32 & 0xff) | ((px.wrapping_add(0x800) >> 12) as u32 & 0x1ff) << 16;
            m.ram.set_u32(d, v);
            kept += 1;
        }
    }
    m.spend(cost);
    kept as i32
}

/// 0x20485d0 Sprite_BuildPriorityLists(): the two lists of 64 priorities (heads; 3D sprites, flag bit 0 at +0x24, in
/// the second) cleared, then each of the 256 sprite slots (0x38 bytes) that is active (+0x1b) and not hidden (bit 1
/// at +0x24) pushed onto the head of its priority's list (+0x19, at most 63), linked through +0x28
pub fn sprite_build_priority_lists(m: &mut Machine, _: S, _: &mut [Arg]) -> i32 {
    let (list2d, list3d) = (m.ram.u32(0x2048668), m.ram.u32(0x204866c));
    for k in 0..64 {
        m.ram.set_u32(list2d + 4 * k, 0);
        m.ram.set_u32(list3d + 4 * k, 0);
    }
    let mut slot = m.ram.u32(m.ram.u32(0x2048670));
    let mut cost = 0u64;
    for _ in 0..256 {
        let active = m.ram.u8(slot + 0x1b) != 0;
        let fl = m.ram.u8(slot + 0x24);
        if active && fl & 2 == 0 {
            let list = if fl & 1 == 0 { list2d } else { list3d };
            let pri = (m.ram.u8(slot + 0x19) as u32).min(0x3f);
            let head = m.ram.u32(list + 4 * pri);
            m.ram.set_u32(list + 4 * pri, slot);
            m.ram.set_u32(slot + 0x28, head);
            cost += 1;
        } else if active {
            cost += 1 << 16;
        }
        slot = slot.wrapping_add(0x38);
    }
    // the game's instructions: 2124, 14 a sprite listed, 3 an active one hidden
    m.spend(2124 + 14 * (cost & 0xffff) + 3 * (cost >> 16));
    0
}

/// 0x20490e0 Sprite_DrawAll(): the priority lists built; the 2D sprites from priority 63 down, each active one's
/// animation updated and submitted to the OAM manager; the 3D sprites likewise between a matrix push and pop
/// (Sprite_Setup3DOrtho first), each drawn; then every active slot's animation time advanced. The list and the
/// links are read afresh as the game does (the calls may change them).
pub fn sprite_draw_all(m: &mut Machine, s: S, _: &mut [Arg]) -> i32 {
    let lit = |k: u32| m.ram.u32(0x20491c0 + 4 * k);
    let (list2d, push, list3d, pop, slots) = (lit(0), lit(1), lit(2), lit(3), lit(4));
    let mut f = [0u64; 5];
    ext::call(m, s, 0x20485d0, &mut []); // Sprite_BuildPriorityLists
    for pri in (0..64).rev() {
        let mut p = m.ram.u32(list2d + 4 * pri);
        while p != 0 {
            f[0] += 1;
            if m.ram.u8(p + 0x1b) != 0 {
                f[1] += 1;
                ext::call(m, s, 0x204933c, &mut [Arg::I(p as i32)]); // Sprite_UpdateAnim
                ext::call(m, s, 0x2048c34, &mut [Arg::I(p as i32)]); // Sprite_SubmitToOamManager
            }
            p = m.ram.u32(p + 0x28);
        }
    }
    ext::call(m, s, 0x2049064, &mut []); // Sprite_Setup3DOrtho
    let _ = m.ram.w32(push, 0);
    for pri in (0..64).rev() {
        let mut p = m.ram.u32(list3d + 4 * pri);
        while p != 0 {
            f[2] += 1;
            if m.ram.u8(p + 0x1b) != 0 {
                f[3] += 1;
                ext::call(m, s, 0x204933c, &mut [Arg::I(p as i32)]); // Sprite_UpdateAnim
                ext::call(m, s, 0x2048cec, &mut [Arg::I(p as i32)]); // Sprite_Draw3D
            }
            p = m.ram.u32(p + 0x28);
        }
    }
    let _ = m.ram.w32(pop, 1);
    let mut slot = m.ram.u32(slots);
    for _ in 0..256 {
        if m.ram.u8(slot + 0x1b) != 0 {
            f[4] += 1;
            ext::call(m, s, 0x2049308, &mut [Arg::I(slot as i32)]); // Sprite_AdvanceAnimTime
        }
        slot = slot.wrapping_add(0x38);
    }
    // the game's own instructions: 2449, 5 a sprite in a list, 5 more an active one, 2 an active slot
    m.spend(2449 + 5 * (f[0] + f[1] + f[2] + f[3]) + 2 * f[4]);
    0
}
