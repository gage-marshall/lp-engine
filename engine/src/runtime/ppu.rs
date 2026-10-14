//! The DS 2D engines as the game programs them: each engine's picture (256 x 192) composed from its registers
//! (engine A 0x4000000, B 0x4001000), the banked VRAM (`vram`), palette RAM and OAM, by the hardware's rules:
//! text / affine / extended BGs (affine tile maps, 256-colour and direct-colour bitmaps), tiled and bitmap OBJs with
//! affine transforms and 1D / 2D mapping, extended palettes, priorities, windows, the colour effects (alpha,
//! brighten, darken, semi-transparent OBJs) and master brightness. Engine A's BG0 can be the 3D layer: the caller
//! supplies that picture (the engine's own 3D renderer), or it stays transparent.
use super::ram::Ram;
use super::vram::Place;

pub const W: usize = 256;
pub const H: usize = 192;

/// a pixel candidate: 15-bit colour (bit 15: opaque), layer (0..3 BG, 4 OBJ, 5 backdrop), priority
#[derive(Clone, Copy)]
struct Px {
    c: u16,
    layer: u8,
    prio: u8,
    /// 1: OBJ semi-transparent (BLDALPHA); 2..17: OBJ bitmap alpha 0..15 + 2 (EVA = alpha + 1); 0: none
    alpha: u8,
}

struct Regs<'a> {
    ram: &'a Ram,
    io: u32,
}

impl Regs<'_> {
    fn r16(&self, o: u32) -> u32 {
        self.ram.u16(self.io + o) as u32
    }
    fn r32(&self, o: u32) -> u32 {
        self.ram.u32(self.io + o)
    }
}

/// the picture of one engine (0: A, 1: B), RGBA8, with its master brightness applied
pub fn render(ram: &Ram, engine: u32, layer3d: Option<&[u16]>) -> Vec<u8> {
    let g = Regs { ram, io: if engine == 0 { 0x4000000 } else { 0x4001000 } };
    let disp = g.r32(0);
    let mut out = vec![0u8; W * H * 4];
    let mode_display = (disp >> 16) & 3;
    if mode_display == 0 || disp & 0x80 != 0 {
        // display off / forced blank: white
        for p in out.chunks_mut(4) {
            p.copy_from_slice(&[255, 255, 255, 255]);
        }
        return out;
    }
    if engine == 0 && mode_display == 2 {
        // VRAM display: the LCDC bank picked by bits 18..19, direct colour
        let bank = ((disp >> 18) & 3) as usize;
        for i in 0..W * H {
            let c = ram.vram().banks[bank][2 * i] as u16 | (ram.vram().banks[bank][2 * i + 1] as u16) << 8;
            put(&mut out, i, c);
        }
        return brightness(&g, out);
    }
    for (i, c) in graphics(ram, engine, layer3d).into_iter().enumerate() {
        put(&mut out, i, c);
    }
    brightness(&g, out)
}

/// display capture (DISPCAPCNT 0x4000064), done once per frame when enabled: source A (engine A's graphics, or
/// its 3D picture alone), source B (a VRAM bank, at the read offset), or the two blended by EVA / EVB, written as
/// opaque RGB555 into the destination bank at the write offset and size; the enable bit then clears
pub fn capture(ram: &mut Ram, layer3d: Option<&[u16]>) {
    let cnt = ram.u32(0x4000064);
    if cnt & 0x8000_0000 == 0 {
        return;
    }
    let (eva, evb) = ((cnt & 0x1f).min(16), ((cnt >> 8) & 0x1f).min(16));
    let dst_bank = ((cnt >> 16) & 3) as usize;
    let dst_off = ((cnt >> 18) & 3) as usize * 0x8000;
    let (cw, ch) = [(128, 128), (256, 64), (256, 128), (256, 192)][((cnt >> 20) & 3) as usize];
    let src_a: Vec<u16> = if cnt & (1 << 24) != 0 {
        layer3d.map(|l| l.to_vec()).unwrap_or_else(|| vec![0; W * H])
    } else {
        graphics(ram, 0, layer3d).into_iter().map(|c| c | 0x8000).collect()
    };
    let disp = ram.u32(0x4000000);
    let src_bank = ((disp >> 18) & 3) as usize;
    let src_off = ((cnt >> 26) & 3) as usize * 0x8000;
    let select = (cnt >> 29) & 3;
    for y in 0..ch {
        for x in 0..cw {
            let a = src_a[y * W + x];
            let bi = (src_off + 2 * (y * W + x)) % 0x20000;
            let bk = &ram.vram().banks[src_bank];
            let b = bk[bi] as u16 | (bk[bi + 1] as u16) << 8;
            let c = match select {
                0 => a,
                1 => b,
                _ => {
                    let (aa, ba) = ((a >> 15) as u32, (b >> 15) as u32);
                    let ch = |s: u32| ((((a as u32 >> s) & 0x1f) * aa * eva + ((b as u32 >> s) & 0x1f) * ba * evb) / 16).min(31);
                    (ch(0) | ch(5) << 5 | ch(10) << 10) as u16 | if aa | ba != 0 { 0x8000 } else { 0 }
                }
            };
            let o = (dst_off + 2 * (y * cw + x)) % 0x20000;
            let bank = &mut ram.vram.banks[dst_bank];
            bank[o] = c as u8;
            bank[o + 1] = (c >> 8) as u8;
        }
    }
    let v = cnt & 0x7fff_ffff;
    ram.write(0x4000064, v, 4);
}

