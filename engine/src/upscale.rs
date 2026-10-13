//! CPU image resampling for the enhanced renderer: Lanczos upscaling and mip chains (premultiplied alpha, so cut-out textures keep clean edges).
use crate::nns2d::Image;

fn lanczos3(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    if x.abs() >= 3.0 {
        return 0.0;
    }
    let px = std::f64::consts::PI * x;
    3.0 * px.sin() * (px / 3.0).sin() / (px * px)
}

fn premultiply(img: &Image) -> Vec<f32> {
    let mut v = Vec::with_capacity(img.buf.len());
    for p in img.buf.chunks_exact(4) {
        let a = p[3] as f32 / 255.0;
        v.extend_from_slice(&[p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, p[3] as f32]);
    }
    v
}

fn unpremultiply(w: usize, h: usize, src: &[f32]) -> Image {
    let mut out = Image::new(w, h);
    for (i, p) in src.chunks_exact(4).enumerate() {
        let a = p[3].clamp(0.0, 255.0);
        let k = if a > 0.5 { 255.0 / a } else { 0.0 };
        for c in 0..3 {
            out.buf[i * 4 + c] = (p[c] * k).clamp(0.0, 255.0).round() as u8;
        }
        out.buf[i * 4 + 3] = a.round() as u8;
    }
    out
}

/// one separable pass: resample `src` (w x h) along x to `nw` columns
fn pass_x(src: &[f32], w: usize, h: usize, nw: usize, wrap: bool) -> Vec<f32> {
    let scale = w as f64 / nw as f64;
    let support = if scale > 1.0 { 3.0 * scale } else { 3.0 };
    let mut out = vec![0.0f32; nw * h * 4];
    for x in 0..nw {
        let c = (x as f64 + 0.5) * scale - 0.5;
        let (lo, hi) = ((c - support).floor() as i64, (c + support).ceil() as i64);
        let mut taps: Vec<(usize, f64)> = Vec::new();
        let mut sum = 0.0;
        for i in lo..=hi {
            let wgt = lanczos3((i as f64 - c) / scale.max(1.0));
            if wgt == 0.0 {
                continue;
            }
            let ii = if wrap { i.rem_euclid(w as i64) as usize } else { i.clamp(0, w as i64 - 1) as usize };
            taps.push((ii, wgt));
            sum += wgt;
        }
        for y in 0..h {
            let mut acc = [0.0f64; 4];
            for &(i, wgt) in &taps {
                for k in 0..4 {
                    acc[k] += src[(y * w + i) * 4 + k] as f64 * wgt;
                }
            }
            for k in 0..4 {
                out[(y * nw + x) * 4 + k] = (acc[k] / sum) as f32;
            }
        }
    }
    out
}

fn transpose(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; src.len()];
    for y in 0..h {
        for x in 0..w {
            for k in 0..4 {
                out[(x * h + y) * 4 + k] = src[(y * w + x) * 4 + k];
            }
        }
    }
    out
}

/// resample to `nw` x `nh` with a Lanczos-3 kernel (`wrap` = tile the texture at its borders)
pub fn lanczos_resize(img: &Image, nw: usize, nh: usize, wrap: bool) -> Image {
    let p = premultiply(img);
    let a = pass_x(&p, img.w, img.h, nw, wrap);
    let t = transpose(&a, nw, img.h);
    let b = pass_x(&t, img.h, nw, nh, wrap);
    let t2 = transpose(&b, nh, nw);
    unpremultiply(nw, nh, &t2)
}

pub fn lanczos_upscale(img: &Image, factor: usize, wrap: bool) -> Image {
    lanczos_resize(img, img.w * factor, img.h * factor, wrap)
}

/// 2x2 box-filtered mip chain (premultiplied), level 0 first, down to 1x1
pub fn mip_chain(img: &Image) -> Vec<Image> {
    let mut out = vec![img.clone()];
    while out.last().unwrap().w > 1 || out.last().unwrap().h > 1 {
        let s = out.last().unwrap();
        let (nw, nh) = ((s.w / 2).max(1), (s.h / 2).max(1));
        let p = premultiply(s);
        let mut v = vec![0.0f32; nw * nh * 4];
        for y in 0..nh {
            for x in 0..nw {
                for k in 0..4 {
                    let mut acc = 0.0;
                    for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(s.w - 1), (y * 2 + dy).min(s.h - 1));
                        acc += p[(sy * s.w + sx) * 4 + k];
                    }
                    v[(y * nw + x) * 4 + k] = acc / 4.0;
                }
            }
        }
        out.push(unpremultiply(nw, nh, &v));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(w: usize, h: usize, c: [u8; 4]) -> Image {
        let mut i = Image::new(w, h);
        for p in i.buf.chunks_exact_mut(4) {
            p.copy_from_slice(&c);
        }
        i
    }

    #[test]
    fn flat_images_stay_flat() {
        let u = lanczos_upscale(&flat(8, 8, [10, 200, 30, 255]), 4, false);
        assert_eq!((u.w, u.h), (32, 32));
        assert!(u.buf.chunks_exact(4).all(|p| p == [10, 200, 30, 255]));
    }

    #[test]
    fn transparent_texels_do_not_bleed_colour() {
        let mut i = flat(4, 4, [255, 0, 0, 255]);
        for x in 2..4 {
            for y in 0..4 {
                i.buf[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4].copy_from_slice(&[0, 255, 0, 0]); // transparent green
            }
        }
        let u = lanczos_upscale(&i, 4, false);
        // opaque pixels near the boundary stay red (no green bleed from the transparent half)
        for y in 0..16 {
            for x in 0..8 {
                let p = &u.buf[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4];
                if p[3] > 200 {
                    assert!(p[1] < 30, "green bled into opaque pixel at ({x},{y}): {:?}", p);
                }
            }
        }
    }

    #[test]
    fn mips_halve_down_to_one() {
        let m = mip_chain(&flat(16, 4, [1, 2, 3, 255]));
        assert_eq!(m.iter().map(|i| (i.w, i.h)).collect::<Vec<_>>(), vec![(16, 4), (8, 2), (4, 1), (2, 1), (1, 1)]);
        assert_eq!(&m[4].buf[..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn upscaling_a_step_stays_monotonic_and_sharp() {
        let mut i = flat(8, 1, [0, 0, 0, 255]);
        for x in 4..8 {
            i.buf[x * 4..x * 4 + 3].copy_from_slice(&[255, 255, 255]);
        }
        let u = lanczos_upscale(&i, 4, false);
        assert!(u.buf[0] < 5 && u.buf[(31) * 4] > 250);
        assert!(u.buf[(14) * 4] < 80 && u.buf[(17) * 4] > 175); // transition confined to a few output pixels
    }
}
