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
    /// where the colour came from, for drawing above the DS resolution (`Hi`)
    src: Src,
}

/// a pixel's source when it can be drawn finer than the DS resolution: a VRAM texel shown 1:1 on the screen
/// (a bitmap OBJ without affine transform, an unscaled direct-colour BG) at CPU address `addr`
#[derive(Clone, Copy, PartialEq)]
enum Src {
    None,
    Texel { addr: u32, hflip: bool, vflip: bool },
    /// a text BG's pixel at (x, y) of its plane (`plane`), finer when the plane has an enhanced version
    Plane { x: u16, y: u16 },
}

/// drawing `s` times the DS resolution: the 3D layer at that resolution, and finer versions of pictures the game
/// captured into VRAM (`Shadow`), used where the game shows those pictures 1:1. Everything else is scaled up.
pub struct Hi<'a> {
    pub s: usize,
    pub layer3d: Option<&'a [u16]>,
    /// the 3D layer's drawn share per pixel (0..=16; None: all drawn pixels fully): partly drawn pixels are blended
    /// with what lies behind them
    pub cov3d: Option<&'a [u8]>,
    pub shadows: &'a [&'a Shadow],
}

/// a finer version of a captured picture: the capture's bank, its pixels as the game has them (byte offset,
/// RGB555, row by row, `cw` wide) and the picture at `s` x `s` pixels per pixel
pub struct Shadow {
    pub bank: usize,
    pub px: Vec<(usize, u16)>,
    pub cw: usize,
    /// the picture (empty when it is kept on the GPU, in `slot`)
    pub hi: Vec<u16>,
    pub slot: u32,
}

impl Shadow {
    /// still what the bank holds (the game has not drawn over it)
    pub fn valid(&self, ram: &Ram) -> bool {
        let b = &ram.vram().banks[self.bank];
        self.px.iter().all(|&(o, c)| b[o] == c as u8 && b[o + 1] == (c >> 8) as u8)
    }
    /// the pixel (x, y) of the picture at byte `o` of `bank`, when it is one
    fn place(&self, bank: usize, o: usize) -> Option<(usize, usize)> {
        if bank != self.bank || self.px.is_empty() {
            return None;
        }
        let i = o.wrapping_sub(self.px[0].0) % 0x20000 / 2;
        if o & 1 != 0 || i >= self.px.len() {
            return None;
        }
        Some((i % self.cw, i / self.cw))
    }
    fn at(&self, bank: usize, o: usize, s: usize, sx: usize, sy: usize) -> Option<u16> {
        let (x, y) = self.place(bank, o)?;
        Some(self.hi[(y * s + sy) * self.cw * s + x * s + sx])
    }
}

impl Hi<'_> {
    fn shadow(&self, bank: usize, o: usize, sx: usize, sy: usize) -> Option<u16> {
        self.shadows.iter().find_map(|sh| sh.at(bank, o, self.s, sx, sy))
    }
    /// the finer picture holding the texel at CPU address `addr`, and the texel's place in it
    fn find(&self, ram: &Ram, addr: u32) -> Option<(&Shadow, usize, usize)> {
        ram.vram().hits(addr).find_map(|(b, o)| self.shadows.iter().find_map(|sh| sh.place(b, o).map(|(x, y)| (*sh, x, y))))
    }
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

/// the picture of one engine at `hi.s` times the DS resolution, RGBA8 (`render` above it)
pub fn render_hi(ram: &Ram, engine: u32, hi: &Hi) -> Vec<u8> {
    let g = Regs { ram, io: if engine == 0 { 0x4000000 } else { 0x4001000 } };
    let disp = g.r32(0);
    let s = hi.s;
    let (ow, oh) = (W * s, H * s);
    let mut out = vec![0u8; ow * oh * 4];
    let mode_display = (disp >> 16) & 3;
    if mode_display == 0 || disp & 0x80 != 0 {
        out.fill(255);
        return out;
    }
    if engine == 0 && mode_display == 2 {
        let bank = ((disp >> 18) & 3) as usize;
        let b = &ram.vram().banks[bank];
        for y in 0..oh {
            for x in 0..ow {
                let o = 2 * ((y / s) * W + x / s);
                let c = hi.shadow(bank, o, x % s, y % s).unwrap_or(b[o] as u16 | (b[o + 1] as u16) << 8);
                put(&mut out, y * ow + x, c);
            }
        }
        return brightness(&g, out);
    }
    for (i, c) in graphics_hi(ram, engine, hi).into_iter().enumerate() {
        put(&mut out, i, c);
    }
    brightness(&g, out)
}

