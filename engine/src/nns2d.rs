//! NitroSystem G2D resources: NCLR (palette), NCGR (tiles), NSCR (screen), NCER (cells), NANR (animations).
//! Reference implementation: tools/nns2d.py (the loaders are diffed against it, see docs/CONTENT_LAYOUT.md).
//!
//! File = header{sig u32, byteOrder u16, version u16, fileSize u32, headerSize u16, nBlocks u16}, then blocks {kind u32, size u32, data}.
//! Signatures are stored byte-reversed ('RLCN' = NCLR).  Offsets inside a block struct are relative to the byte after the block header.

use std::collections::HashMap;

fn u16at(d: &[u8], p: usize) -> Result<u32, String> {
    d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32).ok_or_else(|| format!("read past end at {p:#x}"))
}
fn u32at(d: &[u8], p: usize) -> Result<u32, String> {
    d.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| format!("read past end at {p:#x}"))
}
fn i16at(d: &[u8], p: usize) -> Result<i32, String> {
    Ok(u16at(d, p)? as u16 as i16 as i32)
}
fn i32at(d: &[u8], p: usize) -> Result<i32, String> {
    Ok(u32at(d, p)? as i32)
}

/// signature (e.g. "NCLR") and block name -> (data offset, data size)
pub fn blocks(d: &[u8]) -> Result<(String, HashMap<String, (usize, usize)>), String> {
    if d.len() < 16 {
        return Err("too short".into());
    }
    let hsz = u16at(d, 12)? as usize;
    let nb = u16at(d, 14)? as usize;
    let mut out = HashMap::new();
    let mut p = hsz;
    for _ in 0..nb {
        let size = u32at(d, p + 4)? as usize;
        let name: String = d.get(p..p + 4).ok_or("block header past end")?.iter().rev().map(|&b| b as char).collect();
        out.insert(name, (p + 8, size.wrapping_sub(8)));
        if size < 8 {
            return Err("bad block size".into());
        }
        p += size;
    }
    let sig: String = d[0..4].iter().rev().map(|&b| b as char).collect();
    Ok((sig, out))
}

fn block<'a>(b: &'a HashMap<String, (usize, usize)>, name: &str) -> Result<usize, String> {
    b.get(name).map(|x| x.0).ok_or_else(|| format!("missing block {name}"))
}

pub type Rgb = [u8; 3];

pub fn rgb555(c: u32) -> Rgb {
    [((c & 31) * 255 / 31) as u8, (((c >> 5) & 31) * 255 / 31) as u8, (((c >> 10) & 31) * 255 / 31) as u8]
}

// ------------------------------------------------------------------ images
#[derive(Clone, Debug)]
pub struct Image {
    pub w: usize,
    pub h: usize,
    /// RGBA8
    pub buf: Vec<u8>,
}

