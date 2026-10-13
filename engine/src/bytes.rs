//! Bounds-checked little-endian readers shared by the format loaders.
pub fn u8at(d: &[u8], p: usize) -> Result<u32, String> {
    d.get(p).map(|&b| b as u32).ok_or_else(|| format!("read past end at {p:#x}"))
}
pub fn u16at(d: &[u8], p: usize) -> Result<u32, String> {
    d.get(p..p + 2).map(|b| u16::from_le_bytes([b[0], b[1]]) as u32).ok_or_else(|| format!("read past end at {p:#x}"))
}
pub fn u32at(d: &[u8], p: usize) -> Result<u32, String> {
    d.get(p..p + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| format!("read past end at {p:#x}"))
}
pub fn i16at(d: &[u8], p: usize) -> Result<i32, String> {
    Ok(u16at(d, p)? as u16 as i16 as i32)
}
pub fn i32at(d: &[u8], p: usize) -> Result<i32, String> {
    Ok(u32at(d, p)? as i32)
}
/// fixed point with `shift` fractional bits
pub fn fx(v: i32) -> f64 {
    v as f64 / 4096.0
}
