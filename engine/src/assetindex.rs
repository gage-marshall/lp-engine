//! The asset index: what the runtime's keys (`runtime::texcache`) are in the game's files, so dumps and mods use
//! names. Built from the content tree (`asset_index CONTENT` writes `content/index/textures.json`) and read by the
//! game binary. A texture is keyed with every palette of its own TEX0 block (the materials pick theirs; pairs the
//! game never makes simply never come up); its name is `<file>/<texture>@<palette>` (`<file>/<texture>` for
//! direct-colour textures), `<file>` its path under `content/assets` without the extension. Colour variants in the
//! model bundles are added: a `<texture>_NN.tmbpl` palette as `<file>/<texture>@<texture>_NN`, a variant texture
//! `<texture>_NN` (texels and palette) as `<file>/<texture>_NN`.
//!
//! Text BG planes (`content/index/planes.json`) are named by what the game loaded rather than by their pixels (a
//! plane's rows beyond its screen file hold whatever VRAM had): each screen file (NSCR) by the first 512 bytes of
//! its map (8 rows; smaller screens are not indexed) and, to tell screens with the same map apart, the first 256
//! bytes of the tile file before it in its directory; the runtime reads the same bytes at the BG's screen and
//! tile bases (`ppu::plane_refs`). A plane's name is its screen file's path without the extension.
use crate::nns3d;
use crate::runtime::texcache;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// an indexed texture
#[derive(Clone, Debug, PartialEq)]
pub struct Texture {
    pub name: String,
    pub w: usize,
    pub h: usize,
    /// TEXIMAGE_PARAM's format, size and colour-0 bits
    pub param: u32,
}

/// how many bytes of a screen's map and of its tiles name a plane
pub const PLANE_MAP_BYTES: usize = 512;
pub const PLANE_CHAR_BYTES: usize = 256;

/// the key of a plane's map or tile bytes
pub fn plane_ref_key(bytes: &[u8]) -> u64 {
    let mut k = 0xcbf29ce484222325u64;
    texcache::fnv(&mut k, bytes);
    k
}

/// every text BG screen in the content tree: by its map key, (its tile file's key, name) for each screen with that
/// map
pub fn planes(content: &Path) -> BTreeMap<u64, Vec<(u64, String)>> {
    let root = content.join("assets");
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut out: BTreeMap<u64, Vec<(u64, String)>> = BTreeMap::new();
    for (i, f) in files.iter().enumerate() {
        let ext = f.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if ext != "nscr" {
            continue;
        }
        let Ok(d) = std::fs::read(f) else { continue };
        let Ok(sc) = crate::nns2d::parse_nscr(&d) else { continue };
        if sc.raw.len() < PLANE_MAP_BYTES {
            continue;
        }
        // the tile file before it in its directory
        let char_key = files[..i]
            .iter()
            .rev()
            .take_while(|g| g.parent() == f.parent())
            .find(|g| g.extension().is_some_and(|e| e.eq_ignore_ascii_case("ncgr")))
            .and_then(|g| std::fs::read(g).ok())
            .and_then(|b| crate::nns2d::parse_ncgr(&b).ok())
            .filter(|c| c.raw.len() >= PLANE_CHAR_BYTES)
            .map_or(0, |c| plane_ref_key(&c.raw[..PLANE_CHAR_BYTES]));
        let rel = f.strip_prefix(&root).unwrap_or(f).with_extension("");
        let name = rel.to_string_lossy().replace('\\', "/");
        out.entry(plane_ref_key(&sc.raw[..PLANE_MAP_BYTES])).or_default().push((char_key, name));
    }
    out
}

/// the plane index as JSON: `{ "<map key>": [["<tile key>", "<name>"], ...], ... }`
pub fn planes_to_json(index: &BTreeMap<u64, Vec<(u64, String)>>) -> String {
    let m: serde_json::Map<String, serde_json::Value> = index
        .iter()
        .map(|(k, v)| (format!("{k:016x}"), serde_json::Value::Array(v.iter().map(|(c, n)| serde_json::json!([format!("{c:016x}"), n])).collect())))
        .collect();
    serde_json::to_string_pretty(&serde_json::Value::Object(m)).unwrap_or_default()
}

