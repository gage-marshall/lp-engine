//! NNS G2D bitmap fonts (NFTR).  Reference: tools/nftr.py.  File: 'RTFN' header, blocks FINF (font info), CGLP (glyph bitmaps),
//! CWDH (per-glyph widths, linked list), PAMC (character maps, linked list); block signatures are stored byte-reversed.

use crate::bytes::*;
use crate::nns2d::Image;
use std::collections::HashMap;

pub struct Font {
    pub cell_w: usize,
    pub cell_h: usize,
    pub bpp: usize,
    pub rot: usize,
    pub height: usize,
    pub linefeed: i32,
    pub alter: usize,
    default_width: (i32, u32, i32),
    cell_size: usize,
    glyphs: Vec<u8>,
    nglyphs: usize,
    /// glyph index -> (left bearing, glyph width, advance)
    widths: HashMap<usize, (i32, u32, i32)>,
    cmap: HashMap<u32, usize>,
}

impl Font {
    pub fn parse(d: &[u8]) -> Result<Font, String> {
        if d.get(0..4) != Some(b"RTFN") {
            return Err("not an NFTR".into());
        }
        let (ver, hsz, nb) = (u16at(d, 6)?, u16at(d, 12)? as usize, u16at(d, 14)? as usize);
        let mut blocks: HashMap<String, (usize, usize)> = HashMap::new();
        let mut p = hsz;
        for _ in 0..nb {
            let size = u32at(d, p + 4)? as usize;
            let name: String = d.get(p..p + 4).ok_or("block past end")?.iter().rev().map(|&b| b as char).collect();
            blocks.entry(name).or_insert((p + 8, size - 8));
            p += size;
        }
        let o = blocks.get("FINF").ok_or("no FINF")?.0;
        let linefeed = u8at(d, o + 1)? as u8 as i8 as i32;
        let alter = u16at(d, o + 2)? as usize;
        let default_width = (u8at(d, o + 4)? as u8 as i8 as i32, u8at(d, o + 5)?, u8at(d, o + 6)? as u8 as i8 as i32);
        let (pglyph, pwidth, pmap) = (u32at(d, o + 8)? as usize, u32at(d, o + 12)? as usize, u32at(d, o + 16)? as usize);
        let height = if ver >= 0x102 { u8at(d, o + 20)? as usize } else { 0 };
        let g = pglyph;
        let (cell_w, cell_h, cell_size) = (u8at(d, g)? as usize, u8at(d, g + 1)? as usize, u16at(d, g + 2)? as usize);
        let (bpp, flags) = (u8at(d, g + 6)? as usize, u8at(d, g + 7)?);
        let glyph_off = g + 8;
        let nglyphs = (blocks.get("CGLP").ok_or("no CGLP")?.1 - 8) / cell_size.max(1);
        let mut widths = HashMap::new();
        let mut w = pwidth;
        while w != 0 {
            let (b, e, nxt) = (u16at(d, w)? as usize, u16at(d, w + 2)? as usize, u32at(d, w + 4)? as usize);
            for i in 0..=(e - b) {
                widths.insert(b + i, (u8at(d, w + 8 + 3 * i)? as u8 as i8 as i32, u8at(d, w + 9 + 3 * i)?, u8at(d, w + 10 + 3 * i)? as u8 as i8 as i32));
            }
            w = nxt;
        }
        let mut cmap = HashMap::new();
        let mut m = pmap;
        while m != 0 {
            let (cb, ce, meth, nxt) = (u16at(d, m)?, u16at(d, m + 2)?, u16at(d, m + 4)?, u32at(d, m + 8)? as usize);
            match meth {
                0 => {
                    let idx0 = u16at(d, m + 12)? as usize;
                    for c in cb..=ce {
                        cmap.insert(c, idx0 + (c - cb) as usize);
                    }
                }
                1 => {
                    for c in cb..=ce {
                        let i = u16at(d, m + 12 + 2 * (c - cb) as usize)?;
                        if i != 0xffff {
                            cmap.insert(c, i as usize);
                        }
                    }
                }
                _ => {
                    let cnt = u16at(d, m + 12)? as usize;
                    for k in 0..cnt {
                        cmap.insert(u16at(d, m + 14 + 4 * k)?, u16at(d, m + 16 + 4 * k)? as usize);
                    }
                }
            }
            m = nxt;
        }
        Ok(Font { cell_w, cell_h, bpp, rot: ((flags >> 1) & 3) as usize, height, linefeed, alter, default_width, cell_size, glyphs: d[glyph_off..glyph_off + nglyphs * cell_size].to_vec(), nglyphs, widths, cmap })
    }

