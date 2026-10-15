//! Enhanced textures for the GPU renderer (`gpu3d`, `Look::upscale`): every texture the polygons use is known by a
//! key (FNV-1a over the bytes its format reads, its palette entries and its format bits, so a key is one picture
//! wherever it sits in VRAM) and replaced, the first time it is seen, by a finer version: a mod's PNG named after
//! the key (any size) or, without one, the texture decoded as the DS decodes it (`raster3d::texel`) and upscaled
//! 4x with AMD FidelityFX Super Resolution 1 (EASU, then RCAS sharpening; MIT). Taps beyond the edge follow the
//! texture's own wrap (repeat, mirror, clamp), so tiled textures stay seamless; transparent texels take their
//! opaque neighbours' colour first so cut-outs get no dark fringe, and alpha is scaled bilinearly.
use super::ram::Ram;
use super::raster3d;
use std::path::{Path, PathBuf};

/// the upscaling factor without a mod
pub const FACTOR: usize = 4;
/// how the enhancement looks: RCAS sharpness in stops (0 strongest, higher softer; FSR's default is 0.2) for
/// textures and backgrounds, and the backgrounds' denoising (0..1) before upscaling: their photos are 16 colours
/// per tile, and EASU keeps the palette's steps as edges (a painted look) unless they are smoothed first
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub tex_sharpness: f32,
    /// the textures' denoising before upscaling (0..1, as `bg_denoise`)
    pub tex_denoise: f32,
    pub bg_sharpness: f32,
    pub bg_denoise: f32,
}

impl Default for Params {
    fn default() -> Self {
        // the Smooth preset (denoised FSR) for both, preferred over plain FSR on the game's photos and textures
        Params { tex_sharpness: 1.0, tex_denoise: 0.5, bg_sharpness: 1.0, bg_denoise: 0.5 }
    }
}

impl Params {
    /// the background presets: 0 sharp (FSR as it is), 1 smooth, 2 soft
    pub fn bg_preset(self, n: usize) -> Params {
        let (bg_sharpness, bg_denoise) = PRESETS[n.min(2)];
        Params { bg_sharpness, bg_denoise, ..self }
    }

    /// the same presets for the textures
    pub fn tex_preset(self, n: usize) -> Params {
        let (tex_sharpness, tex_denoise) = PRESETS[n.min(2)];
        Params { tex_sharpness, tex_denoise, ..self }
    }
}

/// the upscaling presets: (RCAS sharpness, denoise) for sharp, smooth, soft
const PRESETS: [(f32, f32); 3] = [(0.25, 0.0), (1.0, 0.5), (2.0, 1.0)];

impl Params {
}

/// a texture's size from TEXIMAGE_PARAM
pub fn size(tex: u32) -> (usize, usize) {
    (8 << ((tex >> 20) & 7), 8 << ((tex >> 23) & 7))
}