impl Image {
    pub fn new(w: usize, h: usize) -> Image {
        Image { w, h, buf: vec![0; w * h * 4] }
    }
    pub fn put(&mut self, x: i64, y: i64, c: Rgb) {
        if x >= 0 && y >= 0 && (x as usize) < self.w && (y as usize) < self.h {
            let o = (y as usize * self.w + x as usize) * 4;
            self.buf[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
}

// ------------------------------------------------------------------ NCLR
pub struct Nclr {
    pub fmt: u32,
    pub colors: Vec<Rgb>,
    pub bpp: u32,
}

pub fn parse_nclr(d: &[u8]) -> Result<Nclr, String> {
    let (_, b) = blocks(d)?;
    let o = block(&b, "PLTT")?;
    let (fmt, sz, roff) = (u32at(d, o)?, u32at(d, o + 8)? as usize, u32at(d, o + 12)? as usize);
    let start = (o + roff).min(d.len());
    let raw = &d[start..(start + sz).min(d.len())];
    let colors = raw.chunks_exact(2).map(|c| rgb555(u16::from_le_bytes([c[0], c[1]]) as u32)).collect();
    Ok(Nclr { fmt, colors, bpp: if fmt == 3 { 4 } else { 8 } })
}

// ------------------------------------------------------------------ NCGR
pub struct Ncgr {
    pub tiles_h: u32,
    pub tiles_w: u32,
    pub bpp: u32,
    pub mapping: u32,
    pub bitmap: bool,
    pub raw: Vec<u8>,
}

pub fn parse_ncgr(d: &[u8]) -> Result<Ncgr, String> {
    let (_, b) = blocks(d)?;
    let o = block(&b, "CHAR")?;
    let (h, w) = (u16at(d, o)?, u16at(d, o + 2)?);
    let (pfmt, mapping, cfmt, sz, roff) = (u32at(d, o + 4)?, u32at(d, o + 8)?, u32at(d, o + 12)?, u32at(d, o + 16)? as usize, u32at(d, o + 20)? as usize);
    let start = (o + roff).min(d.len());
    let raw = d[start..(start + sz).min(d.len())].to_vec();
    Ok(Ncgr { tiles_h: h, tiles_w: w, bpp: if pfmt == 3 { 4 } else { 8 }, mapping, bitmap: (cfmt & 0xff) == 1, raw })
}

impl Ncgr {
    fn tile_bytes(&self) -> usize {
        if self.bpp == 4 { 32 } else { 64 }
    }
    /// 8x8 palette indices of the tile at byte offset `off`
    pub fn tile_at(&self, off: usize) -> Option<[u8; 64]> {
        let bs = self.tile_bytes();
        let t = self.raw.get(off..off + bs)?;
        let mut px = [0u8; 64];
        if self.bpp == 8 {
            px.copy_from_slice(t);
        } else {
            for (i, &b) in t.iter().enumerate() {
                px[i * 2] = b & 15;
                px[i * 2 + 1] = b >> 4;
            }
        }
        Some(px)
    }
    pub fn tile(&self, idx: usize) -> Option<[u8; 64]> {
        self.tile_at(idx * self.tile_bytes())
    }
}

fn pal_color(pal: Option<&Nclr>, bpp: u32, pal_index: u32, v: u8) -> Rgb {
    match pal {
        None => {
            let g = if bpp == 4 { v.wrapping_mul(17) } else { v };
            [g, g, g]
        }
        Some(p) => {
            let base = if bpp == 4 { pal_index as usize * 16 } else { 0 };
            p.colors.get(base + v as usize).copied().unwrap_or([255, 0, 255])
        }
    }
}

/// the whole tile sheet as one image
pub fn ncgr_image(ch: &Ncgr, pal: Option<&Nclr>) -> Image {
    let bs = ch.tile_bytes();
    let n = ch.raw.len() / bs;
    let w = if ch.tiles_w != 0 && ch.tiles_w != 0xffff { ch.tiles_w as usize } else { n.max(1).min(32) };
    let h = if ch.tiles_h != 0 && ch.tiles_h != 0xffff { ch.tiles_h as usize } else { (n + w - 1) / w };
    let mut cv = Image::new(w * 8, h * 8);
    for t in 0..n.min(w * h) {
        let px = match ch.tile(t) {
            Some(p) => p,
            None => continue,
        };
        let (tx, ty) = ((t % w) * 8, (t / w) * 8);
        for y in 0..8 {
            for x in 0..8 {
                let v = px[y * 8 + x];
                if v != 0 || (ch.bpp == 8 && pal.is_none()) {
                    cv.put((tx + x) as i64, (ty + y) as i64, pal_color(pal, ch.bpp, 0, v));
                }
            }
        }
    }
    cv
}

// ------------------------------------------------------------------ NSCR
pub struct Nscr {
    pub w: u32,
    pub h: u32,
    pub color_mode: u32,
    pub format: u32,
    pub raw: Vec<u8>,
}

pub fn parse_nscr(d: &[u8]) -> Result<Nscr, String> {
    let (_, b) = blocks(d)?;
    let o = block(&b, "SCRN")?;
    let (w, h, cmode, fmt, sz) = (u16at(d, o)?, u16at(d, o + 2)?, u16at(d, o + 4)?, u16at(d, o + 6)?, u32at(d, o + 8)? as usize);
    let start = (o + 12).min(d.len());
    Ok(Nscr { w, h, color_mode: cmode, format: fmt, raw: d[start..(start + sz).min(d.len())].to_vec() })
}

pub fn nscr_image(sc: &Nscr, ch: &Ncgr, pal: Option<&Nclr>) -> Image {
    let (tw, th) = ((sc.w / 8) as usize, (sc.h / 8) as usize);
    let mut cv = Image::new(sc.w as usize, sc.h as usize);
    let ents: Vec<u32> = if sc.format == 1 {
        sc.raw.iter().map(|&b| b as u32).collect()
    } else {
        sc.raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect()
    };
    for (i, &e) in ents.iter().take(tw * th).enumerate() {
        let (tile, hf, vf, pl) = if sc.format == 1 { (e, 0, 0, 0) } else { (e & 0x3ff, (e >> 10) & 1, (e >> 11) & 1, e >> 12) };
        let px = match ch.tile(tile as usize) {
            Some(p) => p,
            None => continue,
        };
        let key = if ch.bpp == 4 { pl } else { 0 };
        let (tx, ty) = ((i % tw) * 8, (i / tw) * 8);
        for y in 0..8usize {
            let sy = if vf == 1 { 7 - y } else { y };
            for x in 0..8usize {
                let v = px[sy * 8 + if hf == 1 { 7 - x } else { x }];
                if v == 0 && ch.bpp == 4 {
                    continue;
                }
                cv.put((tx + x) as i64, (ty + y) as i64, pal_color(pal, ch.bpp, key, v));
            }
        }
    }
    cv
}

/// A full-screen BG picture (the game's backdrops): the first 32x24 tiles of the screen map (256x192), turned into the reading orientation of the
/// sideways-held console (168 wide, 256 high after dropping the three non-picture tile rows).  The NSCR header claims 384x256 but the rest of the map is not part of the picture.
pub fn bg_picture(sc: &Nscr, ch: &Ncgr, pal: Option<&Nclr>) -> Image {
    let ents: Vec<u32> = sc.raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect();
    let mut cv = Image::new(256, 192);
    for (i, &e) in ents.iter().take(32 * 24).enumerate() {
        let (tile, hf, vf, pl) = (e & 0x3ff, (e >> 10) & 1, (e >> 11) & 1, e >> 12);
        let px = match ch.tile(tile as usize) {
            Some(p) => p,
            None => continue,
        };
        let key = if ch.bpp == 4 { pl } else { 0 };
        let (tx, ty) = ((i % 32) * 8, (i / 32) * 8);
        for y in 0..8usize {
            let sy = if vf == 1 { 7 - y } else { y };
            for x in 0..8usize {
                let v = px[sy * 8 + if hf == 1 { 7 - x } else { x }];
                cv.put((tx + x) as i64, (ty + y) as i64, pal_color(pal, ch.bpp, key, v));
            }
        }
    }
    // rotate 90 degrees counter-clockwise: (x, y) -> (y, 255 - x); the last three tile rows (rows 21-23) hold other tile data, not picture
    let mut out = Image::new(168, 256);
    for y in 0..168usize {
        for x in 0..256usize {
            let (nx, ny) = (y, 255 - x);
            let (s, d) = ((y * 256 + x) * 4, (ny * 168 + nx) * 4);
            out.buf[d..d + 4].copy_from_slice(&cv.buf[s..s + 4]);
        }
    }
    out
}

/// A backdrop's BG1 picture as the game lays it out: 48 x 32 tiles (384 x 256). The character data is the picture in linear
/// tile order (tile = ty * 48 + tx); the screen map (a 32-column block, then the remaining 16 columns at 1024 + ty * 16 +
/// tx - 32) holds that same index in only 10 bits, so every tile from 1024 on is stored as 0: the tile comes from the position,
/// only the palette and flips from the map. (Reading tiles through the map was the source of the noise behind the heroine's
/// head; docs/RENDER_PRIMITIVES.md "Backdrop".)
pub fn bg_panorama(sc: &Nscr, ch: &Ncgr, pal: Option<&Nclr>) -> Image {
    let ents: Vec<u32> = sc.raw.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]]) as u32).collect();
    let mut img = Image::new(384, 256);
    for ty in 0..32usize {
        for tx in 0..48usize {
            let i = if tx < 32 { ty * 32 + tx } else { 1024 + ty * 16 + tx - 32 };
            let e = ents.get(i).copied().unwrap_or(0);
            let px = match ch.tile(ty * 48 + tx) {
                Some(p) => p,
                None => continue,
            };
            let (hf, vf, pl) = ((e >> 10) & 1, (e >> 11) & 1, e >> 12);
            let key = if ch.bpp == 4 { pl } else { 0 };
            for y in 0..8usize {
                let sy = if vf == 1 { 7 - y } else { y };
                for x in 0..8usize {
                    let v = px[sy * 8 + if hf == 1 { 7 - x } else { x }];
                    img.put((tx * 8 + x) as i64, (ty * 8 + y) as i64, pal_color(pal, ch.bpp, key, v));
                }
            }
        }
    }
    img
}

