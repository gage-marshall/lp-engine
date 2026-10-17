//! The content extractor: the player's LovePlus+ ROM read once into the content tree the engine runs from (the Rust
//! port of `tools/lp_extract.py`, the parts the game needs; the analysis-only tables stay with the Python tool).
//!
//! Layout written under the content directory:
//!   manifest.json   ROM identity (sha1, game code, title, size) and the extractor version
//!   fs/             every NitroFS file at its ROM path as the game's file code reads it, plus the card tables
//!                   (header.bin, fnt.bin, fat.bin, ovt9.bin, signature.bin, arm9.bin, rom.json) and index.json
//!                   ([[file id, path, FAT start, FAT end]])
//!   assets/         the NitroFS again with game archives (0x15618 header) unpacked one level into directories
//!                   (entries LZ-decompressed) and the self-keyed SBIN / TXRC cipher removed, for the asset index
//!   engine/         static.bin (the ARM9 static module, BLZ undone), itcm.bin, dtcm.bin, ovNN.bin (overlays, BLZ
//!                   undone), memory_map.json, g7221.pack (the voice decoder's tables)
//!
//! Nothing is interpreted beyond what the ROM's containers require.

use sha1::{Digest, Sha1};
use std::fs;
use std::io;
use std::path::Path;

/// bumped when the layout changes (the app re-extracts content written by an older version)
pub const VERSION: u32 = 6;
pub const GAME_CODE: &[u8; 4] = b"BOXJ";

/// archive magic of the game's own containers
const ARC_MAGIC: u32 = 0x15618;

fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn u32le(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// what went wrong, said so the player can act on it
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    NotLovePlus(String),
    Damaged(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::NotLovePlus(s) => write!(f, "this is not a LovePlus+ ROM ({s})"),
            Error::Damaged(s) => write!(f, "the ROM looks damaged ({s})"),
        }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::Io(e)
    }
}

/// a NitroFS file: id, path, start and end in the ROM
pub struct FsFile {
    pub id: u16,
    pub path: String,
    pub start: u32,
    pub end: u32,
}

/// the NitroFS: the FNT names the files, the FAT places them
pub fn nitrofs(rom: &[u8]) -> Result<Vec<FsFile>, Error> {
    let (fnt, fat_off, fat_sz) = (u32le(rom, 0x40) as usize, u32le(rom, 0x48) as usize, u32le(rom, 0x4c) as usize);
    if fnt >= rom.len() || fat_off + fat_sz > rom.len() {
        return Err(Error::Damaged("file tables outside the image".into()));
    }
    let fat = |id: usize| (u32le(rom, fat_off + id * 8), u32le(rom, fat_off + id * 8 + 4));
    let mut out = Vec::new();
    fn walk(rom: &[u8], fnt: usize, did: u16, path: String, fat: &dyn Fn(usize) -> (u32, u32), out: &mut Vec<FsFile>, depth: u32) -> Result<(), Error> {
        if depth > 32 {
            return Err(Error::Damaged("directory loop".into()));
        }
        let e = fnt + (did as usize & 0xfff) * 8;
        let mut p = fnt + u32le(rom, e) as usize;
        let mut fid = u16le(rom, e + 4);
        loop {
            let t = *rom.get(p).ok_or_else(|| Error::Damaged("name table cut short".into()))?;
            p += 1;
            if t == 0 {
                break;
            }
            let n = (t & 0x7f) as usize;
            let name: String = rom[p..p + n].iter().map(|&c| c as char).collect();
            p += n;
            if t & 0x80 != 0 {
                let sub = u16le(rom, p);
                p += 2;
                walk(rom, fnt, sub, format!("{path}{name}/"), fat, out, depth + 1)?;
            } else {
                let (s, en) = fat(fid as usize);
                if en < s || en as usize > rom.len() {
                    return Err(Error::Damaged(format!("{path}{name} outside the image")));
                }
                out.push(FsFile { id: fid, path: format!("{path}{name}"), start: s, end: en });
                fid += 1;
            }
        }
        Ok(())
    }
    walk(rom, fnt, 0xf000, String::new(), &fat, &mut out, 0)?;
    Ok(out)
}

