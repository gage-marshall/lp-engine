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
//! - `use_gpu`: the finer 3D layers drawn on the GPU (`gpu3d`, same rules) and the finer pictures composed there
//!   (`gpu2d`: the CPU gives each DS pixel's candidates, the GPU each sub-pixel), instead of on the CPU.
//! - `smooth`: in-between pictures of 3D the game draws once per cycle. The heroine scenes draw her once every
//!   4 VBlanks (the game's own pacing, VramBank_Process 0x2044ee8, holds the draw pass for 2). When two successive
//!   captures with the same control word come from the same polygons (count, vertex counts, attributes,
//!   textures), the halfway geometry is drawn and captured as the game's capture was and shown while the draw
//!   pass is held; the game's own picture follows. She then moves at 30 updates a second, 2 VBlanks behind.
use super::gx::{Frame, Poly};
use super::ppu::{self, Hi, Shadow};
use super::gpu2d::SLOTS;
use super::gpu3d::Gpu3d;
use super::texcache;
use super::raster3d::{self, Look};
use std::sync::{Arc, Mutex};
use super::ram::Ram;
use std::collections::HashMap;

/// the GPU slot of the in-between picture (the game's captures use their bank's slot, 0..3)
const MID_SLOT: u32 = 4;

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
    /// the GPU drawing the finer 3D layers and composing the finer pictures (None: the CPU, `raster3d`, `ppu`)
    gpu: Option<Arc<Mutex<Gpu3d>>>,
    /// on the GPU: the swap whose 3D layer is in the displayed slot, and each pool slot's picture width
    gpu_l3: Option<u64>,
    slot_cw: [u32; SLOTS as usize],
    /// enhanced text BG planes on the GPU (slot 5 + engine * 4 + BG): the key of the plane in each, and the
    /// upscaled planes made so far (by key and scale)
    plane_keys: [Option<u64>; 8],
    plane_cache: HashMap<(u64, usize), Arc<Vec<u32>>>,
    /// each BG's plane key in the last frame drawn (a plane is enhanced once it holds still: one being loaded
    /// changes every frame)
    plane_seen: [Option<u64>; 8],
    /// planes dumped, and whether a key has a mod
    plane_dumped: std::collections::HashSet<u64>,
    plane_has_mod: HashMap<u64, bool>,
    /// LP_PLANE_LOG: each BG's last description, logged on change
    plane_log: [String; 8],
    /// texture mods and dumping (`texcache`), for the GPU
    pub textures: (Option<std::path::PathBuf>, Option<std::path::PathBuf>),
    /// how textures and backgrounds are enhanced
    params: texcache::Params,
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


/// the distinct colours of the drawn pixels
fn colours(px: &[u16]) -> usize {
    let mut seen = vec![false; 0x8000];
    px.iter().filter(|&&v| v & 0x8000 != 0).filter(|&&v| !std::mem::replace(&mut seen[(v & 0x7fff) as usize], true)).count()
}

/// a picture of many colours (a photograph, not a frame, a menu or text): at least 48 distinct colours (the
/// game's frames, menus and text have 15 or fewer; its photos 125 and more)
fn photo_like(px: &[u16]) -> bool {
    let mut seen = vec![0u64; 512];
    let mut n = 0;
    for &v in px {
        if v & 0x8000 != 0 {
            let c = (v & 0x7fff) as usize;
            if seen[c / 64] & 1 << (c % 64) == 0 {
                seen[c / 64] |= 1 << (c % 64);
                n += 1;
                if n >= 48 {
                    return true;
                }
            }
        }
    }
    false
}

/// the 3D layer as the DS draws it (the game's own captures are made from this)
fn native_layer(ram: &Ram, f: &Frame) -> Vec<u16> {
    let mut t = raster3d::Target::new(1);
    t.draw(ram, f);
    t.layer()
}