/// an engine's graphics picture (BGs, 3D, OBJs, effects) as RGB555, before master brightness
pub fn graphics(ram: &Ram, engine: u32, layer3d: Option<&[u16]>) -> Vec<u16> {
    let g = Regs { ram, io: if engine == 0 { 0x4000000 } else { 0x4001000 } };
    let disp = g.r32(0);
    let mut out = vec![0u16; W * H];
    let pal_base = if engine == 0 { 0x5000000 } else { 0x5000400 };
    let backdrop = ram.u16(pal_base) & 0x7fff;
    let bgmode = disp & 7;
    let bldcnt = g.r16(0x50);
    let bldalpha = g.r16(0x52);
    let bldy = (g.r16(0x54) & 0x1f).min(16);
    let objs = if disp & 0x1000 != 0 { render_objs(&g, engine, disp) } else { vec![None; W * H] };
    let wins = windows(&g, disp, &objs);
    for y in 0..H {
        let bg_lines: Vec<Vec<Option<Px>>> = (0..4)
            .map(|b| {
                if disp & (0x100 << b) == 0 {
                    return vec![None; W];
                }
                if engine == 0 && b == 0 && disp & 8 != 0 {
                    let prio = (g.r16(8) & 3) as u8;
                    return (0..W)
                        .map(|x| layer3d.and_then(|l| {
                            let c = l[y * W + x];
                            (c & 0x8000 != 0).then_some(Px { c, layer: 0, prio, alpha: 0 })
                        }))
                        .collect();
                }
                bg_line(&g, engine, disp, bgmode, b, y)
            })
            .collect();
        for x in 0..W {
            let win = wins.as_ref().map_or(0x3f, |w| w[y * W + x]);
            // the two topmost visible candidates
            let mut cands: Vec<Px> = Vec::with_capacity(5);
            if let Some(o) = objs[y * W + x] {
                if win & 0x10 != 0 {
                    cands.push(o);
                }
            }
            for (b, l) in bg_lines.iter().enumerate() {
                if let Some(p) = l[x] {
                    if win & (1 << b) != 0 {
                        cands.push(p);
                    }
                }
            }
            // OBJ in front of a BG of the same priority; lower BG number in front of higher
            cands.sort_by_key(|p| (p.prio, if p.layer == 4 { 0 } else { p.layer + 1 }));
            let bd = Px { c: backdrop | 0x8000, layer: 5, prio: 4, alpha: 0 };
            cands.push(bd);
            let top = cands[0];
            let second = cands.get(1).copied().unwrap_or(bd);
            let effects = win & 0x20 != 0;
            let first_target = |p: &Px| bldcnt & (1 << p.layer) != 0;
            let second_target = |p: &Px| bldcnt & (0x100 << p.layer) != 0;
            let c = if top.layer == 4 && top.alpha != 0 && second_target(&second) {
                // semi-transparent / bitmap OBJ: alpha blend regardless of the effect mode
                let (ea, eb) = if top.alpha > 1 { ((top.alpha - 1) as u32, 16 - (top.alpha - 1) as u32) } else { (bldalpha & 0x1f, (bldalpha >> 8) & 0x1f) };
                blend(top.c, second.c, ea, eb)
            } else if effects && first_target(&top) {
                match (bldcnt >> 6) & 3 {
                    1 if second_target(&second) => blend(top.c, second.c, bldalpha & 0x1f, (bldalpha >> 8) & 0x1f),
                    2 => fade(top.c, bldy, true),
                    3 => fade(top.c, bldy, false),
                    _ => top.c,
                }
            } else {
                top.c
            };
            out[y * W + x] = c & 0x7fff;
        }
    }
    out
}