/// the plane names from `content/index/planes.json`, by map key
pub fn load_plane_names(content: &Path) -> std::collections::HashMap<u64, Vec<(u64, String)>> {
    let Ok(b) = std::fs::read(content.join("index/planes.json")) else { return Default::default() };
    let Ok(serde_json::Value::Object(m)) = serde_json::from_slice::<serde_json::Value>(&b) else { return Default::default() };
    let hex = |v: &serde_json::Value| v.as_str().and_then(|s| u64::from_str_radix(s, 16).ok());
    m.into_iter()
        .filter_map(|(k, v)| {
            let list = v.as_array()?.iter().filter_map(|e| Some((hex(e.get(0)?)?, e.get(1)?.as_str()?.to_string()))).collect();
            Some((u64::from_str_radix(&k, 16).ok()?, list))
        })
        .collect()
}

/// a plane's name from its map and tile keys: the screen with that map whose tile file matches, else the first
pub fn plane_name(names: &std::collections::HashMap<u64, Vec<(u64, String)>>, map: u64, chars: u64) -> Option<&str> {
    let v = names.get(&map)?;
    v.iter().find(|(c, _)| *c == chars).or(v.first()).map(|(_, n)| n.as_str())
}

/// every texture in the content tree by key; the second value counts keys found more than once (the same
/// picture in several files: the first by path keeps the name)
pub fn textures(content: &Path) -> (BTreeMap<u64, Texture>, usize) {
    let root = content.join("assets");
    let mut files = Vec::new();
    walk(&root, &mut files);
    files.sort();
    let mut out = BTreeMap::new();
    let mut dup = 0;
    for f in files {
        let Ok(data) = std::fs::read(&f) else { continue };
        if !(data.starts_with(b"BTX0") || data.starts_with(b"BMD0")) {
            continue;
        }
        let Ok((_, blocks)) = nns3d::file_blocks(&data) else { continue };
        let Some(&(base, _)) = blocks.get("TEX0") else { continue };
        let data = Arc::new(data);
        let Ok(t0) = nns3d::parse_tex0(&data, base) else { continue };
        let rel = f.strip_prefix(&root).unwrap_or(&f).with_extension("");
        let file = rel.to_string_lossy().replace('\\', "/");
        let d = &data[..];
        let from = |o: usize| d.get(o..).unwrap_or(&[]);
        for t in &t0.textures {
            if t.fmt == 0 {
                continue;
            }
            let (texels, idx4) = if t.fmt == 5 { (from(t0.ofs_tex4 + t.addr), from(t0.ofs_idx4 + t.addr / 2)) } else { (from(t0.ofs_tex + t.addr), &[][..]) };
            let pals: Vec<(Option<&str>, &[u8])> = if t.fmt == 7 { vec![(None, &[][..])] } else { t0.palettes.iter().map(|p| (Some(p.name.as_str()), from(t0.ofs_pltt + p.offset))).collect() };
            for (pname, pal) in pals {
                let k = texcache::key_of(t.param, texels, idx4, pal);
                let name = match pname {
                    Some(p) => format!("{file}/{}@{p}", t.name),
                    None => format!("{file}/{}", t.name),
                };
                if out.contains_key(&k) {
                    dup += 1;
                } else {
                    out.insert(k, Texture { name, w: t.w, h: t.h, param: t.param });
                }
            }
        }
    }
    // colour variants from the model bundles: `<texture>_NN.tmbpl` palettes recolour `<texture>`, and variant
    // textures `<texture>_NN` replace its texels and palette (both keyed with the base texture's format)
    let mut by_name: std::collections::HashMap<String, Vec<(String, u32)>> = Default::default();
    for (k, t) in &out {
        let _ = k;
        if let Some((file_tex, _)) = t.name.split_once('@').or(Some((t.name.as_str(), ""))) {
            if let Some((file, tex)) = file_tex.rsplit_once('/') {
                let e = by_name.entry(tex.to_string()).or_default();
                if !e.iter().any(|(f, _)| f == file) {
                    e.push((file.to_string(), t.param));
                }
            }
        }
    }
    let mut bundles = Vec::new();
    walk(&root, &mut bundles);
    bundles.sort();
    for f in bundles {
        let Ok(b) = std::fs::read(&f) else { continue };
        let Some(entries) = bundle_entries(&b) else { continue };
        for (name, off, size, sub) in entries {
            let base_of = |n: &str| n.rsplit_once('_').map(|(b, _)| b.to_string());
            if let Some(stem) = name.strip_suffix(".tmbpl") {
                let (Some(base), Some(pal)) = (base_of(stem), b.get(off..off + size)) else { continue };
                for (file, param) in by_name.get(&base).cloned().unwrap_or_default() {
                    let Some((texels, idx4)) = texels_of(content, &file, &base) else { continue };
                    let k = texcache::key_of(param, &texels, &idx4, pal);
                    let (w, h) = texcache::size(param);
                    if out.contains_key(&k) {
                        dup += 1;
                    } else {
                        out.insert(k, Texture { name: format!("{file}/{base}@{stem}"), w, h, param });
                    }
                }
            } else if let Some((t_off, p_off, t_size, p_size)) = sub {
                let (Some(base), Some(texels), Some(pal)) = (base_of(&name), b.get(t_off..t_off + t_size), b.get(p_off..p_off + p_size)) else { continue };
                for (file, param) in by_name.get(&base).cloned().unwrap_or_default() {
                    let (w, h) = texcache::size(param);
                    let k = texcache::key_of(param, texels, &[], pal);
                    if out.contains_key(&k) {
                        dup += 1;
                    } else {
                        out.insert(k, Texture { name: format!("{file}/{name}"), w, h, param });
                    }
                }
            }
        }
    }
    (out, dup)
}