/// write capture pixels into a bank, returning what was there
fn swap_in(ram: &mut Ram, bank: usize, px: &[(usize, u16)]) -> Vec<(usize, u16)> {
    let b = &mut ram.vram.banks[bank];
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
        Present { scale: scale.max(1), smooth, look: Look::default(), aa: false, last: HashMap::new(), shadows: HashMap::new(), mid: None, mid_shadow: None, layer3d: None, gpu: None, gpu_l3: None, slot_cw: [0; SLOTS as usize], plane_keys: [None; 8], plane_cache: HashMap::new(), plane_seen: [None; 8], plane_dumped: Default::default(), plane_has_mod: HashMap::new(), plane_log: Default::default(), textures: (None, None), params: texcache::Params::default() }
    }

    /// draw the finer 3D layers on the GPU (true) or the CPU; returns the GPU's name, or None when there is none
    /// (the CPU then draws)
    pub fn use_gpu(&mut self, on: bool) -> Option<String> {
        if !on {
            self.gpu = None;
        } else if self.gpu.is_none() {
            self.gpu = Gpu3d::new().map(|mut g| {
                (g.tc.mods, g.tc.dump) = self.textures.clone();
                g.tc.params = self.params;
                Arc::new(Mutex::new(g))
            });
        }
        self.layer3d = None;
        self.gpu_l3 = None;
        self.shadows.clear();
        self.gpu.as_ref().map(|g| g.lock().unwrap().name.clone())
    }

    /// change the resolution / 3D options (the finer pictures are made again from the game's next captures)
    pub fn set(&mut self, scale: usize, look: Look, aa: bool) {
        *self = Present { scale: scale.max(1), look, aa, gpu: self.gpu.take(), textures: self.textures.clone(), params: self.params, ..Present::new(scale, self.smooth) };
    }

    /// drawing beyond the DS at all (else the game's own pictures are shown as the DS shows them)
    fn enhanced(&self) -> bool {
        self.scale > 1 || self.aa || self.look != Look::default()
    }

    /// the text BGs of both engines as finer planes in their GPU slots: a mod's picture (`<key>.png`, any size)
    /// for any plane, else FSR 1 for the photo-like ones (many colours: room and place pictures, not frames, menus
    /// or text); per engine and BG the slot, when it has one
    /// (`drawn`: for a drawn frame, which is what counts towards a plane holding still)
    fn enhanced_planes(&mut self, ram: &Ram, g: &mut Gpu3d, drawn: bool) -> [[Option<u32>; 4]; 2] {
        let mut out = [[None; 4]; 2];
        for e in 0..2u32 {
            for b in 0..4u32 {
                let i = (e * 4 + b) as usize;
                if drawn && std::env::var("LP_PLANE_LOG").is_ok() {
                    let mut d = ppu::bg_kind(ram, e, b);
                    if let Some((w, h, px)) = ppu::plane(ram, e, b) {
                        d += &format!(", {} colours{}", colours(&px), if w * h > 512 * 512 { " (too large: not enhanced)" } else if photo_like(&px) { " (photo-like: enhanced)" } else { " (not photo-like: mods only)" });
                    } else if d != "off" {
                        d += " (not enhanced)";
                    }
                    // (the 3D layer and BGs switching on and off every frame are left out)
                    if self.plane_log[i] != d && d != "3D (not enhanced)" && d != "off" {
                        eprintln!("engine {} BG {b}: {d}", if e == 0 { "A" } else { "B" });
                        self.plane_log[i] = d;
                    }
                }
                let Some((w, h, px)) = ppu::plane(ram, e, b).filter(|p| p.0 * p.1 <= 512 * 512) else {
                    if drawn {
                        self.plane_seen[i] = None;
                    }
                    continue;
                };
                let mut key = 0xcbf29ce484222325u64;
                texcache::fnv(&mut key, &(w as u32).to_le_bytes());
                texcache::fnv(&mut key, &px.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>());
                let still = self.plane_seen[i] == Some(key);
                if drawn {
                    self.plane_seen[i] = Some(key);
                }
                if !still {
                    continue;
                }
                if self.plane_dumped.insert(key) {
                    if let Some(d) = &self.textures.1 {
                        texcache::dump(d, key, &texcache::from_rgb555(w, h, &px));
                    }
                }
                let mods = self.textures.0.clone();
                let has_mod = *self.plane_has_mod.entry(key).or_insert_with(|| mods.as_ref().is_some_and(|d| d.join(format!("{key:016x}.png")).is_file()));
                if !has_mod && !photo_like(&px) {
                    continue;
                }
                let slot = 5 + i as u32;
                if self.plane_keys[i] != Some(key) && std::env::var("LP_PLANE_LOG").is_ok() {
                    eprintln!("plane engine {e} BG {b}: {w}x{h} key {key:016x} (was {:?})", self.plane_keys[i].map(|k| format!("{k:016x}")));
                }
                if self.plane_keys[i] != Some(key) {
                    if self.plane_cache.len() > 32 {
                        self.plane_cache.clear();
                    }
                    let (s, params) = (self.scale, self.params);
                    // a mod's picture (any size), else FSR 1
                    let modded = self.textures.0.as_deref().and_then(|d| texcache::load_mod(d, key));
                    let hi = self
                        .plane_cache
                        .entry((key, s))
                        .or_insert_with(|| {
                            Arc::new(match modded {
                                Some((mw, mh, rgba)) => texcache::resample_rgb555(mw, mh, &rgba, w * s, h * s),
                                None => texcache::upscale_plane(w, h, &px, s, params.bg_sharpness, params.bg_denoise),
                            })
                        })
                        .clone();
                    g.put_slot(s, slot, &hi);
                    self.plane_keys[i] = Some(key);
                    self.slot_cw[slot as usize] = w as u32;
                }
                out[e as usize][b as usize] = Some(slot);
            }
        }
        out
    }

    /// how textures and backgrounds are enhanced (what was made with other parameters is made again)
    pub fn set_params(&mut self, p: texcache::Params) {
        if p == self.params {
            return;
        }
        self.params = p;
        self.plane_keys = [None; 8];
        self.plane_cache.clear();
        if let Some(g) = &self.gpu {
            let mut g = g.lock().unwrap();
            g.tc.params = p;
            g.tc.clear();
        }
    }

    pub fn params(&self) -> texcache::Params {
        self.params
    }

    /// texture mods (`<dir>/<key>.png`) and a directory to dump the textures seen into, for the GPU renderer
    pub fn set_textures(&mut self, mods: Option<std::path::PathBuf>, dump: Option<std::path::PathBuf>) {
        if let Some(d) = &dump {
            let _ = std::fs::create_dir_all(d);
        }
        self.textures = (mods, dump);
        if let Some(g) = &self.gpu {
            let mut g = g.lock().unwrap();
            (g.tc.mods, g.tc.dump) = self.textures.clone();
        }
    }

    /// the 3D layer of a frame at the presentation's resolution and options
    fn raster(&self, ram: &Ram, f: &Frame) -> Layer {
        let k = if self.aa { 2 } else { 1 };
        if let Some(g) = &self.gpu {
            return g.lock().unwrap().draw(ram, f, self.scale, k, self.look);
        }
        let mut t = raster3d::Target::new(self.scale * k);
        t.look = Look { outline: self.look.outline * k, ..self.look };
        t.draw(ram, f);
        if self.aa {
            let (l, c) = t.layer_aa(2);
            (l, Some(c))
        } else {
            (t.layer_full(), None)
        }
    }

    fn shadow(&self, ram: &Ram, cnt: u32, f: &Frame, shadows: &[&Shadow]) -> Vec<u16> {
        let (l3, cov) = self.raster(ram, f);
        ppu::capture_image_hi(ram, cnt, &Hi { s: self.scale, layer3d: Some(&l3), cov3d: cov.as_deref(), shadows })
    }

    /// after a game pass (`ram` as it left it; `capture`: the display capture it made, `Machine::last_capture`):
    /// when it captured, keep a finer version of the capture and prepare the in-between picture
    pub fn after_pass(&mut self, ram: &Ram, capture: Option<u32>) {
        self.mid = None;
        self.mid_shadow = None;
        let Some(cnt) = capture else { return };
        let key = cnt & 0x7fff_ffff;
        let cur = &ram.gx.frame;
        let bank = ((cnt >> 16) & 3) as usize;
        let dst_off = ((cnt >> 18) & 3) as usize * 0x8000;
        let (cw, ch) = [(128, 128), (256, 64), (256, 128), (256, 192)][((cnt >> 20) & 3) as usize];
        let mid = if self.smooth { self.last.get(&key).and_then(|prev| between(prev, cur, 0.5)) } else { None };
        // the pixels as the game wrote them, to tell later whether they are still there
        let written = || {
            let b = &ram.vram().banks[bank];
            (0..cw * ch).map(|i| (dst_off + 2 * i) % 0x20000).map(|o| (o, b[o] as u16 | (b[o + 1] as u16) << 8)).collect::<Vec<(usize, u16)>>()
        };
        if self.enhanced() && self.gpu.is_some() {
            // on the GPU: the capture's 3D layer into the scratch slot, the finer capture into the bank's pool slot
            let gpu = self.gpu.clone().unwrap();
            let mut g = gpu.lock().unwrap();
            let k = if self.aa { 2 } else { 1 };
            // the BGs in the capture as finer planes too (in the heroine scenes the background reaches the screen
            // inside the captured picture)
            let planes = if self.look.upscale && self.scale > 1 { self.enhanced_planes(ram, &mut g, false) } else { [[None; 4]; 2] };
            let rec = {
                let prior: Vec<&Shadow> = self.shadows.values().filter(|s| s.bank != bank || s.valid(ram)).collect();
                ppu::records(ram, 0, &prior, true, &planes[0])
            };
            self.slot_cw[bank] = cw as u32;
            self.slot_cw[MID_SLOT as usize] = cw as u32;
            g.draw_layer(ram, cur, self.scale, k, self.look, 1);
            if g.capture_hi(self.scale, &rec, cnt, Some(1), bank as u32, &self.slot_cw) {
                self.shadows.insert(bank, Shadow { bank, px: written(), cw, hi: Vec::new(), slot: bank as u32 });
            } else {
                self.shadows.remove(&bank);
            }
            if let Some(f) = &mid {
                g.draw_layer(ram, f, self.scale, k, self.look, 1);
                let (b, mpx) = ppu::capture_image(ram, cnt, Some(&native_layer(ram, f)));
                if g.capture_hi(self.scale, &rec, cnt, Some(1), MID_SLOT, &self.slot_cw) {
                    self.mid_shadow = Some(Shadow { bank: b, px: mpx.clone(), cw, hi: Vec::new(), slot: MID_SLOT });
                }
                self.mid = Some((b, mpx));
            }
        } else if self.enhanced() {
            // the capture's sources as they were: the bank's own earlier shadow may be source B (and is what the
            // in-between picture's source B is too)
            let prior: Vec<&Shadow> = self.shadows.values().filter(|s| s.bank != bank || s.valid(ram)).collect();
            let (hi, mid_hi) = rayon::join(|| self.shadow(ram, cnt, cur, &prior), || mid.as_ref().map(|f| self.shadow(ram, cnt, f, &prior)));
            self.shadows.insert(bank, Shadow { bank, px: written(), cw, hi, slot: bank as u32 });
            if let (Some(f), Some(hi)) = (&mid, mid_hi) {
                let (b, mpx) = ppu::capture_image(ram, cnt, Some(&native_layer(ram, f)));
                self.mid_shadow = Some(Shadow { bank: b, px: mpx.clone(), cw, hi, slot: MID_SLOT });
                self.mid = Some((b, mpx));
            }
        } else if let Some(f) = &mid {
            self.mid = Some(ppu::capture_image(ram, cnt, Some(&native_layer(ram, f))));
        }
        self.last.insert(key, cur.clone());
    }

    /// both screens (top, bottom) as RGBA8, `256 * scale` x `192 * scale` each (`ram` is left as it was)
    pub fn screens(&mut self, ram: &mut Ram) -> (Vec<u8>, Vec<u8>) {
        let restore = self.mid.as_ref().map(|(bank, px)| (*bank, swap_in(ram, *bank, px)));
        let out = if !self.enhanced() {
            let l3 = native_layer(ram, &ram.gx.frame);
            let (a, b) = (ppu::render(ram, 0, Some(&l3)), ppu::render(ram, 1, None));
            if ppu::a_on_top(ram) { (a, b) } else { (b, a) }
        } else if let Some(gpu) = self.gpu.clone() {
            let mut g = gpu.lock().unwrap();
            let swaps = ram.gx.frames;
            if self.gpu_l3 != Some(swaps) {
                g.draw_layer(ram, &ram.gx.frame, self.scale, if self.aa { 2 } else { 1 }, self.look, 0);
                self.gpu_l3 = Some(swaps);
            }
            let planes = if self.look.upscale && self.scale > 1 { self.enhanced_planes(ram, &mut g, true) } else { [[None; 4]; 2] };
            let mut shadows: Vec<&Shadow> = Vec::new();
            if let Some(s) = &self.mid_shadow {
                shadows.push(s);
            }
            shadows.extend(self.shadows.values().filter(|s| s.valid(ram)));
            let r = &*ram;
            let (ra, rb) = rayon::join(|| ppu::records(r, 0, &shadows, false, &planes[0]), || ppu::records(r, 1, &shadows, false, &planes[1]));
            let (a, b) = g.screens_hi(self.scale, &ra, &rb, Some(0), &self.slot_cw);
            if ppu::a_on_top(ram) { (a, b) } else { (b, a) }
        } else {
            let swaps = ram.gx.frames;
            if self.layer3d.as_ref().map_or(true, |(n, _)| *n != swaps) {
                let l3 = self.raster(ram, &ram.gx.frame);
                self.layer3d = Some((swaps, l3));
            }
            let mut shadows: Vec<&Shadow> = Vec::new();
            if let Some(s) = &self.mid_shadow {
                shadows.push(s);
            }
            shadows.extend(self.shadows.values().filter(|s| s.valid(ram)));
            let (l3, cov) = self.layer3d.as_ref().map_or((None, None), |(_, (l, c))| (Some(l.as_slice()), c.as_deref()));
            let (ram, s) = (&*ram, self.scale);
            let (a, b) = rayon::join(
                || ppu::render_hi(ram, 0, &Hi { s, layer3d: l3, cov3d: cov, shadows: &shadows }),
                || ppu::render_hi(ram, 1, &Hi { s, layer3d: None, cov3d: None, shadows: &shadows }),
            );
            if ppu::a_on_top(ram) { (a, b) } else { (b, a) }
        };
        if let Some((bank, old)) = restore {
            swap_in(ram, bank, &old);
        }
        out
    }
}