pub(crate) fn fnv(h: &mut u64, bytes: &[u8]) {
    for &b in bytes {
        *h ^= b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

/// what a texture of format `fmt` and `n` texels reads: texel bytes, and palette colours (0: none or, for 4x4,
/// as far as its blocks reach)
fn reads(fmt: u32, n: usize) -> (usize, usize) {
    match fmt {
        1 => (n, 32),
        2 => (n / 4, 4),
        3 => (n / 2, 16),
        4 => (n, 256),
        5 => (n / 4, 0),
        6 => (n, 8),
        _ => (n * 2, 0),
    }
}

/// a texture's key from what it reads: TEXIMAGE_PARAM (its format, size and colour-0 bits count), its texel
/// bytes, for 4x4 its block index words, and its palette from its first colour (bytes beyond `pal` count as 0).
/// The runtime (`key`) and the asset index (`assetindex`) both key textures with this.
pub fn key_of(tex: u32, texels: &[u8], idx4: &[u8], pal: &[u8]) -> u64 {
    let fmt = (tex >> 26) & 7;
    let (w, h) = size(tex);
    let (bytes, colours) = reads(fmt, w * h);
    let mut k = 0xcbf29ce484222325u64;
    fnv(&mut k, &(tex & (7 << 26 | 7 << 20 | 7 << 23 | 1 << 29)).to_le_bytes());
    let take = |b: &[u8], len: usize| -> Vec<u8> { (0..len).map(|i| b.get(i).copied().unwrap_or(0)).collect() };
    fnv(&mut k, &take(texels, bytes));
    if fmt == 5 {
        let words = take(idx4, w * h / 16 * 2);
        fnv(&mut k, &words);
        let top = words.chunks_exact(2).map(|w| (u16::from_le_bytes([w[0], w[1]]) & 0x3fff) as usize * 4 + 8).max().unwrap_or(0);
        fnv(&mut k, &take(pal, top));
    } else if colours > 0 {
        fnv(&mut k, &take(pal, colours * 2));
    }
    k
}

/// the key of the texture (TEXIMAGE_PARAM, PLTT_BASE) in the texture image slots `img` and palette slots `pal`
/// (as the 3D engine sees them); None for "no texture"
pub fn key(img: &[u8], pals: &[u8], tex: u32, pal: u32) -> Option<u64> {
    let fmt = (tex >> 26) & 7;
    if fmt == 0 {
        return None;
    }
    let (w, h) = size(tex);
    let base = ((tex & 0xffff) * 8) as usize;
    let span = |b: &[u8], o: usize, len: usize| -> Vec<u8> { (0..len).map(|i| b[(o + i) & (b.len() - 1)]).collect() };
    let (bytes, _) = reads(fmt, w * h);
    // 4x4 compressed: the block words are in slot 1
    let idx4 = if fmt == 5 { span(img, 0x20000 + (base & 0x1ffff) / 2 + if base >= 0x40000 { 0x1000 } else { 0 }, w * h / 16 * 2) } else { Vec::new() };
    let palbase = if fmt == 2 { (pal * 8) as usize } else { (pal * 16) as usize };
    let pal_from = &pals[palbase.min(pals.len())..];
    Some(key_of(tex, &span(img, base, bytes), &idx4, pal_from))
}

/// an RGBA picture, 0..1 per channel
#[derive(Clone)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 4]>,
}

/// the texture as the DS decodes it (transparent texels have alpha 0)
pub fn decode(ram: &Ram, tex: u32, pal: u32) -> Image {
    let (w, h) = size(tex);
    let mut px = Vec::with_capacity(w * h);
    for t in 0..h as i32 {
        for s in 0..w as i32 {
            px.push(match raster3d::texel(ram, tex, pal, s, t) {
                Some((c, a)) => [c[0], c[1], c[2], a],
                None => [0.0; 4],
            });
        }
    }
    Image { w, h, px }
}

/// the texture's wrap per axis: (repeat, mirror)
fn wraps(tex: u32) -> [(bool, bool); 2] {
    [(tex & 1 << 16 != 0, tex & 1 << 18 != 0), (tex & 1 << 17 != 0, tex & 1 << 19 != 0)]
}

fn wrap(v: i32, n: usize, (rep, flip): (bool, bool)) -> usize {
    let n = n as i32;
    (if !rep {
        v.clamp(0, n - 1)
    } else if flip {
        let m = v.rem_euclid(2 * n);
        if m >= n { 2 * n - 1 - m } else { m }
    } else {
        v.rem_euclid(n)
    }) as usize
}

impl Image {
    fn at(&self, x: i32, y: i32, wr: [(bool, bool); 2]) -> [f32; 4] {
        self.px[wrap(y, self.h, wr[1]) * self.w + wrap(x, self.w, wr[0])]
    }

    /// transparent texels coloured from their opaque neighbours (ring by ring outward, alpha left at 0), so
    /// filtering across a cut-out's edge blends towards its own colours
    fn fill_transparent(&mut self, wr: [(bool, bool); 2]) {
        let mut known: Vec<bool> = self.px.iter().map(|p| p[3] > 0.0).collect();
        for _ in 0..8 {
            let mut next = Vec::new();
            for y in 0..self.h as i32 {
                for x in 0..self.w as i32 {
                    let i = y as usize * self.w + x as usize;
                    if known[i] {
                        continue;
                    }
                    let (mut sum, mut n) = ([0.0f32; 3], 0);
                    for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, -1), (1, -1), (-1, 1)] {
                        let j = wrap(y + dy, self.h, wr[1]) * self.w + wrap(x + dx, self.w, wr[0]);
                        if known[j] {
                            for k in 0..3 {
                                sum[k] += self.px[j][k];
                            }
                            n += 1;
                        }
                    }
                    if n > 0 {
                        next.push((i, [sum[0] / n as f32, sum[1] / n as f32, sum[2] / n as f32]));
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            for (i, c) in next {
                self.px[i] = [c[0], c[1], c[2], 0.0];
                known[i] = true;
            }
        }
    }
}

fn luma(c: [f32; 4]) -> f32 {
    0.5 * c[0] + c[1] + 0.5 * c[2]
}

/// FSR 1 EASU: edge-adaptive upscaling of the colour by `f` (alpha is scaled bilinearly)
fn easu(src: &Image, f: usize, wr: [(bool, bool); 2]) -> Image {
    let (w, h) = (src.w * f, src.h * f);
    let mut px = vec![[0.0f32; 4]; w * h];
    let sc = 1.0 / f as f32;
    use rayon::prelude::*;
    px.par_chunks_mut(w).enumerate().for_each(|(oy, row)| {
        for ox in 0..w {
            // the output pixel's centre in input texels, relative to the input pixel centres
            let ppx = (ox as f32 + 0.5) * sc - 0.5;
            let ppy = (oy as f32 + 0.5) * sc - 0.5;
            let (fx, fy) = (ppx.floor(), ppy.floor());
            let (px_, py_) = (ppx - fx, ppy - fy);
            let (ix, iy) = (fx as i32, fy as i32);
            let t = |dx: i32, dy: i32| src.at(ix + dx, iy + dy, wr);
            //    b c
            //  e f g h
            //  i j k l
            //    n o
            let (b, c, e, f_, g, h_, i, j, k, l, n, o) = (t(0, -1), t(1, -1), t(-1, 0), t(0, 0), t(1, 0), t(2, 0), t(-1, 1), t(0, 1), t(1, 1), t(2, 1), t(0, 2), t(1, 2));
            let (bl, cl, el, fl, gl, hl, il, jl, kl, ll, nl, ol) = (luma(b), luma(c), luma(e), luma(f_), luma(g), luma(h_), luma(i), luma(j), luma(k), luma(l), luma(n), luma(o));
            let mut dir = [0.0f32; 2];
            let mut len = 0.0f32;
            let mut set = |w: f32, la: f32, lb: f32, lc: f32, ld: f32, le: f32| {
                let (dc, cb) = (ld - lc, lc - lb);
                let lenx = dc.abs().max(cb.abs());
                let dirx = ld - lb;
                dir[0] += dirx * w;
                let lx = if lenx > 0.0 { (dirx.abs() / lenx).min(1.0) } else { 0.0 };
                len += lx * lx * w;
                let (ec, ca) = (le - lc, lc - la);
                let leny = ec.abs().max(ca.abs());
                let diry = le - la;
                dir[1] += diry * w;
                let ly = if leny > 0.0 { (diry.abs() / leny).min(1.0) } else { 0.0 };
                len += ly * ly * w;
            };
            set((1.0 - px_) * (1.0 - py_), bl, el, fl, gl, jl);
            set(px_ * (1.0 - py_), cl, fl, gl, hl, kl);
            set((1.0 - px_) * py_, fl, il, jl, kl, nl);
            set(px_ * py_, gl, jl, kl, ll, ol);
            let d2 = dir[0] * dir[0] + dir[1] * dir[1];
            if d2 < 1.0 / 32768.0 {
                dir = [1.0, 0.0];
            } else {
                let r = d2.sqrt().recip();
                dir = [dir[0] * r, dir[1] * r];
            }
            let len = len * 0.5;
            let len = len * len;
            let stretch = (dir[0] * dir[0] + dir[1] * dir[1]) / dir[0].abs().max(dir[1].abs());
            let len2 = [1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len];
            let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
            let clp = 1.0 / lob;
            let (mut acc, mut aw) = ([0.0f32; 3], 0.0f32);
            for (ox_, oy_, cc) in [(0.0, -1.0, b), (1.0, -1.0, c), (-1.0, 1.0, i), (0.0, 1.0, j), (0.0, 0.0, f_), (-1.0, 0.0, e), (1.0, 1.0, k), (2.0, 1.0, l), (2.0, 0.0, h_), (1.0, 0.0, g), (1.0, 2.0, o), (0.0, 2.0, n)] {
                let off = [ox_ - px_, oy_ - py_];
                let v = [(off[0] * dir[0] + off[1] * dir[1]) * len2[0], (off[0] * -dir[1] + off[1] * dir[0]) * len2[1]];
                let d2 = (v[0] * v[0] + v[1] * v[1]).min(clp);
                let wb = 2.0 / 5.0 * d2 - 1.0;
                let wa = lob * d2 - 1.0;
                let w = (25.0 / 16.0 * wb * wb - (25.0 / 16.0 - 1.0)) * (wa * wa);
                for q in 0..3 {
                    acc[q] += cc[q] * w;
                }
                aw += w;
            }
            // deringing: within the four nearest texels' range
            let mut out = [0.0f32; 4];
            for q in 0..3 {
                let lo = f_[q].min(g[q]).min(j[q]).min(k[q]);
                let hi = f_[q].max(g[q]).max(j[q]).max(k[q]);
                out[q] = (acc[q] / aw).clamp(lo, hi);
            }
            let ab = |a: f32, b: f32, t: f32| a + (b - a) * t;
            out[3] = ab(ab(f_[3], g[3], px_), ab(j[3], k[3], px_), py_);
            row[ox] = out;
        }
    });
    Image { w, h, px }
}

/// edge-preserving smoothing (a bilateral filter over the 5 x 5 neighbourhood): neighbours count by their distance
/// and by how close their colour is, so a palette's small steps blend and real edges stay; `strength` 0..1 sets how
/// large a colour difference still blends
fn smooth(src: &Image, wr: [(bool, bool); 2], strength: f32) -> Image {
    let sigma_c = 0.02 + 0.10 * strength.clamp(0.0, 1.0);
    let k = -0.5 / (sigma_c * sigma_c);
    let mut px = vec![[0.0f32; 4]; src.px.len()];
    use rayon::prelude::*;
    px.par_chunks_mut(src.w).enumerate().for_each(|(y, row)| {
        for x in 0..src.w {
            let c = src.px[y * src.w + x];
            let (mut acc, mut aw) = ([0.0f32; 3], 0.0f32);
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    let n = src.at(x as i32 + dx, y as i32 + dy, wr);
                    let d2 = (n[0] - c[0]).powi(2) + (n[1] - c[1]).powi(2) + (n[2] - c[2]).powi(2);
                    let w = (k * d2).exp() * (-((dx * dx + dy * dy) as f32) / 4.5).exp();
                    for q in 0..3 {
                        acc[q] += n[q] * w;
                    }
                    aw += w;
                }
            }
            row[x] = [acc[0] / aw, acc[1] / aw, acc[2] / aw, c[3]];
        }
    });
    Image { w: src.w, h: src.h, px }
}

