//! Builds the asset index (`lp_engine::assetindex`): `asset_index CONTENT` writes `CONTENT/index/textures.json`,
//! the name of every texture (with each palette of its file) by the key the runtime gives it, and
//! `CONTENT/index/planes.json`, every text BG screen by its map and tiles, for dumps and mods.
use lp_engine::assetindex;
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let Some(content) = a.get(1) else {
        eprintln!("usage: asset_index CONTENT");
        std::process::exit(1)
    };
    let content = Path::new(content);
    let t0 = std::time::Instant::now();
    let (textures, dup) = assetindex::textures(content);
    let dir = content.join("index");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("textures.json"), assetindex::to_json(&textures)).unwrap();
    println!("{} textures (texture x palette) indexed, {} found again in other files -> {}", textures.len(), dup, dir.join("textures.json").display());
    let planes = assetindex::planes(content);
    std::fs::write(dir.join("planes.json"), assetindex::planes_to_json(&planes)).unwrap();
    let screens: usize = planes.values().map(|v| v.len()).sum();
    let shared = planes.values().filter(|v| v.len() > 1).count();
    println!("{screens} screens indexed by map ({shared} maps shared by several screens) -> {}", dir.join("planes.json").display());
    println!("in {:.1} s", t0.elapsed().as_secs_f64());
}