/// BLZ (bottom-up LZ) as the ARM9 static module and the overlays are stored; `end` is the compressed part's end
pub fn blz_decode(data: &[u8], end: usize) -> Result<Vec<u8>, Error> {
    let bad = || Error::Damaged("BLZ stream".into());
    if end < 8 || end > data.len() {
        return Err(bad());
    }
    let word = u32le(data, end - 8);
    let extra = u32le(data, end - 4) as usize;
    let (hdr, enc) = ((word >> 24) as usize, (word & 0xff_ffff) as usize);
    let mut out = data[..end].to_vec();
    out.resize(end + extra, 0);
    let (mut pin, mut pout) = (end.checked_sub(hdr).ok_or_else(bad)?, end + extra);
    let stop = end.checked_sub(enc).ok_or_else(bad)?;
    while pin > stop {
        let mut flags = out[pin - 1];
        pin -= 1;
        for _ in 0..8 {
            if pin <= stop {
                break;
            }
            if flags & 0x80 != 0 {
                if pin < 2 {
                    return Err(bad());
                }
                let (b1, b2) = (out[pin - 1] as usize, out[pin - 2] as usize);
                pin -= 2;
                let (n, disp) = ((b1 >> 4) + 3, (((b1 & 0xf) << 8) | b2) + 3);
                for _ in 0..n {
                    pout = pout.checked_sub(1).ok_or_else(bad)?;
                    out[pout] = *out.get(pout + disp).ok_or_else(bad)?;
                }
            } else {
                pin -= 1;
                pout = pout.checked_sub(1).ok_or_else(bad)?;
                out[pout] = out[pin];
            }
            flags <<= 1;
        }
    }
    if pout != stop {
        return Err(bad());
    }
    Ok(out)
}

/// LZ10 / LZ11 as the archives store entries (None: not a valid stream)
pub fn lz(d: &[u8]) -> Option<Vec<u8>> {
    if d.len() < 4 {
        return None;
    }
    if d[0] == 0x10 {
        let n = d[1] as usize | (d[2] as usize) << 8 | (d[3] as usize) << 16;
        // a last match may run past the declared size: the size wins (as the game's own decoder stops there)
        return crate::save::lz10(d).map(|mut v| {
            v.truncate(n);
            v
        });
    }
    if d[0] != 0x11 {
        return None;
    }
    let n = d[1] as usize | (d[2] as usize) << 8 | (d[3] as usize) << 16;
    let mut out = Vec::with_capacity(n);
    let mut p = 4;
    while out.len() < n {
        let fl = *d.get(p)?;
        p += 1;
        for b in 0..8 {
            if out.len() >= n {
                break;
            }
            if fl & (0x80 >> b) != 0 {
                let a = *d.get(p)? as usize;
                let (l, disp) = match a >> 4 {
                    0 => {
                        let (b1, b2) = (*d.get(p + 1)? as usize, *d.get(p + 2)? as usize);
                        p += 3;
                        (((a & 15) << 4 | b1 >> 4) + 0x11, ((b1 & 15) << 8 | b2) + 1)
                    }
                    1 => {
                        let (b1, b2, b3) = (*d.get(p + 1)? as usize, *d.get(p + 2)? as usize, *d.get(p + 3)? as usize);
                        p += 4;
                        (((a & 15) << 12 | b1 << 4 | b2 >> 4) + 0x111, ((b2 & 15) << 8 | b3) + 1)
                    }
                    k => {
                        let b1 = *d.get(p + 1)? as usize;
                        p += 2;
                        (k + 1, ((a & 15) << 8 | b1) + 1)
                    }
                };
                if disp > out.len() {
                    return None;
                }
                for _ in 0..l {
                    out.push(out[out.len() - disp]);
                }
            } else {
                out.push(*d.get(p)?);
                p += 1;
            }
        }
    }
    out.truncate(n);
    Some(out)
}

pub fn maybe_lz(b: &[u8], unpacked: usize) -> Vec<u8> {
    if b.len() >= 4 && (b[0] == 0x10 || b[0] == 0x11) && (b[1] as usize | (b[2] as usize) << 8 | (b[3] as usize) << 16) == unpacked && unpacked > 0 {
        if let Some(d) = lz(b) {
            return d;
        }
    }
    b.to_vec()
}

pub fn is_arc(d: &[u8]) -> bool {
    d.len() > 12 && u32le(d, 0) == ARC_MAGIC
}

