//! Drawing a swapped geometry frame (`gx::Frame`) the way the DS 3D engine draws it: clipping against the view
//! volume, the viewport, front / back culling, Z- or W-buffering, perspective-correct colours and texture
//! coordinates, textures from the VRAM texture slots in all seven formats, the four polygon modes (modulate, decal,
//! toon / highlight, shadow), the alpha test and translucency, edge marking, and the rear plane (clear colour /
//! depth). The registers come from DISP3DCNT 0x4000060 and the rendering engine's tables (0x4000330..0x40003bf).
//! Output: the 3D layer for engine A's BG0 (RGB555 with bit 15 set where something was drawn), `scale` x the DS
//! resolution.
use super::gx::{Frame, Poly, Vert};
use super::ram::Ram;
use super::vram::Place;

/// drawing beyond the DS (`Look::default()` is the DS's way)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    /// textures sampled bilinearly (the DS: nearest texel)
    pub bilinear: bool,
    /// toon colours interpolated between the table's entries (the DS: the entry at the vertex colour's level)
    pub smooth_toon: bool,
    /// edge-marking width in pixels (the DS: 1)
    pub outline: usize,
    /// textures replaced by enhanced versions (`texcache`: a mod's, or FSR 1 at 4x); the GPU renderer only
    pub upscale: bool,
}

impl Default for Look {
    fn default() -> Self {
        Look { bilinear: false, smooth_toon: false, outline: 1, upscale: false }
    }
}

pub struct Target {
    pub w: usize,
    pub h: usize,
    pub look: Look,
    /// the first row this target holds (a band of a larger picture, drawn in parallel with the others)
    y0: usize,
    pub color: Vec<[f32; 4]>,
    depth: Vec<f32>,
    id: Vec<u8>,
    opaque_id: Vec<u8>,
}

fn tex_byte(ram: &Ram, off: u32) -> u8 {
    ram.vram().slot(Place::TexImage(0), off as usize)
}
fn pal_color(ram: &Ram, off: u32) -> u16 {
    let v = ram.vram();
    v.slot(Place::TexPalette(0), off as usize) as u16 | (v.slot(Place::TexPalette(0), off as usize + 1) as u16) << 8
}
fn c5(c: u16) -> [f32; 3] {
    [(c & 0x1f) as f32 / 31.0, ((c >> 5) & 0x1f) as f32 / 31.0, ((c >> 10) & 0x1f) as f32 / 31.0]
}

/// bilinear filtering of `texel` at texture coordinate (u, v): colours weighted by their alpha; transparent where
/// less than half the weight is opaque, and cut-out formats stay opaque where they are drawn
fn texel_bilinear(ram: &Ram, tex: u32, pal: u32, u: f32, v: f32) -> Option<([f32; 3], f32)> {
    let (u, v) = (u - 0.5, v - 0.5);
    let (s0, t0) = (u.floor() as i32, v.floor() as i32);
    let (fu, fv) = (u - u.floor(), v - v.floor());
    let (mut c, mut a) = ([0.0f32; 3], 0.0f32);
    for (ds, dt, w) in [(0, 0, (1.0 - fu) * (1.0 - fv)), (1, 0, fu * (1.0 - fv)), (0, 1, (1.0 - fu) * fv), (1, 1, fu * fv)] {
        if let Some((tc, ta)) = texel(ram, tex, pal, s0 + ds, t0 + dt) {
            for k in 0..3 {
                c[k] += tc[k] * ta * w;
            }
            a += ta * w;
        }
    }
    let fmt = (tex >> 26) & 7;
    let translucent = fmt == 1 || fmt == 6;
    if a <= 0.0 || (!translucent && a < 0.5) {
        return None;
    }
    Some((c.map(|x| x / a), if translucent { a } else { 1.0 }))
}