/// the 256 x 192 screen window of a BG1 panorama at scroll (`hofs`, `vofs`): 512 x 256 wrap (the map's columns past 384 are
/// empty), as the BG engine reads it
pub fn bg_window(pano: &Image, hofs: i32, vofs: i32) -> Image {
    let mut out = Image::new(256, 192);
    for y in 0..192usize {
        let sy = ((y as i32 + vofs).rem_euclid(256)) as usize;
        for x in 0..256usize {
            let sx = ((x as i32 + hofs).rem_euclid(512)) as usize;
            if sx >= pano.w || sy >= pano.h {
                continue;
            }
            let (s, d) = ((sy * pano.w + sx) * 4, (y * 256 + x) * 4);
            out.buf[d..d + 4].copy_from_slice(&pano.buf[s..s + 4]);
        }
    }
    out
}

/// the console is held sideways: a landscape screen image turned into reading orientation (90 degrees counter-clockwise,
/// (x, y) -> (y, w - 1 - x))
pub fn rotate_ccw(img: &Image) -> Image {
    let mut out = Image::new(img.h, img.w);
    for y in 0..img.h {
        for x in 0..img.w {
            let (s, d) = ((y * img.w + x) * 4, ((img.w - 1 - x) * img.h + y) * 4);
            out.buf[d..d + 4].copy_from_slice(&img.buf[s..s + 4]);
        }
    }
    out
}