/// what a capture with control word `cnt` writes, at `hi.s` times the DS resolution: the picture (`cw * s` wide)
pub fn capture_image_hi(ram: &Ram, cnt: u32, hi: &Hi) -> Vec<u16> {
    let s = hi.s;
    let (eva, evb) = ((cnt & 0x1f).min(16), ((cnt >> 8) & 0x1f).min(16));
    let (cw, ch) = [(128, 128), (256, 64), (256, 128), (256, 192)][((cnt >> 20) & 3) as usize];
    let src_a: Vec<u16> = if cnt & (1 << 24) != 0 {
        hi.layer3d.map(|l| l.to_vec()).unwrap_or_else(|| vec![0; W * H * s * s])
    } else {
        graphics_hi(ram, 0, hi).into_iter().map(|c| c | 0x8000).collect()
    };
    let disp = ram.u32(0x4000000);
    let src_bank = ((disp >> 18) & 3) as usize;
    let src_off = ((cnt >> 26) & 3) as usize * 0x8000;
    let select = (cnt >> 29) & 3;
    let bk = &ram.vram().banks[src_bank];
    let mut out = Vec::with_capacity(cw * ch * s * s);
    for y in 0..ch * s {
        for x in 0..cw * s {
            let a = src_a[y * W * s + x];
            let bi = (src_off + 2 * ((y / s) * W + x / s)) % 0x20000;
            let b = hi.shadow(src_bank, bi, x % s, y % s).unwrap_or(bk[bi] as u16 | (bk[bi + 1] as u16) << 8);
            out.push(match select {
                0 => a,
                1 => b,
                _ => {
                    let (aa, ba) = ((a >> 15) as u32, (b >> 15) as u32);
                    let ch = |sh: u32| ((((a as u32 >> sh) & 0x1f) * aa * eva + ((b as u32 >> sh) & 0x1f) * ba * evb) / 16).min(31);
                    (ch(0) | ch(5) << 5 | ch(10) << 10) as u16 | if aa | ba != 0 { 0x8000 } else { 0 }
                }
            });
        }
    }
    out
}

/// display capture (DISPCAPCNT 0x4000064), done once per frame when enabled: source A (engine A's graphics, or
/// its 3D picture alone), source B (a VRAM bank, at the read offset), or the two blended by EVA / EVB, written as
/// opaque RGB555 into the destination bank at the write offset and size; the enable bit then clears
pub fn capture(ram: &mut Ram, layer3d: Option<&[u16]>) {
    let cnt = ram.u32(0x4000064);
    if cnt & 0x8000_0000 == 0 {
        return;
    }
    let (bank, px) = capture_image(ram, cnt, layer3d);
    for (o, c) in px {
        let b = &mut ram.vram.banks[bank];
        b[o] = c as u8;
        b[o + 1] = (c >> 8) as u8;
    }
    let v = cnt & 0x7fff_ffff;
    ram.write(0x4000064, v, 4);
}

/// what a capture with control word `cnt` writes: the destination bank and (byte offset, RGB555) per pixel
pub fn capture_image(ram: &Ram, cnt: u32, layer3d: Option<&[u16]>) -> (usize, Vec<(usize, u16)>) {
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
    let mut out = Vec::with_capacity(cw * ch);
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
            out.push(((dst_off + 2 * (y * cw + x)) % 0x20000, c));
        }
    }
    (dst_bank, out)
}

/// an engine's graphics picture (BGs, 3D, OBJs, effects) as RGB555, before master brightness
pub fn graphics(ram: &Ram, engine: u32, layer3d: Option<&[u16]>) -> Vec<u16> {
    graphics_at(ram, engine, layer3d, None)
}

/// an engine's graphics picture at `hi.s` times the DS resolution, RGB555
pub fn graphics_hi(ram: &Ram, engine: u32, hi: &Hi) -> Vec<u16> {
    graphics_at(ram, engine, None, Some(hi))
}

/// the colour effects' registers, for `compose`
struct Fx {
    backdrop: u16,
    bldcnt: u32,
    bldalpha: u32,
    bldy: u32,
}