    pub fn glyph_count(&self) -> usize {
        self.nglyphs
    }

    pub fn char_count(&self) -> usize {
        self.cmap.len()
    }

    pub fn chars_sorted(&self) -> Vec<(u32, usize)> {
        let mut v: Vec<(u32, usize)> = self.cmap.iter().map(|(&c, &g)| (c, g)).collect();
        v.sort();
        v
    }

    pub fn glyph_index(&self, ch: char) -> usize {
        self.cmap.get(&(ch as u32)).copied().unwrap_or(self.alter)
    }

    /// (left bearing, glyph width, advance)
    pub fn metrics(&self, gi: usize) -> (i32, u32, i32) {
        self.widths.get(&gi).copied().unwrap_or(self.default_width)
    }

    /// glyph bitmap in display orientation: (width, height, rows of 0..2^bpp-1 values)
    pub fn glyph_bitmap(&self, gi: usize) -> (usize, usize, Vec<Vec<u8>>) {
        if gi >= self.nglyphs {
            return (0, 0, Vec::new());
        }
        let raw = &self.glyphs[gi * self.cell_size..(gi + 1) * self.cell_size];
        let (w, h, bpp) = (self.cell_w, self.cell_h, self.bpp);
        let total = raw.len() * 8;
        let mut px = Vec::with_capacity(w * h);
        for i in 0..w * h {
            let bit0 = i * bpp;
            let mut v = 0u8;
            for b in 0..bpp {
                let bi = bit0 + b;
                if bi < total {
                    v = (v << 1) | ((raw[bi / 8] >> (7 - bi % 8)) & 1);
                } else {
                    v <<= 1;
                }
            }
            px.push(v);
        }
        let mut rows: Vec<Vec<u8>> = (0..h).map(|y| px[y * w..(y + 1) * w].to_vec()).collect();
        for _ in 0..self.rot {
            // stored rotated clockwise: rotate back counter-clockwise
            let (rh, rw) = (rows.len(), rows[0].len());
            let mut n = vec![vec![0u8; rh]; rw];
            for y in 0..rh {
                for x in 0..rw {
                    n[rw - 1 - x][y] = rows[y][x];
                }
            }
            rows = n;
        }
        (rows[0].len(), rows.len(), rows)
    }

    /// width in font pixels of a line of text
    pub fn line_width(&self, line: &str) -> i32 {
        line.chars().map(|c| self.metrics(self.glyph_index(c)).2).sum()
    }

