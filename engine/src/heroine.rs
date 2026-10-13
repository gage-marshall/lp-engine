//! Heroine assembly: costume parts, motions, expressions, colour variants and toon shading tables.
//! Reference implementation: tools/heroine.py (+ tools/face_tables.py); the rules are documented in docs/CONTENT_LAYOUT.md.
//!
//! Parts (body, larm, rarm, waist, leg, hair, face, hands) are separate skinned models with different joint orders; a motion's
//! animation tracks are matched to each part's joints by name.  Expressions are texture swaps; shading comes from per-model toon/edge tables.

use crate::nns3d::*;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Bundle {
    pub bca: Vec<Anim>,
    pub bva: Vec<VisAnim>,
}

/// game bundle container: {count u32, entry_size u32 = 0x28} then entries {offset u32, size u32, name[32]}
pub fn bundle_entries(b: &[u8]) -> Option<Vec<(String, Vec<u8>)>> {
    if b.len() < 8 {
        return None;
    }
    let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let esz = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
    if !(1..=4096).contains(&n) || esz != 0x28 || 8 + n * esz > b.len() {
        return None;
    }
    let mut out = Vec::new();
    for k in 0..n {
        let e = 8 + k * esz;
        let off = u32::from_le_bytes([b[e], b[e + 1], b[e + 2], b[e + 3]]) as usize;
        let size = u32::from_le_bytes([b[e + 4], b[e + 5], b[e + 6], b[e + 7]]) as usize;
        let nm = &b[e + 8..e + 40];
        let end = nm.iter().position(|&c| c == 0).unwrap_or(32);
        let name: String = nm[..end].iter().map(|&c| c as char).collect();
        out.push((name, b.get(off..(off + size).min(b.len())).unwrap_or(&[]).to_vec()));
    }
    Some(out)
}

fn sorted_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut v: Vec<_> = fs::read_dir(dir).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_file()).collect()).unwrap_or_default();
    v.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    v
}

/// insertion-ordered map where overriding keeps the original position (like a Python dict)
#[derive(Default)]
pub struct Parts {
    order: Vec<(String, Model)>,
    index: HashMap<String, usize>,
}

impl Parts {
    pub fn insert(&mut self, name: String, m: Model) {
        match self.index.get(&name) {
            Some(&i) => self.order[i].1 = m,
            None => {
                self.index.insert(name.clone(), self.order.len());
                self.order.push((name, m));
            }
        }
    }
    pub fn get(&self, name: &str) -> Option<&Model> {
        self.index.get(name).map(|&i| &self.order[i].1)
    }
    pub fn iter(&self) -> impl Iterator<Item = &(String, Model)> {
        self.order.iter()
    }
}

fn load_models(dir: &Path) -> Vec<Model> {
    let mut out = Vec::new();
    for f in sorted_files(dir) {
        if let Ok(b) = fs::read(&f) {
            if b.get(0..4) == Some(b"BMD0") {
                if let Ok(r) = parse_file(Arc::new(b)) {
                    out.extend(r.models);
                }
            }
        }
    }
    out
}

fn index_textures(dir: &Path) -> TexIndex {
    let mut t = TexIndex::new();
    for f in sorted_files(dir) {
        if let Ok(b) = fs::read(&f) {
            t.add_file(b);
        }
    }
    t
}

pub struct FaceTables {
    pub moods: Vec<[i32; 3]>, // brow, mouth_row, cheek
    pub mouth_rows: Vec<[i32; 4]>,
    pub eye_table: Vec<Vec<i32>>,
    pub viseme_col: Vec<usize>,
    pub next_mode: Vec<usize>,
    pub closed_column: usize,
    pub blink_interval: Vec<i32>,
    pub blink_hold: Vec<i32>,
}