/// a texel of a texture (TEXIMAGE_PARAM, PLTT_BASE) at integer coordinates (after wrapping): rgb, alpha 0..1
pub(crate) fn texel(ram: &Ram, tex: u32, pal: u32, s: i32, t: i32) -> Option<([f32; 3], f32)> {
    let fmt = (tex >> 26) & 7;
    if fmt == 0 {
        return None;
    }
    let base = (tex & 0xffff) * 8;
    let w = 8 << ((tex >> 20) & 7);
    let h = 8 << ((tex >> 23) & 7);
    let wrap = |v: i32, n: i32, rep: bool, flip: bool| {
        if !rep {
            v.clamp(0, n - 1)
        } else if flip {
            let m = v.rem_euclid(2 * n);
            if m >= n { 2 * n - 1 - m } else { m }
        } else {
            v.rem_euclid(n)
        }
    };
    let s = wrap(s, w, tex & (1 << 16) != 0, tex & (1 << 18) != 0) as u32;
    let t = wrap(t, h, tex & (1 << 17) != 0, tex & (1 << 19) != 0) as u32;
    let i = t * w as u32 + s;
    let transparent0 = tex & (1 << 29) != 0;
    let palbase = pal * 16;
    let indexed = |idx: u32, palbase: u32| -> Option<([f32; 3], f32)> {
        if idx == 0 && transparent0 {
            return None;
        }
        Some((c5(pal_color(ram, palbase + idx * 2)), 1.0))
    };
    match fmt {
        1 => {
            // A3I5
            let b = tex_byte(ram, base + i) as u32;
            let a = (b >> 5) * 4 + (b >> 5) / 2;
            (a != 0).then(|| (c5(pal_color(ram, palbase + (b & 31) * 2)), a as f32 / 31.0))
        }
        2 => indexed((tex_byte(ram, base + i / 4) as u32 >> (2 * (i % 4))) & 3, pal * 8),
        3 => indexed((tex_byte(ram, base + i / 2) as u32 >> (4 * (i % 2))) & 15, palbase),
        4 => indexed(tex_byte(ram, base + i) as u32, palbase),
        5 => {
            // 4x4 compressed: 2-bit texels per 4x4 block, the block's palette word in slot 1
            let (bx, by) = (s / 4, t / 4);
            let block = by * (w as u32 / 4) + bx;
            let word = base + block * 4;
            let bits = tex_byte(ram, word + t % 4) as u32 >> (2 * (s % 4)) & 3;
            let slot = (word & 0x1ffff) / 2 + if word >= 0x40000 { 0x1000 } else { 0 };
            let pw = tex_byte(ram, 0x20000 + slot) as u32 | (tex_byte(ram, 0x20000 + slot + 1) as u32) << 8;
            let poff = palbase + (pw & 0x3fff) * 4;
            let mode = pw >> 14;
            let col = |k: u32| c5(pal_color(ram, poff + k * 2));
            let mix = |a: [f32; 3], b: [f32; 3], wa: f32, wb: f32| [(a[0] * wa + b[0] * wb) / (wa + wb), (a[1] * wa + b[1] * wb) / (wa + wb), (a[2] * wa + b[2] * wb) / (wa + wb)];
            match (mode, bits) {
                (_, 0) => Some((col(0), 1.0)),
                (_, 1) => Some((col(1), 1.0)),
                (0 | 2, 2) => Some((col(2), 1.0)),
                (0 | 2, 3) => if mode == 0 { None } else { Some((col(3), 1.0)) },
                (1, 2) => Some((mix(col(0), col(1), 1.0, 1.0), 1.0)),
                (1, 3) => None,
                (3, 2) => Some((mix(col(0), col(1), 5.0, 3.0), 1.0)),
                (3, 3) => Some((mix(col(0), col(1), 3.0, 5.0), 1.0)),
                _ => None,
            }
        }
        6 => {
            // A5I3
            let b = tex_byte(ram, base + i) as u32;
            let a = b >> 3;
            (a != 0).then(|| (c5(pal_color(ram, palbase + (b & 7) * 2)), a as f32 / 31.0))
        }
        _ => {
            // direct colour
            let c = tex_byte(ram, base + i * 2) as u16 | (tex_byte(ram, base + i * 2 + 1) as u16) << 8;
            (c & 0x8000 != 0).then(|| (c5(c), 1.0))
        }
    }
}