/// the file extension of an archive entry, by its magic (as tools/unpack_arc.py names them)
fn ext_of(magic: &[u8]) -> &'static str {
    match magic {
        b"RGCN" => "ncgr",
        b"RLCN" => "nclr",
        b"RCSN" => "nscr",
        b"RECN" => "ncer",
        b"RNAN" => "nanr",
        b"RCMN" => "nmcr",
        b"RNFN" => "nftr",
        b"0DMB" => "nsbmd",
        b"0XTB" => "nsbtx",
        b"0ACB" => "nsbca",
        b"0TAB" => "nsbta",
        b"0APB" => "nsbtp",
        b"0AMB" => "nsbma",
        b"0CVB" => "nsbva",
        b"SBIN" => "sbin",
        b"TERM" => "term",
        _ => "bin",
    }
}

/// a game archive unpacked into `dir`: entries `NNNN[_name].ext`, nested archives as directories
fn unpack_arc(d: &[u8], dir: &Path, count: &mut usize) -> Result<(), Error> {
    fs::create_dir_all(dir)?;
    let cnt = u16le(d, 4) as usize;
    if cnt == 0 {
        return Ok(());
    }
    let first = u32le(d, 12) as usize;
    let stride = if first > 12 { (first - 12) / cnt } else { 12 };
    for i in 0..cnt {
        let r = 12 + i * stride;
        if r + 12 > d.len() {
            return Err(Error::Damaged("archive record table".into()));
        }
        let (o, s, u) = (u32le(d, r) as usize, u32le(d, r + 4) as usize, u32le(d, r + 8) as usize);
        let raw = d.get(o..o + s).ok_or_else(|| Error::Damaged("archive entry outside its archive".into()))?;
        let name = if stride > 12 {
            let n: Vec<u8> = d[r + 12..12 + (i + 1) * stride].iter().copied().take_while(|&c| c != 0).collect();
            let n: String = n.iter().map(|&c| c as char).collect();
            n.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string()
        } else {
            String::new()
        };
        let b = maybe_lz(raw, u);
        let nm = if name.is_empty() { format!("{i:04}") } else { format!("{i:04}_{name}") };
        if is_arc(&b) {
            unpack_arc(&b, &dir.join(&nm), count)?;
            continue;
        }
        let ext = ext_of(&b[..4.min(b.len())]);
        let file = if nm.to_lowercase().ends_with(&format!(".{ext}")) { nm } else { format!("{nm}.{ext}") };
        fs::write(dir.join(file), &b)?;
        *count += 1;
    }
    Ok(())
}

/// the decompressed ARM9 static module (the nitrocode marker in ModuleParams gives the compressed end)
pub fn arm9_static(rom: &[u8]) -> Result<Vec<u8>, Error> {
    let (off, ram, size) = (u32le(rom, 0x20) as usize, u32le(rom, 0x28) as usize, u32le(rom, 0x2c) as usize);
    let raw = rom.get(off..off + size).ok_or_else(|| Error::Damaged("ARM9 outside the image".into()))?;
    let Some(k) = raw.windows(4).position(|w| w == 0xdec0_0621u32.to_le_bytes()) else { return Ok(raw.to_vec()) };
    let comp_end = u32le(raw, k - 8) as usize;
    if comp_end == 0 {
        return Ok(raw.to_vec());
    }
    blz_decode(raw, comp_end - ram)
}

/// the ITCM / DTCM initial images from the static module's autoload list: (name, address, bytes, bss)
fn arm9_autoloads(rom: &[u8], st: &[u8]) -> Vec<(String, u32, Vec<u8>, u32)> {
    let ram = u32le(rom, 0x28) as usize;
    let Some(k) = st.windows(4).position(|w| w == 0xdec0_0621u32.to_le_bytes()) else { return Vec::new() };
    let (ls, le, auto) = (u32le(st, k - 28) as usize, u32le(st, k - 24) as usize, u32le(st, k - 20) as usize);
    let mut out = Vec::new();
    let mut p = auto - ram;
    let mut a = ls;
    while a < le {
        let (addr, sz, bss) = (u32le(st, a - ram), u32le(st, a - ram + 4) as usize, u32le(st, a - ram + 8));
        let name = match addr {
            0x1ff8000 => "itcm".to_string(),
            0x27e0000 => "dtcm".to_string(),
            _ => format!("{addr:08x}"),
        };
        out.push((name, addr, st[p..p + sz].to_vec(), bss));
        p += sz;
        a += 12;
    }
    out
}