fn put(out: &mut [u8], i: usize, c: u16) {
    let f = |v: u16| ((v & 0x1f) as u32 * 255 / 31) as u8;
    out[4 * i..4 * i + 4].copy_from_slice(&[f(c), f(c >> 5), f(c >> 10), 255]);
}

fn blend(a: u16, b: u16, ea: u32, eb: u32) -> u16 {
    let (ea, eb) = (ea.min(16), eb.min(16));
    let ch = |s: u32| (((a as u32 >> s) & 0x1f) * ea + ((b as u32 >> s) & 0x1f) * eb).div_euclid(16).min(31);
    (ch(0) | ch(5) << 5 | ch(10) << 10) as u16
}

fn fade(a: u16, y: u32, up: bool) -> u16 {
    let ch = |s: u32| {
        let v = (a as u32 >> s) & 0x1f;
        if up { v + (31 - v) * y / 16 } else { v - v * y / 16 }
    };
    (ch(0) | ch(5) << 5 | ch(10) << 10) as u16
}

/// master brightness (0x6c): mode 1 brighter, 2 darker, factor 0..16
fn brightness(g: &Regs, mut out: Vec<u8>) -> Vec<u8> {
    let v = g.r16(0x6c);
    let (mode, f) = (v >> 14, (v & 0x1f).min(16));
    if mode == 0 || mode == 3 || f == 0 {
        return out;
    }
    for p in out.chunks_mut(4) {
        for c in &mut p[..3] {
            let x = *c as u32;
            *c = if mode == 1 { x + (255 - x) * f / 16 } else { x - x * f / 16 } as u8;
        }
    }
    out
}

fn vram_bg(g: &Regs, engine: u32, off: u32) -> u8 {
    g.ram.u8(if engine == 0 { 0x6000000 } else { 0x6200000 } + off)
}

fn pal(g: &Regs, engine: u32, obj: bool, idx: u32) -> u16 {
    let base = if engine == 0 { 0x5000000 } else { 0x5000400 } + if obj { 0x200 } else { 0 };
    g.ram.u16(base + 2 * idx) & 0x7fff
}

/// an extended palette entry: BG slot (0..3) or OBJ, 256 x 16 colours
fn ext_pal(g: &Regs, engine: u32, obj: bool, slot: u32, idx: u32) -> u16 {
    let kind = match (engine, obj) {
        (0, false) => Place::BgExtA(0),
        (0, true) => Place::ObjExtA,
        (_, false) => Place::BgExtB,
        (_, true) => Place::ObjExtB,
    };
    let o = (slot * 0x2000 + idx * 2) as usize;
    let v = g.ram.vram();
    (v.slot(kind, o) as u16 | (v.slot(kind, o + 1) as u16) << 8) & 0x7fff
}