/// clip a polygon against the view volume (-w <= x, y, z <= w)
fn clip(verts: &[Vert]) -> Vec<Vert> {
    let mut poly = verts.to_vec();
    for plane in 0..6 {
        if poly.is_empty() {
            break;
        }
        let (axis, sign) = (plane / 2, if plane % 2 == 0 { 1.0 } else { -1.0 });
        let d = |v: &Vert| v.clip[3] + sign * v.clip[axis];
        let mut out = Vec::new();
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            let (da, db) = (d(&a), d(&b));
            if da >= 0.0 {
                out.push(a);
            }
            if (da >= 0.0) != (db >= 0.0) {
                let t = da / (da - db);
                let lerp = |x: f64, y: f64| x + (y - x) * t;
                out.push(Vert {
                    clip: [0, 1, 2, 3].map(|k| lerp(a.clip[k], b.clip[k])),
                    color: [0, 1, 2].map(|k| lerp(a.color[k], b.color[k])),
                    uv: [0, 1].map(|k| lerp(a.uv[k], b.uv[k])),
                });
            }
        }
        poly = out;
    }
    poly
}


/// a polygon is drawn in the translucent pass (after the opaque ones)
pub(crate) fn translucent(p: &Poly) -> bool {
    let a = (p.attr >> 16) & 31;
    let fmt = (p.tex >> 26) & 7;
    (a != 0 && a < 31) || fmt == 1 || fmt == 6
}