/// a pixel from its visible candidates (in layer order, any order of priority) and its window bits
fn compose(cands: &mut Vec<Px>, win: u8, fx: &Fx) -> u16 {
    // OBJ in front of a BG of the same priority; lower BG number in front of higher
    cands.sort_by_key(|p| (p.prio, if p.layer == 4 { 0 } else { p.layer + 1 }));
    let bd = Px { c: fx.backdrop | 0x8000, layer: 5, prio: 4, alpha: 0, src: Src::None };
    let top = cands.first().copied().unwrap_or(bd);
    let second = cands.get(1).copied().unwrap_or(bd);
    let (bldcnt, bldalpha) = (fx.bldcnt, fx.bldalpha);
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
            2 => fade(top.c, fx.bldy, true),
            3 => fade(top.c, fx.bldy, false),
            _ => top.c,
        }
    } else {
        top.c
    };
    c & 0x7fff
}

/// what an engine's picture is made of, read once: registers, OBJs, windows (`cands` gives a pixel's candidates)
struct Scan<'a> {
    g: Regs<'a>,
    engine: u32,
    disp: u32,
    fx: Fx,
    objs: Vec<Option<Px>>,
    wins: Option<Vec<u8>>,
    /// BG0 is the 3D layer
    bg0_3d: bool,
    bg0_prio: u8,
}

