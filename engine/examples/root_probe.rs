use lp_engine::heroine::*;
use lp_engine::nns3d::*;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let w = Wardrobe::load(&Path::new("../content/assets/arc"), &a[1]);
    let body = w.part_sets(0, 200).into_iter().find(|(k, _)| *k == "body").unwrap().1;
    let md = w.parts.get(&body).unwrap();
    let names: Vec<String> = md.nodes.iter().map(|n| n.name.clone()).collect();
    for stem in &a[2..] {
        let ent = match w.motions.get(stem) { Some(e) => e, None => { println!("{stem}: missing"); continue } };
        let tr = ent.bca.iter().find(|x| x.name.ends_with("_body"));
        let tr = match tr { Some(t) => t, None => { println!("{stem}: no body track"); continue } };
        let frames = tr.frames;
        let src = names.clone();
        let mut line = format!("{stem} frames {frames}: hips(x,y,z) at ");
        for f in [0usize, frames / 4, frames / 2, frames.saturating_sub(1)] {
            if let Ok(n) = sample_anim_named(tr, &src, &md.nodes, f) { line += &format!("f{f}=({:.1},{:.1},{:.1}) ", n[0].trans[0], n[0].trans[1], n[0].trans[2]); }
        }
        println!("{line}");
    }
}
