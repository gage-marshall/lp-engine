//! NitroSystem G3D: NSBMD models, NSBTX textures, NSBCA joint animations, NSBVA visibility animations.
//! Reference implementation: tools/nns3d.py (the loaders are diffed against it, see tools/loader_check.sh).
//!
//! Container: header{sig 'BMD0'/'BTX0'/'BCA0'/'BVA0', byteOrder, version, fileSize, headerSize, nBlocks} + u32 block offsets; blocks start
//! with {kind 'MDL0'/'TEX0'/'JNT0'/'VIS0', size}.  Dictionaries: {revision u8, numEntry u8, size u16, dummy u16, ofsEntry u16, tree...},
//! entry header {sizeUnit u16, ofsName u16}, data[numEntry*sizeUnit], names[numEntry]*16.

use crate::bytes::*;
use crate::nns2d::Image;
use std::collections::HashMap;
use std::sync::Arc;

pub type Mat = ([[f64; 3]; 3], [f64; 3]);
pub const IDENT: Mat = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0; 3]);

// ------------------------------------------------------------------ containers
pub fn read_dict(d: &[u8], o: usize) -> Result<Vec<(String, Vec<u8>)>, String> {
    let n = u8at(d, o + 1)? as usize;
    let e = o + u16at(d, o + 6)? as usize;
    let (unit, ofs_name) = (u16at(d, e)? as usize, u16at(d, e + 2)? as usize);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a = e + 4 + i * unit;
        let data = d.get(a..a + unit).ok_or("dictionary data past end")?.to_vec();
        let na = e + ofs_name + i * 16;
        let raw = d.get(na..na + 16).ok_or("dictionary name past end")?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(16);
        out.push((raw[..end].iter().map(|&b| b as char).collect(), data));
    }
    Ok(out)
}

/// signature and {block kind -> (offset, size)}
pub fn file_blocks(d: &[u8]) -> Result<(String, HashMap<String, (usize, usize)>), String> {
    let sig: String = d.get(0..4).ok_or("short file")?.iter().rev().map(|&b| b as char).collect();
    let (hsz, nb) = (u16at(d, 12)? as usize, u16at(d, 14)? as usize);
    let mut m = HashMap::new();
    for i in 0..nb {
        let o = u32at(d, hsz + 4 * i)? as usize;
        let kind: String = d.get(o..o + 4).ok_or("block past end")?.iter().map(|&b| b as char).collect();
        m.insert(kind, (o, u32at(d, o + 4)? as usize));
    }
    Ok((sig, m))
}

fn u32le(d: &[u8]) -> u32 {
    u32::from_le_bytes([d[0], d[1], d[2], d[3]])
}
fn u16le(d: &[u8]) -> u32 {
    u16::from_le_bytes([d[0], d[1]]) as u32
}

pub fn rgb555(c: u32) -> [u8; 3] {
    [((c & 31) * 255 / 31) as u8, (((c >> 5) & 31) * 255 / 31) as u8, (((c >> 10) & 31) * 255 / 31) as u8]
}

// ------------------------------------------------------------------ textures
#[derive(Clone, Debug)]
pub struct TexEntry {
    pub name: String,
    pub param: u32,
    pub addr: usize,
    pub w: usize,
    pub h: usize,
    pub fmt: u32,
    pub color0_transp: bool,
}

#[derive(Clone, Debug)]
pub struct PalEntry {
    pub name: String,
    pub offset: usize,
}

pub struct Tex0 {
    pub textures: Vec<TexEntry>,
    pub palettes: Vec<PalEntry>,
    pub ofs_tex: usize,
    pub ofs_tex4: usize,
    pub ofs_idx4: usize,
    pub ofs_pltt: usize,
    pub data: Arc<Vec<u8>>,
}

pub fn parse_tex0(data: &Arc<Vec<u8>>, base: usize) -> Result<Tex0, String> {
    let d = &data[..];
    let ti = base + 8;
    let (ofs_dict, ofs_tex) = (u16at(d, ti + 6)? as usize, u32at(d, ti + 12)? as usize);
    let t4 = base + 24;
    let (ofs_tex4, ofs_idx4) = (u32at(d, t4 + 12)? as usize, u32at(d, t4 + 16)? as usize);
    let pi = base + 44;
    let (ofs_dict_pl, ofs_pl) = (u16at(d, pi + 8)? as usize, u32at(d, pi + 12)? as usize);
    let mut textures = Vec::new();
    for (name, dd) in read_dict(d, base + ofs_dict)? {
        let param = u32le(&dd);
        textures.push(TexEntry {
            name,
            param,
            addr: ((param & 0xffff) << 3) as usize,
            w: 8 << ((param >> 20) & 7),
            h: 8 << ((param >> 23) & 7),
            fmt: (param >> 26) & 7,
            color0_transp: (param >> 29) & 1 == 1,
        });
    }
    let mut palettes = Vec::new();
    for (name, dd) in read_dict(d, base + ofs_dict_pl)? {
        palettes.push(PalEntry { name, offset: (u16le(&dd) << 3) as usize });
    }
    Ok(Tex0 { textures, palettes, ofs_tex: base + ofs_tex, ofs_tex4: base + ofs_tex4, ofs_idx4: base + ofs_idx4, ofs_pltt: base + ofs_pl, data: data.clone() })
}

/// where palette colours come from when decoding a texture
pub enum PalSel<'a> {
    None,
    /// palette block offset (relative to the palette data)
    Offset(usize),
    /// explicit RGB555 colours (colour-variant override)
    Colors(&'a [u16]),
}

/// raw sources for decode_texture (a Tex0, or a standalone variant texture + palette blob)
pub struct TexSrc<'a> {
    pub data: &'a [u8],
    pub ofs_tex: usize,
    pub ofs_tex4: Option<usize>,
    pub ofs_idx4: Option<usize>,
    pub ofs_pltt: usize,
}

impl Tex0 {
    pub fn src(&self) -> TexSrc<'_> {
        TexSrc { data: &self.data, ofs_tex: self.ofs_tex, ofs_tex4: Some(self.ofs_tex4), ofs_idx4: Some(self.ofs_idx4), ofs_pltt: self.ofs_pltt }
    }
}

