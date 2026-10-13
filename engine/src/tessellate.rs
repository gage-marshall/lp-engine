//! Geometry smoothing for the enhanced renderer: crease-aware smooth normals and curved-PN-triangle tessellation of a posed mesh.
//!
//! Curved PN triangles (Vlachos et al., "Curved PN Triangles") build a cubic Bezier patch per triangle from its corner positions and
//! normals.  Corner normals used for the *patch* are welded by position (one normal per position, so neighbouring patches agree on shared
//! edges and no cracks open); the *shading* normals keep creases (a corner only averages with corners within `crease_deg`).

use crate::nns3d::{Tri, TriVert};
use std::collections::HashMap;

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn norm(a: V3) -> V3 {
    let l = dot(a, a).sqrt();
    if l < 1e-12 { [0.0, 0.0, 1.0] } else { mul(a, 1.0 / l) }
}

fn key(p: V3) -> (i64, i64, i64) {
    ((p[0] * 2000.0).round() as i64, (p[1] * 2000.0).round() as i64, (p[2] * 2000.0).round() as i64)
}

/// Replace each triangle by `n * n` sub-triangles on its PN patch.  `strength` in 0..=1 blends between the flat triangle (0) and
/// the full patch (1).  `crease_deg`: shading normals only average corners whose normals differ by less than this angle.
pub fn smooth(tris: &[Tri], n: u32, strength: f64, crease_deg: f64) -> Vec<Tri> {
    let n = n.max(1) as usize;
    // per-position normal groups
    let mut groups: HashMap<(i64, i64, i64), Vec<V3>> = HashMap::new();
    for t in tris {
        for v in &t.verts {
            groups.entry(key(v.pos)).or_default().push(norm(v.nrm));
        }
    }
    let cos_c = crease_deg.to_radians().cos();
    let welded: HashMap<(i64, i64, i64), V3> = groups.iter().map(|(k, ns)| (*k, norm(ns.iter().fold([0.0; 3], |a, &n| add(a, n))))).collect();
    let shade_normal = |p: V3, own: V3| -> V3 {
        let own = norm(own);
        let acc = groups[&key(p)].iter().filter(|&&o| dot(o, own) >= cos_c).fold([0.0; 3], |a, &o| add(a, o));
        norm(acc)
    };
    let mut out = Vec::with_capacity(tris.len() * n * n);
    for t in tris {
        let p = [t.verts[0].pos, t.verts[1].pos, t.verts[2].pos];
        let nw = [welded[&key(p[0])], welded[&key(p[1])], welded[&key(p[2])]];
        let ns = [shade_normal(p[0], t.verts[0].nrm), shade_normal(p[1], t.verts[1].nrm), shade_normal(p[2], t.verts[2].nrm)];
        if n == 1 {
            let mut nt = t.clone();
            for i in 0..3 {
                nt.verts[i].nrm = ns[i];
            }
            out.push(nt);
            continue;
        }
        // control points
        let edge = |i: usize, j: usize| -> V3 {
            let w = dot(sub(p[j], p[i]), nw[i]);
            mul(sub(add(mul(p[i], 2.0), p[j]), mul(nw[i], w)), 1.0 / 3.0)
        };
        let (b210, b120, b021, b012, b102, b201) = (edge(0, 1), edge(1, 0), edge(1, 2), edge(2, 1), edge(2, 0), edge(0, 2));
        let e = mul(add(add(add(b210, b120), add(b021, b012)), add(b102, b201)), 1.0 / 6.0);
        let v = mul(add(add(p[0], p[1]), p[2]), 1.0 / 3.0);
        let b111 = add(e, mul(sub(e, v), 0.5));
        let eval = |u: f64, vv: f64, w: f64| -> V3 {
            // u, vv, w barycentric weights of corners 0, 1, 2
            let mut r = [0.0; 3];
            let mut acc = |k: f64, b: V3| {
                for c in 0..3 {
                    r[c] += k * b[c];
                }
            };
            acc(u * u * u, p[0]);
            acc(vv * vv * vv, p[1]);
            acc(w * w * w, p[2]);
            acc(3.0 * u * u * vv, b210);
            acc(3.0 * u * vv * vv, b120);
            acc(3.0 * vv * vv * w, b021);
            acc(3.0 * vv * w * w, b012);
            acc(3.0 * w * w * u, b102);
            acc(3.0 * w * u * u, b201);
            acc(6.0 * u * vv * w, b111);
            r
        };
        let vert = |i: usize, j: usize| -> TriVert {
            // grid point (i, j) with i + j <= n: weights for corners 1 and 2
            let (vv, w) = (i as f64 / n as f64, j as f64 / n as f64);
            let u = 1.0 - vv - w;
            let flat = add(add(mul(p[0], u), mul(p[1], vv)), mul(p[2], w));
            let pn = eval(u, vv, w);
            let pos = add(mul(flat, 1.0 - strength), mul(pn, strength));
            let lerp3 = |a: V3, b: V3, c: V3| add(add(mul(a, u), mul(b, vv)), mul(c, w));
            let (v0, v1, v2) = (&t.verts[0], &t.verts[1], &t.verts[2]);
            TriVert {
                pos,
                nrm: norm(lerp3(ns[0], ns[1], ns[2])),
                uv: [v0.uv[0] * u + v1.uv[0] * vv + v2.uv[0] * w, v0.uv[1] * u + v1.uv[1] * vv + v2.uv[1] * w],
                col: lerp3(v0.col, v1.col, v2.col),
                joints: None,
            }
        };
        for i in 0..n {
            for j in 0..n - i {
                out.push(Tri { material: t.material, verts: [vert(i, j), vert(i + 1, j), vert(i, j + 1)] });
                if i + j + 1 < n {
                    out.push(Tri { material: t.material, verts: [vert(i + 1, j), vert(i + 1, j + 1), vert(i, j + 1)] });
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(p: V3, n: V3) -> TriVert {
        TriVert { pos: p, nrm: n, uv: [p[0], p[1]], col: [1.0, 1.0, 1.0], joints: None }
    }

    /// octahedron: every corner normal is the corner direction, so the PN patch should bulge towards the unit sphere
    fn octahedron() -> Vec<Tri> {
        let c = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]];
        let faces = [(0, 2, 4), (2, 1, 4), (1, 3, 4), (3, 0, 4), (2, 0, 5), (1, 2, 5), (3, 1, 5), (0, 3, 5)];
        faces.iter().map(|&(a, b, d)| Tri { material: 0, verts: [v(c[a], c[a]), v(c[b], c[b]), v(c[d], c[d])] }).collect()
    }

    #[test]
    fn counts_and_identity_at_level_one() {
        let o = octahedron();
        assert_eq!(smooth(&o, 1, 1.0, 60.0).len(), 8);
        assert_eq!(smooth(&o, 4, 1.0, 60.0).len(), 8 * 16);
        let same = smooth(&o, 1, 1.0, 60.0);
        assert_eq!(same[0].verts[1].pos, o[0].verts[1].pos);
    }

    #[test]
    fn patches_approach_the_sphere_without_cracks() {
        let o = octahedron();
        let t = smooth(&o, 6, 1.0, 100.0);
        let err = |tris: &[Tri]| tris.iter().flat_map(|t| t.verts.iter()).map(|v| (dot(v.pos, v.pos).sqrt() - 1.0).abs()).fold(0.0f64, f64::max);
        let flat_mid_err = (1.0f64 / 3f64.sqrt() - 1.0).abs(); // centroid of a flat octahedron face
        assert!(err(&t) < 0.5 * 1.0, "unexpectedly far from the sphere: {}", err(&t));
        let centre = t.iter().flat_map(|t| t.verts.iter()).map(|v| (dot(v.pos, v.pos).sqrt() - 1.0).abs()).fold(1.0f64, f64::min);
        assert!(centre < flat_mid_err, "patch centre {centre} not closer to the sphere than the flat face {flat_mid_err}");
        // watertight: every vertex position on an edge shared by two input faces is produced identically by both patches
        let mut seen: HashMap<(i64, i64, i64), V3> = HashMap::new();
        for t in &t {
            for v in &t.verts {
                let k = key(v.pos);
                if let Some(p) = seen.insert(k, v.pos) {
                    assert!(dot(sub(p, v.pos), sub(p, v.pos)) < 1e-12);
                }
            }
        }
        // no vertex strayed far from every other (crack check via matching counts): positions are shared across patches
        assert!(seen.len() < t.len() * 3 / 2, "vertices are not shared between patches");
    }

    #[test]
    fn flat_triangles_stay_flat_and_strength_zero_is_identity() {
        let n = [0.0, 0.0, 1.0];
        let flat = vec![Tri { material: 2, verts: [v([0.0, 0.0, 0.0], n), v([1.0, 0.0, 0.0], n), v([0.0, 1.0, 0.0], n)] }];
        let s = smooth(&flat, 5, 1.0, 60.0);
        assert!(s.iter().flat_map(|t| t.verts.iter()).all(|v| v.pos[2].abs() < 1e-12));
        assert!(s.iter().all(|t| t.material == 2));
        let o = octahedron();
        let z = smooth(&o, 3, 0.0, 60.0);
        // strength 0: all vertices stay on the original flat faces (inside the octahedron: |x|+|y|+|z| <= 1)
        assert!(z.iter().flat_map(|t| t.verts.iter()).all(|v| v.pos.iter().map(|c| c.abs()).sum::<f64>() <= 1.0 + 1e-9));
    }

    #[test]
    fn creases_keep_their_own_shading_normals() {
        // two faces meeting at 90 degrees along a shared edge: shading normals are not averaged across the crease at crease_deg = 30
        let a = Tri { material: 0, verts: [v([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]), v([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]), v([0.0, 1.0, 0.0], [0.0, 0.0, 1.0])] };
        let b = Tri { material: 0, verts: [v([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]), v([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]), v([0.0, 0.0, -1.0], [1.0, 0.0, 0.0])] };
        let s = smooth(&[a, b], 1, 1.0, 30.0);
        assert_eq!(s[0].verts[0].nrm, [0.0, 0.0, 1.0]);
        assert_eq!(s[1].verts[0].nrm, [1.0, 0.0, 0.0]);
        let smooth_all = smooth(&[s[0].clone(), s[1].clone()], 1, 1.0, 120.0);
        assert!(smooth_all[0].verts[0].nrm[0] > 0.5); // averaged when the threshold allows it
    }
}