/// a model bundle's entries (`{count, 0x28}` then `{offset, size, name[32]}`; an entry without an extension is a
/// variant texture with texels and palette at +24..+40): name, offset, size and, for variant textures, (texel
/// offset, palette offset, texel size, palette size)
fn bundle_entries(b: &[u8]) -> Option<Vec<(String, usize, usize, Option<(usize, usize, usize, usize)>)>> {
    let rd = |p: usize| b.get(p..p + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]) as usize);
    let (n, esz) = (rd(0)?, rd(4)?);
    if esz != 0x28 || !(0 < n && n < 4096) || 8 + n * esz > b.len() {
        return None;
    }
    let mut out = Vec::new();
    for k in 0..n {
        let e = 8 + k * esz;
        let nm = &b[e + 8..e + 40];
        let end = nm.iter().position(|&c| c == 0).unwrap_or(32);
        let name: String = nm[..end].iter().map(|&c| c as char).collect();
        let sub = if name.contains('.') {
            None
        } else {
            let (t_off, p_off, t_size, p_size) = (rd(e + 24)?, rd(e + 28)?, rd(e + 32)?, rd(e + 36)?);
            (t_size != 0 && p_size != 0 && t_off + t_size <= b.len() && p_off + p_size <= b.len()).then_some((t_off, p_off, t_size, p_size))
        };
        let name = if sub.is_some() { name.chars().take(16).collect() } else { name };
        out.push((name, rd(e)?, rd(e + 4)?, sub));
    }
    Some(out)
}

/// a texture's texel bytes (and 4x4 block index words) from its file, by name
fn texels_of(content: &Path, file: &str, tex: &str) -> Option<(Vec<u8>, Vec<u8>)> {
    let root = content.join("assets");
    let dir = root.join(file).parent()?.to_path_buf();
    let stem = Path::new(file).file_name()?.to_string_lossy().to_string();
    let path = std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()).find(|p| p.file_stem().is_some_and(|s| s.to_string_lossy() == stem))?;
    let data = Arc::new(std::fs::read(path).ok()?);
    let (_, blocks) = nns3d::file_blocks(&data).ok()?;
    let &(base, _) = blocks.get("TEX0")?;
    let t0 = nns3d::parse_tex0(&data, base).ok()?;
    let t = t0.textures.iter().find(|t| t.name == tex)?;
    let from = |o: usize| data.get(o..).unwrap_or(&[]).to_vec();
    Some(if t.fmt == 5 { (from(t0.ofs_tex4 + t.addr), from(t0.ofs_idx4 + t.addr / 2)) } else { (from(t0.ofs_tex + t.addr), Vec::new()) })
}

fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// the index as JSON: `{ "<key hex>": "<name>", ... }`
pub fn to_json(index: &BTreeMap<u64, Texture>) -> String {
    let m: serde_json::Map<String, serde_json::Value> = index.iter().map(|(k, t)| (format!("{k:016x}"), t.name.clone().into())).collect();
    serde_json::to_string_pretty(&serde_json::Value::Object(m)).unwrap_or_default()
}

/// the texture names by key from `content/index/textures.json` (empty when it has not been built)
pub fn load_texture_names(content: &Path) -> std::collections::HashMap<u64, String> {
    let Ok(b) = std::fs::read(content.join("index/textures.json")) else { return Default::default() };
    let Ok(serde_json::Value::Object(m)) = serde_json::from_slice::<serde_json::Value>(&b) else { return Default::default() };
    m.into_iter().filter_map(|(k, v)| Some((u64::from_str_radix(&k, 16).ok()?, v.as_str()?.to_string()))).collect()
}