impl<'a> Scan<'a> {
    fn new(ram: &'a Ram, engine: u32) -> Scan<'a> {
        let g = Regs { ram, io: if engine == 0 { 0x4000000 } else { 0x4001000 } };
        let disp = g.r32(0);
        let pal_base = if engine == 0 { 0x5000000 } else { 0x5000400 };
        let fx = Fx { backdrop: ram.u16(pal_base) & 0x7fff, bldcnt: g.r16(0x50), bldalpha: g.r16(0x52), bldy: (g.r16(0x54) & 0x1f).min(16) };
        let objs = if disp & 0x1000 != 0 { render_objs(&g, engine, disp) } else { vec![None; W * H] };
        let wins = windows(&g, disp, &objs);
        let bg0_3d = engine == 0 && disp & 0x100 != 0 && disp & 8 != 0;
        let bg0_prio = (g.r16(8) & 3) as u8;
        Scan { g, engine, disp, fx, objs, wins, bg0_3d, bg0_prio }
    }

    /// the BG lines of row `y`; BG0 as the 3D layer comes from `layer3d` (None: left out, for drawing per sub-pixel)
    fn lines(&self, y: usize, layer3d: Option<&[u16]>) -> Vec<Vec<Option<Px>>> {
        (0..4)
            .map(|b| {
                if self.disp & (0x100 << b) == 0 {
                    return vec![None; W];
                }
                if b == 0 && self.bg0_3d {
                    return (0..W)
                        .map(|x| layer3d.and_then(|l| {
                            let c = l[y * W + x];
                            (c & 0x8000 != 0).then_some(Px { c, layer: 0, prio: self.bg0_prio, alpha: 0, src: Src::None })
                        }))
                        .collect();
                }
                bg_line(&self.g, self.engine, self.disp, self.disp & 7, b, y)
            })
            .collect()
    }

    /// pixel (x, y)'s visible candidates (in layer order) into `out`, and its window bits
    fn cands(&self, lines: &[Vec<Option<Px>>], x: usize, y: usize, out: &mut Vec<Px>) -> u8 {
        let win = self.wins.as_ref().map_or(0x3f, |w| w[y * W + x]);
        out.clear();
        if let Some(o) = self.objs[y * W + x] {
            if win & 0x10 != 0 {
                out.push(o);
            }
        }
        for (b, l) in lines.iter().enumerate() {
            if let Some(p) = l[x] {
                if win & (1 << b) != 0 {
                    out.push(p);
                }
            }
        }
        win
    }
}

/// the picture at the DS resolution (`hi` None: the 3D layer from `layer3d`) or above it (`hi`: each pixel whose
/// candidates include the 3D layer or a texel with a finer version is composed per sub-pixel; the rest is repeated)
fn graphics_at(ram: &Ram, engine: u32, layer3d: Option<&[u16]>, hi: Option<&Hi>) -> Vec<u16> {
    let sc = Scan::new(ram, engine);
    let s = hi.map_or(1, |h| h.s);
    let (ow, oh) = (W * s, H * s);
    let mut out = vec![0u16; ow * oh];
    let (fx, bg0_3d, bg0_prio) = (&sc.fx, sc.bg0_3d, sc.bg0_prio);
    use rayon::prelude::*;
    out.par_chunks_mut(ow * s).enumerate().for_each(|(y, rows)| {
        let mut cands: Vec<Px> = Vec::with_capacity(6);
        let mut sub: Vec<Px> = Vec::with_capacity(6);
        let lines = sc.lines(y, if hi.is_some() { None } else { layer3d });
        for x in 0..W {
            let win = sc.cands(&lines, x, y, &mut cands);
            let Some(h) = hi else {
                rows[x] = compose(&mut cands, win, fx);
                continue;
            };
            let l3 = h.layer3d.filter(|_| bg0_3d && win & 1 != 0);
            let l3_at = |sx: usize, sy: usize| l3.map_or(0, |l| l[(y * s + sy) * ow + x * s + sx]);
            let fine = cands.iter().any(|p| matches!(p.src, Src::Texel { .. })) || (0..s * s).any(|k| l3_at(k % s, k / s) & 0x8000 != 0);
            if !fine {
                let c = compose(&mut cands, win, fx);
                for sy in 0..s {
                    rows[sy * ow + x * s..sy * ow + x * s + s].fill(c);
                }
                continue;
            }
            // the finer pictures behind this pixel's texel candidates, found once for its sub-pixels
            let found: Vec<Option<(&Shadow, usize, usize, bool, bool)>> = cands
                .iter()
                .map(|p| match p.src {
                    Src::Texel { addr, hflip, vflip } => h.find(ram, addr).map(|(sh, tx, ty)| (sh, tx, ty, hflip, vflip)),
                    Src::None | Src::Plane { .. } => None,
                })
                .collect();
            for sy in 0..s {
                for sx in 0..s {
                    sub.clear();
                    for (p, f) in cands.iter().zip(&found) {
                        let c = match f {
                            Some((sh, tx, ty, hflip, vflip)) => {
                                let (fx, fy) = (if *hflip { s - 1 - sx } else { sx }, if *vflip { s - 1 - sy } else { sy });
                                sh.hi[(ty * s + fy) * sh.cw * s + tx * s + fx]
                            }
                            None => p.c,
                        };
                        sub.push(Px { c, ..*p });
                    }
                    let c3 = l3_at(sx, sy);
                    let cov = if c3 & 0x8000 != 0 { h.cov3d.map_or(16, |c| c[(y * s + sy) * ow + x * s + sx] as u32) } else { 0 };
                    let behind = if cov > 0 && cov < 16 { Some(compose(&mut sub.clone(), win, fx)) } else { None };
                    if c3 & 0x8000 != 0 {
                        sub.push(Px { c: c3, layer: 0, prio: bg0_prio, alpha: 0, src: Src::None });
                    }
                    let c = compose(&mut sub, win, fx);
                    rows[sy * ow + x * s + sx] = match behind {
                        Some(b) => blend(c, b, cov, 16 - cov),
                        None => c,
                    };
                }
            }
        }
    });
    out
}

/// how the GPU compositor (`gpu2d`) is to show an engine
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Show {
    /// display off / forced blank: white
    White,
    /// the layers, per pixel as in `records`
    Layers,
}

/// one DS pixel for the GPU compositor: its two front-most candidates (without the 3D layer, which the GPU adds
/// per sub-pixel) and where their colours have finer versions.
/// Words 0, 1 (candidates): bit 31 present, 0..14 colour, 15..17 layer, 18..19 priority, 20..24 OBJ alpha (`Px`).
/// Word 0 also holds the pixel's window bits: 25 effects, 26 the 3D layer shows here.
/// Words 2, 3 (their sources): bit 31 a finer picture, 27..30 its slot, 26 / 25 h / v flip, 0..8 x, 9..17 y in it
/// (a capture's pixel, or a text BG's plane position when `planes` gives the slot of that BG's finer plane).
/// Also the engine's effect registers (backdrop, BLDCNT, BLDALPHA, BLDY), BG0's priority and master brightness.
/// `capture`: the engine's graphics as a display capture takes them (whatever the display mode), else as displayed.
pub fn records(ram: &Ram, engine: u32, shadows: &[&Shadow], capture: bool, planes: &[Option<u32>; 4]) -> (Show, Vec<[u32; 4]>, [u32; 6]) {
    let sc = Scan::new(ram, engine);
    let regs = [sc.fx.backdrop as u32, sc.fx.bldcnt, sc.fx.bldalpha, sc.fx.bldy, sc.bg0_prio as u32, sc.g.r16(0x6c)];
    let mode_display = (sc.disp >> 16) & 3;
    if !capture && (mode_display == 0 || sc.disp & 0x80 != 0) {
        return (Show::White, Vec::new(), regs);
    }
    let cand = |p: &Px| 1 << 31 | (p.c & 0x7fff) as u32 | (p.layer as u32) << 15 | (p.prio as u32) << 18 | (p.alpha as u32) << 20;
    let place = |slot: u32, x: usize, y: usize, hflip: bool, vflip: bool| 1 << 31 | slot << 27 | (hflip as u32) << 26 | (vflip as u32) << 25 | x as u32 | (y as u32) << 9;
    let mut out = vec![[0u32; 4]; W * H];
    if !capture && engine == 0 && mode_display == 2 {
        // VRAM display: the bank's pixels, no effects
        let bank = ((sc.disp >> 18) & 3) as usize;
        let b = &ram.vram().banks[bank];
        for (i, r) in out.iter_mut().enumerate() {
            let c = b[2 * i] as u16 | (b[2 * i + 1] as u16) << 8;
            r[0] = cand(&Px { c, layer: 0, prio: 0, alpha: 0, src: Src::None });
            if let Some((sh, (x, y))) = shadows.iter().find_map(|sh| sh.place(bank, 2 * i).map(|p| (*sh, p))) {
                r[2] = place(sh.slot, x, y, false, false);
            }
        }
        return (Show::Layers, out, [regs[0], 0, 0, 0, 0, regs[5]]);
    }
    let hi = Hi { s: 1, layer3d: None, cov3d: None, shadows };
    use rayon::prelude::*;
    out.par_chunks_mut(W).enumerate().for_each(|(y, row)| {
        let mut cands: Vec<Px> = Vec::with_capacity(6);
        let lines = sc.lines(y, None);
        for x in 0..W {
            let win = sc.cands(&lines, x, y, &mut cands);
            cands.sort_by_key(|p| (p.prio, if p.layer == 4 { 0 } else { p.layer + 1 }));
            let r = &mut row[x];
            for (k, p) in cands.iter().take(2).enumerate() {
                r[k] = cand(p);
                match p.src {
                    Src::Texel { addr, hflip, vflip } => {
                        if let Some((sh, tx, ty)) = hi.find(ram, addr) {
                            r[2 + k] = place(sh.slot, tx, ty, hflip, vflip);
                        }
                    }
                    Src::Plane { x, y } => {
                        if let Some(slot) = planes[p.layer as usize & 3] {
                            r[2 + k] = place(slot, x as usize, y as usize, false, false);
                        }
                    }
                    Src::None => {}
                }
            }
            r[0] |= ((win >> 5) as u32 & 1) << 25 | ((sc.bg0_3d && win & 1 != 0) as u32) << 26;
        }
    });
    (Show::Layers, out, regs)
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

/// a text BG's tile layout from its BGCNT: char base, screen base, size in tiles, 256 colours, the extended
/// palette slot
struct Text {
    char_base: u32,
    scr_base: u32,
    wtiles: u32,
    htiles: u32,
    c256: bool,
    ext: bool,
    ext_slot: u32,
}

impl Text {
    fn new(g: &Regs, engine: u32, disp: u32, b: u32) -> Text {
        let cnt = g.r16(8 + 2 * b);
        let size = (cnt >> 14) & 3;
        let (wtiles, htiles) = [(32, 32), (64, 32), (32, 64), (64, 64)][size as usize];
        Text {
            char_base: ((cnt >> 2) & 0xf) * 0x4000 + if engine == 0 { ((disp >> 24) & 7) * 0x10000 } else { 0 },
            scr_base: ((cnt >> 8) & 0x1f) * 0x800 + if engine == 0 { ((disp >> 27) & 7) * 0x10000 } else { 0 },
            wtiles,
            htiles,
            c256: cnt & 0x80 != 0,
            ext: disp & (1 << 30) != 0,
            ext_slot: if b < 2 && cnt & 0x2000 != 0 { b + 2 } else { b },
        }
    }

    /// the colour at (px, py) of the BG's plane (RGB555), None where transparent
    fn at(&self, g: &Regs, engine: u32, px: u32, py: u32) -> Option<u16> {
        self.at_with(&|o| vram_bg(g, engine, o), &|i| pal(g, engine, false, i), &|slot, i| ext_pal(g, engine, false, slot, i), px, py)
    }

    /// `at` with the BG VRAM, the BG palette and the extended palettes read through the given functions
    fn at_with(&self, bg: &dyn Fn(u32) -> u8, pal: &dyn Fn(u32) -> u16, ext_pal: &dyn Fn(u32, u32) -> u16, px: u32, py: u32) -> Option<u16> {
        let (tx, ty) = (px / 8, py / 8);
        let block = (tx / 32) + (ty / 32) * if self.wtiles == 64 { 2 } else { 1 };
        let se_addr = self.scr_base + block * 0x800 + ((ty % 32) * 32 + tx % 32) * 2;
        let se = bg(se_addr) as u32 | (bg(se_addr + 1) as u32) << 8;
        let tile = se & 0x3ff;
        let (mut fx, mut fy) = (px % 8, py % 8);
        if se & 0x400 != 0 {
            fx = 7 - fx;
        }
        if se & 0x800 != 0 {
            fy = 7 - fy;
        }
        let palno = se >> 12;
        let (ci, c) = if self.c256 {
            let ci = bg(self.char_base + tile * 64 + fy * 8 + fx) as u32;
            (ci, if self.ext { ext_pal(self.ext_slot, palno * 256 + ci) } else { pal(ci) })
        } else {
            let byte = bg(self.char_base + tile * 32 + fy * 4 + fx / 2) as u32;
            let ci = if fx & 1 != 0 { byte >> 4 } else { byte & 0xf };
            (ci, pal(palno * 16 + ci))
        };
        (ci != 0).then_some(c)
    }
}

/// a text BG of an engine as a whole picture (its plane, without scrolling): width, height and RGB555 with bit 15
/// where drawn; None when the BG is off or not a text BG
pub fn plane(ram: &Ram, engine: u32, b: u32) -> Option<(usize, usize, Vec<u16>)> {
    let g = Regs { ram, io: if engine == 0 { 0x4000000 } else { 0x4001000 } };
    let disp = g.r32(0);
    let mode = disp & 7;
    if disp & (0x100 << b) == 0 || (engine == 0 && b == 0 && disp & 8 != 0) || !is_text(mode, b) {
        return None;
    }
    let t = Text::new(&g, engine, disp, b);
    let (w, h) = (t.wtiles as usize * 8, t.htiles as usize * 8);
    // the BG VRAM, the palette and the extended palettes copied once (reading them byte by byte is slow)
    let vram = ram.vram().window_copy(if engine == 0 { 0x6000000 } else { 0x6200000 }, if engine == 0 { 0x80000 } else { 0x20000 });
    let pals: Vec<u16> = (0..256).map(|i| pal(&g, engine, false, i)).collect();
    let ext: Vec<u16> = if t.c256 && t.ext { (0..256 * 16).map(|i| ext_pal(&g, engine, false, t.ext_slot, i)).collect() } else { Vec::new() };
    let bg = |o: u32| vram.get(o as usize).copied().unwrap_or(0);
    let pl = |i: u32| pals[i as usize & 255];
    let ex = |_: u32, i: u32| ext.get(i as usize).copied().unwrap_or(0);
    use rayon::prelude::*;
    let mut px = vec![0u16; w * h];
    px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            if let Some(c) = t.at_with(&bg, &pl, &ex, x as u32, y as u32) {
                *p = c | 0x8000;
            }
        }
    });
    Some((w, h, px))
}

