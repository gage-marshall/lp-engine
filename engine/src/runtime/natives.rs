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
    let global = m.ram.i32(m.lit(0x206fc78)); // CharId_GetGlobalValue 0x206fc6c
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
