//! Optional in-between pictures for 3D the game draws less often than it displays (presentation only: nothing here
//! changes the game's memory or timing).
//!
//! The heroine scenes draw her once per 4 VBlanks: the draw pass captures engine A's picture into VRAM and the
//! game's own pacing (VramBank_Process 0x2044ee8) holds that pass for 2 VBlanks, then shows the captured picture
//! for the rest of the cycle. When two successive captures with the same control word came from the same
//! polygons (same count, vertex counts, attributes, textures), the halfway geometry is drawn and captured the way
//! the game's capture was, and that picture is shown while the draw pass is held; the game's own picture follows.
//! The heroine then moves at 30 updates a second, half a cycle (2 VBlanks) behind the game.
use super::gx::{Frame, Poly};
use super::{ppu, raster3d, Machine};
use std::collections::HashMap;

#[derive(Default)]
pub struct Smooth {
    /// the geometry of the last capture per capture control word (enable bit cleared)
    last: HashMap<u32, Frame>,
    /// the in-between capture shown for the current pass: bank and (offset, RGB555) per pixel
    shown: Option<(usize, Vec<(usize, u16)>)>,
}

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

impl Smooth {
    /// after a game pass: when it captured, keep its geometry and prepare the halfway picture for this pass
    pub fn after_pass(&mut self, m: &Machine) {
        self.shown = None;
        let Some(cnt) = m.last_capture else { return };
        let key = cnt & 0x7fff_ffff;
        let cur = &m.ram.gx.frame;
        if let Some(mid) = self.last.get(&key).and_then(|prev| between(prev, cur, 0.5)) {
            let mut t = raster3d::Target::new(1);
            t.draw(&m.ram, &mid);
            self.shown = Some(ppu::capture_image(&m.ram, cnt, Some(&t.layer())));
        }
        self.last.insert(key, cur.clone());
    }

    /// both screens as `Machine::screens` draws them, with the halfway picture in place of the pass's capture
    pub fn screens(&self, m: &mut Machine) -> (Vec<u8>, Vec<u8>) {
        let l3 = m.layer3d();
        let Some((bank, px)) = &self.shown else { return m.screens(Some(&l3)) };
        let b = &mut m.ram.vram.banks[*bank];
        let saved: Vec<(usize, u8, u8)> = px.iter().map(|&(o, _)| (o, b[o], b[o + 1])).collect();
        for &(o, c) in px {
            b[o] = c as u8;
            b[o + 1] = (c >> 8) as u8;
        }
        let out = m.screens(Some(&l3));
        let b = &mut m.ram.vram.banks[*bank];
        for (o, lo, hi) in saved {
            b[o] = lo;
            b[o + 1] = hi;
        }
        out
    }
}
