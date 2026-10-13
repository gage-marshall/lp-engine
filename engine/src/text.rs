//! TXRC text resources (system / UI / mail text).  Layout from TextRes_GetCurrentEntry @0x209eccc; reference parser:
//! tools/txrc.py.  Input is the layer-0 file (cipher already removed); `from_encrypted` is provided for raw ROM data.
//!
//! header: +8 groups, +0xc table A (group -> (nsub, base) u16x2), +0x10 table B (sub -> (nline, base) u16x2),
//! +0x14 table C (line -> u32 len<<22 | offset), +0x18 data base.

use crate::script::decrypt;

pub struct Txrc {
    /// groups -> subs -> lines (raw bytes, UTF-8 text in this game)
    pub groups: Vec<Vec<Vec<Vec<u8>>>>,
}

fn u16le(d: &[u8], p: usize) -> Result<usize, String> {
    d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or_else(|| format!("TXRC: read past end at {p:#x}"))
}
fn u32le(d: &[u8], p: usize) -> Result<usize, String> {
    d.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize).ok_or_else(|| format!("TXRC: read past end at {p:#x}"))
}

impl Txrc {
    pub fn from_encrypted(raw: &[u8]) -> Result<Txrc, String> {
        Self::parse(&decrypt(raw))
    }

    pub fn parse(d: &[u8]) -> Result<Txrc, String> {
        if d.get(0..4) != Some(b"TXRC") {
            return Err("not a TXRC resource".into());
        }
        let (ng, a, b, c, base) = (u32le(d, 8)?, u32le(d, 12)?, u32le(d, 16)?, u32le(d, 20)?, u32le(d, 24)?);
        let mut groups = Vec::with_capacity(ng);
        for g in 0..ng {
            let (ns, sb) = (u16le(d, a + g * 4)?, u16le(d, a + g * 4 + 2)?);
            let mut subs = Vec::with_capacity(ns);
            for s in 0..ns {
                let (nl, lb) = (u16le(d, b + (sb + s) * 4)?, u16le(d, b + (sb + s) * 4 + 2)?);
                let mut lines = Vec::with_capacity(nl);
                for l in 0..nl {
                    let v = u32le(d, c + (lb + l) * 4)?;
                    let o = base + (v & 0x3f_ffff);
                    let end = (o + (v >> 22)).min(d.len());
                    lines.push(d.get(o..end).unwrap_or(&[]).to_vec());
                }
                subs.push(lines);
            }
            groups.push(subs);
        }
        Ok(Txrc { groups })
    }

    pub fn line(&self, group: usize, sub: usize, line: usize) -> Option<&[u8]> {
        self.groups.get(group)?.get(sub)?.get(line).map(|v| v.as_slice())
    }

    pub fn line_str(&self, group: usize, sub: usize, line: usize) -> Option<String> {
        self.line(group, sub, line).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    pub fn line_count(&self) -> usize {
        self.groups.iter().flatten().map(|s| s.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_synthetic_resource() {
        // 1 group with 1 sub holding 2 lines: "ab", "あ"
        let mut d = vec![0u8; 0x1c];
        d[0..4].copy_from_slice(b"TXRC");
        let (a, b, c, base) = (0x1cu32, 0x20u32, 0x24u32, 0x2cu32);
        d[8..12].copy_from_slice(&1u32.to_le_bytes());
        d[12..16].copy_from_slice(&a.to_le_bytes());
        d[16..20].copy_from_slice(&b.to_le_bytes());
        d[20..24].copy_from_slice(&c.to_le_bytes());
        d[24..28].copy_from_slice(&base.to_le_bytes());
        d.extend_from_slice(&[1, 0, 0, 0]); // group 0: 1 sub, base 0
        d.extend_from_slice(&[2, 0, 0, 0]); // sub 0: 2 lines, base 0
        d.extend_from_slice(&((2u32 << 22) | 0).to_le_bytes());
        d.extend_from_slice(&((3u32 << 22) | 2).to_le_bytes());
        d.extend_from_slice("abあ".as_bytes());
        let t = Txrc::parse(&d).unwrap();
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.line_str(0, 0, 0).unwrap(), "ab");
        assert_eq!(t.line_str(0, 0, 1).unwrap(), "あ");
        assert!(Txrc::parse(b"nope").is_err());
    }
}