/// the voice decoder's tables in the static module (LPTB pack: magic, count, (addr, len)*, data)
const G7221_RANGES: [(u32, u32); 2] = [(0x20fbc00, 0x80), (0x2112e00, 0x3600)];

fn hex_sha1(b: &[u8]) -> String {
    Sha1::digest(b).iter().map(|x| format!("{x:02x}")).collect()
}

/// how far the extraction is (stage name, done, of) for a progress bar
pub type Progress<'a> = &'a mut dyn FnMut(&str, usize, usize);

/// the ROM's identity: (title, game code, sha1, size); Err when it is not LovePlus+
pub fn identify(rom: &[u8]) -> Result<(String, String, String, usize), Error> {
    if rom.len() < 0x4000 {
        return Err(Error::NotLovePlus("too small for a DS ROM".into()));
    }
    let code = &rom[0x0c..0x10];
    if code != GAME_CODE {
        return Err(Error::NotLovePlus(format!("game code {}", String::from_utf8_lossy(code))));
    }
    let title: String = rom[..12].iter().take_while(|&&c| c != 0).map(|&c| c as char).collect();
    Ok((title, String::from_utf8_lossy(code).into(), hex_sha1(rom), rom.len()))
}

/// the ROM read into `out` (written to `out.partial` first and moved into place, so a cancelled or failed run never
/// leaves a half tree the engine would try to run)
pub fn extract(rom: &[u8], out: &Path, progress: Progress) -> Result<(), Error> {
    let (title, code, sha1, size) = identify(rom)?;
    let tmp = out.with_extension("partial");
    if tmp.exists() {
        fs::remove_dir_all(&tmp)?;
    }
    fs::create_dir_all(&tmp)?;
    let files = nitrofs(rom)?;
    let total = files.len();
    // 1) fs/: the files as the game reads them, and the card tables
    for (i, f) in files.iter().enumerate() {
        let dst = tmp.join("fs").join(&f.path);
        fs::create_dir_all(dst.parent().unwrap())?;
        fs::write(&dst, &rom[f.start as usize..f.end as usize])?;
        if i % 64 == 0 {
            progress("Copying game files", i, total);
        }
    }
    let index: Vec<serde_json::Value> = files.iter().map(|f| serde_json::json!([f.id, f.path, f.start, f.end])).collect();
    fs::write(tmp.join("fs/index.json"), serde_json::to_vec(&index).unwrap())?;
    let parts: [(&str, usize, usize); 6] = [
        ("header.bin", 0, 0x4000),
        ("fnt.bin", u32le(rom, 0x40) as usize, u32le(rom, 0x44) as usize),
        ("fat.bin", u32le(rom, 0x48) as usize, u32le(rom, 0x4c) as usize),
        ("ovt9.bin", u32le(rom, 0x50) as usize, u32le(rom, 0x54) as usize),
        ("signature.bin", u32le(rom, 0x80) as usize, 0x88),
        ("arm9.bin", u32le(rom, 0x20) as usize, u32le(rom, 0x2c) as usize),
    ];
    let mut romj = serde_json::Map::new();
    for (name, o, n) in parts {
        let b = rom.get(o..(o + n).min(rom.len())).ok_or_else(|| Error::Damaged(format!("{name} outside the image")))?;
        fs::write(tmp.join("fs").join(name), b)?;
        romj.insert(name.into(), serde_json::json!([o, n]));
    }
    fs::write(tmp.join("fs/rom.json"), serde_json::to_vec(&romj).unwrap())?;
    // 2) assets/: archives unpacked, the cipher removed
    let mut entries = 0;
    for (i, f) in files.iter().enumerate() {
        let data = &rom[f.start as usize..f.end as usize];
        let dst = tmp.join("assets").join(&f.path);
        if data.len() >= 4 && data[..4] == [0x18, 0x56, 0x01, 0x00] {
            unpack_arc(data, &dst, &mut entries)?;
        } else {
            fs::create_dir_all(dst.parent().unwrap())?;
            fs::write(&dst, data)?;
        }
        if i % 32 == 0 {
            progress("Unpacking archives", i, total);
        }
    }
    let mut stack = vec![tmp.join("assets")];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d)? {
            let p = e?.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("");
            if ext == "sbin" || ext == "txrc" {
                let raw = fs::read(&p)?;
                if raw.len() >= 8 && (&raw[..4] == b"SBIN" || &raw[..4] == b"TXRC") {
                    fs::write(&p, crate::script::decrypt(&raw))?;
                }
            }
        }
    }
    // 3) engine/: the code images
    progress("Reading the game's code", 0, 1);
    let eng = tmp.join("engine");
    fs::create_dir_all(&eng)?;
    let st = arm9_static(rom)?;
    fs::write(eng.join("static.bin"), &st)?;
    let mut mm = serde_json::Map::new();
    mm.insert("static".into(), serde_json::json!({"base": 0x2000000, "size": st.len(), "sha1": hex_sha1(&st)}));
    for (name, addr, blob, bss) in arm9_autoloads(rom, &st) {
        if name == "itcm" || name == "dtcm" {
            fs::write(eng.join(format!("{name}.bin")), &blob)?;
            mm.insert(name, serde_json::json!({"base": addr, "size": blob.len(), "bss": bss, "sha1": hex_sha1(&blob)}));
        }
    }
    let (fat_off, tab_off, tab_sz) = (u32le(rom, 0x48) as usize, u32le(rom, 0x50) as usize, u32le(rom, 0x54) as usize);
    for i in 0..tab_sz / 32 {
        let r = tab_off + 32 * i;
        let w: Vec<u32> = (0..8).map(|k| u32le(rom, r + 4 * k)).collect();
        let (oid, ram, bss, s0, s1, fid, comp) = (w[0], w[1], w[3], w[4], w[5], w[6] as usize, w[7]);
        let (fo, fe) = (u32le(rom, fat_off + 8 * fid) as usize, u32le(rom, fat_off + 8 * fid + 4) as usize);
        let raw = rom.get(fo..fe).ok_or_else(|| Error::Damaged(format!("overlay {oid} outside the image")))?;
        let (csize, flag) = ((comp & 0xff_ffff) as usize, comp >> 24);
        let data = if flag & 1 != 0 { blz_decode(raw, if csize != 0 && csize <= raw.len() { csize } else { raw.len() })? } else { raw.to_vec() };
        fs::write(eng.join(format!("ov{oid:02}.bin")), &data)?;
        mm.insert(format!("ov{oid:02}"), serde_json::json!({"base": ram, "size": data.len(), "bss": bss, "sinit": [s0, s1], "sha1": hex_sha1(&data)}));
    }
    fs::write(eng.join("memory_map.json"), serde_json::to_vec_pretty(&mm).unwrap())?;
    let mut pack = b"LPTB".to_vec();
    pack.extend((G7221_RANGES.len() as u32).to_le_bytes());
    for (a, n) in G7221_RANGES {
        pack.extend(a.to_le_bytes());
        pack.extend(n.to_le_bytes());
    }
    for (a, n) in G7221_RANGES {
        let o = (a - 0x2000000) as usize;
        pack.extend(st.get(o..o + n as usize).ok_or_else(|| Error::Damaged("voice tables".into()))?);
    }
    fs::write(eng.join("g7221.pack"), pack)?;
    let man = serde_json::json!({
        "extractor_version": VERSION,
        "rom": {"game_code": code, "title": title, "sha1": sha1, "size": size},
        "stages": {"fs": {"files": files.len()}, "archive_entries": entries},
    });
    fs::write(tmp.join("manifest.json"), serde_json::to_vec_pretty(&man).unwrap())?;
    // the finished tree replaces any older one
    if out.exists() {
        fs::remove_dir_all(out)?;
    }
    fs::rename(&tmp, out)?;
    progress("Done", 1, 1);
    Ok(())
}

/// the extractor version and ROM sha1 a content tree was written with (None: no usable tree)
pub fn installed(content: &Path) -> Option<(u32, String)> {
    let m: serde_json::Value = serde_json::from_slice(&fs::read(content.join("manifest.json")).ok()?).ok()?;
    Some((m["extractor_version"].as_u64()? as u32, m["rom"]["sha1"].as_str()?.to_string()))
}

/// the content tree is complete enough to run (what the runtime opens first)
pub fn usable(content: &Path) -> bool {
    ["engine/static.bin", "engine/memory_map.json", "fs/arm9.bin", "fs/fat.bin", "fs/index.json"].iter().all(|p| content.join(p).is_file())
}