/// FSR 1 RCAS: contrast-adaptive sharpening (alpha kept)
fn rcas(src: &Image, wr: [(bool, bool); 2], sharpness: f32) -> Image {
    let con = 2f32.powf(-sharpness);
    let mut px = vec![[0.0f32; 4]; src.px.len()];
    use rayon::prelude::*;
    px.par_chunks_mut(src.w).enumerate().for_each(|(y, row)| {
        let y = y as i32;
        for x in 0..src.w as i32 {
            let (b, d, e, f, h) = (src.at(x, y - 1, wr), src.at(x - 1, y, wr), src.at(x, y, wr), src.at(x + 1, y, wr), src.at(x, y + 1, wr));
            let mut lobe = f32::MIN;
            for q in 0..3 {
                let mn = b[q].min(d[q]).min(f[q]).min(h[q]);
                let mx = b[q].max(d[q]).max(f[q]).max(h[q]);
                let hit_min = mn / (4.0 * mx).max(1e-6);
                let hit_max = (1.0 - mx) / (4.0 * mn - 4.0).min(-1e-6);
                lobe = lobe.max((-hit_min).max(hit_max));
            }
            let lobe = lobe.min(0.0).max(-(0.25 - 1.0 / 16.0)) * con;
            let r = 1.0 / (4.0 * lobe + 1.0);
            let mut o = [0.0f32; 4];
            for q in 0..3 {
                o[q] = ((lobe * (b[q] + d[q] + f[q] + h[q]) + e[q]) * r).clamp(0.0, 1.0);
            }
            o[3] = e[3];
            row[x as usize] = o;
        }
    });
    Image { w: src.w, h: src.h, px }
}