// ------------------------------------------------------------------ NCER
#[derive(Clone, Debug, PartialEq)]
pub struct Oam {
    pub x: i32,
    pub y: i32,
    pub rot: u32,
    pub hide: bool,
    pub c256: u32,
    pub w: u32,
    pub h: u32,
    pub hflip: u32,
    pub vflip: u32,
    pub tile: u32,
    pub prio: u32,
    pub pal: u32,
}

pub struct Cell {
    pub attr: u32,
    pub bounds: Option<[i32; 4]>,
    pub oams: Vec<[u32; 3]>,
}

pub struct Ncer {
    pub mapping: u32,
    pub has_br: bool,
    pub cells: Vec<Cell>,
}

pub fn parse_ncer(d: &[u8]) -> Result<Ncer, String> {
    let (_, b) = blocks(d)?;
    let o = block(&b, "CEBK")?;
    let (ncell, attr) = (u16at(d, o)? as usize, u16at(d, o + 2)?);
    let (cells_off, mapping) = (u32at(d, o + 4)? as usize, u32at(d, o + 8)?);
    let has_br = attr & 1 == 1;
    let stride = if has_br { 16 } else { 8 };
    let oam_base = o + cells_off + ncell * stride;
    let mut cells = Vec::with_capacity(ncell);
    for i in 0..ncell {
        let p = o + cells_off + i * stride;
        let (noam, cattr, oam_off) = (u16at(d, p)? as usize, u16at(d, p + 2)?, u32at(d, p + 4)? as usize);
        let bounds = if has_br { Some([i16at(d, p + 8)?, i16at(d, p + 10)?, i16at(d, p + 12)?, i16at(d, p + 14)?]) } else { None };
        let mut oams = Vec::with_capacity(noam);
        for k in 0..noam {
            let q = oam_base + oam_off + 6 * k;
            oams.push([u16at(d, q)?, u16at(d, q + 2)?, u16at(d, q + 4)?]);
        }
        cells.push(Cell { attr: cattr, bounds, oams });
    }
    Ok(Ncer { mapping, has_br, cells })
}

fn shape_size(shape: u32, size: u32) -> Result<(u32, u32), String> {
    Ok(match (shape, size) {
        (0, 0) => (8, 8),
        (0, 1) => (16, 16),
        (0, 2) => (32, 32),
        (0, 3) => (64, 64),
        (1, 0) => (16, 8),
        (1, 1) => (32, 8),
        (1, 2) => (32, 16),
        (1, 3) => (64, 32),
        (2, 0) => (8, 16),
        (2, 1) => (8, 32),
        (2, 2) => (16, 32),
        (2, 3) => (32, 64),
        _ => return Err(format!("invalid OAM shape/size {shape}/{size}")),
    })
}