/// what BG `b` of an engine is, for diagnostics: off, the 3D layer, a text BG (size), affine, or extended (tiles,
/// 256-colour or direct-colour bitmap; size; scaled or not)
pub fn bg_kind(ram: &Ram, engine: u32, b: u32) -> String {
    let g = Regs { ram, io: if engine == 0 { 0x4000000 } else { 0x4001000 } };
    let disp = g.r32(0);
    let mode = disp & 7;
    let cnt = g.r16(8 + 2 * b);
    if disp & (0x100 << b) == 0 {
        return "off".into();
    }
    if engine == 0 && b == 0 && disp & 8 != 0 {
        return "3D".into();
    }
    let size = (cnt >> 14) & 3;
    if is_text(mode, b) {
        let (w, h) = [(256, 256), (512, 256), (256, 512), (512, 512)][size as usize];
        return format!("text {w}x{h}, {}", if cnt & 0x80 != 0 { "256 colours" } else { "16 colours" });
    }
    let o = 0x20 + (b - 2) * 0x10;
    let scaled = !(g.r16(o) == 0x100 && g.r16(o + 2) == 0 && g.r16(o + 4) == 0 && g.r16(o + 6) == 0x100);
    let extended = matches!((mode, b), (3, 3) | (4, 3) | (5, 2) | (5, 3));
    let kind = if !extended {
        "affine tiles".to_string()
    } else if cnt & 0x80 == 0 {
        "extended tiles".to_string()
    } else {
        let (w, h) = [(128, 128), (256, 256), (512, 256), (512, 512)][size as usize];
        format!("{} bitmap {w}x{h}", if cnt & 4 != 0 { "direct-colour" } else { "256-colour" })
    };
    format!("{kind}{}", if scaled { ", scaled" } else { "" })
}