/// the polygons as drawn, in drawing order: opaque ones first, then translucent ones (in submission order, or by y
/// when sorting is automatic); each clipped to the view volume, projected to the screen at `s` pixels per DS pixel
/// and kept when its facing is drawn
pub(crate) fn prepare(frame: &Frame, s: f32) -> Vec<(&Poly, Vec<SV>)> {
    let w_buffer = frame.swap & 2 != 0;
    let mut order: Vec<&Poly> = frame.polys.iter().filter(|p| !translucent(p)).collect();
    let mut trans: Vec<&Poly> = frame.polys.iter().filter(|p| translucent(p)).collect();
    if frame.swap & 1 == 0 {
        trans.sort_by(|a, b| {
            let y = |p: &Poly| p.verts.iter().map(|v| v.clip[1] / v.clip[3].max(1e-9)).fold(f64::MIN, f64::max);
            y(b).partial_cmp(&y(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    order.extend(trans);
    let (vx1, vy1, vx2, vy2) = frame.viewport;
    let mut out = Vec::with_capacity(order.len());
    for p in order {
        let v = clip(&p.verts);
        if v.len() < 3 {
            continue;
        }
        let sv: Vec<SV> = v
            .iter()
            .map(|q| {
                let w = q.clip[3].max(1e-9);
                let (nx, ny, nz) = (q.clip[0] / w, q.clip[1] / w, q.clip[2] / w);
                let x = (nx + 1.0) * (vx2 as f64 - vx1 as f64 + 1.0) / 2.0 + vx1 as f64;
                let y = 191.0 - ((ny + 1.0) * (vy2 as f64 - vy1 as f64 + 1.0) / 2.0 + vy1 as f64);
                SV { x: x as f32 * s, y: (y + 1.0) as f32 * s, z: if w_buffer { w as f32 } else { ((nz + 1.0) / 2.0) as f32 }, w: w as f32, c: q.color.map(|c| c as f32 / 31.0), uv: q.uv.map(|u| u as f32) }
            })
            .collect();
        // culling by screen winding (DS screen y grows downward)
        let area: f32 = (0..sv.len()).map(|i| {
            let (a, b) = (&sv[i], &sv[(i + 1) % sv.len()]);
            a.x * b.y - b.x * a.y
        }).sum();
        let front = area < 0.0;
        if (front && p.attr & 0x80 == 0) || (!front && p.attr & 0x40 == 0) {
            continue;
        }
        out.push((p, sv));
    }
    out
}

/// a vertex on the screen: position in pixels, depth (z or w), w, colour 0..1, texture coordinate
pub(crate) struct SV {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
    pub c: [f32; 3],
    pub uv: [f32; 2],
}

impl Target {
    pub fn new(scale: usize) -> Target {
        let (w, h) = (256 * scale, 192 * scale);
        Self::band(w, 0, h, Look::default())
    }

    fn band(w: usize, y0: usize, h: usize, look: Look) -> Target {
        Target { w, h, y0, look, color: vec![[0.0; 4]; w * h], depth: vec![f32::MAX; w * h], id: vec![0; w * h], opaque_id: vec![0xff; w * h] }
    }

    /// draw a frame
    pub fn draw(&mut self, ram: &Ram, frame: &Frame) {
        if self.w > 256 {
            // above the DS resolution: horizontal bands in parallel (each draws every polygon in the same order,
            // clipped to its rows), then the edges over the whole picture
            use rayon::prelude::*;
            let rows = self.h.div_ceil(16);
            let mut bands: Vec<Target> = (0..self.h).step_by(rows).map(|y0| Self::band(self.w, y0, rows.min(self.h - y0), self.look)).collect();
            bands.par_iter_mut().for_each(|b| b.draw_polys(ram, frame));
            for b in bands {
                let o = b.y0 * self.w;
                let n = b.color.len();
                self.color[o..o + n].copy_from_slice(&b.color);
                self.depth[o..o + n].copy_from_slice(&b.depth);
                self.id[o..o + n].copy_from_slice(&b.id);
                self.opaque_id[o..o + n].copy_from_slice(&b.opaque_id);
            }
        } else {
            self.draw_polys(ram, frame);
        }
        if ram.u32(0x4000060) & 0x20 != 0 {
            self.edges(ram);
        }
    }

    fn draw_polys(&mut self, ram: &Ram, frame: &Frame) {
        let disp = ram.u32(0x4000060);
        let clear = ram.u32(0x4000350);
        let cc = c5(clear as u16);
        let ca = ((clear >> 16) & 0x1f) as f32 / 31.0;
        let clear_id = ((clear >> 24) & 0x3f) as u8;
        for i in 0..self.w * self.h {
            self.color[i] = [cc[0], cc[1], cc[2], ca];
            self.depth[i] = f32::MAX;
            self.id[i] = clear_id;
            self.opaque_id[i] = clear_id;
        }
        let alpha_blend = disp & 8 != 0;
        let alpha_test = if disp & 4 != 0 { (ram.u8(0x4000340) & 0x1f) as f32 / 31.0 } else { -1.0 };
        let toon: Vec<[f32; 3]> = (0..32).map(|i| c5(ram.u16(0x4000380 + 2 * i))).collect();
        let highlight = disp & 2 != 0;
        for (p, sv) in prepare(frame, (self.w / 256) as f32) {
            for k in 1..sv.len() - 1 {
                self.triangle(ram, p, [&sv[0], &sv[k], &sv[k + 1]], alpha_blend, alpha_test, &toon, highlight);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn triangle(&mut self, ram: &Ram, p: &Poly, v: [&SV; 3], blend: bool, alpha_test: f32, toon: &[[f32; 3]], highlight: bool) {
        let (minx, maxx) = (v.iter().map(|q| q.x).fold(f32::MAX, f32::min).max(0.0) as usize, v.iter().map(|q| q.x).fold(f32::MIN, f32::max).min(self.w as f32 - 1.0) as usize);
        let (miny, maxy) = (v.iter().map(|q| q.y).fold(f32::MAX, f32::min).max(self.y0 as f32) as usize, v.iter().map(|q| q.y).fold(f32::MIN, f32::max).min((self.y0 + self.h) as f32 - 1.0) as usize);
        if miny > maxy {
            return;
        }
        let area = (v[1].x - v[0].x) * (v[2].y - v[0].y) - (v[2].x - v[0].x) * (v[1].y - v[0].y);
        if area.abs() < 1e-6 {
            return;
        }
        let alpha = ((p.attr >> 16) & 31) as f32 / 31.0;
        if alpha == 0.0 {
            return; // wireframe polygons: not drawn as filled
        }
        let mode = (p.attr >> 4) & 3;
        let pid = ((p.attr >> 24) & 0x3f) as u8;
        let depth_equal = p.attr & (1 << 14) != 0;
        let ts = (8 << ((p.tex >> 20) & 7)) as f32;
        let _ = ts;
        let iw: [f32; 3] = [1.0 / v[0].w, 1.0 / v[1].w, 1.0 / v[2].w];
        for y in miny..=maxy {
            for x in minx..=maxx {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let w0 = ((v[1].x - px) * (v[2].y - py) - (v[2].x - px) * (v[1].y - py)) / area;
                let w1 = ((v[2].x - px) * (v[0].y - py) - (v[0].x - px) * (v[2].y - py)) / area;
                let w2 = 1.0 - w0 - w1;
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * v[0].z + w1 * v[1].z + w2 * v[2].z;
                let i = (y - self.y0) * self.w + x;
                let pass = if depth_equal { (z - self.depth[i]).abs() < 1e-3 } else { z < self.depth[i] };
                if !pass {
                    continue;
                }
                // perspective-correct attributes
                let q = w0 * iw[0] + w1 * iw[1] + w2 * iw[2];
                let a = |f: &dyn Fn(&SV) -> f32| (w0 * f(v[0]) * iw[0] + w1 * f(v[1]) * iw[1] + w2 * f(v[2]) * iw[2]) / q;
                let vc = [a(&|s: &SV| s.c[0]), a(&|s: &SV| s.c[1]), a(&|s: &SV| s.c[2])];
                let (u, t) = (a(&|s: &SV| s.uv[0]), a(&|s: &SV| s.uv[1]));
                let tx = if self.look.bilinear { texel_bilinear(ram, p.tex, p.pal, u, t) } else { texel(ram, p.tex, p.pal, u.floor() as i32, t.floor() as i32) };
                if (p.tex >> 26) & 7 != 0 && tx.is_none() {
                    continue; // transparent texel
                }
                let (tc, ta) = tx.unwrap_or(([1.0; 3], 1.0));
                let (rgb, a_out) = match mode {
                    1 => {
                        // decal: the texture over the vertex colour by texture alpha
                        ([0, 1, 2].map(|k| tc[k] * ta + vc[k] * (1.0 - ta)), alpha)
                    }
                    2 => {
                        let tn = if self.look.smooth_toon {
                            let l = (vc[0] * 31.0).clamp(0.0, 31.0);
                            let (i, f) = (l.floor() as usize, l.fract());
                            let (a, b) = (toon[i], toon[(i + 1).min(31)]);
                            [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * f)
                        } else {
                            toon[((vc[0] * 31.0) as usize).min(31)]
                        };
                        if highlight {
                            ([0, 1, 2].map(|k| (tc[k] * vc[0] + tn[k]).min(1.0)), ta * alpha)
                        } else {
                            ([0, 1, 2].map(|k| tc[k] * tn[k]), ta * alpha)
                        }
                    }
                    _ => ([0, 1, 2].map(|k| tc[k] * vc[k]), ta * alpha),
                };
                if a_out <= alpha_test {
                    continue;
                }
                if mode == 3 {
                    continue; // shadow polygons need the stencil pass (not drawn)
                }
                let dst = self.color[i];
                if a_out < 1.0 && blend && dst[3] > 0.0 {
                    self.color[i] = [rgb[0] * a_out + dst[0] * (1.0 - a_out), rgb[1] * a_out + dst[1] * (1.0 - a_out), rgb[2] * a_out + dst[2] * (1.0 - a_out), a_out.max(dst[3])];
                    if p.attr & (1 << 11) != 0 {
                        self.depth[i] = z; // translucent polygons update depth only when asked
                    }
                } else {
                    self.color[i] = [rgb[0], rgb[1], rgb[2], a_out];
                    self.depth[i] = z;
                    if a_out >= 1.0 {
                        self.opaque_id[i] = pid;
                    }
                }
                self.id[i] = pid;
            }
        }
    }

    /// edge marking: an opaque pixel whose neighbour has another polygon id and lies behind it takes the edge colour
    /// of its id group (0x4000330 + 2 * (id >> 3))
    fn edges(&mut self, ram: &Ram) {
        let (w, h) = (self.w, self.h);
        let src_id = self.opaque_id.clone();
        let depth = self.depth.clone();
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                if src_id[i] == 0xff || self.color[i][3] < 1.0 {
                    continue;
                }
                let me = src_id[i];
                let mut edge = false;
                let r = self.look.outline.max(1) as i32;
                for (dx, dy) in (1..=r).flat_map(|k| [(k, 0i32), (-k, 0), (0, k), (0, -k)]) {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0 || ny < 0 || nx >= w as i32 || ny >= h as i32 {
                        continue;
                    }
                    let j = ny as usize * w + nx as usize;
                    if src_id[j] != me && depth[i] < depth[j] {
                        edge = true;
                    }
                }
                if edge {
                    let c = c5(ram.u16(0x4000330 + 2 * (me as u32 >> 3)));
                    self.color[i] = [c[0], c[1], c[2], 1.0];
                }
            }
        }
    }

    /// the 3D layer at the target's own resolution (256 * scale x 192 * scale), RGB555 with bit 15 where drawn
    pub fn layer_full(&self) -> Vec<u16> {
        let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0).round() as u16) & 0x1f;
        self.color.iter().map(|c| if c[3] > 0.0 { 0x8000 | q(c[0]) | q(c[1]) << 5 | q(c[2]) << 10 } else { 0 }).collect()
    }

    /// the layer from a target `k` times finer in each direction: per pixel the average of the drawn samples
    /// (RGB555, bit 15 where any is drawn) and the drawn share (0..=16), for blending against what lies behind
    pub fn layer_aa(&self, k: usize) -> (Vec<u16>, Vec<u8>) {
        let (w, h) = (self.w / k, self.h / k);
        let mut out = vec![0u16; w * h];
        let mut cov = vec![0u8; w * h];
        let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0).round() as u16) & 0x1f;
        for y in 0..h {
            for x in 0..w {
                let (mut sum, mut n) = ([0.0f32; 3], 0usize);
                for sy in 0..k {
                    for sx in 0..k {
                        let c = self.color[(y * k + sy) * self.w + x * k + sx];
                        if c[3] > 0.0 {
                            for i in 0..3 {
                                sum[i] += c[i];
                            }
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    let a = sum.map(|v| v / n as f32);
                    out[y * w + x] = 0x8000 | q(a[0]) | q(a[1]) << 5 | q(a[2]) << 10;
                    cov[y * w + x] = (n * 16 / (k * k)) as u8;
                }
            }
        }
        (out, cov)
    }

    /// the layer as the 2D engine composes it: RGB555 + bit 15 where drawn (at 1x: one sample per DS pixel)
    pub fn layer(&self) -> Vec<u16> {
        let s = self.w / 256;
        let mut out = vec![0u16; 256 * 192];
        for y in 0..192 {
            for x in 0..256 {
                let c = self.color[(y * s + s / 2) * self.w + x * s + s / 2];
                if c[3] > 0.0 {
                    let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0).round() as u16) & 0x1f;
                    out[y * 256 + x] = 0x8000 | q(c[0]) | q(c[1]) << 5 | q(c[2]) << 10;
                }
            }
        }
        out
    }
}
