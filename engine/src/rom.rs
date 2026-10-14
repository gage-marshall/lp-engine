//! The game's code and data image, extracted once into `content/engine/` (static.bin, ovNN.bin, memory_map.json).
//!
//! Game rules ported from the code read their tables here by the address the game uses (the address is the documentation:
//! each caller names the function it was decoded from), so no table is copied into the engine's source.

use std::collections::HashMap;
use std::path::Path;

pub struct Rom {
    main: Vec<u8>,
    main_base: u32,
    /// overlay id -> (load address, bytes); overlays share load addresses, so reads name the overlay
    ovl: HashMap<u32, (u32, Vec<u8>)>,
    /// overlay id -> (bss size, static initialiser table [start, end))
    ovl_info: HashMap<u32, (u32, (u32, u32))>,
    dir: std::path::PathBuf,
}

impl Rom {
    pub fn open(content: &Path) -> Result<Rom, String> {
        let dir = content.join("engine");
        let map: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("memory_map.json")).map_err(|e| format!("memory_map.json: {e}"))?).map_err(|e| e.to_string())?;
        let main = std::fs::read(dir.join("static.bin")).map_err(|e| format!("static.bin: {e}"))?;
        let main_base = map["static"]["base"].as_u64().ok_or("memory_map: static.base")? as u32;
        let mut ovl = HashMap::new();
        let mut ovl_info = HashMap::new();
        if let Some(m) = map.as_object() {
            for (k, v) in m {
                if let Some(n) = k.strip_prefix("ov").and_then(|n| n.parse::<u32>().ok()) {
                    if let (Some(base), Ok(d)) = (v["base"].as_u64(), std::fs::read(dir.join(format!("{k}.bin")))) {
                        ovl.insert(n, (base as u32, d));
                    }
                    let sinit = (v["sinit"][0].as_u64().unwrap_or(0) as u32, v["sinit"][1].as_u64().unwrap_or(0) as u32);
                    ovl_info.insert(n, (v["bss"].as_u64().unwrap_or(0) as u32, sinit));
                }
            }
        }
        Ok(Rom { main, main_base, ovl, ovl_info, dir: content.to_path_buf() })
    }

    /// an image for tests: `main` loaded at `base`, no overlays
    pub fn from_bytes(main: Vec<u8>, base: u32) -> Rom {
        Rom { main, main_base: base, ovl: HashMap::new(), ovl_info: HashMap::new(), dir: Default::default() }
    }

    fn bytes(&self, addr: u32, n: usize) -> &[u8] {
        let o = addr.wrapping_sub(self.main_base) as usize;
        self.main.get(o..o + n).unwrap_or_else(|| panic!("address {addr:#x} outside the static image"))
    }

    /// the content directory the image was read from
    pub fn content_dir(&self) -> &Path {
        &self.dir
    }

    pub fn u8(&self, addr: u32) -> u8 {
        self.bytes(addr, 1)[0]
    }
    pub fn i8(&self, addr: u32) -> i8 {
        self.u8(addr) as i8
    }
    pub fn u16(&self, addr: u32) -> u16 {
        let b = self.bytes(addr, 2);
        u16::from_le_bytes([b[0], b[1]])
    }
    pub fn i16(&self, addr: u32) -> i16 {
        self.u16(addr) as i16
    }
    pub fn u32(&self, addr: u32) -> u32 {
        let b = self.bytes(addr, 4);
        u32::from_le_bytes([b[0], b[1], b[2], b[3]])
    }
    pub fn i32(&self, addr: u32) -> i32 {
        self.u32(addr) as i32
    }
    /// `n` consecutive words starting at `addr`
    pub fn i32s(&self, addr: u32, n: usize) -> Vec<i32> {
        (0..n as u32).map(|i| self.i32(addr + 4 * i)).collect()
    }
    pub fn i8s(&self, addr: u32, n: usize) -> Vec<i8> {
        (0..n as u32).map(|i| self.i8(addr + i)).collect()
    }

    /// the static image: (load address, bytes)
    pub fn static_image(&self) -> (u32, &[u8]) {
        (self.main_base, &self.main)
    }
    /// overlay `ov`'s image: (load address, bytes)
    pub fn overlay_image(&self, ov: u32) -> Option<(u32, &[u8])> {
        self.ovl.get(&ov).map(|(b, d)| (*b, d.as_slice()))
    }

    /// overlay `ov`'s bss size and static initialiser table [start, end) (FS_StartOverlay runs the table's functions)
    pub fn overlay_info(&self, ov: u32) -> Option<(u32, (u32, u32))> {
        self.ovl_info.get(&ov).copied()
    }

    /// a word of overlay `ov` (loaded at its base address)
    pub fn ov_u32(&self, ov: u32, addr: u32) -> u32 {
        let (base, d) = self.ovl.get(&ov).unwrap_or_else(|| panic!("overlay {ov} not extracted"));
        let o = addr.wrapping_sub(*base) as usize;
        u32::from_le_bytes(d[o..o + 4].try_into().unwrap())
    }
    pub fn ov_u16(&self, ov: u32, addr: u32) -> u16 {
        let (base, d) = self.ovl.get(&ov).unwrap_or_else(|| panic!("overlay {ov} not extracted"));
        let o = addr.wrapping_sub(*base) as usize;
        u16::from_le_bytes(d[o..o + 2].try_into().unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_by_game_address() {
        let r = Rom::from_bytes(vec![1, 0xff, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12], 0x200_0000);
        assert_eq!(r.u8(0x200_0000), 1);
        assert_eq!(r.i8(0x200_0001), -1);
        assert_eq!(r.u16(0x200_0002), 0x1234);
        assert_eq!(r.u32(0x200_0004), 0x1234_5678);
    }
}