    /// draw `text` (newlines break lines) into `img` at (x, y) in `scale`x enlarged pixels; `chars` limits how many characters are drawn (typewriter).
    /// Returns the number of characters drawn.  `color` is the text colour; the glyph value is the alpha.
    pub fn draw(&self, img: &mut Image, text: &str, x: i32, y: i32, scale: i32, color: [u8; 3], chars: usize) -> usize {
        let lh = if self.linefeed != 0 { self.height as i32 + self.linefeed } else { self.cell_h as i32 + 1 };
        let mx = ((1u32 << self.bpp) - 1) as f32;
        let mut drawn = 0usize;
        for (li, line) in text.split('\n').enumerate() {
            let mut cx = 0i32;
            for ch in line.chars() {
                if drawn >= chars {
                    return drawn;
                }
                let gi = self.glyph_index(ch);
                let (l, _gw, cw) = self.metrics(gi);
                let (w, h, rows) = self.glyph_bitmap(gi);
                for gy in 0..h {
                    for gx in 0..w {
                        let v = rows[gy][gx];
                        if v == 0 {
                            continue;
                        }
                        let a = (255.0 * v as f32 / mx) as u8;
                        let (px0, py0) = (x + (cx + l + gx as i32) * scale, y + (li as i32 * lh + gy as i32) * scale);
                        for sy in 0..scale {
                            for sx in 0..scale {
                                let (px, py) = (px0 + sx, py0 + sy);
                                if px >= 0 && py >= 0 && (px as usize) < img.w && (py as usize) < img.h {
                                    let o = (py as usize * img.w + px as usize) * 4;
                                    // source-over onto the (possibly transparent) overlay
                                    let da = img.buf[o + 3] as f32 / 255.0;
                                    let sa = a as f32 / 255.0;
                                    let oa = sa + da * (1.0 - sa);
                                    for k in 0..3 {
                                        let dc = img.buf[o + k] as f32;
                                        let sc = color[k] as f32;
                                        img.buf[o + k] = if oa > 0.0 { ((sc * sa + dc * da * (1.0 - sa)) / oa) as u8 } else { 0 };
                                    }
                                    img.buf[o + 3] = (oa * 255.0) as u8;
                                }
                            }
                        }
                    }
                }
                cx += cw;
                drawn += 1;
            }
        }
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a tiny synthetic NFTR with one 2x2 1-bpp glyph for 'A' (stored rotated 90 degrees: value rows are transposed back on read)
    fn tiny() -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(b"RTFN");
        d.extend_from_slice(&[0xff, 0xfe, 0x02, 0x01]); // BOM, version 0x0102
        d.extend_from_slice(&0u32.to_le_bytes()); // file size (unused)
        d.extend_from_slice(&16u16.to_le_bytes());
        d.extend_from_slice(&4u16.to_le_bytes());
        let finf_at = d.len();
        d.extend_from_slice(b"FNIF");
        d.extend_from_slice(&(8 + 28u32).to_le_bytes());
        d.extend_from_slice(&[0u8; 28]);
        let cglp_at = d.len();
        d.extend_from_slice(b"PLGC");
        d.extend_from_slice(&(8 + 8 + 1u32).to_le_bytes());
        // cell 2x2, size 1 byte, baseline, max width, bpp 1, flags rot=1
        d.extend_from_slice(&[2, 2, 1, 0, 0, 2, 1, (1 << 1)]);
        d.push(0b1000_0000); // top-left pixel set in stored (rotated) orientation
        let cwdh_at = d.len();
        d.extend_from_slice(b"HDWC");
        d.extend_from_slice(&(8 + 8 + 3u32).to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&[0, 2, 3]); // left 0, width 2, advance 3
        let pamc_at = d.len();
        d.extend_from_slice(b"CMAP");
        d.extend_from_slice(&(8 + 12 + 2u32).to_le_bytes());
        d.extend_from_slice(&0x41u16.to_le_bytes());
        d.extend_from_slice(&0x41u16.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes()); // direct mapping
        d.extend_from_slice(&0u16.to_le_bytes());
        d.extend_from_slice(&0u32.to_le_bytes());
        d.extend_from_slice(&0u16.to_le_bytes()); // first glyph index
        // patch FINF pointers (body offsets): alter glyph 0, default width, pGlyph, pWidth, pMap, height
        let o = finf_at + 8;
        d[o + 2..o + 4].copy_from_slice(&0u16.to_le_bytes());
        d[o + 8..o + 12].copy_from_slice(&((cglp_at + 8) as u32).to_le_bytes());
        d[o + 12..o + 16].copy_from_slice(&((cwdh_at + 8) as u32).to_le_bytes());
        d[o + 16..o + 20].copy_from_slice(&((pamc_at + 8) as u32).to_le_bytes());
        d[o + 20] = 2;
        d
    }

    #[test]
    fn parses_metrics_and_rotates_glyphs() {
        let f = Font::parse(&tiny()).unwrap();
        assert_eq!((f.cell_w, f.cell_h, f.bpp, f.rot), (2, 2, 1, 1));
        let gi = f.glyph_index('A');
        assert_eq!(gi, 0);
        assert_eq!(f.metrics(gi), (0, 2, 3));
        assert_eq!(f.glyph_index('?'), f.alter);
        // stored top-left pixel, rotated back counter-clockwise, ends up bottom-left
        let (w, h, rows) = f.glyph_bitmap(0);
        assert_eq!((w, h), (2, 2));
        assert_eq!(rows[1][0], 1);
        assert_eq!(rows[0][0] + rows[0][1] + rows[1][1], 0);
        assert_eq!(f.line_width("AA"), 6);
    }

    #[test]
    fn draws_with_scale_and_typewriter_limit() {
        let f = Font::parse(&tiny()).unwrap();
        let mut img = Image::new(16, 8);
        let n = f.draw(&mut img, "AA", 0, 0, 2, [255, 0, 0], 1);
        assert_eq!(n, 1, "only one character revealed");
        // bottom-left font pixel (0,1) scaled 2x covers screen pixels x 0..2, y 2..4
        let px = |img: &Image, x: usize, y: usize| img.buf[(y * 16 + x) * 4 + 3];
        assert_eq!((px(&img, 0, 2), px(&img, 1, 3)), (255, 255));
        assert_eq!(px(&img, 0, 0), 0);
        assert_eq!(px(&img, 6, 2), 0, "second character not drawn yet");
        let n = f.draw(&mut img, "AA", 0, 0, 2, [255, 0, 0], 99);
        assert_eq!(n, 2);
        assert_eq!(px(&img, 6, 2), 255);
    }

    #[test]
    fn rejects_other_files() {
        assert!(Font::parse(b"NOPE....").is_err());
    }
}
