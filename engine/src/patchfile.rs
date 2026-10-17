//! The distributed translation (`*.lpdelta`, written by tools/lp_delta.py) applied to the player's content tree: it
//! rebuilds the patch overlay (`patch/` beside the content) from copy / insert operations against a reference made of
//! the player's own original file and its views (every archive entry decompressed, recursively, and every SBIN / TXRC
//! decrypted). What is copied comes from the player's ROM; the delta carries only the translation's own data.
//!
//! Format: b"LPDELTA1", u32 header length, the header (JSON: {"version": 1, "rom_sha1", "files": [{"path", "size",
//! "sha1", "ref", "ops"}]}), then zlib: the files' operation streams back to back. An operation is a varint: (n << 1)
//! then the source offset (copy n bytes of the reference), or (n << 1 | 1) then n bytes (insert them).

use crate::extract::{is_arc, maybe_lz};
use sha1::{Digest, Sha1};
use std::io::Read;
use std::path::Path;

const MAGIC: &[u8; 8] = b"LPDELTA1";

/// the file and what is inside it, appended to `out` (as tools/lp_delta.py views)
fn views(b: &[u8], include_self: bool, out: &mut Vec<u8>) {
    if include_self {
        out.extend_from_slice(b);
    }
    if is_arc(b) {
        let cnt = u16::from_le_bytes([b[4], b[5]]) as usize;
        if cnt == 0 {
            return;
        }
        let first = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as usize;
        let stride = if first > 12 { (first - 12) / cnt } else { 12 };
        for i in 0..cnt {
            let r = 12 + i * stride;
            if r + 12 > b.len() {
                return;
            }
            let rd = |o: usize| u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as usize;
            let (o, s, u) = (rd(r), rd(r + 4), rd(r + 8));
            let Some(raw) = b.get(o..o + s) else { return };
            let e = maybe_lz(raw, u);
            let decompressed = e.as_slice() != raw;
            views(&e, decompressed, out);
        }
    } else if b.len() >= 8 && (&b[..4] == b"SBIN" || &b[..4] == b"TXRC") {
        out.extend(crate::script::decrypt(b));
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// what the delta says about itself: (rom sha1 it was built against, number of files)
pub fn describe(delta: &[u8]) -> Result<(String, usize), String> {
    let (header, _) = split(delta)?;
    Ok((header["rom_sha1"].as_str().unwrap_or("").to_string(), header["files"].as_array().map_or(0, |f| f.len())))
}

fn split(delta: &[u8]) -> Result<(serde_json::Value, &[u8]), String> {
    if delta.len() < 12 || &delta[..8] != MAGIC {
        return Err("not a LovePlus+ translation file".into());
    }
    let hl = u32::from_le_bytes([delta[8], delta[9], delta[10], delta[11]]) as usize;
    let header: serde_json::Value = serde_json::from_slice(delta.get(12..12 + hl).ok_or("header cut short")?).map_err(|e| e.to_string())?;
    Ok((header, &delta[12 + hl..]))
}

/// the patch overlay rebuilt into `patch` (written to `patch.partial` first, then moved into place) from the delta
/// and the content tree `content`; every file checked against its sha1
pub fn apply(content: &Path, delta: &[u8], patch: &Path, progress: &mut dyn FnMut(&str, usize, usize)) -> Result<(), String> {
    let (header, body) = split(delta)?;
    let mut ops_all = Vec::new();
    flate2::read::ZlibDecoder::new(body).read_to_end(&mut ops_all).map_err(|e| format!("translation file damaged: {e}"))?;
    let files = header["files"].as_array().ok_or("no file list")?;
    let tmp = patch.with_extension("partial");
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp).map_err(|e| e.to_string())?;
    }
    let mut at = 0usize;
    for (k, f) in files.iter().enumerate() {
        let path = f["path"].as_str().ok_or("file without a path")?;
        if path.contains("..") || path.starts_with('/') {
            return Err(format!("bad path in the translation file: {path}"));
        }
        let n = f["ops"].as_u64().ok_or("file without operations")? as usize;
        let ops = ops_all.get(at..at + n).ok_or("operations cut short")?;
        at += n;
        let mut reference = Vec::new();
        if let Some(r) = f["ref"].as_str() {
            let orig = std::fs::read(content.join(r)).map_err(|e| format!("{r}: {e} (extract the game first)"))?;
            views(&orig, true, &mut reference);
        }
        let out = run(&reference, ops).ok_or_else(|| format!("{path}: operations do not fit the game's files"))?;
        let want = f["sha1"].as_str().unwrap_or("");
        if hex(&Sha1::digest(&out)) != want {
            return Err(format!("{path}: the result does not match (is the ROM the same release the translation was made for?)"));
        }
        let dst = tmp.join(path);
        std::fs::create_dir_all(dst.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&dst, out).map_err(|e| e.to_string())?;
        progress("Applying the translation", k + 1, files.len());
    }
    if patch.exists() {
        std::fs::remove_dir_all(patch).map_err(|e| e.to_string())?;
    }
    std::fs::rename(&tmp, patch).map_err(|e| e.to_string())?;
    Ok(())
}

fn run(reference: &[u8], ops: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut p = 0;
    let varint = |p: &mut usize| -> Option<usize> {
        let (mut v, mut s) = (0usize, 0);
        loop {
            let b = *ops.get(*p)?;
            *p += 1;
            v |= ((b & 0x7f) as usize) << s;
            s += 7;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
    };
    while p < ops.len() {
        let t = varint(&mut p)?;
        let n = t >> 1;
        if t & 1 != 0 {
            out.extend_from_slice(ops.get(p..p + n)?);
            p += n;
        } else {
            let o = varint(&mut p)?;
            out.extend_from_slice(reference.get(o..o + n)?);
        }
    }
    Some(out)
}