/// the texture upscaled by `FACTOR` with FSR 1
pub fn fsr(img: &Image, tex: u32, sharpness: f32, denoise: f32) -> Image {
    let wr = wraps(tex);
    let mut img = img.clone();
    img.fill_transparent(wr);
    if denoise > 0.0 {
        img = smooth(&img, wr, denoise);
    }
    rcas(&easu(&img, FACTOR, wr), wr, sharpness)
}

/// a 2D picture (RGB555, bit 15 where drawn) upscaled by `f` with FSR 1, wrapping around at its edges (a BG's
/// plane), as RGB555 | bit 15 per pixel
pub fn upscale_plane(w: usize, h: usize, px: &[u16], f: usize, sharpness: f32, denoise: f32) -> Vec<u32> {
    let c = |v: u16, s: u32| ((v >> s) & 31) as f32 / 31.0;
    let mut img = Image { w, h, px: px.iter().map(|&v| [c(v, 0), c(v, 5), c(v, 10), if v & 0x8000 != 0 { 1.0 } else { 0.0 }]).collect() };
    let wr = [(true, false); 2];
    img.fill_transparent(wr);
    if denoise > 0.0 {
        img = smooth(&img, wr, denoise);
    }
    let up = rcas(&easu(&img, f, wr), wr, sharpness);
    let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0).round() as u32) & 31;
    up.px.iter().map(|p| 0x8000 | q(p[0]) | q(p[1]) << 5 | q(p[2]) << 10).collect()
}