impl FaceTables {
    pub fn from_json(v: &serde_json::Value) -> Result<FaceTables, String> {
        let ints = |x: &serde_json::Value| -> Vec<i32> { x.as_array().map(|a| a.iter().map(|y| y.as_i64().unwrap_or(0) as i32).collect()).unwrap_or_default() };
        let moods = v["moods"].as_array().ok_or("moods")?.iter().map(|m| [m["brow"].as_i64().unwrap_or(0) as i32, m["mouth_row"].as_i64().unwrap_or(0) as i32, m["cheek"].as_i64().unwrap_or(0) as i32]).collect();
        let rows = v["mouth_rows"].as_array().ok_or("mouth_rows")?.iter().map(|r| { let a = ints(r); [a[0], a[1], a[2], a[3]] }).collect();
        let eye = v["eye_table"].as_array().ok_or("eye_table")?.iter().map(ints).collect();
        Ok(FaceTables {
            moods,
            mouth_rows: rows,
            eye_table: eye,
            viseme_col: ints(&v["viseme"]["col"]).iter().map(|&x| x as usize).collect(),
            next_mode: ints(&v["viseme"]["next_mode"]).iter().map(|&x| x as usize).collect(),
            closed_column: v["blink"]["closed_column"].as_u64().unwrap_or(2) as usize,
            blink_interval: ints(&v["blink"]["interval"]),
            blink_hold: ints(&v["blink"]["hold"]),
        })
    }

    /// is the eye closed at logic tick `tick`?  Open for interval[i] ticks, then closed for hold[i] ticks, cycling through the tables (30 ticks/s)
    pub fn blink_closed(&self, tick: u64) -> bool {
        let period: i32 = self.blink_interval.iter().sum::<i32>() + self.blink_hold.iter().sum::<i32>();
        if period <= 0 {
            return false;
        }
        let mut t = (tick % period as u64) as i32;
        for (a, b) in self.blink_interval.iter().zip(&self.blink_hold) {
            if t < *a {
                return false;
            }
            t -= a;
            if t < *b {
                return true;
            }
            t -= b;
        }
        false
    }

    pub fn mouth_variant(&self, mood: usize, viseme: usize) -> i32 {
        let row = &self.mouth_rows[self.moods[mood][1] as usize];
        row[self.viseme_col[self.next_mode[viseme]]]
    }

