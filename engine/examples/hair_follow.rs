//! does the hair part move with the face part?  hair_follow PREFIX HAIR MOTION  -> bbox centre of the face and hair parts over the motion's frames
use lp_engine::heroine::*;
use std::path::Path;
fn centre(tris: &[lp_engine::nns3d::Tri]) -> [f64; 3] {
    let (mut lo, mut hi) = ([1e9f64; 3], [-1e9f64; 3]);
    for t in tris { for v in &t.verts { for k in 0..3 { lo[k] = lo[k].min(v.pos[k]); hi[k] = hi[k].max(v.pos[k]); } } }
    [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0]
}
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = Wardrobe::load(&Path::new("../content/assets/arc"), &a[1]);
    let hair: u32 = a[2].parse().unwrap();
    let stem = a[3].clone();
    let n = w.motion_frames(&stem);
    let all = w.part_sets(0, hair);
    for f in (0..n).step_by((n / 8).max(1)) {
        let get = |kind: &str| { let p: Vec<(&str, String)> = all.iter().filter(|(k, _)| *k == kind).cloned().collect(); w.build(&p, &Some(stem.clone()), f).map(|x| centre(&x.0)).unwrap_or([0.0; 3]) };
        let (fc, hc) = (get("face"), get("hair"));
        println!("f{f:3} face ({:6.1},{:6.1},{:6.1})  hair ({:6.1},{:6.1},{:6.1})  d=({:5.1},{:5.1},{:5.1})", fc[0], fc[1], fc[2], hc[0], hc[1], hc[2], hc[0] - fc[0], hc[1] - fc[1], hc[2] - fc[2]);
    }
}
