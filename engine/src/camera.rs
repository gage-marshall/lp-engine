//! Camera presets (`camera_data_N.bin` archives; 164-byte records).  Decoded from Camera_SetPreset; reference: tools/nns3d_export.py
//! (`export_camera_presets`).  id = record word 0.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Preset {
    pub id: i32,
    pub eye: [f64; 3],
    pub target: [f64; 3],
    /// (yaw, pitch) in degrees, view space, for the three scene lights
    pub lights: [(i32, i32); 3],
}

pub fn parse_record(b: &[u8]) -> Option<Preset> {
    if b.len() != 164 {
        return None;
    }
    let w: Vec<i32> = b.chunks_exact(4).map(|c| i32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect();
    let fx = |x: i32| x as f64 / 4096.0;
    Some(Preset {
        id: w[0],
        eye: [fx(w[1]), fx(w[2]), fx(w[3])],
        target: [fx(w[4]), fx(w[5]), fx(w[6])],
        lights: [(w[11], w[8]), (w[12], w[9]), (w[13], w[10])],
    })
}

/// all presets under `content/assets/arc/camera_data_N.bin/`
pub fn load_presets(arc_root: &Path) -> BTreeMap<i32, Preset> {
    let mut out = BTreeMap::new();
    let mut dirs: Vec<_> = fs::read_dir(arc_root).map(|r| r.filter_map(|e| e.ok()).map(|e| e.path()).collect()).unwrap_or_default();
    dirs.sort();
    for d in dirs {
        let n = d.file_name().unwrap().to_string_lossy().to_string();
        if !(n.starts_with("camera_data_") && n.ends_with(".bin") && d.is_dir()) {
            continue;
        }
        let mut files: Vec<_> = fs::read_dir(&d).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).collect();
        files.sort();
        for f in files {
            if let Some(p) = fs::read(&f).ok().and_then(|b| parse_record(&b)) {
                out.insert(p.id, p);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_a_record() {
        let mut w = vec![0i32; 41];
        w[0] = 1000;
        w[2] = 144 * 4096;
        w[8] = -18;
        w[11] = -48;
        let b: Vec<u8> = w.iter().flat_map(|x| x.to_le_bytes()).collect();
        let p = parse_record(&b).unwrap();
        assert_eq!((p.id, p.eye[1], p.lights[0]), (1000, 144.0, (-48, -18)));
        assert!(parse_record(&b[..100]).is_none());
    }
}