pub fn oam_decode(a0: u32, a1: u32, a2: u32) -> Result<Oam, String> {
    let mut y = (a0 & 0xff) as i32;
    let rot = (a0 >> 8) & 1;
    let dbl = (a0 >> 9) & 1;
    let c256 = (a0 >> 13) & 1;
    let shape = a0 >> 14;
    let mut x = (a1 & 0x1ff) as i32;
    if x >= 256 {
        x -= 512;
    }
    if y >= 128 {
        y -= 256;
    }
    let (w, h) = shape_size(shape, a1 >> 14)?;
    Ok(Oam { x, y, rot, hide: rot == 0 && dbl == 1, c256, w, h, hflip: (a1 >> 12) & 1, vflip: (a1 >> 13) & 1, tile: a2 & 0x3ff, prio: (a2 >> 10) & 3, pal: a2 >> 12 })
}

/// Compose one cell into an image; returns (image, origin) or None for an empty cell.
pub fn render_cell(cell: &Cell, mapping: u32, ch: &Ncgr, pal: Option<&Nclr>) -> Result<Option<(Image, (i32, i32))>, String> {
    let items: Result<Vec<Oam>, String> = cell.oams.iter().map(|o| oam_decode(o[0], o[1], o[2])).collect();
    let items = items?;
    if items.is_empty() {
        return Ok(None);
    }
    let x0 = items.iter().map(|i| i.x).min().unwrap();
    let y0 = items.iter().map(|i| i.y).min().unwrap();
    let x1 = items.iter().map(|i| i.x + i.w as i32).max().unwrap();
    let y1 = items.iter().map(|i| i.y + i.h as i32).max().unwrap();
    let mut cv = Image::new((x1 - x0).max(1) as usize, (y1 - y0).max(1) as usize);
    let boundary = 32usize << (if mapping <= 3 { mapping } else { 0 });
    let bs = ch.tile_bytes();
    for it in items.iter().rev() {
        if it.hide {
            continue;
        }
        let (tw, th) = ((it.w / 8) as usize, (it.h / 8) as usize);
        for ty in 0..th {
            for tx in 0..tw {
                let px = match ch.tile_at(it.tile as usize * boundary + (ty * tw + tx) * bs) {
                    Some(p) => p,
                    None => continue,
                };
                for y in 0..8usize {
                    for x in 0..8usize {
                        let v = px[y * 8 + x];
                        if v == 0 {
                            continue;
                        }
                        let xx = (if it.hflip == 1 { 7 - x } else { x }) + (if it.hflip == 1 { tw - 1 - tx } else { tx }) * 8;
                        let yy = (if it.vflip == 1 { 7 - y } else { y }) + (if it.vflip == 1 { th - 1 - ty } else { ty }) * 8;
                        cv.put((it.x - x0) as i64 + xx as i64, (it.y - y0) as i64 + yy as i64, pal_color(pal, ch.bpp, it.pal, v));
                    }
                }
            }
        }
    }
    Ok(Some((cv, (x0, y0))))
}