pub fn decode_texture(src: &TexSrc, tex: &TexEntry, pal: PalSel) -> Result<Image, String> {
    let d = src.data;
    let (w, h, fmt) = (tex.w, tex.h, tex.fmt);
    let pcols: Option<Vec<u16>> = match &pal {
        PalSel::Colors(c) => Some(c.to_vec()),
        PalSel::Offset(off) => {
            let po = src.ofs_pltt + off;
            let n = match fmt { 2 => 4, 3 => 16, 4 => 256, 1 => 32, 6 => 8, _ => 256 };
            Some((0..n).filter(|i| po + 2 * i + 2 <= d.len()).map(|i| u16le(&d[po + 2 * i..]) as u16).collect())
        }
        PalSel::None => None,
    };
    let mut img = Image::new(w, h);
    let base = src.ofs_tex + tex.addr;
    let transp0 = tex.color0_transp;
    let pc = |idx: usize| -> [u8; 3] {
        match &pcols {
            Some(p) if idx < p.len() => rgb555(p[idx] as u32),
            _ => [255, 0, 255],
        }
    };
    let mut set = |i: usize, c: [u8; 3], a: u8| {
        let o = (i % w + (i / w) * w) * 4;
        img.buf[o..o + 4].copy_from_slice(&[c[0], c[1], c[2], a]);
    };
    let need = |p: usize| -> Result<u8, String> { d.get(p).copied().ok_or_else(|| format!("texture data past end at {p:#x}")) };
    match fmt {
        7 => {
            for i in 0..w * h {
                let c = u16at(d, base + 2 * i)?;
                set(i, rgb555(c), if c & 0x8000 != 0 { 255 } else { 0 });
            }
        }
        1 => {
            for i in 0..w * h {
                let v = need(base + i)? as usize;
                let (idx, a) = (v & 31, v >> 5);
                set(i, pc(idx), ((a * 255) / 7) as u8);
            }
        }
        6 => {
            for i in 0..w * h {
                let v = need(base + i)? as usize;
                let (idx, a) = (v & 7, v >> 3);
                set(i, pc(idx), ((a * 255) / 31) as u8);
            }
        }
        4 => {
            for i in 0..w * h {
                let idx = need(base + i)? as usize;
                set(i, pc(idx), if transp0 && idx == 0 { 0 } else { 255 });
            }
        }
        3 => {
            for i in 0..w * h {
                let v = need(base + i / 2)? as usize;
                let idx = if i & 1 == 1 { v >> 4 } else { v & 15 };
                set(i, pc(idx), if transp0 && idx == 0 { 0 } else { 255 });
            }
        }
        2 => {
            for i in 0..w * h {
                let v = need(base + i / 4)? as usize;
                let idx = (v >> (2 * (i & 3))) & 3;
                set(i, pc(idx), if transp0 && idx == 0 { 0 } else { 255 });
            }
        }
        5 => {
            let (o4, oi) = (src.ofs_tex4.ok_or("no 4x4 block")?, src.ofs_idx4.ok_or("no 4x4 index")?);
            let tb = o4 + tex.addr;
            let ib = oi + (tex.addr >> 1);
            let po = src.ofs_pltt + match &pal { PalSel::Offset(o) => *o, _ => 0 };
            for ty in 0..h / 4 {
                for tx in 0..w / 4 {
                    let bi = ty * (w / 4) + tx;
                    let texel = u32at(d, tb + 4 * bi)?;
                    let pi = u16at(d, ib + 2 * bi)?;
                    let (mode, poff) = (pi >> 14, ((pi & 0x3fff) * 4) as usize);
                    let col = |i: usize| -> Result<[u8; 3], String> { Ok(rgb555(u16at(d, po + poff + 2 * i)?)) };
                    let (c0, c1) = (col(0)?, col(1)?);
                    let mix = |wa: u32, wb: u32, sh: u32| -> [u8; 3] { [0, 1, 2].map(|k| ((wa * c0[k] as u32 + wb * c1[k] as u32) >> sh) as u8) };
                    let cols: [([u8; 3], u8); 4] = match mode {
                        0 => [(c0, 255), (c1, 255), (col(2)?, 255), ([0; 3], 0)],
                        1 => [(c0, 255), (c1, 255), (mix(1, 1, 1), 255), ([0; 3], 0)],
                        2 => [(c0, 255), (c1, 255), (col(2)?, 255), (col(3)?, 255)],
                        _ => [(c0, 255), (c1, 255), (mix(5, 3, 3), 255), (mix(3, 5, 3), 255)],
                    };
                    for y in 0..4 {
                        for x in 0..4 {
                            let (c, a) = cols[((texel >> (2 * (y * 4 + x))) & 3) as usize];
                            set((ty * 4 + y) * w + tx * 4 + x, c, a);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(img)
}

// ------------------------------------------------------------------ model
#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub flag: u32,
    pub trans: [f64; 3],
    pub rot: [[f64; 3]; 3],
    pub scale: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub diffuse: u32,
    pub ambient: u32,
    pub polyattr: u32,
    pub alpha: u32,
    pub cull: u32,
    pub tex_param: u32,
    pub pltt_base: u32,
    pub flag: u32,
    pub orig_w: u32,
    pub orig_h: u32,
    pub tex: Option<String>,
    pub pal: Option<String>,
    /// resolved override (variant texture / recoloured palette)
    pub image: Option<Arc<Image>>,
}

#[derive(Clone, Debug)]
pub struct Shape {
    pub name: String,
    pub flag: u32,
    pub dl_off: usize,
    pub dl_size: usize,
}

#[derive(Clone, Debug, Default)]
pub struct ModelInfo {
    pub num_node: u32,
    pub num_mat: u32,
    pub num_shp: u32,
    pub pos_scale: f64,
    pub scaling_rule: u32,
    pub tex_mtx_mode: u32,
    pub first_unused_mtx: u32,
    pub num_vertex: u32,
    pub num_polygon: u32,
}

#[derive(Clone)]
pub struct Model {
    pub name: String,
    pub info: ModelInfo,
    pub nodes: Vec<Node>,
    pub materials: Vec<Material>,
    pub shapes: Vec<Shape>,
    pub sbc_off: usize,
    pub sbc_end: usize,
    pub evp_off: Option<usize>,
    pub data: Arc<Vec<u8>>,
}

pub struct Nsbmd {
    pub models: Vec<Model>,
    pub tex0: Option<Tex0>,
}

pub fn parse_file(data: Arc<Vec<u8>>) -> Result<Nsbmd, String> {
    let (_, blocks) = file_blocks(&data)?;
    let mut res = Nsbmd { models: Vec::new(), tex0: None };
    if let Some(&(o, _)) = blocks.get("MDL0") {
        for (name, dd) in read_dict(&data, o + 8)? {
            let mo = o + u32le(&dd) as usize;
            res.models.push(parse_model(&data, mo, &name)?);
        }
        for m in &mut res.models {
            m.data = data.clone();
        }
    }
    if let Some(&(o, _)) = blocks.get("TEX0") {
        res.tex0 = Some(parse_tex0(&data, o)?);
    }
    Ok(res)
}

fn parse_model(d: &Arc<Vec<u8>>, mo: usize, name: &str) -> Result<Model, String> {
    let b = &d[..];
    let (ofs_sbc, ofs_mat, ofs_shp, ofs_evp) = (u32at(b, mo + 4)? as usize, u32at(b, mo + 8)? as usize, u32at(b, mo + 12)? as usize, u32at(b, mo + 16)? as usize);
    let ii = mo + 20;
    let g = |k: usize| u8at(b, ii + k);
    let info = ModelInfo {
        num_node: g(3)?,
        num_mat: g(4)?,
        num_shp: g(5)?,
        pos_scale: fx(i32at(b, ii + 8)?),
        scaling_rule: g(1)?,
        tex_mtx_mode: g(2)?,
        first_unused_mtx: g(6)?,
        num_vertex: u16at(b, ii + 16)?,
        num_polygon: u16at(b, ii + 18)?,
    };
    let ni = ii + 44;
    let mut nodes = Vec::new();
    for (nm, dd) in read_dict(b, ni)? {
        nodes.push(parse_node(b, ni + u32le(&dd) as usize, &nm)?);
    }
    Ok(Model {
        name: name.to_string(),
        info,
        nodes,
        materials: parse_materials(b, mo + ofs_mat)?,
        shapes: parse_shapes(b, mo + ofs_shp)?,
        sbc_off: mo + ofs_sbc,
        sbc_end: mo + ofs_mat,
        evp_off: if ofs_evp != 0 { Some(mo + ofs_evp) } else { None },
        data: d.clone(),
    })
}

fn parse_node(d: &[u8], o: usize, name: &str) -> Result<Node, String> {
    let flag = u16at(d, o)?;
    let m00 = i16at(d, o + 2)?;
    let mut p = o + 4;
    let mut t = [0.0; 3];
    let mut s = [1.0; 3];
    if flag & 1 == 0 {
        for i in 0..3 {
            t[i] = fx(i32at(d, p + 4 * i)?);
        }
        p += 12;
    }
    let mut rot = IDENT.0;
    if flag & 2 == 0 {
        if flag & 8 == 0 {
            let mut v = [0.0; 8];
            for i in 0..8 {
                v[i] = fx(i16at(d, p + 2 * i)?);
            }
            p += 16;
            rot = [[fx(m00), v[0], v[1]], [v[2], v[3], v[4]], [v[5], v[6], v[7]]];
        } else {
            let (a, b) = (fx(i16at(d, p)?), fx(i16at(d, p + 2)?));
            p += 4;
            let piv = ((flag & 0xf0) >> 4) as usize;
            let minus = if flag & 0x100 != 0 { -1.0 } else { 1.0 };
            let c = if flag & 0x200 != 0 { -b } else { b };
            let dd = if flag & 0x400 != 0 { -a } else { a };
            rot = [[0.0; 3]; 3];
            let (pr, pc) = (piv / 3, piv % 3);
            rot[pr][pc] = minus;
            let rows: Vec<usize> = (0..3).filter(|&r| r != pr).collect();
            let cols: Vec<usize> = (0..3).filter(|&c| c != pc).collect();
            rot[rows[0]][cols[0]] = a;
            rot[rows[0]][cols[1]] = b;
            rot[rows[1]][cols[0]] = c;
            rot[rows[1]][cols[1]] = dd;
        }
    }
    if flag & 4 == 0 {
        for i in 0..3 {
            s[i] = fx(i32at(d, p + 4 * i)?);
        }
    }
    Ok(Node { name: name.to_string(), flag, trans: t, rot, scale: s })
}

fn parse_materials(d: &[u8], o: usize) -> Result<Vec<Material>, String> {
    let (ofs_t2m, ofs_p2m) = (u16at(d, o)? as usize, u16at(d, o + 2)? as usize);
    let mut mats = Vec::new();
    for (name, dd) in read_dict(d, o + 4)? {
        let mo = o + u32le(&dd) as usize;
        let diffamb = u32at(d, mo + 4)?;
        let polyattr = u32at(d, mo + 12)?;
        mats.push(Material {
            name,
            diffuse: diffamb & 0x7fff,
            ambient: (diffamb >> 16) & 0x7fff,
            polyattr,
            alpha: (polyattr >> 16) & 31,
            cull: (polyattr >> 6) & 3,
            tex_param: u32at(d, mo + 20)?,
            pltt_base: u16at(d, mo + 28)?,
            flag: u16at(d, mo + 30)?,
            orig_w: u16at(d, mo + 32)?,
            orig_h: u16at(d, mo + 34)?,
            tex: None,
            pal: None,
            image: None,
        });
    }
    for (key, ofs) in [(0, ofs_t2m), (1, ofs_p2m)] {
        for (nm, dd) in read_dict(d, o + ofs)? {
            let (off, num) = (u16le(&dd) as usize, dd[2] as usize);
            for i in 0..num {
                let mi = u8at(d, o + off + i)? as usize;
                let m = mats.get_mut(mi).ok_or("material binding index out of range")?;
                if key == 0 { m.tex = Some(nm.clone()) } else { m.pal = Some(nm.clone()) }
            }
        }
    }
    Ok(mats)
}

fn parse_shapes(d: &[u8], o: usize) -> Result<Vec<Shape>, String> {
    let mut out = Vec::new();
    for (name, dd) in read_dict(d, o)? {
        let so = o + u32le(&dd) as usize;
        out.push(Shape { name, flag: u32at(d, so + 4)?, dl_off: so + u32at(d, so + 8)? as usize, dl_size: u32at(d, so + 12)? as usize });
    }
    Ok(out)
}

// ------------------------------------------------------------------ geometry: display lists
fn cmd_params(c: u8) -> Option<usize> {
    Some(match c {
        0x00 | 0x15 | 0x41 => 0,
        0x10..=0x14 => 1,
        0x16 | 0x18 => 16,
        0x17 | 0x19 => 12,
        0x1a => 9,
        0x1b | 0x1c => 3,
        0x20..=0x22 => 1,
        0x23 => 2,
        0x24..=0x2b => 1,
        0x30..=0x33 => 1,
        0x34 => 32,
        0x40 | 0x50 | 0x60 => 1,
        0x70 => 3,
        0x71 => 2,
        0x72 => 1,
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug)]
pub struct Vtx {
    pub pos: [f64; 3],
    pub nrm: [f64; 3],
    pub uv: [f64; 2],
    pub col: [f64; 3],
    pub mtx: Option<u32>,
}

fn s10(v: u32) -> f64 {
    (if v & 0x200 != 0 { v as i32 - 1024 } else { v as i32 }) as f64
}

/// Decode a GX display list into triangles (quads and strips are split like tools/nns3d.py).
pub fn decode_dl(dl: &[u8]) -> Vec<[Vtx; 3]> {
    let n = dl.len();
    let mut polys: Vec<[Vtx; 3]> = Vec::new();
    let mut p = 0usize;
    let mut cur_nrm = [0.0, 0.0, 1.0];
    let mut cur_uv = [0.0, 0.0];
    let mut cur_col = [1.0, 1.0, 1.0];
    let mut cur_mtx: Option<u32> = None;
    let mut pos = [0.0f64; 3];
    let mut mode: Option<u32> = None;
    let mut verts: Vec<Vtx> = Vec::new();
    fn emit(polys: &mut Vec<[Vtx; 3]>, verts: &mut Vec<Vtx>, mode: Option<u32>) {
        let v = std::mem::take(verts);
        let m = match mode {
            Some(m) if !v.is_empty() => m,
            _ => return,
        };
        let l = v.len();
        match m {
            0 => {
                let mut i = 0;
                while i + 2 < l {
                    polys.push([v[i], v[i + 1], v[i + 2]]);
                    i += 3;
                }
            }
            1 => {
                let mut i = 0;
                while i + 3 < l {
                    polys.push([v[i], v[i + 1], v[i + 2]]);
                    polys.push([v[i], v[i + 2], v[i + 3]]);
                    i += 4;
                }
            }
            2 => {
                for i in 0..l.saturating_sub(2) {
                    polys.push(if i % 2 == 0 { [v[i], v[i + 1], v[i + 2]] } else { [v[i + 1], v[i], v[i + 2]] });
                }
            }
            _ => {
                let mut i = 0;
                while i + 3 < l {
                    let (a, b, c, d) = (v[i], v[i + 1], v[i + 3], v[i + 2]);
                    polys.push([a, b, c]);
                    polys.push([a, c, d]);
                    i += 2;
                }
            }
        }
    }
    'outer: while p + 4 <= n {
        let cmds = [dl[p], dl[p + 1], dl[p + 2], dl[p + 3]];
        p += 4;
        for &c in &cmds {
            let k = match cmd_params(c) {
                Some(k) => k,
                None => continue,
            };
            if c == 0 {
                continue;
            }
            if p + 4 * k > n {
                break 'outer;
            }
            let w: Vec<u32> = (0..k).map(|i| u32le(&dl[p + 4 * i..])).collect();
            p += 4 * k;
            let vtx = |pos: &[f64; 3]| -> Vtx { Vtx { pos: *pos, nrm: cur_nrm, uv: cur_uv, col: cur_col, mtx: cur_mtx } };
            let halves = |v: u32| ((v & 0xffff) as u16 as i16 as f64, (v >> 16) as u16 as i16 as f64);
            match c {
                0x14 => cur_mtx = Some(w[0] & 31),
                0x20 => {
                    let v = w[0];
                    cur_col = [(v & 31) as f64 / 31.0, ((v >> 5) & 31) as f64 / 31.0, ((v >> 10) & 31) as f64 / 31.0];
                }
                0x21 => {
                    let v = w[0];
                    cur_nrm = [s10(v & 0x3ff) / 512.0, s10((v >> 10) & 0x3ff) / 512.0, s10((v >> 20) & 0x3ff) / 512.0];
                }
                0x22 => {
                    let (s, t) = halves(w[0]);
                    cur_uv = [s / 16.0, t / 16.0];
                }
                0x23 => {
                    let (x, y) = halves(w[0]);
                    let z = (w[1] & 0xffff) as u16 as i16 as f64;
                    pos = [x / 4096.0, y / 4096.0, z / 4096.0];
                    let v = vtx(&pos);
                    verts.push(v);
                }
                0x24 => {
                    let v = w[0];
                    pos = [s10(v & 0x3ff) / 64.0, s10((v >> 10) & 0x3ff) / 64.0, s10((v >> 20) & 0x3ff) / 64.0];
                    let v = vtx(&pos);
                    verts.push(v);
                }
                0x25 => {
                    let (x, y) = halves(w[0]);
                    pos[0] = x / 4096.0;
                    pos[1] = y / 4096.0;
                    let v = vtx(&pos);
                    verts.push(v);
                }
                0x26 => {
                    let (x, z) = halves(w[0]);
                    pos[0] = x / 4096.0;
                    pos[2] = z / 4096.0;
                    let v = vtx(&pos);
                    verts.push(v);
                }
                0x27 => {
                    let (y, z) = halves(w[0]);
                    pos[1] = y / 4096.0;
                    pos[2] = z / 4096.0;
                    let v = vtx(&pos);
                    verts.push(v);
                }
                0x28 => {
                    let v = w[0];
                    pos[0] += s10(v & 0x3ff) / 4096.0;
                    pos[1] += s10((v >> 10) & 0x3ff) / 4096.0;
                    pos[2] += s10((v >> 20) & 0x3ff) / 4096.0;
                    let v = vtx(&pos);
                    verts.push(v);
                }
                0x40 => {
                    emit(&mut polys, &mut verts, mode);
                    mode = Some(w[0] & 3);
                }
                0x41 => {
                    emit(&mut polys, &mut verts, mode);
                    mode = None;
                }
                _ => {}
            }
        }
    }
    emit(&mut polys, &mut verts, mode);
    polys
}

// ------------------------------------------------------------------ matrices
pub fn mat_mul(a: &Mat, b: &Mat) -> Mat {
    let (ra, ta) = a;
    let (rb, tb) = b;
    let mut r = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            r[i][j] = (0..3).map(|k| ra[i][k] * rb[k][j]).sum::<f64>();
        }
    }
    let mut t = [0.0; 3];
    for i in 0..3 {
        t[i] = (0..3).map(|k| ra[i][k] * tb[k]).sum::<f64>() + ta[i];
    }
    (r, t)
}

pub fn mat_apply(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let (r, t) = m;
    [0, 1, 2].map(|i| r[i][0] * p[0] + r[i][1] * p[1] + r[i][2] * p[2] + t[i])
}

pub fn mat_apply_dir(m: &Mat, p: [f64; 3]) -> [f64; 3] {
    let r = &m.0;
    [0, 1, 2].map(|i| r[i][0] * p[0] + r[i][1] * p[1] + r[i][2] * p[2])
}

pub fn node_matrix(n: &Node, transpose_rot: bool) -> Mat {
    let mut r = n.rot;
    if transpose_rot {
        r = [[n.rot[0][0], n.rot[1][0], n.rot[2][0]], [n.rot[0][1], n.rot[1][1], n.rot[2][1]], [n.rot[0][2], n.rot[1][2], n.rot[2][2]]];
    }
    let mut rs = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            rs[i][j] = r[i][j] * n.scale[j];
        }
    }
    (rs, n.trans)
}

pub type Prov = Vec<(usize, f64)>;

pub struct Draw {
    pub shape: usize,
    pub mat: usize,
    pub node: usize,
    pub stack: Vec<Mat>,
    pub cur: Mat,
    pub sprov: Vec<Prov>,
    pub cprov: Prov,
}

pub fn read_evp(model: &Model) -> Option<Vec<Mat>> {
    let o = model.evp_off?;
    let d = &model.data[..];
    let n = model.info.num_node as usize;
    if o + 84 * n > d.len() {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..n {
        let mut m = [0.0; 12];
        for k in 0..12 {
            m[k] = fx(i32at(d, o + i * 84 + 4 * k).ok()?);
        }
        out.push(([[m[0], m[3], m[6]], [m[1], m[4], m[7]], [m[2], m[5], m[8]]], [m[9], m[10], m[11]]));
    }
    Some(out)
}

/// Execute the model's structure byte code and return the draw list.  `nodes` overrides the node SRT (animation);
/// `vis(node)` overrides node visibility.
pub fn run_sbc(model: &Model, transpose_rot: bool, nodes: Option<&[Node]>, evp: Option<&[Mat]>, vis: Option<&dyn Fn(usize) -> bool>) -> Result<Vec<Draw>, String> {
    let d = &model.data[..];
    let nodes = nodes.unwrap_or(&model.nodes);
    let (mut p, end) = (model.sbc_off, model.sbc_end);
    let mut stack: Vec<Mat> = vec![IDENT; 32];
    let mut sprov: Vec<Prov> = vec![Vec::new(); 32];
    let mut cur = IDENT;
    let mut cprov: Prov = Vec::new();
    let (mut node, mut mat) = (0usize, 0usize);
    let mut node_vis: HashMap<usize, bool> = HashMap::new();
    let mut draws = Vec::new();
    let b = |p: usize| u8at(d, p).map(|v| v as usize);
    while p < end {
        let byte = b(p)?;
        let (cmd, opt) = (byte & 31, byte >> 5);
        p += 1;
        match cmd {
            0 => continue,
            1 => break,
            2 => {
                node = b(p)?;
                node_vis.insert(node, b(p + 1)? & 1 == 1);
                p += 2;
            }
            3 => {
                let i = b(p)? & 31;
                cur = stack[i];
                cprov = sprov[i].clone();
                p += 1;
            }
            4 => {
                mat = b(p)?;
                p += 1;
            }
            5 => {
                let shown = match vis {
                    Some(f) => f(node),
                    None => node_vis.get(&node).copied().unwrap_or(true),
                };
                if shown {
                    draws.push(Draw { shape: b(p)?, mat, node, stack: stack.clone(), cur, sprov: sprov.clone(), cprov: cprov.clone() });
                }
                p += 1;
            }
            6 => {
                let (nid, _pid, _fl) = (b(p)?, b(p + 1)?, b(p + 2)?);
                p += 3;
                let (mut dest, mut src) = (None, None);
                if opt == 1 || opt == 3 {
                    dest = Some(b(p)? & 31);
                    p += 1;
                }
                if opt == 2 || opt == 3 {
                    src = Some(b(p)? & 31);
                    p += 1;
                }
                if let Some(s) = src {
                    cur = stack[s];
                }
                let nm = node_matrix(nodes.get(nid).ok_or("node index out of range")?, transpose_rot);
                cur = mat_mul(&cur, &nm);
                cprov = vec![(nid, 1.0)];
                if let Some(dst) = dest {
                    stack[dst] = cur;
                    sprov[dst] = cprov.clone();
                }
            }
            7 | 8 => {
                p += 1;
                if opt == 1 || opt == 3 {
                    p += 1;
                }
                if opt == 2 || opt == 3 {
                    p += 1;
                }
            }
            9 => {
                let (dest, num) = (b(p)?, b(p + 1)?);
                p += 2;
                let mut blend: Option<Mat> = None;
                let mut pr: Prov = Vec::new();
                for _ in 0..num {
                    let (src, nid, ratio) = (b(p)?, b(p + 1)?, b(p + 2)?);
                    p += 3;
                    let mut m = stack[src & 31];
                    if let Some(e) = evp {
                        m = mat_mul(&m, e.get(nid).ok_or("envelope index out of range")?);
                    }
                    let r = ratio as f64 / 256.0;
                    pr.push((nid, r));
                    let mut part = m;
                    for i in 0..3 {
                        for j in 0..3 {
                            part.0[i][j] = m.0[i][j] * r;
                        }
                        part.1[i] = m.1[i] * r;
                    }
                    blend = Some(match blend {
                        None => part,
                        Some(mut bl) => {
                            for i in 0..3 {
                                for j in 0..3 {
                                    bl.0[i][j] += part.0[i][j];
                                }
                                bl.1[i] += part.1[i];
                            }
                            bl
                        }
                    });
                }
                stack[dest & 31] = blend.unwrap_or(IDENT);
                sprov[dest & 31] = pr.clone();
                cur = stack[dest & 31];
                cprov = pr;
            }
            0x0a => p += 8,
            0x0b => {}
            0x0c | 0x0d => p += 2,
            _ => break,
        }
    }
    Ok(draws)
}

#[derive(Clone, Debug)]
pub struct TriVert {
    pub pos: [f64; 3],
    pub nrm: [f64; 3],
    pub uv: [f64; 2],
    pub col: [f64; 3],
    pub joints: Option<Prov>,
}

#[derive(Clone, Debug)]
pub struct Tri {
    pub material: usize,
    pub verts: [TriVert; 3],
}

/// Triangles baked to model space for the given node pose (bind pose when `nodes` is None).
pub fn build_mesh(model: &Model, nodes: Option<&[Node]>, skin: bool, vis: Option<&dyn Fn(usize) -> bool>) -> Result<Vec<Tri>, String> {
    let evp = read_evp(model);
    let ps = model.info.pos_scale;
    let d = &model.data[..];
    let mut tris = Vec::new();
    for dr in run_sbc(model, true, nodes, evp.as_deref(), vis)? {
        let shp = model.shapes.get(dr.shape).ok_or("shape index out of range")?;
        let dl = d.get(shp.dl_off..shp.dl_off + shp.dl_size).ok_or("display list past end")?;
        for poly in decode_dl(dl) {
            let tv = |v: &Vtx| {
                let m = match v.mtx {
                    Some(i) => &dr.stack[i as usize],
                    None => &dr.cur,
                };
                TriVert {
                    pos: mat_apply(m, [v.pos[0] * ps, v.pos[1] * ps, v.pos[2] * ps]),
                    nrm: mat_apply_dir(m, v.nrm),
                    uv: v.uv,
                    col: v.col,
                    joints: if skin { Some(match v.mtx { Some(i) => dr.sprov[i as usize].clone(), None => dr.cprov.clone() }) } else { None },
                }
            };
            tris.push(Tri { material: dr.mat, verts: [tv(&poly[0]), tv(&poly[1]), tv(&poly[2])] });
        }
    }
    Ok(tris)
}

// ------------------------------------------------------------------ texture binding
/// all BTX0 textures in a set of files -> name lookup (first definition wins)
pub struct TexIndex {
    pub files: Vec<Tex0>,
    pub by_name: HashMap<String, (usize, usize)>,
}

impl TexIndex {
    pub fn new() -> TexIndex {
        TexIndex { files: Vec::new(), by_name: HashMap::new() }
    }
    pub fn add_tex0(&mut self, t: Tex0) {
        let fi = self.files.len();
        for (i, tx) in t.textures.iter().enumerate() {
            self.by_name.entry(tx.name.clone()).or_insert((fi, i));
        }
        self.files.push(t);
    }
    /// add a BTX0 file (silently ignores other data, like the reference)
    pub fn add_file(&mut self, data: Vec<u8>) {
        if data.get(0..4) != Some(b"BTX0") {
            return;
        }
        if let Ok(f) = parse_file(Arc::new(data)) {
            if let Some(t) = f.tex0 {
                self.add_tex0(t);
            }
        }
    }
    pub fn get(&self, name: &str) -> Option<(&Tex0, &TexEntry)> {
        self.by_name.get(name).map(|&(f, i)| (&self.files[f], &self.files[f].textures[i]))
    }
    pub fn pal(&self, name: &str, pal: &str) -> Option<&PalEntry> {
        let &(f, _) = self.by_name.get(name)?;
        self.files[f].palettes.iter().rev().find(|p| p.name == pal)
    }
    pub fn palette_names(&self, name: &str) -> Vec<String> {
        match self.by_name.get(name) {
            Some(&(f, _)) => self.files[f].palettes.iter().map(|p| p.name.clone()).collect(),
            None => Vec::new(),
        }
    }
}

pub fn material_image(mat: &Material, idx: &TexIndex) -> Option<Arc<Image>> {
    if let Some(i) = &mat.image {
        return Some(i.clone());
    }
    let tn = mat.tex.as_ref()?;
    let (t, tx) = idx.get(tn)?;
    let pal = match &mat.pal {
        Some(pn) => match idx.pal(tn, pn) {
            Some(p) => PalSel::Offset(p.offset),
            None => PalSel::None,
        },
        None => PalSel::None,
    };
    decode_texture(&t.src(), tx, pal).ok().map(Arc::new)
}

// ------------------------------------------------------------------ joint animation (NSBCA)
#[derive(Clone, Debug, PartialEq)]
pub enum Spec {
    ConstF(f64),
    ConstRaw(u32),
    Anim(u32, u32),
}

#[derive(Clone, Debug)]
pub struct Track {
    pub node: usize,
    pub flag: u32,
    pub t: Option<[Spec; 3]>,
    pub r: Option<Spec>,
    pub s: Option<[Spec; 3]>,
}

#[derive(Clone)]
pub struct Anim {
    pub name: String,
    pub frames: usize,
    pub tracks: Vec<Track>,
    ja: usize,
    ofs3: usize,
    ofs5: usize,
    d: Arc<Vec<u8>>,
}

pub fn parse_bca(data: &Arc<Vec<u8>>) -> Result<Vec<Anim>, String> {
    let d = &data[..];
    let (_, blocks) = file_blocks(d)?;
    let o = blocks.get("JNT0").ok_or("no JNT0 block")?.0;
    let mut anims = Vec::new();
    for (name, dd) in read_dict(d, o + 8)? {
        let ja = o + u32le(&dd) as usize;
        let (nframes, nnode) = (u16at(d, ja + 4)? as usize, u16at(d, ja + 6)? as usize);
        let (ofs3, ofs5) = (u32at(d, ja + 12)? as usize, u32at(d, ja + 16)? as usize);
        let mut tracks = Vec::new();
        for i in 0..nnode {
            let to = ja + u16at(d, ja + 20 + 2 * i)? as usize;
            let tag = u32at(d, to)?;
            let mut p = to + 4;
            let mut tr = Track { node: (tag >> 24) as usize, flag: tag, t: None, r: None, s: None };
            if tag & 1 == 0 {
                if tag & 0x6 == 0 {
                    let mut v = Vec::new();
                    for cbit in [0x8u32, 0x10, 0x20] {
                        if tag & cbit != 0 {
                            v.push(Spec::ConstF(fx(i32at(d, p)?)));
                            p += 4;
                        } else {
                            v.push(Spec::Anim(u32at(d, p)?, u32at(d, p + 4)?));
                            p += 8;
                        }
                    }
                    tr.t = Some([v[0].clone(), v[1].clone(), v[2].clone()]);
                }
                if tag & 0xc0 == 0 {
                    if tag & 0x100 != 0 {
                        tr.r = Some(Spec::ConstRaw(u32at(d, p)?));
                        p += 4;
                    } else {
                        tr.r = Some(Spec::Anim(u32at(d, p)?, u32at(d, p + 4)?));
                        p += 8;
                    }
                }
                if tag & 0x600 == 0 {
                    let mut v = Vec::new();
                    for cbit in [0x800u32, 0x1000, 0x2000] {
                        if tag & cbit != 0 {
                            v.push(Spec::ConstF(fx(i32at(d, p)?)));
                        } else {
                            v.push(Spec::Anim(u32at(d, p)?, u32at(d, p + 4)?));
                        }
                        p += 8;
                    }
                    tr.s = Some([v[0].clone(), v[1].clone(), v[2].clone()]);
                }
            }
            tracks.push(tr);
        }
        anims.push(Anim { name, frames: nframes, tracks, ja, ofs3, ofs5, d: data.clone() });
    }
    Ok(anims)
}

fn step_of(info: u32) -> usize {
    if info & 0x4000_0000 != 0 { 2 } else if info & 0x8000_0000 != 0 { 4 } else { 1 }
}

fn num_keys(info: u32, nframes: usize) -> usize {
    let step = step_of(info);
    let last = ((info & 0x1fff_0000) >> 16) as usize;
    if step == 1 {
        nframes
    } else {
        (last as i64 / step as i64 + 1 + (nframes as i64 - 1 - last as i64)).max(0) as usize
    }
}

/// (key index, interpolation t, hold)
fn key_index(frame: usize, step: usize, last: usize) -> (usize, f64, bool) {
    if step == 1 {
        return (frame, 0.0, true);
    }
    if frame > last {
        return (last / step + (frame & (step - 1)), 0.0, true);
    }
    (frame / step, (frame % step) as f64 / step as f64, frame % step == 0)
}

fn rot_from_idx(a: &Anim, idx: u32) -> Result<[[f64; 3]; 3], String> {
    let d = &a.d[..];
    if idx & 0x8000 != 0 {
        let o = a.ja + a.ofs3 + (idx & 0x7fff) as usize * 6;
        let info = u16at(d, o)?;
        let (aa, bb) = (fx(i16at(d, o + 2)?), fx(i16at(d, o + 4)?));
        let piv = (info & 15) as usize;
        let mut r = [[0.0; 3]; 3];
        let (pr, pc) = (piv / 3, piv % 3);
        r[pr][pc] = if info & 0x10 != 0 { -1.0 } else { 1.0 };
        let rows: Vec<usize> = (0..3).filter(|&x| x != pr).collect();
        let cols: Vec<usize> = (0..3).filter(|&x| x != pc).collect();
        r[rows[0]][cols[0]] = aa;
        r[rows[0]][cols[1]] = bb;
        r[rows[1]][cols[0]] = if info & 0x20 != 0 { -bb } else { bb };
        r[rows[1]][cols[1]] = if info & 0x40 != 0 { -aa } else { aa };
        return Ok(r);
    }
    let o = a.ja + a.ofs5 + (idx & 0x7fff) as usize * 10;
    let mut v = [0i32; 5];
    for (i, x) in v.iter_mut().enumerate() {
        *x = i16at(d, o + 2 * i)?;
    }
    let mut m12 = v[4] & 7;
    let m11 = v[4] >> 3;
    m12 = (m12 << 3) | (v[0] & 7);
    let m00 = v[0] >> 3;
    m12 = (m12 << 3) | (v[1] & 7);
    let m01 = v[1] >> 3;
    m12 = (m12 << 3) | (v[2] & 7);
    let m02 = v[2] >> 3;
    m12 = (m12 << 3) | (v[3] & 7);
    let m10 = v[3] >> 3;
    m12 &= 0x1fff;
    if m12 & 0x1000 != 0 {
        m12 -= 0x2000;
    }
    let r0 = [m00 as f64 / 4096.0, m01 as f64 / 4096.0, m02 as f64 / 4096.0];
    let r1 = [m10 as f64 / 4096.0, m11 as f64 / 4096.0, m12 as f64 / 4096.0];
    let r2 = [r0[1] * r1[2] - r0[2] * r1[1], r0[2] * r1[0] - r0[0] * r1[2], r0[0] * r1[1] - r0[1] * r1[0]];
    Ok([r0, r1, r2])
}

fn sample_scalar(a: &Anim, spec: &Spec, frame: usize, scale_pair: bool) -> Result<f64, String> {
    let (info, off) = match spec {
        Spec::ConstF(v) => return Ok(*v),
        Spec::ConstRaw(v) => return Ok(*v as f64),
        Spec::Anim(i, o) => (*i, *o as usize),
    };
    let d = &a.d[..];
    let fx16 = info & 0x2000_0000 != 0;
    let (step, last) = (step_of(info), ((info & 0x1fff_0000) >> 16) as usize);
    let stride = if scale_pair { 2 } else { 1 };
    let n = num_keys(info, a.frames) * stride;
    let frame = frame.min(a.frames.saturating_sub(1));
    let (i, t, hold) = key_index(frame, step, last);
    let rd = |k: usize| -> Result<f64, String> {
        if k >= n {
            return Err("animation key index out of range".into());
        }
        Ok(if fx16 { i16at(d, a.ja + off + 2 * k)? as f64 } else { i32at(d, a.ja + off + 4 * k)? as f64 } / 4096.0)
    };
    let av = rd(i * stride)?;
    if hold || i * stride + stride >= n {
        return Ok(av);
    }
    let bv = rd((i + 1) * stride)?;
    Ok(av + (bv - av) * t)
}

fn sample_rot(a: &Anim, spec: &Spec, frame: usize) -> Result<[[f64; 3]; 3], String> {
    let (info, off) = match spec {
        Spec::ConstRaw(v) => return rot_from_idx(a, v & 0xffff),
        Spec::Anim(i, o) => (*i, *o as usize),
        Spec::ConstF(_) => return Err("bad rotation spec".into()),
    };
    let d = &a.d[..];
    let (step, last) = (step_of(info), ((info & 0x1fff_0000) >> 16) as usize);
    let n = num_keys(info, a.frames);
    let frame = frame.min(a.frames.saturating_sub(1));
    let (i, t, hold) = key_index(frame, step, last);
    let idx = |k: usize| -> Result<u32, String> {
        if k >= n { Err("rotation key index out of range".into()) } else { u16at(d, a.ja + off + 2 * k) }
    };
    let ra = rot_from_idx(a, idx(i)?)?;
    if hold || i + 1 >= n {
        return Ok(ra);
    }
    let rb = rot_from_idx(a, idx(i + 1)?)?;
    let mut r = [[0.0; 3]; 3];
    for x in 0..3 {
        for y in 0..3 {
            r[x][y] = ra[x][y] + (rb[x][y] - ra[x][y]) * t;
        }
    }
    Ok(r)
}

/// Per-node SRT at an integer frame; nodes without a track keep the model's pose.
pub fn sample_anim(a: &Anim, model_nodes: &[Node], frame: usize) -> Result<Vec<Node>, String> {
    let mut out = model_nodes.to_vec();
    for tr in &a.tracks {
        if tr.node >= out.len() {
            continue;
        }
        let tag = tr.flag;
        let node = &mut out[tr.node];
        if tag & 1 != 0 {
            node.trans = [0.0; 3];
            node.rot = IDENT.0;
            node.scale = [1.0; 3];
            continue;
        }
        if tag & 0x2 != 0 {
            node.trans = [0.0; 3];
        } else if let Some(t) = &tr.t {
            node.trans = [sample_scalar(a, &t[0], frame, false)?, sample_scalar(a, &t[1], frame, false)?, sample_scalar(a, &t[2], frame, false)?];
        }
        if tag & 0x40 != 0 {
            node.rot = IDENT.0;
        } else if let Some(r) = &tr.r {
            node.rot = sample_rot(a, r, frame)?;
        }
        if tag & 0x200 != 0 {
            node.scale = [1.0; 3];
        } else if let Some(s) = &tr.s {
            node.scale = [sample_scalar(a, &s[0], frame, true)?, sample_scalar(a, &s[1], frame, true)?, sample_scalar(a, &s[2], frame, true)?];
        }
    }
    Ok(out)
}

/// Like `sample_anim`, but track nodes are matched to model joints by NAME (`src_names` = joint names of the skeleton the animation targets).
pub fn sample_anim_named(a: &Anim, src_names: &[String], model_nodes: &[Node], frame: usize) -> Result<Vec<Node>, String> {
    let mut out = model_nodes.to_vec();
    let mut by_name: HashMap<&str, usize> = HashMap::new();
    for (i, n) in model_nodes.iter().enumerate() {
        by_name.insert(&n.name, i);
    }
    for tr in &a.tracks {
        if tr.node >= src_names.len() {
            continue;
        }
        let tgt = match by_name.get(src_names[tr.node].as_str()) {
            Some(&t) => t,
            None => continue,
        };
        let mut sub = a.clone();
        sub.tracks = vec![Track { node: tgt, ..tr.clone() }];
        out[tgt] = sample_anim(&sub, model_nodes, frame)?.swap_remove(tgt);
    }
    Ok(out)
}

/// Cross-fade two poses of the same skeleton (the game weights the old and new animation linearly over the blend time, `Gfx3D_InitializeModel`):
/// translation and scale interpolate, rotation interpolates as a quaternion.
pub fn blend_nodes(a: &[Node], b: &[Node], w: f64) -> Vec<Node> {
    let w = w.clamp(0.0, 1.0);
    a.iter().zip(b.iter()).map(|(x, y)| {
        let l = |p: f64, q: f64| p + (q - p) * w;
        let (qa, qb) = (mat_to_quat(&x.rot), mat_to_quat(&y.rot));
        let mut dot = qa[0] * qb[0] + qa[1] * qb[1] + qa[2] * qb[2] + qa[3] * qb[3];
        let sgn = if dot < 0.0 { -1.0 } else { 1.0 };
        dot *= sgn;
        let q = if dot > 0.9995 {
            [l(qa[0], sgn * qb[0]), l(qa[1], sgn * qb[1]), l(qa[2], sgn * qb[2]), l(qa[3], sgn * qb[3])]
        } else {
            let th = dot.clamp(-1.0, 1.0).acos();
            let (sa, sb) = (((1.0 - w) * th).sin() / th.sin(), (w * th).sin() / th.sin());
            [sa * qa[0] + sb * sgn * qb[0], sa * qa[1] + sb * sgn * qb[1], sa * qa[2] + sb * sgn * qb[2], sa * qa[3] + sb * sgn * qb[3]]
        };
        Node { name: x.name.clone(), flag: x.flag, trans: [l(x.trans[0], y.trans[0]), l(x.trans[1], y.trans[1]), l(x.trans[2], y.trans[2])], rot: quat_to_mat(q), scale: [l(x.scale[0], y.scale[0]), l(x.scale[1], y.scale[1]), l(x.scale[2], y.scale[2])] }
    }).collect()
}

fn mat_to_quat(m: &[[f64; 3]; 3]) -> [f64; 4] {
    // rows are the basis vectors as stored; w, x, y, z via the standard trace method on the matrix as given
    let tr = m[0][0] + m[1][1] + m[2][2];
    let q = if tr > 0.0 {
        let s = (tr + 1.0).sqrt() * 2.0;
        [0.25 * s, (m[2][1] - m[1][2]) / s, (m[0][2] - m[2][0]) / s, (m[1][0] - m[0][1]) / s]
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        [(m[2][1] - m[1][2]) / s, 0.25 * s, (m[0][1] + m[1][0]) / s, (m[0][2] + m[2][0]) / s]
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        [(m[0][2] - m[2][0]) / s, (m[0][1] + m[1][0]) / s, 0.25 * s, (m[1][2] + m[2][1]) / s]
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        [(m[1][0] - m[0][1]) / s, (m[0][2] + m[2][0]) / s, (m[1][2] + m[2][1]) / s, 0.25 * s]
    };
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-12);
    [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
}

fn quat_to_mat(q: [f64; 4]) -> [[f64; 3]; 3] {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt().max(1e-12);
    let (w, x, y, z) = (q[0] / n, q[1] / n, q[2] / n, q[3] / n);
    [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]
}

// ------------------------------------------------------------------ visibility animation (NSBVA)
#[derive(Clone, Debug)]
pub struct VisAnim {
    pub name: String,
    pub frames: usize,
    pub nodes: usize,
    pub bits: Vec<u32>,
}

pub fn parse_bva(d: &[u8]) -> Result<Vec<VisAnim>, String> {
    let (_, blocks) = file_blocks(d)?;
    let o = blocks.get("VIS0").ok_or("no VIS0 block")?.0;
    let mut out = Vec::new();
    for (name, dd) in read_dict(d, o + 8)? {
        let va = o + u32le(&dd) as usize;
        let (nframes, nnodes, size) = (u16at(d, va + 4)? as usize, u16at(d, va + 6)? as usize, u16at(d, va + 8)? as usize);
        let mut nwords = if size >= 16 { ((size - 12) / 4).max(1) } else { 1 + ((nframes * nnodes) >> 5) };
        nwords = nwords.min((d.len().saturating_sub(va + 12)) / 4);
        let bits = (0..nwords).map(|i| u32le(&d[va + 12 + 4 * i..])).collect();
        out.push(VisAnim { name, frames: nframes, nodes: nnodes, bits });
    }
    Ok(out)
}

pub fn visible(a: &VisAnim, frame: usize, node: usize) -> bool {
    if node >= a.nodes {
        return true;
    }
    let i = frame.min(a.frames.saturating_sub(1)) * a.nodes + node;
    a.bits.get(i >> 5).map_or(false, |w| (w >> (i & 31)) & 1 == 1)
}

#[cfg(test)]
mod blend_tests {
    use super::*;

    fn node(rot: [[f64; 3]; 3], t: [f64; 3]) -> Node {
        Node { name: "n".into(), flag: 0, trans: t, rot, scale: [1.0; 3] }
    }

    #[test]
    fn blending_interpolates_translation_and_rotation() {
        let id = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let (c, s) = (std::f64::consts::FRAC_PI_2.cos(), std::f64::consts::FRAC_PI_2.sin());
        let rz = [[c, s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0]]; // 90 degrees about z
        let (a, b) = (vec![node(id, [0.0; 3])], vec![node(rz, [10.0, 0.0, 0.0])]);
        let mid = blend_nodes(&a, &b, 0.5);
        assert!((mid[0].trans[0] - 5.0).abs() < 1e-9);
        let (c45, s45) = (std::f64::consts::FRAC_PI_4.cos(), std::f64::consts::FRAC_PI_4.sin());
        assert!((mid[0].rot[0][0] - c45).abs() < 1e-6 && (mid[0].rot[0][1].abs() - s45).abs() < 1e-6);
        let end = blend_nodes(&a, &b, 1.0);
        for i in 0..3 {
            for j in 0..3 {
                assert!((end[0].rot[i][j] - rz[i][j]).abs() < 1e-9);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    fn vtx16(x: i16, y: i16, z: i16) -> [u32; 2] {
        [(x as u16 as u32) | ((y as u16 as u32) << 16), z as u16 as u32]
    }

    #[test]
    fn display_list_triangle_and_strips() {
        // BEGIN_VTXS(tris), 3 x VTX_16, END_VTXS
        let mut w = vec![u32::from_le_bytes([0x40, 0x23, 0x23, 0x23]), 0];
        for v in [vtx16(4096, 0, 0), vtx16(0, 4096, 0), vtx16(0, 0, 4096)] {
            w.extend(v);
        }
        w.push(u32::from_le_bytes([0x41, 0, 0, 0]));
        let polys = decode_dl(&dl(&w));
        assert_eq!(polys.len(), 1);
        assert_eq!(polys[0][0].pos, [1.0, 0.0, 0.0]);
        assert_eq!(polys[0][2].pos, [0.0, 0.0, 1.0]);

        // quad strip with 4 vertices -> two triangles, ordered like the reference ([v0,v1,v3], [v0,v3,v2])
        let pts = [vtx16(0, 0, 0), vtx16(4096, 0, 0), vtx16(0, 4096, 0), vtx16(4096, 4096, 0)];
        let mut w = vec![u32::from_le_bytes([0x40, 0x23, 0x23, 0x23]), 3];
        for v in &pts[..3] {
            w.extend(*v);
        }
        w.push(u32::from_le_bytes([0x23, 0x41, 0, 0]));
        w.extend(pts[3]);
        let polys = decode_dl(&dl(&w));
        assert_eq!(polys.len(), 2);
        assert_eq!(polys[0][2].pos, [1.0, 1.0, 0.0]);
        assert_eq!(polys[1][1].pos, [1.0, 1.0, 0.0]);
    }

    #[test]
    fn display_list_attributes() {
        // NORMAL, TEXCOORD, COLOR then a vertex: the vertex carries the current attributes
        let mut w = vec![u32::from_le_bytes([0x21, 0x22, 0x20, 0x40]), (0x1ff & 0x3ff) << 20, (16 * 2) | ((16 * 3) << 16), 0x7fff, 0];
        w.push(u32::from_le_bytes([0x23, 0x41, 0, 0]));
        w.extend(vtx16(1, 2, 3));
        let polys = decode_dl(&dl(&w));
        assert!(polys.is_empty()); // a single vertex does not form a triangle
    }

    #[test]
    fn dictionary_roundtrip() {
        // revision, numEntry=2, size, dummy, ofsEntry=8 ; entry header {unit=4, ofsName=12}; data; names
        let mut d = vec![0u8, 2, 0, 0, 0, 0, 8, 0];
        d.extend_from_slice(&[4, 0, 12, 0]);
        d.extend_from_slice(&7u32.to_le_bytes());
        d.extend_from_slice(&9u32.to_le_bytes());
        // names start at e + ofs_name = 8 + 12 = 20: we have 8 + 4 + 8 = 20 bytes so far
        let mut n = [0u8; 32];
        n[..3].copy_from_slice(b"abc");
        n[16..18].copy_from_slice(b"xy");
        d.extend_from_slice(&n);
        let r = read_dict(&d, 0).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!((r[0].0.as_str(), u32le(&r[0].1)), ("abc", 7));
        assert_eq!((r[1].0.as_str(), u32le(&r[1].1)), ("xy", 9));
    }

    #[test]
    fn texture_formats() {
        let mut tex = TexEntry { name: "t".into(), param: 0, addr: 0, w: 8, h: 8, fmt: 3, color0_transp: true };
        // 16-colour: palette[1] = red, index 0 transparent
        let mut data = vec![0x10u8; 32]; // pixel pairs (idx 0, idx 1)
        let pal_off = data.len();
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&0x001fu16.to_le_bytes());
        let src = TexSrc { data: &data, ofs_tex: 0, ofs_tex4: None, ofs_idx4: None, ofs_pltt: pal_off };
        let im = decode_texture(&src, &tex, PalSel::Offset(0)).unwrap();
        assert_eq!(&im.buf[0..4], &[0, 0, 0, 0]); // index 0, transparent
        assert_eq!(&im.buf[4..8], &[255, 0, 0, 255]); // index 1, red
        tex.color0_transp = false;
        assert_eq!(decode_texture(&src, &tex, PalSel::Offset(0)).unwrap().buf[3], 255);
        // explicit palette colours override
        let im = decode_texture(&src, &tex, PalSel::Colors(&[0x03e0, 0x7c00])).unwrap();
        assert_eq!(&im.buf[4..8], &[0, 0, 255, 255]);
        // direct colour A1B5G5R5
        let tex7 = TexEntry { fmt: 7, ..tex.clone() };
        let mut d7 = Vec::new();
        for i in 0..64u16 {
            d7.extend_from_slice(&(if i == 0 { 0x8000u16 | 0x03e0 } else { 0 }).to_le_bytes());
        }
        let src7 = TexSrc { data: &d7, ofs_tex: 0, ofs_tex4: None, ofs_idx4: None, ofs_pltt: 0 };
        let im = decode_texture(&src7, &tex7, PalSel::None).unwrap();
        assert_eq!(&im.buf[0..4], &[0, 255, 0, 255]);
        assert_eq!(im.buf[7], 0);
        // 4x4 compressed needs its blocks
        let tex5 = TexEntry { fmt: 5, ..tex };
        assert!(decode_texture(&src, &tex5, PalSel::None).is_err());
        // truncated data is an error, not a panic
        let short = TexSrc { data: &data[..10], ofs_tex: 0, ofs_tex4: None, ofs_idx4: None, ofs_pltt: pal_off };
        assert!(decode_texture(&short, &TexEntry { fmt: 3, ..tex5 }, PalSel::None).is_err());
    }

    fn node(flag: u32) -> Vec<u8> {
        let mut b = (flag as u16).to_le_bytes().to_vec();
        b.extend_from_slice(&4096i16.to_le_bytes());
        b
    }

    #[test]
    fn node_flags() {
        // flag 7: no translation, no rotation, no scale -> identity
        let n = parse_node(&node(7), 0, "n").unwrap();
        assert_eq!((n.trans, n.rot, n.scale), ([0.0; 3], IDENT.0, [1.0; 3]));
        // translation only
        let mut b = node(2 | 4);
        for v in [1i32, 2, 3] {
            b.extend_from_slice(&(v * 4096).to_le_bytes());
        }
        assert_eq!(parse_node(&b, 0, "n").unwrap().trans, [1.0, 2.0, 3.0]);
        // pivot rotation: pivot 4 (centre) = 1, A = cos, B = sin
        let mut b = node(1 | 4 | 8 | (4 << 4));
        b.extend_from_slice(&0i16.to_le_bytes());
        b.extend_from_slice(&4096i16.to_le_bytes());
        let r = parse_node(&b, 0, "n").unwrap().rot;
        assert_eq!(r[1][1], 1.0);
        assert_eq!((r[0][0], r[0][2], r[2][0], r[2][2]), (0.0, 1.0, 1.0, 0.0));
        assert!(parse_node(&node(0), 0, "n").is_err()); // translation data missing
    }

    #[test]
    fn matrices() {
        let rz = ([[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]], [1.0, 0.0, 0.0]);
        assert_eq!(mat_apply(&rz, [1.0, 0.0, 0.0]), [1.0, 1.0, 0.0]);
        assert_eq!(mat_apply_dir(&rz, [1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
        let m = mat_mul(&rz, &rz);
        assert_eq!(m.0[0][0], -1.0);
        assert_eq!(m.1, [1.0, 1.0, 0.0]);
        assert_eq!(mat_mul(&IDENT, &rz), rz);
        let n = Node { name: "n".into(), flag: 0, trans: [0.0; 3], rot: [[1.0, 2.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], scale: [2.0, 1.0, 1.0] };
        assert_eq!(node_matrix(&n, true).0[1][0], 4.0); // transposed then scaled by column
    }

    #[test]
    fn animation_keys() {
        assert_eq!(key_index(5, 1, 0), (5, 0.0, true));
        assert_eq!(key_index(5, 2, 10), (2, 0.5, false));
        assert_eq!(key_index(4, 2, 10), (2, 0.0, true));
        assert_eq!(key_index(13, 2, 10), (5 + 1, 0.0, true)); // past the last interpolated key
        assert_eq!(num_keys(0, 30), 30);
        assert_eq!(num_keys(0x4000_0000 | (10 << 16), 30), 5 + 1 + 19);
        assert_eq!(step_of(0x8000_0000), 4);
    }

    fn anim_with(d: Vec<u8>, ofs3: usize, ofs5: usize) -> Anim {
        Anim { name: "a".into(), frames: 10, tracks: vec![], ja: 0, ofs3, ofs5, d: Arc::new(d) }
    }

    #[test]
    fn rotation_encodings() {
        // compressed pivot form: pivot 0, A = 1, B = 0 -> identity; A = 0, B = 1 -> 90 degree rotation about x
        let mut d = Vec::new();
        for (a, b) in [(4096i16, 0i16), (0, 4096)] {
            d.extend_from_slice(&0u16.to_le_bytes());
            d.extend_from_slice(&a.to_le_bytes());
            d.extend_from_slice(&b.to_le_bytes());
        }
        let an = anim_with(d, 0, 0);
        assert_eq!(rot_from_idx(&an, 0x8000).unwrap(), IDENT.0);
        let r = rot_from_idx(&an, 0x8001).unwrap();
        assert_eq!((r[1][2], r[2][1], r[1][1]), (1.0, 1.0, 0.0));
        // 5-value form: 13-bit components packed as value<<3 | low bits of m12; here r0 = (0.5, 0, 0), r1 = (0, 0.5, 0)
        let mut d = Vec::new();
        for v in [2048i16 << 3, 0, 0, 0, 2048i16 << 3] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        let an = anim_with(d, 0, 0);
        let r = rot_from_idx(&an, 0).unwrap();
        assert_eq!((r[0][0], r[1][1]), (0.5, 0.5));
        assert_eq!(r[2][2], 0.25);
        // out-of-range data is an error
        assert!(rot_from_idx(&an, 0x8005).is_err());
    }

    #[test]
    fn scalar_sampling_and_visibility() {
        // 16-bit keys, step 1: values 0, 1, 2 (fx12)
        let mut d = Vec::new();
        for v in [0i16, 4096, 8192] {
            d.extend_from_slice(&v.to_le_bytes());
        }
        let mut an = anim_with(d, 0, 0);
        an.frames = 3;
        let spec = Spec::Anim(0x2000_0000, 0);
        assert_eq!(sample_scalar(&an, &spec, 1, false).unwrap(), 1.0);
        assert_eq!(sample_scalar(&an, &spec, 99, false).unwrap(), 2.0); // clamped to the last frame
        assert_eq!(sample_scalar(&an, &Spec::ConstF(0.5), 0, false).unwrap(), 0.5);
        // visibility bits: frame-major, little endian
        let v = VisAnim { name: "v".into(), frames: 2, nodes: 3, bits: vec![0b101_010] };
        assert!(!visible(&v, 0, 0) && visible(&v, 0, 1) && !visible(&v, 0, 2));
        assert!(visible(&v, 1, 0) && !visible(&v, 1, 1) && visible(&v, 1, 2));
        assert!(visible(&v, 0, 9)); // nodes beyond the track are visible
    }
}
