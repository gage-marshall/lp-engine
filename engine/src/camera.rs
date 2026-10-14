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
    /// preset word 7 as a 16-bit angle in degrees.  In one RAM snapshot the renderer's world matrix was a Y rotation equal to this angle (see docs/CAMERA_NOTES.md); NOT applied by the player
    /// (sign and meaning unverified; applying it by eye produced a mirrored scene).
    pub yaw_deg: f64,
    /// the record's 41 words as stored
    pub words: Vec<i32>,
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
        yaw_deg: (w[7] & 0xffff) as f64 / 65536.0 * 360.0,
        words: w.clone(),
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

/// The game's camera for a preset (docs/RENDER_PRIMITIVES.md "Global state (FlushVP)", all verified packet-exact):
/// * projection `MTX_PerspectiveW(sin/cos at index 1820 = 706/4035, aspect 0x1555, near 1, far 400)`;
/// * camera `MTX_LookAt(eye = words 1-3, up (-1, 0, 0), target = (words 4, 5, 0))`: the DS renders a landscape screen;
/// * base per drawn object: RotY(word 7) (NNS convention, row 0 = (cos, 0, -sin)) and translation (0, height, 0) with the
///   per-heroine height `chara+0x44` from the table at 0x2102e2c (`Gfx3D_DrawActor`);
/// * lights (`Model_ApplyGlobalLights`): world-space vectors (0, 0, -1) x RotX(words 8-10) x RotY(words 11-13).
/// The console is held sideways, so the engine draws the screen turned 90 degrees counter-clockwise (portrait): the view
/// below maps model coordinates straight to that portrait view (x right, y up, z forward).
#[derive(Clone, Debug)]
pub struct GameCamera {
    /// model -> portrait view: rows of the 3x3 and the translation
    pub rot: [[f64; 3]; 3],
    pub trans: [f64; 3],
    /// cot of the half angle across the portrait width (the landscape height)
    pub cot: f64,
    /// landscape aspect (0x1555 / 4096)
    pub aspect: f64,
    pub near: f64,
    pub far: f64,
    /// light travel directions in the portrait view
    pub lights: [[f64; 3]; 3],
    /// landscape view (for projecting world points the way the game does): right, up, look rows and the eye
    land: [[f64; 3]; 3],
    eye: [f64; 3],
}

/// `FX_SinCosTable_` (0x20fc090): (sin, cos) of a 16-bit angle index
pub fn sin_cos(rom: &crate::rom::Rom, idx: u32) -> (f64, f64) {
    let i = ((idx & 0xffff) >> 4) << 1;
    (rom.i16(0x20fc090 + 2 * i) as f64 / 4096.0, rom.i16(0x20fc090 + 2 * i + 2) as f64 / 4096.0)
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt().max(1e-12);
    [a[0] / l, a[1] / l, a[2] / l]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

/// per-heroine model height `chara+0x44` (`Gfx3D_DrawActor`, table at 0x2102e2c reached through the literal at 0x207d95c)
pub fn heroine_height(rom: &crate::rom::Rom, heroine: u32) -> f64 {
    rom.i32(rom.u32(0x207d95c) + 4 * heroine) as f64 / 4096.0
}

impl GameCamera {
    pub fn new(p: &Preset, rom: &crate::rom::Rom, height: f64) -> GameCamera {
        let w = &p.words;
        let fx = |x: i32| x as f64 / 4096.0;
        let eye = [fx(w[1]), fx(w[2]), fx(w[3])];
        let tgt = [fx(w[4]), fx(w[5]), 0.0];
        let look = norm([eye[0] - tgt[0], eye[1] - tgt[1], eye[2] - tgt[2]]);
        let right = norm(cross([-1.0, 0.0, 0.0], look));
        let up = cross(look, right);
        // portrait view: x' = -y_land, y' = x_land, z' = -z_land (forward)
        let e = [[-up[0], -up[1], -up[2]], right, [-look[0], -look[1], -look[2]]];
        let (s, c) = sin_cos(rom, w[7] as u32);
        let base = [[c, 0.0, s], [0.0, 1.0, 0.0], [-s, 0.0, c]]; // RotY in column form (NNS rows (c,0,-s), (0,1,0), (s,0,c))
        let mut rot = [[0.0; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                rot[i][j] = (0..3).map(|k| e[i][k] * base[k][j]).sum();
            }
        }
        let rel = [-eye[0], height - eye[1], -eye[2]];
        let trans = [dot(e[0], rel), dot(e[1], rel), dot(e[2], rel)];
        let (s1820, c1820) = sin_cos(rom, 1820);
        let mut lights = [[0.0; 3]; 3];
        for i in 0..3 {
            let idx = |deg: i32| (((deg as i64) << 16) / 360) as u32 & 0xffff;
            let (sy, cy) = sin_cos(rom, idx(w[11 + i]));
            let (sp, cp) = sin_cos(rom, idx(w[8 + i]));
            let d = [-cp * sy, sp, -cp * cy];
            lights[i] = [dot(e[0], d), dot(e[1], d), dot(e[2], d)];
        }
        GameCamera { rot, trans, cot: c1820 / s1820, aspect: 0x1555 as f64 / 4096.0, near: 1.0, far: 400.0, lights, land: [right, up, look], eye }
    }

    /// a world point on the landscape screen (pixels, 256 x 192), as `NNS_G3dWorldPosToScrPos` with viewport 0,0,255,191
    pub fn screen_pos(&self, p: [f64; 3]) -> (f64, f64) {
        let r = [p[0] - self.eye[0], p[1] - self.eye[1], p[2] - self.eye[2]];
        let (x, y, z) = (dot(self.land[0], r), dot(self.land[1], r), dot(self.land[2], r));
        let (cx, cy) = (x * self.cot / self.aspect / -z, y * self.cot / -z);
        ((cx + 1.0) * 128.0, (1.0 - cy) * 96.0)
    }

    /// BG1 scroll of the backdrop (`Projection_UpdateScreenPosition`): project the camera target (z = 0), then
    /// (64 + (128 - sx) * 20 / 100, 32 + (95 - sy) * 20 / 100), clamped to [0, 128] x [0, 64]; no camera shake
    pub fn bg_scroll(&self, p: &Preset) -> (i32, i32) {
        let (sx, sy) = self.screen_pos([p.words[4] as f64 / 4096.0, p.words[5] as f64 / 4096.0, 0.0]);
        let (sx, sy) = (sx.round() as i32, sy.round() as i32);
        (((0x80 - sx) * 0x14 / 100 + 0x40).clamp(0, 0x80), ((0x5f - sy) * 0x14 / 100 + 0x20).clamp(0, 0x40))
    }
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