// ------------------------------------------------------------------ NANR
#[derive(Debug, Clone, PartialEq)]
pub struct AnimFrame {
    pub index: u32,
    pub rot: Option<u32>,
    pub sx: Option<f64>,
    pub sy: Option<f64>,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub frames: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AnimSeq {
    pub loop_start: u32,
    pub kind: u32,
    pub element: u32,
    pub play_mode: u32,
    pub frames: Vec<AnimFrame>,
}

pub fn parse_nanr(d: &[u8]) -> Result<Vec<AnimSeq>, String> {
    let (_, b) = blocks(d)?;
    let o = block(&b, "ABNK")?;
    let nseq = u16at(d, o)? as usize;
    let (seq_off, frm_off, cont_off) = (u32at(d, o + 4)? as usize, u32at(d, o + 8)? as usize, u32at(d, o + 12)? as usize);
    let mut seqs = Vec::with_capacity(nseq);
    for i in 0..nseq {
        let p = o + seq_off + i * 16;
        let (nf, lp, atype, mode, fa) = (u16at(d, p)? as usize, u16at(d, p + 2)?, u32at(d, p + 4)?, u32at(d, p + 8)?, u32at(d, p + 12)? as usize);
        let (elem, kind) = (atype & 0xffff, atype >> 16);
        let mut frames = Vec::with_capacity(nf);
        for k in 0..nf {
            let q0 = o + frm_off + fa + k * 8;
            let (content, dur) = (u32at(d, q0)? as usize, u16at(d, q0 + 4)?);
            let q = o + cont_off + content;
            let fr = match elem {
                0 => AnimFrame { index: u16at(d, q)?, rot: None, sx: None, sy: None, x: None, y: None, frames: dur },
                1 => AnimFrame {
                    index: u16at(d, q)?,
                    rot: Some(u16at(d, q + 2)?),
                    sx: Some(i32at(d, q + 4)? as f64 / 4096.0),
                    sy: Some(i32at(d, q + 8)? as f64 / 4096.0),
                    x: Some(i16at(d, q + 12)?),
                    y: Some(i16at(d, q + 14)?),
                    frames: dur,
                },
                _ => AnimFrame { index: u16at(d, q)?, rot: None, sx: None, sy: None, x: Some(i16at(d, q + 4)?), y: Some(i16at(d, q + 6)?), frames: dur },
            };
            frames.push(fr);
        }
        seqs.push(AnimSeq { loop_start: lp, kind, element: elem, play_mode: mode, frames });
    }
    Ok(seqs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(sig: &[u8; 4], block: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend(sig.iter().rev());
        d.extend_from_slice(&[0xff, 0xfe, 0, 1]);
        let size = 16 + 8 + body.len();
        d.extend_from_slice(&(size as u32).to_le_bytes());
        d.extend_from_slice(&[16, 0, 1, 0]);
        d.extend(block.iter().rev());
        d.extend_from_slice(&((8 + body.len()) as u32).to_le_bytes());
        d.extend_from_slice(body);
        d
    }

    #[test]
    fn palette_tiles_and_screen() {
        // 16-colour palette: entry 1 = pure red
        let mut pl = vec![];
        for v in [3u32, 0, 0, 0, 0x20, 0x18] {
            pl.extend_from_slice(&v.to_le_bytes());
        }
        // PLTT: fmt, ext, size, then offset to data (0x18): colours start at +0x10 -> use roff=0x10
        let mut body = Vec::new();
        for v in [3u32, 0, 32, 0x10] {
            body.extend_from_slice(&v.to_le_bytes());
        }
        let mut cols = vec![0u16; 16];
        cols[1] = 0x001f;
        for c in cols {
            body.extend_from_slice(&c.to_le_bytes());
        }
        let nclr = parse_nclr(&file(b"NCLR", b"PLTT", &body)).unwrap();
        assert_eq!(nclr.colors[1], [255, 0, 0]);
        assert_eq!(nclr.bpp, 4);

        // 1x1 tile, 4bpp, every pixel = index 1
        let mut cb = Vec::new();
        cb.extend_from_slice(&1u16.to_le_bytes());
        cb.extend_from_slice(&1u16.to_le_bytes());
        for v in [3u32, 0, 0, 32, 0x18] {
            cb.extend_from_slice(&v.to_le_bytes());
        }
        cb.extend_from_slice(&[0x11; 32]);
        let ncgr = parse_ncgr(&file(b"NCGR", b"CHAR", &cb)).unwrap();
        let img = ncgr_image(&ncgr, Some(&nclr));
        assert_eq!((img.w, img.h), (8, 8));
        assert_eq!(&img.buf[0..4], &[255, 0, 0, 255]);

        // 1x1 screen, entry = tile 0, hflip
        let mut sb = Vec::new();
        for v in [8u16, 8, 0, 0] {
            sb.extend_from_slice(&v.to_le_bytes());
        }
        sb.extend_from_slice(&2u32.to_le_bytes());
        sb.extend_from_slice(&(1u16 << 10).to_le_bytes());
        let nscr = parse_nscr(&file(b"NSCR", b"SCRN", &sb)).unwrap();
        let s = nscr_image(&nscr, &ncgr, Some(&nclr));
        assert_eq!(&s.buf[28..32], &[255, 0, 0, 255]);
    }

    #[test]
    fn oam_decoding() {
        let o = oam_decode(0x4005, 0x8000 | 0x1ff, 0).unwrap(); // shape 1 (wide), size 2 -> 32x16, x = -1
        assert_eq!((o.w, o.h, o.x, o.y), (32, 16, -1, 5));
        assert!(oam_decode(0xc000, 0, 0).is_err());
    }
}
