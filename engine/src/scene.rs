//! Scene presentation helpers: the camera track and the dialogue window. Game state comes from `sim` (`SceneFrame`, script
//! events); nothing here decides it.

use crate::camera::Preset;
use crate::font::Font;
use crate::host::Effect;
use crate::nns2d::Image;
use std::collections::BTreeMap;

/// The camera preset over time. Command 6 (set the preset) is verified: it snaps. Commands 0xb / 0x7 (move to a preset over
/// `frames`) are NOT verified against the game yet: they ease linearly over the preset words as a placeholder
/// (docs/ENGINE_PORT_PLAN.md "Unverified rules are flagged, not tuned").
#[derive(Clone, Debug)]
pub struct CameraTrack {
    from: Option<Preset>,
    to: Option<Preset>,
    /// game frames of the move: (start, length)
    span: (i64, i64),
}

impl CameraTrack {
    pub fn new(preset: Option<&Preset>) -> CameraTrack {
        CameraTrack { from: preset.cloned(), to: preset.cloned(), span: (0, 0) }
    }

    /// a camera command issued at game frame `frame`
    pub fn apply(&mut self, frame: i64, e: &Effect, presets: &BTreeMap<i32, Preset>) {
        if let Effect::Camera { preset, frames, snap } = e {
            if let Some(p) = presets.get(&(*preset as i32)) {
                let len = if *snap { 0 } else { frames.unwrap_or(0) as i64 }; // UNVERIFIED: easing of 0xb / 0x7
                self.from = if len > 0 { Some(self.at(frame as f64)) } else { Some(p.clone()) };
                self.to = Some(p.clone());
                self.span = (frame, len);
            }
        }
    }

    /// the preset in force at (fractional) game frame `frame`
    pub fn at(&self, frame: f64) -> Preset {
        let (Some(a), Some(b)) = (&self.from, &self.to) else {
            return self.to.clone().or_else(|| self.from.clone()).unwrap_or(Preset { id: -1, eye: [0.0, 144.0, 150.0], target: [0.0; 3], lights: [(0, 0); 3], yaw_deg: 0.0, words: vec![0; 41] });
        };
        if self.span.1 <= 0 {
            return b.clone();
        }
        let k = ((frame - self.span.0 as f64) / self.span.1 as f64).clamp(0.0, 1.0);
        if k >= 1.0 {
            return b.clone();
        }
        let mut p = b.clone();
        for i in 1..8 {
            p.words[i] = (a.words[i] as f64 + (b.words[i] as f64 - a.words[i] as f64) * k).round() as i32;
        }
        p
    }

    pub fn id(&self) -> i32 {
        self.to.as_ref().map_or(-1, |p| p.id)
    }
}

/// Draw the dialogue window into `img` (straight-alpha RGBA, transparent elsewhere) with the first `revealed` characters of
/// `text` (the typewriter's count from `sim::sched`). The window art is a placeholder (the game's message window layout is
/// not decoded yet).
pub fn draw_dialogue(img: &mut Image, name: &str, text: &str, revealed: usize, font: &Font) -> bool {
    if text.trim().is_empty() && name.trim().is_empty() {
        return false;
    }
    let (w, h) = (img.w as i32, img.h as i32);
    let box_h = (h as f32 * 0.24) as i32;
    let (bx, by, bw) = ((w as f32 * 0.04) as i32, h - box_h - (h as f32 * 0.035) as i32, (w as f32 * 0.92) as i32);
    let scale = ((box_h as f32 * 0.78) / (3.0 * 12.0 + 6.0)).floor().max(1.0) as i32;
    let fill = |img: &mut Image, x0: i32, y0: i32, x1: i32, y1: i32, c: [u8; 4]| {
        for y in y0.max(0)..y1.min(h) {
            for x in x0.max(0)..x1.min(w) {
                let o = (y as usize * img.w + x as usize) * 4;
                img.buf[o..o + 4].copy_from_slice(&c);
            }
        }
    };
    let b = (scale / 2).max(1);
    fill(img, bx, by, bx + bw, by + box_h, [0, 190, 215, 230]);
    fill(img, bx + b, by + b, bx + bw - b, by + box_h - b, [8, 18, 40, 215]);
    let name = name.trim();
    if !name.is_empty() {
        let (tw, th) = (font.line_width(name) * scale + 6 * scale, 14 * scale);
        fill(img, bx + 2 * scale, by - th + b, bx + 2 * scale + tw, by + b, [250, 250, 250, 245]);
        font.draw(img, name, bx + 5 * scale, by - th + 2 * scale, scale, [20, 30, 50], usize::MAX);
    }
    // the typewriter counts characters without line breaks; the font counts every char
    let mut shown = 0usize;
    let mut drawn = 0usize;
    for c in text.chars() {
        if drawn >= revealed {
            break;
        }
        shown += 1;
        if c != '\n' {
            drawn += 1;
        }
    }
    font.draw(img, text, bx + 5 * scale, by + 4 * scale, scale, [255, 255, 255], shown);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(id: i32, z: i32) -> Preset {
        let mut w = vec![0; 41];
        w[0] = id;
        w[3] = z;
        Preset { id, eye: [0.0, 0.0, z as f64 / 4096.0], target: [0.0; 3], lights: [(0, 0); 3], yaw_deg: 0.0, words: w }
    }

    #[test]
    fn camera_commands_snap_or_move() {
        let mut presets = BTreeMap::new();
        presets.insert(1, preset(1, 0));
        presets.insert(2, preset(2, 4096 * 10));
        let mut c = CameraTrack::new(presets.get(&1));
        c.apply(100, &Effect::Camera { preset: 2, frames: None, snap: true }, &presets);
        assert_eq!(c.at(100.0).words[3], 4096 * 10, "cmd 6 snaps");
        c.apply(200, &Effect::Camera { preset: 1, frames: Some(10), snap: false }, &presets);
        assert_eq!(c.at(205.0).words[3], 4096 * 5);
        assert_eq!(c.at(250.0).words[3], 0);
        assert_eq!(c.id(), 1);
    }
}
