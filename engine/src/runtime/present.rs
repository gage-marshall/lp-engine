//! How the engine shows the game's screens beyond what the DS does (presentation only: nothing here changes the
//! game's memory or timing).
//!
//! - `scale` > 1: drawn at `scale` times the DS resolution. The 3D layer is rasterised at that resolution, and
//!   when the game captures a picture into VRAM (the heroine is drawn once, captured, then shown from VRAM as a
//!   bitmap OBJ or by VRAM display) a finer version of the capture is made from the same registers and the
//!   finer 3D layer. Wherever the game shows the captured pixels 1:1 and they are still what it captured, the
//!   finer version is drawn (`ppu::Hi`); everything else (2D art, text) is scaled up.
//! - `look`: the 3D rasteriser's options beyond the DS (bilinear textures, smooth toon shading, outline width);
//!   `aa`: the 3D layer drawn with 2 x 2 samples per pixel, partly covered pixels blended with what lies behind.
//!   Any of these (or `scale` > 1) draws through the finer path above, captures included.
//! - `smooth`: in-between pictures of 3D the game draws once per cycle. The heroine scenes draw her once every
//!   4 VBlanks (the game's own pacing, VramBank_Process 0x2044ee8, holds the draw pass for 2). When two successive
//!   captures with the same control word come from the same polygons (count, vertex counts, attributes,
//!   textures), the halfway geometry is drawn and captured as the game's capture was and shown while the draw
//!   pass is held; the game's own picture follows. She then moves at 30 updates a second, 2 VBlanks behind.
use super::gx::{Frame, Poly};
use super::ppu::{self, Hi, Shadow};
use super::raster3d::{self, Look};
use super::Machine;
use std::collections::HashMap;

pub struct Present {
    pub scale: usize,
    pub smooth: bool,
    pub look: Look,
    pub aa: bool,
    /// the geometry of the last capture per capture control word (enable bit cleared)
    last: HashMap<u32, Frame>,
    /// finer versions of the captures the game made, per bank
    shadows: HashMap<usize, Shadow>,
    /// the in-between capture shown during the current pass (written over the game's for the drawing), and its
    /// finer version
    mid: Option<(usize, Vec<(usize, u16)>)>,
    mid_shadow: Option<Shadow>,
    /// the finer 3D layer of the last swapped frame (by swap count)
    layer3d: Option<(u64, Layer)>,
}

/// a 3D layer and, when anti-aliased, its drawn share per pixel
type Layer = (Vec<u16>, Option<Vec<u8>>);

fn same_shape(a: &Poly, b: &Poly) -> bool {
    a.verts.len() == b.verts.len() && a.attr == b.attr && a.tex == b.tex && a.pal == b.pal
}

/// the geometry a fraction `t` of the way from `a` to `b`, when the two frames are the same polygons
fn between(a: &Frame, b: &Frame, t: f64) -> Option<Frame> {
    if a.polys.len() != b.polys.len() || a.polys.is_empty() || !a.polys.iter().zip(&b.polys).all(|(p, q)| same_shape(p, q)) {
        return None;
    }
    let mut f = b.clone();
    for (p, q) in f.polys.iter_mut().zip(&a.polys) {
        for (v, w) in p.verts.iter_mut().zip(&q.verts) {
            let l = |x: f64, y: f64| y + (x - y) * t;
            for i in 0..4 {
                v.clip[i] = l(v.clip[i], w.clip[i]);
            }
            for i in 0..3 {
                v.color[i] = l(v.color[i], w.color[i]);
            }
            for i in 0..2 {
                v.uv[i] = l(v.uv[i], w.uv[i]);
            }
        }
    }
    Some(f)
}


/// the 3D layer as the DS draws it (the game's own captures are made from this)
fn native_layer(m: &Machine, f: &Frame) -> Vec<u16> {
    let mut t = raster3d::Target::new(1);
    t.draw(&m.ram, f);
    t.layer()
}

/// write capture pixels into a bank, returning what was there
fn swap_in(m: &mut Machine, bank: usize, px: &[(usize, u16)]) -> Vec<(usize, u16)> {
    let b = &mut m.ram.vram.banks[bank];
    px.iter()
        .map(|&(o, c)| {
            let old = b[o] as u16 | (b[o + 1] as u16) << 8;
            b[o] = c as u8;
            b[o + 1] = (c >> 8) as u8;
            (o, old)
        })
        .collect()
}

impl Present {
    pub fn new(scale: usize, smooth: bool) -> Present {
        Present { scale: scale.max(1), smooth, look: Look::default(), aa: false, last: HashMap::new(), shadows: HashMap::new(), mid: None, mid_shadow: None, layer3d: None }
    }

    /// change the resolution / 3D options (the finer pictures are made again from the game's next captures)
    pub fn set(&mut self, scale: usize, look: Look, aa: bool) {
        *self = Present { scale: scale.max(1), look, aa, ..Present::new(scale, self.smooth) };
    }

