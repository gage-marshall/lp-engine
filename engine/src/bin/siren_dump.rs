//! Decode voice clips with the Rust decoder and print the PCM digest; compared with tools/siren14dec.c by tools/loader_check.sh.
//!   siren_dump CONTENT CLIP [CLIP ...]  [--wav OUT_PREFIX]
use lp_engine::siren::*;
use sha1::{Digest, Sha1};
use std::fs;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let c = Path::new(&a[1]);
    let tables = Tables::from_pack(&fs::read(c.join("engine/g7221.pack")).unwrap()).unwrap();
    let vtb = fs::read(c.join("assets/sys/Siren14Voice.vtb")).unwrap();
    let spp = fs::read(c.join("assets/sys/Siren14Voice.spp")).unwrap();
    for s in &a[2..] {
        let clip: usize = match s.parse() { Ok(v) => v, Err(_) => continue };
        let (off, size) = (u32::from_le_bytes(vtb[clip * 8..clip * 8 + 4].try_into().unwrap()) as usize / 16, u32::from_le_bytes(vtb[clip * 8 + 4..clip * 8 + 8].try_into().unwrap()) as usize / 16);
        let (pcm, errs) = decode_clip(&tables, &spp[off..off + size], 16000, 14000);
        let bytes: Vec<u8> = pcm.iter().flat_map(|x| x.to_le_bytes()).collect();
        let mut h = Sha1::new();
        h.update(&bytes);
        println!("{} {} {} {}", clip, pcm.len(), errs, h.finalize().iter().take(8).map(|b| format!("{:02x}", b)).collect::<String>());
    }
}
