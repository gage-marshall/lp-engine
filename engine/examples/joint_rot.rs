//! print the sampled joint rotations (fixed point, as the game stores them) of a motion at a frame: joint_rot PREFIX MOTION FRAME
use lp_engine::heroine::*;
use lp_engine::nns3d::*;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = Wardrobe::load(&Path::new("../content/assets/arc"), &a[1]);
    let frame: usize = a[3].parse().unwrap();
    let body = w.part_sets(0, 200).into_iter().find(|(k, _)| *k == "body").unwrap().1;
    let md = w.parts.get(&body).unwrap();
    let names: Vec<String> = md.nodes.iter().map(|n| n.name.clone()).collect();
    let ent = &w.motions[&a[2]];
    let tr = ent.bca.iter().find(|x| x.name.ends_with("_body")).unwrap();
    let nodes = sample_anim_named(tr, &names, &md.nodes, frame).unwrap();
    for (i, n) in nodes.iter().enumerate() {
        if ["Hips", "Spine", "Spine1", "Spine2", "Neck", "Head"].contains(&n.name.as_str()) {
            let r: Vec<i64> = n.rot.iter().flat_map(|row| row.iter().map(|v| (v * 4096.0).round() as i64)).collect();
            println!("{i} {} {:?} t=({:.1},{:.1},{:.1})", n.name, r, n.trans[0], n.trans[1], n.trans[2]);
        }
    }
}