    /// drawing beyond the DS at all (else the game's own pictures are shown as the DS shows them)
    fn enhanced(&self) -> bool {
        self.scale > 1 || self.aa || self.look != Look::default()
    }

    /// the 3D layer of a frame at the presentation's resolution and options
    fn raster(&self, m: &Machine, f: &Frame) -> Layer {
        let k = if self.aa { 2 } else { 1 };
        let mut t = raster3d::Target::new(self.scale * k);
        t.look = Look { outline: self.look.outline * k, ..self.look };
        t.draw(&m.ram, f);
        if self.aa {
            let (l, c) = t.layer_aa(2);
            (l, Some(c))
        } else {
            (t.layer_full(), None)
        }
    }

    fn shadow(&self, m: &Machine, cnt: u32, f: &Frame, shadows: &[&Shadow]) -> Vec<u16> {
        let (l3, cov) = self.raster(m, f);
        ppu::capture_image_hi(&m.ram, cnt, &Hi { s: self.scale, layer3d: Some(&l3), cov3d: cov.as_deref(), shadows })
    }

    /// after a game pass: when it captured, keep a finer version of the capture and prepare the in-between picture
    pub fn after_pass(&mut self, m: &Machine) {
        self.mid = None;
        self.mid_shadow = None;
        let Some(cnt) = m.last_capture else { return };
        let key = cnt & 0x7fff_ffff;
        let cur = &m.ram.gx.frame;
        let bank = ((cnt >> 16) & 3) as usize;
        let dst_off = ((cnt >> 18) & 3) as usize * 0x8000;
        let (cw, ch) = [(128, 128), (256, 64), (256, 128), (256, 192)][((cnt >> 20) & 3) as usize];
        let mid = if self.smooth { self.last.get(&key).and_then(|prev| between(prev, cur, 0.5)) } else { None };
        if self.enhanced() {
            // the capture's sources as they were: the bank's own earlier shadow may be source B (and is what the
            // in-between picture's source B is too)
            let prior: Vec<&Shadow> = self.shadows.values().filter(|s| s.bank != bank || s.valid(&m.ram)).collect();
            let (hi, mid_hi) = rayon::join(|| self.shadow(m, cnt, cur, &prior), || mid.as_ref().map(|f| self.shadow(m, cnt, f, &prior)));
            // the pixels as the game wrote them, to tell later whether they are still there
            let b = &m.ram.vram().banks[bank];
            let px: Vec<(usize, u16)> = (0..cw * ch)
                .map(|i| (dst_off + 2 * i) % 0x20000)
                .map(|o| (o, b[o] as u16 | (b[o + 1] as u16) << 8))
                .collect();
            self.shadows.insert(bank, Shadow { bank, px, cw, hi });
            if let (Some(f), Some(hi)) = (&mid, mid_hi) {
                let (b, mpx) = ppu::capture_image(&m.ram, cnt, Some(&native_layer(m, f)));
                self.mid_shadow = Some(Shadow { bank: b, px: mpx.clone(), cw, hi });
                self.mid = Some((b, mpx));
            }
        } else if let Some(f) = &mid {
            self.mid = Some(ppu::capture_image(&m.ram, cnt, Some(&native_layer(m, f))));
        }
        self.last.insert(key, cur.clone());
    }

    /// both screens (top, bottom) as RGBA8, `256 * scale` x `192 * scale` each
    pub fn screens(&mut self, m: &mut Machine) -> (Vec<u8>, Vec<u8>) {
        let restore = self.mid.as_ref().map(|(bank, px)| (*bank, swap_in(m, *bank, px)));
        let out = if !self.enhanced() {
            let l3 = m.layer3d();
            m.screens(Some(&l3))
        } else {
            let swaps = m.ram.gx.frames;
            if self.layer3d.as_ref().map_or(true, |(n, _)| *n != swaps) {
                let l3 = self.raster(m, &m.ram.gx.frame);
                self.layer3d = Some((swaps, l3));
            }
            let mut shadows: Vec<&Shadow> = Vec::new();
            if let Some(s) = &self.mid_shadow {
                shadows.push(s);
            }
            shadows.extend(self.shadows.values().filter(|s| s.valid(&m.ram)));
            let (l3, cov) = self.layer3d.as_ref().map_or((None, None), |(_, (l, c))| (Some(l.as_slice()), c.as_deref()));
            let (ram, s) = (&m.ram, self.scale);
            let (a, b) = rayon::join(
                || ppu::render_hi(ram, 0, &Hi { s, layer3d: l3, cov3d: cov, shadows: &shadows }),
                || ppu::render_hi(ram, 1, &Hi { s, layer3d: None, cov3d: None, shadows: &shadows }),
            );
            if ppu::a_on_top(&m.ram) { (a, b) } else { (b, a) }
        };
        if let Some((bank, old)) = restore {
            swap_in(m, bank, &old);
        }
        out
    }
}