/// one line of a BG
fn bg_line(g: &Regs, engine: u32, disp: u32, mode: u32, b: u32, y: usize) -> Vec<Option<Px>> {
    let cnt = g.r16(8 + 2 * b);
    let prio = (cnt & 3) as u8;
    let affine = matches!((mode, b), (1, 3) | (2, 2) | (2, 3) | (4, 2));
    let extended = matches!((mode, b), (3, 3) | (4, 3) | (5, 2) | (5, 3));
    if (affine || extended) && b >= 2 {
        return affine_line(g, engine, disp, cnt, b, y, extended, prio);
    }
    let char_base = ((cnt >> 2) & 0xf) * 0x4000 + if engine == 0 { ((disp >> 24) & 7) * 0x10000 } else { 0 };
    let scr_base = ((cnt >> 8) & 0x1f) * 0x800 + if engine == 0 { ((disp >> 27) & 7) * 0x10000 } else { 0 };
    let size = (cnt >> 14) & 3;
    let (wtiles, htiles) = [(32, 32), (64, 32), (32, 64), (64, 64)][size as usize];
    let hofs = g.r16(0x10 + 4 * b) & 0x1ff;
    let vofs = g.r16(0x12 + 4 * b) & 0x1ff;
    let c256 = cnt & 0x80 != 0;
    let ext = disp & (1 << 30) != 0;
    let ext_slot = if b < 2 && cnt & 0x2000 != 0 { b + 2 } else { b };
    let py = (y as u32 + vofs) % (htiles * 8);
    (0..W)
        .map(|x| {
            let px = (x as u32 + hofs) % (wtiles * 8);
            let (tx, ty) = (px / 8, py / 8);
            let block = (tx / 32) + (ty / 32) * if wtiles == 64 { 2 } else { 1 };
            let se_addr = scr_base + block * 0x800 + ((ty % 32) * 32 + tx % 32) * 2;
            let se = vram_bg(g, engine, se_addr) as u32 | (vram_bg(g, engine, se_addr + 1) as u32) << 8;
            let tile = se & 0x3ff;
            let (mut fx, mut fy) = (px % 8, py % 8);
            if se & 0x400 != 0 {
                fx = 7 - fx;
            }
            if se & 0x800 != 0 {
                fy = 7 - fy;
            }
            let palno = se >> 12;
            let (ci, c) = if c256 {
                let ci = vram_bg(g, engine, char_base + tile * 64 + fy * 8 + fx) as u32;
                (ci, if ext { ext_pal(g, engine, false, ext_slot, palno * 256 + ci) } else { pal(g, engine, false, ci) })
            } else {
                let byte = vram_bg(g, engine, char_base + tile * 32 + fy * 4 + fx / 2) as u32;
                let ci = if fx & 1 != 0 { byte >> 4 } else { byte & 0xf };
                (ci, pal(g, engine, false, palno * 16 + ci))
            };
            (ci != 0).then_some(Px { c: c | 0x8000, layer: b as u8, prio, alpha: 0 })
        })
        .collect()
}

/// one line of an affine / extended BG (BG2 / BG3): the reference point advances by (PB, PD) per line; the
/// engine's line counter is the line number (the registers are not latched per frame here)
fn affine_line(g: &Regs, engine: u32, disp: u32, cnt: u32, b: u32, y: usize, extended: bool, prio: u8) -> Vec<Option<Px>> {
    let o = 0x20 + (b - 2) * 0x10;
    let pa = g.r16(o) as i16 as i32;
    let pb = g.r16(o + 2) as i16 as i32;
    let pc = g.r16(o + 4) as i16 as i32;
    let pd = g.r16(o + 6) as i16 as i32;
    let rx = ((g.r32(o + 8) << 4) as i32) >> 4;
    let ry = ((g.r32(o + 12) << 4) as i32) >> 4;
    let (sx, sy) = (rx + pb * y as i32, ry + pd * y as i32);
    let size = (cnt >> 14) & 3;
    let wrap = cnt & 0x2000 != 0;
    let char_base = ((cnt >> 2) & 0xf) * 0x4000 + if engine == 0 { ((disp >> 24) & 7) * 0x10000 } else { 0 };
    let scr_base = ((cnt >> 8) & 0x1f) * 0x800 + if engine == 0 { ((disp >> 27) & 7) * 0x10000 } else { 0 };
    let bitmap = extended && cnt & 0x80 != 0;
    let (bw, bh) = if bitmap {
        [(128, 128), (256, 256), (512, 256), (512, 512)][size as usize]
    } else if extended {
        [(128, 128), (256, 256), (512, 512), (1024, 1024)][size as usize]
    } else {
        [(128, 128), (256, 256), (512, 512), (1024, 1024)][size as usize]
    };
    let ext = disp & (1 << 30) != 0;
    (0..W)
        .map(|x| {
            let (tx, ty) = ((sx + pa * x as i32) >> 8, (sy + pc * x as i32) >> 8);
            let (mut tx, mut ty) = (tx, ty);
            if wrap {
                tx = tx.rem_euclid(bw);
                ty = ty.rem_euclid(bh);
            } else if tx < 0 || ty < 0 || tx >= bw || ty >= bh {
                return None;
            }
            let (tx, ty) = (tx as u32, ty as u32);
            let base_bitmap = ((cnt >> 8) & 0x1f) * 0x4000;
            if bitmap {
                if cnt & 4 != 0 {
                    // direct colour
                    let a = base_bitmap + (ty * bw as u32 + tx) * 2;
                    let c = vram_bg(g, engine, a) as u16 | (vram_bg(g, engine, a + 1) as u16) << 8;
                    return (c & 0x8000 != 0).then_some(Px { c, layer: b as u8, prio, alpha: 0 });
                }
                let ci = vram_bg(g, engine, base_bitmap + ty * bw as u32 + tx) as u32;
                return (ci != 0).then_some(Px { c: pal(g, engine, false, ci) | 0x8000, layer: b as u8, prio, alpha: 0 });
            }
            let tiles_w = bw as u32 / 8;
            if extended {
                // 16-bit map entries, 256-colour tiles
                let a = scr_base + ((ty / 8) * tiles_w + tx / 8) * 2;
                let se = vram_bg(g, engine, a) as u32 | (vram_bg(g, engine, a + 1) as u32) << 8;
                let (mut fx, mut fy) = (tx % 8, ty % 8);
                if se & 0x400 != 0 {
                    fx = 7 - fx;
                }
                if se & 0x800 != 0 {
                    fy = 7 - fy;
                }
                let ci = vram_bg(g, engine, char_base + (se & 0x3ff) * 64 + fy * 8 + fx) as u32;
                if ci == 0 {
                    return None;
                }
                let c = if ext { ext_pal(g, engine, false, b, (se >> 12) * 256 + ci) } else { pal(g, engine, false, ci) };
                return Some(Px { c: c | 0x8000, layer: b as u8, prio, alpha: 0 });
            }
            let tile = vram_bg(g, engine, scr_base + (ty / 8) * tiles_w + tx / 8) as u32;
            let ci = vram_bg(g, engine, char_base + tile * 64 + (ty % 8) * 8 + tx % 8) as u32;
            (ci != 0).then_some(Px { c: pal(g, engine, false, ci) | 0x8000, layer: b as u8, prio, alpha: 0 })
        })
        .collect()
}