/// a 2D picture (RGB555, bit 15 where drawn) as an `Image` (for dumping)
pub fn from_rgb555(w: usize, h: usize, px: &[u16]) -> Image {
    let c = |v: u16, s: u32| ((v >> s) & 31) as f32 / 31.0;
    Image { w, h, px: px.iter().map(|&v| [c(v, 0), c(v, 5), c(v, 10), if v & 0x8000 != 0 { 1.0 } else { 0.0 }]).collect() }
}

/// an RGBA8 picture (a mod's) resampled bilinearly to `w` x `h`, as RGB555 | bit 15 per pixel
pub fn resample_rgb555(sw: usize, sh: usize, rgba: &[u8], w: usize, h: usize) -> Vec<u32> {
    let at = |x: usize, y: usize, k: usize| rgba[(y.min(sh - 1) * sw + x.min(sw - 1)) * 4 + k] as f32 / 255.0;
    let q = |v: f32| ((v.clamp(0.0, 1.0) * 31.0).round() as u32) & 31;
    let mut out = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let fx = ((x as f32 + 0.5) * sw as f32 / w as f32 - 0.5).max(0.0);
            let fy = ((y as f32 + 0.5) * sh as f32 / h as f32 - 0.5).max(0.0);
            let (x0, y0) = (fx as usize, fy as usize);
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let c = |k: usize| {
                let top = at(x0, y0, k) * (1.0 - tx) + at(x0 + 1, y0, k) * tx;
                let bot = at(x0, y0 + 1, k) * (1.0 - tx) + at(x0 + 1, y0 + 1, k) * tx;
                top * (1.0 - ty) + bot * ty
            };
            out.push(0x8000 | q(c(0)) | q(c(1)) << 5 | q(c(2)) << 10);
        }
    }
    out
}

/// RGBA8 bytes of a picture
pub fn rgba8(img: &Image) -> Vec<u8> {
    img.px.iter().flat_map(|p| p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)).collect()
}

