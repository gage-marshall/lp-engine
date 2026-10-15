//! The asset index: what the runtime's keys (`runtime::texcache`) are in the game's files, so dumps and mods use
//! names. Built from the content tree (`asset_index CONTENT` writes `content/index/textures.json`) and read by the
//! game binary. A texture is keyed with every palette of its own TEX0 block (the materials pick theirs; pairs the
//! game never makes simply never come up); its name is `<file>/<texture>@<palette>` (`<file>/<texture>` for
//! direct-colour textures), `<file>` its path under `content/assets` without the extension.
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
                    out.insert(k, Texture { name, w: t.w, h: t.h });
                }
            }
        }
    }
    (out, dup)
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