/// all OBJs into one buffer (the first in OAM order wins among equal priorities)
fn render_objs(g: &Regs, engine: u32, disp: u32) -> Vec<Option<Px>> {
    let mut buf: Vec<Option<Px>> = vec![None; W * H];
    let oam = if engine == 0 { 0x7000000 } else { 0x7000400 };
    let obj_vram = if engine == 0 { 0x6400000 } else { 0x6600000 };
    let ext = disp & (1 << 31) != 0;
    for i in (0..128u32).rev() {
        let a0 = g.ram.u16(oam + 8 * i) as u32;
        let a1 = g.ram.u16(oam + 8 * i + 2) as u32;
        let a2 = g.ram.u16(oam + 8 * i + 4) as u32;
        let rot = a0 & 0x100 != 0;
        if !rot && a0 & 0x200 != 0 {
            continue; // disabled
        }
        let mode = (a0 >> 10) & 3;
        if mode == 2 {
            continue; // OBJ window (applied in `windows`)
        }
        let shape = (a0 >> 14) & 3;
        let size = (a1 >> 14) & 3;
        if shape == 3 {
            continue;
        }
        let (w, h) = [[(8, 8), (16, 16), (32, 32), (64, 64)], [(16, 8), (32, 8), (32, 16), (64, 32)], [(8, 16), (8, 32), (16, 32), (32, 64)]][shape as usize][size as usize];
        let double = rot && a0 & 0x200 != 0;
        let (bw, bh) = if double { (2 * w, 2 * h) } else { (w, h) };
        let mut oy = (a0 & 0xff) as i32;
        if oy + bh as i32 > 256 {
            oy -= 256;
        }
        let mut ox = (a1 & 0x1ff) as i32;
        if ox >= 256 {
            ox -= 512;
        }
        let prio = ((a2 >> 10) & 3) as u8;
        let palno = a2 >> 12;
        let tile = a2 & 0x3ff;
        let c256 = a0 & 0x2000 != 0;
        let (pa, pb, pc, pd) = if rot {
            let grp = (a1 >> 9) & 0x1f;
            let p = |k: u32| g.ram.u16(oam + grp * 32 + 6 + 8 * k) as i16 as i32;
            (p(0), p(1), p(2), p(3))
        } else {
            (0x100, 0, 0, 0x100)
        };
        let (hflip, vflip) = (!rot && a1 & 0x1000 != 0, !rot && a1 & 0x2000 != 0);
        for sy in 0..bh as i32 {
            let y = oy + sy;
            if !(0..H as i32).contains(&y) {
                continue;
            }
            for sx in 0..bw as i32 {
                let x = ox + sx;
                if !(0..W as i32).contains(&x) {
                    continue;
                }
                let (cx, cy) = (sx - bw as i32 / 2, sy - bh as i32 / 2);
                let (mut tx, mut ty) = if rot {
                    ((pa * cx + pb * cy) >> 8, (pc * cx + pd * cy) >> 8)
                } else {
                    (cx, cy)
                };
                tx += w as i32 / 2;
                ty += h as i32 / 2;
                if tx < 0 || ty < 0 || tx >= w as i32 || ty >= h as i32 {
                    continue;
                }
                if hflip {
                    tx = w as i32 - 1 - tx;
                }
                if vflip {
                    ty = h as i32 - 1 - ty;
                }
                let (tx, ty) = (tx as u32, ty as u32);
                let px = if mode == 3 {
                    // bitmap OBJ: direct colour, alpha from the palette field (0: transparent; 1..15 blends with
                    // EVA = alpha + 1, so 15 is solid)
                    let a = if disp & 0x40 != 0 {
                        tile * (128 << ((disp >> 22) & 1)) + (ty * w + tx) * 2
                    } else {
                        let mask = if disp & 0x20 != 0 { 0x1f } else { 0xf };
                        ((tile & mask) * 0x10 + (tile & !mask) * 0x80) + (ty * if disp & 0x20 != 0 { 256 } else { 128 } + tx) * 2
                    };
                    let c = g.ram.u16(obj_vram + a);
                    if c & 0x8000 == 0 || palno == 0 {
                        continue;
                    }
                    Px { c, layer: 4, prio, alpha: palno as u8 + 2 }
                } else {
                    let one_d = disp & 0x10 != 0;
                    let boundary = if one_d { 32 << ((disp >> 20) & 3) } else { 32 };
                    let (ttx, tty) = (tx / 8, ty / 8);
                    let ci = if c256 {
                        let base = if one_d { tile * boundary + (tty * (w / 8) + ttx) * 64 } else { (tile & !1) * 32 + (tty * 16 + ttx) * 64 };
                        g.ram.u8(obj_vram + base + (ty % 8) * 8 + tx % 8) as u32
                    } else {
                        let base = if one_d { tile * boundary + (tty * (w / 8) + ttx) * 32 } else { tile * 32 + (tty * 32 + ttx) * 32 };
                        let b = g.ram.u8(obj_vram + base + (ty % 8) * 4 + (tx % 8) / 2) as u32;
                        if tx & 1 != 0 { b >> 4 } else { b & 0xf }
                    };
                    if ci == 0 {
                        continue;
                    }
                    let c = if c256 {
                        if ext { ext_pal(g, engine, true, 0, palno * 256 + ci) } else { pal(g, engine, true, ci) }
                    } else {
                        pal(g, engine, true, palno * 16 + ci)
                    };
                    Px { c: c | 0x8000, layer: 4, prio, alpha: (mode == 1) as u8 }
                };
                let i = y as usize * W + x as usize;
                if buf[i].map_or(true, |o| px.prio <= o.prio) {
                    buf[i] = Some(px);
                }
            }
        }
    }
    buf
}