/// where an asset's replacement (or dump) lives under a hires root (`patch/hires`): `<root>/<kind>/<name>.png`
/// when the asset index names it, else `<root>/<kind>/<key>.png`; the named place is tried first
pub fn hires_paths(root: &Path, kind: &str, key: u64, name: Option<&str>) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = name.map(|n| root.join(kind).join(format!("{n}.png"))).into_iter().collect();
    v.push(root.join(kind).join(format!("{key:016x}.png")));
    v
}

/// a mod's replacement for an asset (any size), as RGBA8
pub fn load_mod(root: &Path, kind: &str, key: u64, name: Option<&str>) -> Option<(usize, usize, Vec<u8>)> {
    hires_paths(root, kind, key, name).iter().find_map(|p| load_png(p))
}

/// whether there is a replacement for an asset
pub fn has_mod(root: &Path, kind: &str, key: u64, name: Option<&str>) -> bool {
    hires_paths(root, kind, key, name).iter().any(|p| p.is_file())
}

fn load_png(path: &Path) -> Option<(usize, usize, Vec<u8>)> {
    let f = std::fs::File::open(path).ok()?;
    let mut d = png::Decoder::new(std::io::BufReader::new(f));
    d.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut r = d.read_info().ok()?;
    let mut buf = vec![0; r.output_buffer_size()?];
    let info = r.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf[..w * h * 4].to_vec(),
        png::ColorType::Rgb => buf[..w * h * 3].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..w * h * 2].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Grayscale => buf[..w * h].iter().flat_map(|&c| [c, c, c, 255]).collect(),
        _ => return None,
    };
    Some((w, h, rgba))
}

/// an asset written at its own size where its replacement would go (`hires_paths`' first place), to start a mod
/// from: a dump directory can be copied into `patch/hires` as it is
pub fn dump(root: &Path, kind: &str, key: u64, name: Option<&str>, img: &Image) {
    let path = hires_paths(root, kind, key, name).swap_remove(0);
    if path.exists() {
        return;
    }
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let Ok(f) = std::fs::File::create(&path) else { return };
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), img.w as u32, img.h as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    if let Ok(mut wr) = e.write_header() {
        let _ = wr.write_image_data(&rgba8(img));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fsr_keeps_flat_colour_and_scales() {
        let img = Image { w: 4, h: 4, px: vec![[0.25, 0.5, 0.75, 1.0]; 16] };
        let out = fsr(&img, 3 << 16, 0.25, 0.0);
        assert_eq!((out.w, out.h), (16, 16));
        for p in &out.px {
            for q in 0..4 {
                assert!((p[q] - img.px[0][q]).abs() < 1e-4, "{p:?}");
            }
        }
    }

    #[test]
    fn fsr_stays_within_the_texels_range() {
        // a hard vertical edge: the result never overshoots (deringing) and keeps both sides
        let px: Vec<[f32; 4]> = (0..64).map(|i| if i % 8 < 4 { [0.0, 0.0, 0.0, 1.0] } else { [1.0, 1.0, 1.0, 1.0] }).collect();
        let out = fsr(&Image { w: 8, h: 8, px }, 0, 0.25, 0.0);
        assert!(out.px.iter().all(|p| (0.0..=1.0).contains(&p[0])));
        assert!(out.px[0][0] < 0.01 && out.px[31][0] > 0.99);
    }

    #[test]
    fn keys_depend_on_palette_not_place() {
        let mut img = vec![0u8; 0x80000];
        let mut pals = vec![0u8; 0x20000];
        img[0..32].copy_from_slice(&[0x12; 32]);
        img[0x100..0x120].copy_from_slice(&[0x12; 32]);
        pals[0..32].copy_from_slice(&[7; 32]);
        // 16-colour 8x8 at offset 0 and at 0x100, same palette
        let t = |base: u32| 3 << 26 | base / 8;
        assert_eq!(key(&img, &pals, t(0), 0), key(&img, &pals, t(0x100), 0));
        pals[2] = 9;
        assert_ne!(key(&img, &pals, t(0), 0), key(&img, &pals, t(0x100), 1));
        let a = key(&img, &pals, t(0), 0);
        pals[0..32].copy_from_slice(&[8; 32]);
        assert_ne!(a, key(&img, &pals, t(0), 0));
    }
}