    /// variant numbers for eye, brow, cheek, mouth
    pub fn face_for(&self, mood: usize, viseme: usize, column: usize, blink: bool) -> Expr {
        let mo = self.moods[mood];
        Expr { eye: self.eye_table[mood][if blink { self.closed_column } else { column }], brow: mo[0], cheek: mo[2], mouth: self.mouth_variant(mood, viseme) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Expr {
    pub eye: i32,
    pub brow: i32,
    pub cheek: i32,
    pub mouth: i32,
}

pub struct Wardrobe {
    pub prefix: String,
    pub parts: Parts,
    pub tex: TexIndex,
    pub motions: BTreeMap<String, Bundle>,
    /// shading tables: name -> (toon[32], edge[8]) RGB555
    pub sd: HashMap<String, (Vec<u16>, Vec<u16>)>,
    pub pl: HashMap<String, Vec<u16>>,
    /// variant sub-textures: name -> (texel bytes, palette bytes)
    pub vt: HashMap<String, (Vec<u8>, Vec<u8>)>,
    /// decoded variant / recoloured textures, so animation frames reuse the same image (and the GPU keeps its upload)
    image_cache: Mutex<HashMap<String, Option<Arc<crate::nns2d::Image>>>>,
}

#[derive(Clone, Debug)]
pub struct ComposeOpts {
    pub outfit: u32,
    pub hair: u32,
    pub motion: Option<String>,
    pub frame: usize,
    pub mood: usize,
    pub viseme: usize,
    pub eye_column: usize,
    pub blink: bool,
    pub state: u32,
    pub outfit_group: u32,
    pub outfit_variant: u32,
    pub outfit_color: u32,
    pub hair_color: u32,
    pub ribbon: u32,
    /// motion change in progress: (previous motion, its current frame, weight of the new motion)
    pub prev: Option<(String, usize, f64)>,
}

impl Default for ComposeOpts {
    fn default() -> Self {
        ComposeOpts { outfit: 17, hair: 0, motion: None, frame: 0, mood: 0, viseme: 5, eye_column: 0, blink: false, state: 0, outfit_group: 1, outfit_variant: 0, outfit_color: 0, hair_color: 0, ribbon: 0, prev: None }
    }
}

pub struct Composed {
    pub tris: Vec<Tri>,
    pub mats: Vec<Material>,
    pub toon: Vec<u16>,
    pub edge: Vec<u16>,
}

impl Wardrobe {
    /// `arc_root` = content/assets/arc ; `prefix` = "m" (Manaka), "r" (Rinko) or "n" (Nene)
    pub fn load(arc_root: &Path, prefix: &str) -> Wardrobe {
        let mut w = Wardrobe { prefix: prefix.to_string(), parts: Parts::default(), tex: TexIndex::new(), motions: BTreeMap::new(), sd: HashMap::new(), pl: HashMap::new(), vt: HashMap::new(), image_cache: Mutex::new(HashMap::new()) };
        let archs = [format!("{prefix}_block3d_01.arc"), format!("{prefix}_block3d_00.arc"), "tm_model_block_base.arc".to_string()];
        for a in &archs {
            let dir = arc_root.join(a);
            for m in load_models(&dir) {
                w.parts.insert(m.name.clone(), m);
            }
            w.merge_tex(index_textures(&dir), |_| true);
        }
        // motions
        let adir = arc_root.join("async3d.arc");
        for f in sorted_files(&adir) {
            let b = match fs::read(&f) { Ok(b) => b, Err(_) => continue };
            if b.len() < 8 || matches!(b.get(0..4), Some(b"BTX0") | Some(b"BMD0")) {
                continue;
            }
            let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
            let esz = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
            if !(1..=16).contains(&n) || esz != 0x28 {
                continue;
            }
            if let Some(ents) = bundle_entries(&b) {
                for (name, data) in ents.into_iter().take(n) {
                    let stem = match name.rsplit_once('.') { Some((s, _)) => s.to_string(), None => continue };
                    let ent = w.motions.entry(stem).or_insert(Bundle { bca: Vec::new(), bva: Vec::new() });
                    match data.get(0..4) {
                        Some(b"BCA0") => ent.bca = parse_bca(&Arc::new(data)).unwrap_or_default(),
                        Some(b"BVA0") => ent.bva = parse_bva(&data).unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }
        // motion indices 0-738 are the common set (stems m_*) that every heroine plays on her own model; Rinko/Nene add their own
        let pre = format!("{prefix}_");
        w.motions.retain(|k, _| k.starts_with(&pre) || k.starts_with("m_"));
        // hand pose models and eye textures live in async3d
        let base = format!("{prefix}_base_");
        for m in load_models(&adir) {
            if m.name.starts_with(&base) {
                w.parts.insert(m.name.clone(), m);
            }
        }
        let eye = format!("{prefix}_eye.");
        w.merge_tex(index_textures(&adir), |k| k.starts_with(&eye));
        w.scan(arc_root, &archs);
        w
    }

    fn merge_tex(&mut self, t: TexIndex, keep: impl Fn(&str) -> bool) {
        let off = self.tex.files.len();
        let TexIndex { files, by_name } = t;
        self.tex.files.extend(files);
        for (name, (fi, ti)) in by_name {
            if keep(&name) {
                self.tex.by_name.insert(name, (off + fi, ti));
            }
        }
    }

    /// index every named entry of the model bundles: shading tables, palettes, variant (sub)textures
    fn scan(&mut self, arc_root: &Path, archs: &[String]) {
        for a in [&archs[1], &archs[0], &archs[2]] {
            for f in sorted_files(&arc_root.join(a)) {
                let b = match fs::read(&f) { Ok(b) => b, Err(_) => continue };
                if b.len() < 8 {
                    continue;
                }
                let n = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
                let esz = u32::from_le_bytes([b[4], b[5], b[6], b[7]]) as usize;
                if esz != 0x28 || !(0 < n && n < 4096) || 8 + n * esz > b.len() {
                    continue;
                }
                for k in 0..n {
                    let e = 8 + k * esz;
                    let rd = |p: usize| u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]]) as usize;
                    let (off, size) = (rd(e), rd(e + 4));
                    let nm = &b[e + 8..e + 40];
                    let end = nm.iter().position(|&c| c == 0).unwrap_or(32);
                    let name: String = nm[..end].iter().map(|&c| c as char).collect();
                    if name.ends_with(".tmbsd") && size == 120 {
                        if let Some(c) = b.get(off..off + 120) {
                            let col = |i: usize| c[i] as u16 | (c[i + 1] as u16) << 5 | (c[i + 2] as u16) << 10;
                            let toon = (0..96).step_by(3).map(col).collect();
                            let edge = (96..120).step_by(3).map(col).collect();
                            self.sd.insert(name[..name.len() - 6].to_string(), (toon, edge));
                        }
                    } else if name.ends_with(".tmbpl") && size == 32 {
                        if let Some(c) = b.get(off..off + 32) {
                            self.pl.insert(name[..name.len() - 6].to_string(), (0..16).map(|i| u16::from_le_bytes([c[2 * i], c[2 * i + 1]])).collect());
                        }
                    } else if !name.contains('.') {
                        let name: String = name.chars().take(16).collect();
                        let (t_off, p_off, t_size, p_size) = (rd(e + 24), rd(e + 28), rd(e + 32), rd(e + 36));
                        if t_size != 0 && p_size != 0 && t_off + t_size <= b.len() && p_off + p_size <= b.len() {
                            self.vt.insert(name, (b[t_off..t_off + t_size].to_vec(), b[p_off..p_off + p_size].to_vec()));
                        }
                    }
                }
            }
        }
    }

    pub fn part_sets(&self, outfit: u32, hair: u32) -> Vec<(&'static str, String)> {
        let p = &self.prefix;
        vec![
            ("body", format!("{p}_01_{outfit:03}_body")),
            ("larm", format!("{p}_01_{outfit:03}_larm")),
            ("rarm", format!("{p}_01_{outfit:03}_rarm")),
            ("waist", format!("{p}_01_{outfit:03}_waist")),
            ("leg", format!("{p}_01_{outfit:03}_leg")),
            ("hair", format!("{p}_00_{hair:03}_hair")),
            ("face", format!("{p}_base_face_m")),
            ("lhand", format!("{p}_base_lhand_00")),
            ("rhand", format!("{p}_base_rhand_00")),
        ]
    }

    /// toon[32] and edge[8] RGB555 tables for a time of day (0 day, 1 evening, 2 night)
    pub fn shading(&self, outfit: u32, hair: u32, state: u32, outfit_color: u32, hair_color: u32) -> Result<(Vec<u16>, Vec<u16>), String> {
        let p = &self.prefix;
        let (toon, e) = self.sd.get(&format!("{p}_base_{state}")).ok_or("missing base shading table")?;
        let mut edge = e.clone();
        let pick = |stem: &str, color: u32| -> Option<&(Vec<u16>, Vec<u16>)> {
            (if color != 0 { self.sd.get(&format!("{stem}_plt_{color:02}_{state}")) } else { None }).or_else(|| self.sd.get(&format!("{stem}_{state}")))
        };
        if let Some(h) = pick(&format!("{p}_00_{hair:03}"), hair_color) {
            for i in [6, 7] {
                edge[i] = h.1[i];
            }
        }
        if let Some(o) = pick(&format!("{p}_01_{outfit:03}"), outfit_color) {
            for i in 1..6 {
                edge[i] = o.1[i];
            }
        }
        Ok((toon.clone(), edge))
    }

    fn variant_image(&self, mat: &Material, name: &str) -> Option<Arc<crate::nns2d::Image>> {
        let key = format!("v|{}|{}", mat.tex.as_deref().unwrap_or(""), name);
        if let Some(hit) = self.image_cache.lock().unwrap().get(&key) {
            return hit.clone();
        }
        let r = self.variant_image_uncached(mat, name);
        self.image_cache.lock().unwrap().insert(key, r.clone());
        r
    }

    fn variant_image_uncached(&self, mat: &Material, name: &str) -> Option<Arc<crate::nns2d::Image>> {
        let (tb, pb) = self.vt.get(name)?;
        let (_, tx) = self.tex.get(mat.tex.as_ref()?)?;
        let mut data = tb.clone();
        data.extend_from_slice(pb);
        let src = TexSrc { data: &data, ofs_tex: 0, ofs_tex4: None, ofs_idx4: None, ofs_pltt: tb.len() };
        let mut t2 = tx.clone();
        t2.addr = 0;
        decode_texture(&src, &t2, PalSel::Offset(0)).ok().map(Arc::new)
    }

    pub fn apply_variants(&self, mats: &mut [Material], o: &ComposeOpts) {
        let p = &self.prefix;
        for m in mats.iter_mut() {
            let tn = m.tex.clone().unwrap_or_default();
            if tn.starts_with(&format!("{p}_01_{:03}_", o.outfit)) {
                for cand in [format!("{tn}_{}{}", o.outfit_group, o.outfit_variant), format!("{tn}_10"), format!("{tn}_1{}", o.outfit_variant % 10)] {
                    if let Some(im) = self.variant_image(m, &cand) {
                        m.image = Some(im);
                        break;
                    }
                }
            } else if tn.starts_with(&format!("{p}_00_{:03}_", o.hair)) {
                if tn.ends_with("ha2") {
                    if let Some(im) = self.variant_image(m, &format!("{tn}_0{}", o.ribbon)) {
                        m.image = Some(im);
                    }
                } else if o.hair_color != 0 {
                    if let Some(cols) = self.pl.get(&format!("{tn}_{:02}", o.hair_color)) {
                        if let Some((t, tx)) = self.tex.get(&tn) {
                            let key = format!("h|{}|{}", tn, o.hair_color);
                            let hit = self.image_cache.lock().unwrap().get(&key).cloned();
                            let img = match hit {
                                Some(h) => h,
                                None => {
                                    let r = decode_texture(&t.src(), tx, PalSel::Colors(cols)).ok().map(Arc::new);
                                    self.image_cache.lock().unwrap().insert(key, r.clone());
                                    r
                                }
                            };
                            if let Some(im) = img {
                                m.image = Some(im);
                            }
                        }
                    }
                }
            }
        }
    }

    /// pose index of a hand: the visibility track '<motion>_lhand|rhand' keeps nodes 0-2 on and one node >= 3 visible; pose = node - 3
    pub fn hand_pose(&self, motion: &Option<String>, side: char, frame: usize) -> usize {
        let m = match motion.as_ref().and_then(|m| self.motions.get(m)) { Some(m) => m, None => return 0 };
        for v in &m.bva {
            if v.name.ends_with(&format!("_{side}hand")) {
                for i in 3..v.nodes {
                    if visible(v, frame.min(v.frames.saturating_sub(1)), i) {
                        return i - 3;
                    }
                }
            }
        }
        0
    }

    pub fn apply_expression(&self, mats: &mut [Material], e: &Expr) {
        let pre = format!("{}_", self.prefix);
        for m in mats.iter_mut() {
            let tex = match &m.tex { Some(t) => t.clone(), None => continue };
            let rest = match tex.strip_prefix(&pre) { Some(r) => r, None => continue };
            let slot = ["eye", "brow", "cheek", "mouth"].into_iter().find(|s| rest.strip_prefix(*s).map_or(false, |r| r.starts_with('.') && r[1..].starts_with(|c: char| c.is_ascii_digit())));
            let slot = match slot { Some(s) => s, None => continue };
            let v = match slot { "eye" => e.eye, "brow" => e.brow, "cheek" => e.cheek, _ => e.mouth };
            let tn = format!("{}_{}.{}", self.prefix, slot, v);
            if self.tex.get(&tn).is_some() {
                let pal = self.tex.palette_names(&tn).into_iter().find(|p| p.starts_with(&format!("{tn}_")));
                m.tex = Some(tn);
                if pal.is_some() {
                    m.pal = pal;
                }
            }
        }
    }

    /// length of a motion in updates (the longest of its animation tracks)
    pub fn motion_frames(&self, stem: &str) -> usize {
        self.motions.get(stem).map_or(0, |m| m.bca.iter().map(|a| a.frames).max().unwrap_or(0))
    }

    /// which animation track drives a part, and its name
    fn anim_track<'a>(&'a self, motion: &str, kind: &str, nn: usize, names: &[String]) -> Result<(Option<&'a Anim>, &'static str), String> {
        let ent = self.motions.get(motion).ok_or("unknown motion")?;
        let want = match kind {
            "body" | "face" => "body",
            "larm" | "lhand" => "larm",
            "rarm" | "rhand" => "rarm",
            "waist" | "leg" => "leg",
            "hair" => {
                if nn == 20 { "hair1" } else if names.iter().any(|n| n == "hairT_lb_01") { "hair2" } else { "hair0" }
            }
            _ => return Err("bad part kind".into()),
        };
        Ok((ent.bca.iter().find(|a| a.name.ends_with(&format!("_{want}"))), want))
    }

    /// joint names of the skeleton an animation track was authored for
    fn src_names(&self, track: &str, anim: &Anim) -> Option<Vec<String>> {
        let n_tags = anim.tracks.len();
        let suffix = match track { "body" => "_body", "larm" => "_larm", "rarm" => "_rarm", "leg" => "_leg", "hair0" | "hair1" | "hair2" => "_hair", _ => "item" };
        let marker = match track { "hair0" => Some("lpig"), "hair1" => Some("ribon"), "hair2" => Some("hairT_lb_01"), _ => None };
        for (name, m) in self.parts.iter() {
            if name.ends_with(suffix) && m.nodes.len() == n_tags && marker.map_or(true, |mk| m.nodes.iter().any(|n| n.name == mk)) {
                return Some(m.nodes.iter().map(|n| n.name.clone()).collect());
            }
        }
        // short hair tracks (7) drive only the head chain plus the part's marker joint
        if n_tags == 7 {
            if let Some(mk) = marker {
                let mut v: Vec<String> = ["Hips", "Spine", "Spine1", "Spine2", "Neck", "Head"].iter().map(|s| s.to_string()).collect();
                v.push(mk.to_string());
                return Some(v);
            }
        }
        None
    }

    /// merged triangles and materials for the parts at `frame` of `motion`
    pub fn build(&self, parts: &[(&str, String)], motion: &Option<String>, frame: usize) -> Result<(Vec<Tri>, Vec<Material>), String> {
        self.build_blend(parts, motion, frame, None)
    }

    /// like `build`, cross-fading from the pose of `prev` = (motion, frame, weight of the new motion 0..1) while a motion change blends
    pub fn build_blend(&self, parts: &[(&str, String)], motion: &Option<String>, frame: usize, prev: Option<(&str, usize, f64)>) -> Result<(Vec<Tri>, Vec<Material>), String> {
        self.build_blend_inner(parts, motion, frame, prev)
    }

    fn build_blend_inner(&self, parts: &[(&str, String)], motion: &Option<String>, frame: usize, prev: Option<(&str, usize, f64)>) -> Result<(Vec<Tri>, Vec<Material>), String> {
        let (mut tris, mut mats) = (Vec::new(), Vec::new());
        for (kind, name) in parts {
            let md = match self.parts.get(name) { Some(m) => m, None => continue };
            let names: Vec<String> = md.nodes.iter().map(|n| n.name.clone()).collect();
            let mut nodes: Option<Vec<Node>> = None;
            let mut by_name: HashMap<String, usize> = HashMap::new();
            let mut visanim: Option<&VisAnim> = None;
            if let Some(mo) = motion {
                let (a, tr) = self.anim_track(mo, kind, md.nodes.len(), &names)?;
                if let Some(a) = a {
                    if let Some(src) = self.src_names(tr, a) {
                        let mut ns = sample_anim_named(a, &src, &md.nodes, frame)?;
                        if let Some((pm, pf, w)) = prev {
                            if let Ok((Some(pa), ptr)) = self.anim_track(pm, kind, md.nodes.len(), &names) {
                                if let Some(psrc) = self.src_names(ptr, pa) {
                                    let old = sample_anim_named(pa, &psrc, &md.nodes, pf)?;
                                    ns = blend_nodes(&old, &ns, w);
                                }
                            }
                        }
                        nodes = Some(ns);
                        visanim = self.motions[mo].bva.iter().find(|v| v.name.ends_with(&format!("_{tr}")));
                        for (i, s) in src.iter().enumerate() {
                            by_name.insert(s.clone(), i);
                        }
                    }
                }
            }
            let vis_fn = |nid: usize| -> bool {
                match (visanim, by_name.get(&names[nid])) {
                    (Some(v), Some(&i)) => visible(v, frame, i),
                    _ => true,
                }
            };
            let vis: Option<&dyn Fn(usize) -> bool> = if visanim.is_some() { Some(&vis_fn) } else { None };
            let mut t = build_mesh(md, nodes.as_deref(), false, vis)?;
            let base = mats.len();
            for x in &mut t {
                x.material += base;
            }
            mats.extend(md.materials.iter().cloned());
            tris.extend(t);
        }
        Ok((tris, mats))
    }

    pub fn compose(&self, o: &ComposeOpts, tables: Option<&FaceTables>) -> Result<Composed, String> {
        let mut parts = Vec::new();
        for (kind, mut name) in self.part_sets(o.outfit, o.hair) {
            if kind == "lhand" || kind == "rhand" {
                let alt = format!("{}_base_{}_{:02}", self.prefix, kind, self.hand_pose(&o.motion, kind.chars().next().unwrap(), o.frame));
                if self.parts.get(&alt).is_some() {
                    name = alt;
                }
            }
            parts.push((kind, name));
        }
        let prev = o.prev.as_ref().map(|(m, f, w)| (m.as_str(), *f, *w));
        let (tris, mut mats) = self.build_blend(&parts, &o.motion, o.frame, prev)?;
        if let Some(t) = tables {
            self.apply_expression(&mut mats, &t.face_for(o.mood, o.viseme, o.eye_column, o.blink));
        }
        self.apply_variants(&mut mats, o);
        let (toon, edge) = self.shading(o.outfit, o.hair, o.state, o.outfit_color, o.hair_color)?;
        Ok(Composed { tris, mats, toon, edge })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(name: &str) -> Model {
        Model {
            name: name.into(),
            info: ModelInfo::default(),
            nodes: vec![],
            materials: vec![],
            shapes: vec![],
            sbc_off: 0,
            sbc_end: 0,
            evp_off: None,
            data: Arc::new(vec![]),
        }
    }

    fn material(tex: &str) -> Material {
        Material { name: "m".into(), diffuse: 0, ambient: 0, polyattr: 0, alpha: 31, cull: 0, tex_param: 0, pltt_base: 0, flag: 0, orig_w: 0, orig_h: 0, tex: Some(tex.into()), pal: None, image: None }
    }

    fn empty_wardrobe() -> Wardrobe {
        Wardrobe { prefix: "m".into(), parts: Parts::default(), tex: TexIndex::new(), motions: BTreeMap::new(), sd: HashMap::new(), pl: HashMap::new(), vt: HashMap::new(), image_cache: Mutex::new(HashMap::new()) }
    }

    #[test]
    fn parts_keep_insertion_order_on_override() {
        let mut p = Parts::default();
        p.insert("a".into(), model("a1"));
        p.insert("b".into(), model("b"));
        p.insert("a".into(), model("a2"));
        let names: Vec<_> = p.iter().map(|(n, m)| (n.as_str(), m.name.as_str())).collect();
        assert_eq!(names, vec![("a", "a2"), ("b", "b")]);
        assert!(p.get("zzz").is_none());
    }

    #[test]
    fn bundle_container() {
        let mut b = Vec::new();
        b.extend_from_slice(&2u32.to_le_bytes());
        b.extend_from_slice(&0x28u32.to_le_bytes());
        let data_off = 8 + 2 * 0x28;
        for (i, (name, len)) in [("a.nsbca", 4u32), ("b.nsbva", 2)].iter().enumerate() {
            b.extend_from_slice(&((data_off + 4 * i) as u32).to_le_bytes());
            b.extend_from_slice(&len.to_le_bytes());
            let mut n = [0u8; 32];
            n[..name.len()].copy_from_slice(name.as_bytes());
            b.extend_from_slice(&n);
        }
        b.extend_from_slice(b"BCA0xxBV");
        let e = bundle_entries(&b).unwrap();
        assert_eq!(e[0], ("a.nsbca".to_string(), b"BCA0".to_vec()));
        assert_eq!(e[1].1, b"xx".to_vec());
        assert!(bundle_entries(&b[..20]).is_none());
        assert!(bundle_entries(b"BMD0....").is_none());
    }

    #[test]
    fn face_rules() {
        let j = serde_json::json!({
            "moods": [{"brow": 1, "mouth_row": 0, "cheek": 1}, {"brow": 2, "mouth_row": 1, "cheek": 0}],
            "mouth_rows": [[10, 11, 12, 13], [20, 21, 22, 23]],
            "eye_table": [[5, 6, 7], [8, 9, 10]],
            "viseme": {"col": [0, 1, 2, 3, 2, 3], "next_mode": [1, 2, 3, 4, 5, 0]},
            "blink": {"closed_column": 2}
        });
        let t = FaceTables::from_json(&j).unwrap();
        // viseme 0 -> mode 1 -> column 1 of row 0
        assert_eq!(t.face_for(0, 0, 0, false), Expr { eye: 5, brow: 1, cheek: 1, mouth: 11 });
        // blink picks the closed column; silence (viseme 5) -> mode 0 -> column 0
        assert_eq!(t.face_for(1, 5, 0, true), Expr { eye: 10, brow: 2, cheek: 0, mouth: 20 });
        assert!(FaceTables::from_json(&serde_json::json!({})).is_err());
    }

    #[test]
    fn expression_swaps_textures_and_palettes() {
        let mut w = empty_wardrobe();
        let data = Arc::new(vec![]);
        let tex = |n: &str| TexEntry { name: n.into(), param: 0, addr: 0, w: 8, h: 8, fmt: 3, color0_transp: false };
        w.tex.add_tex0(Tex0 {
            textures: vec![tex("m_eye.0"), tex("m_eye.3"), tex("m_mouth.4")],
            palettes: vec![PalEntry { name: "m_eye.3_pl".into(), offset: 0 }, PalEntry { name: "other".into(), offset: 0 }],
            ofs_tex: 0,
            ofs_tex4: 0,
            ofs_idx4: 0,
            ofs_pltt: 0,
            data,
        });
        let mut mats = vec![material("m_eye.0"), material("m_mouth.0"), material("m_body.1"), material("m_eyebrow_extra")];
        w.apply_expression(&mut mats, &Expr { eye: 3, brow: 0, cheek: 0, mouth: 4 });
        assert_eq!(mats[0].tex.as_deref(), Some("m_eye.3"));
        assert_eq!(mats[0].pal.as_deref(), Some("m_eye.3_pl"));
        assert_eq!(mats[1].tex.as_deref(), Some("m_mouth.4")); // swapped; no matching palette entry, so the palette is left alone
        assert_eq!(mats[1].pal, None);
        assert_eq!(mats[2].tex.as_deref(), Some("m_body.1"));
        assert_eq!(mats[3].tex.as_deref(), Some("m_eyebrow_extra"));
    }

    #[test]
    fn shading_table_selection() {
        let mut w = empty_wardrobe();
        let tbl = |toon: u16, edge: u16| (vec![toon; 32], vec![edge; 8]);
        w.sd.insert("m_base_1".into(), tbl(100, 1));
        w.sd.insert("m_01_017_1".into(), tbl(0, 2));
        w.sd.insert("m_01_017_plt_03_1".into(), tbl(0, 3));
        w.sd.insert("m_00_001_1".into(), tbl(0, 4));
        let (toon, edge) = w.shading(17, 1, 1, 0, 0).unwrap();
        assert_eq!(toon[0], 100);
        // outfit owns edges 1-5, hair owns 6-7, the base keeps edge 0
        assert_eq!(edge, vec![1, 2, 2, 2, 2, 2, 4, 4]);
        let (_, edge) = w.shading(17, 1, 1, 3, 0).unwrap(); // colour variant bundle replaces the outfit table
        assert_eq!(edge[1], 3);
        assert!(w.shading(17, 1, 0, 0, 0).is_err()); // no base table for that time of day
    }

    #[test]
    fn part_names_and_hand_pose() {
        let w = empty_wardrobe();
        let p = w.part_sets(5, 2);
        assert_eq!(p[0], ("body", "m_01_005_body".to_string()));
        assert_eq!(p[5], ("hair", "m_00_002_hair".to_string()));
        assert_eq!(w.hand_pose(&None, 'l', 0), 0);
        assert_eq!(w.hand_pose(&Some("nope".into()), 'l', 0), 0);
    }

    #[test]
    fn variant_images_need_the_texture() {
        let mut w = empty_wardrobe();
        w.vt.insert("m_01_001_x_10".into(), (vec![0u8; 32], vec![0u8; 32]));
        let m = material("m_01_001_x");
        assert!(w.variant_image(&m, "m_01_001_x_10").is_none()); // base texture geometry unknown
        assert!(w.variant_image(&m, "missing").is_none());
    }
}