/// the window mask per pixel (bit n: BG n, 4 OBJ, 5 effects); None when no window is enabled
fn windows(g: &Regs, disp: u32, _objs: &[Option<Px>]) -> Option<Vec<u8>> {
    if disp & 0xe000 == 0 {
        return None;
    }
    let winin = g.r16(0x48);
    let winout = g.r16(0x4a);
    let mut m = vec![(winout & 0x3f) as u8; W * H];
    for w in (0..2).rev() {
        if disp & (0x2000 << w) == 0 {
            continue;
        }
        let h = g.r16(0x40 + 2 * w);
        let v = g.r16(0x44 + 2 * w);
        let (x1, x2, y1, y2) = (h >> 8, h & 0xff, v >> 8, v & 0xff);
        let inside = |a: u32, lo: u32, hi: u32| if lo <= hi { a >= lo && a < hi } else { a >= lo || a < hi };
        let bits = ((winin >> (8 * w)) & 0x3f) as u8;
        for y in 0..H as u32 {
            if !inside(y, y1, y2) {
                continue;
            }
            for x in 0..W as u32 {
                if inside(x, x1, x2) {
                    m[(y * W as u32 + x) as usize] = bits;
                }
            }
        }
    }
    Some(m)
}

/// which engine is on the top screen (POWCNT 0x4000304 bit 15: 1 = engine A on top)
pub fn a_on_top(ram: &Ram) -> bool {
    ram.u16(0x4000304) & 0x8000 != 0
}