/// BG `b` is a text BG in BG mode `mode`
fn is_text(mode: u32, b: u32) -> bool {
    let affine = matches!((mode, b), (1, 3) | (2, 2) | (2, 3) | (4, 2));
    let extended = matches!((mode, b), (3, 3) | (4, 3) | (5, 2) | (5, 3));
    !((affine || extended) && b >= 2)
}

/// one line of a BG
fn bg_line(g: &Regs, engine: u32, disp: u32, mode: u32, b: u32, y: usize) -> Vec<Option<Px>> {
    let cnt = g.r16(8 + 2 * b);
    let prio = (cnt & 3) as u8;
    if !is_text(mode, b) {
        let extended = matches!((mode, b), (3, 3) | (4, 3) | (5, 2) | (5, 3));
        return affine_line(g, engine, disp, cnt, b, y, extended, prio);
    }
    let t = Text::new(g, engine, disp, b);
    let hofs = g.r16(0x10 + 4 * b) & 0x1ff;
    let vofs = g.r16(0x12 + 4 * b) & 0x1ff;
    let py = (y as u32 + vofs) % (t.htiles * 8);
    (0..W)
        .map(|x| {
            let px = (x as u32 + hofs) % (t.wtiles * 8);
            t.at(g, engine, px, py).map(|c| Px { c: c | 0x8000, layer: b as u8, prio, alpha: 0, src: Src::Plane { x: px as u16, y: py as u16 } })
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
    let unscaled = pa == 0x100 && pb == 0 && pc == 0 && pd == 0x100;
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
                    let src = if unscaled { Src::Texel { addr: if engine == 0 { 0x6000000 } else { 0x6200000 } + a, hflip: false, vflip: false } } else { Src::None };
                    return (c & 0x8000 != 0).then_some(Px { c, layer: b as u8, prio, alpha: 0, src });
                }
                let ci = vram_bg(g, engine, base_bitmap + ty * bw as u32 + tx) as u32;
                return (ci != 0).then_some(Px { c: pal(g, engine, false, ci) | 0x8000, layer: b as u8, prio, alpha: 0, src: Src::None });
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
                return Some(Px { c: c | 0x8000, layer: b as u8, prio, alpha: 0, src: Src::None });
            }
            let tile = vram_bg(g, engine, scr_base + (ty / 8) * tiles_w + tx / 8) as u32;
            let ci = vram_bg(g, engine, char_base + tile * 64 + (ty % 8) * 8 + tx % 8) as u32;
            (ci != 0).then_some(Px { c: pal(g, engine, false, ci) | 0x8000, layer: b as u8, prio, alpha: 0, src: Src::None })
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
                    let src = if rot { Src::None } else { Src::Texel { addr: obj_vram + a, hflip, vflip } };
                    Px { c, layer: 4, prio, alpha: palno as u8 + 2, src }
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
                    Px { c: c | 0x8000, layer: 4, prio, alpha: (mode == 1) as u8, src: Src::None }
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
