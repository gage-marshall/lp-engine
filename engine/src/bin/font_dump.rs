//! Canonical dump of an NFTR font; diffed against tools/font_dump.py.   usage: font_dump FONT.NFTR
use lp_engine::font::Font;
use sha1::{Digest, Sha1};

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let f = Font::parse(&std::fs::read(&a[1]).unwrap()).unwrap();
    let ng = f.glyph_count();
    println!("F cell={}x{} bpp={} rot={} height={} linefeed={} alter={} glyphs={} chars={}", f.cell_w, f.cell_h, f.bpp, f.rot, f.height, f.linefeed, f.alter, ng, f.char_count());
    for i in 0..ng {
        let (w, h, rows) = f.glyph_bitmap(i);
        let (l, gw, cw) = f.metrics(i);
        let bytes: Vec<u8> = rows.iter().flatten().copied().collect();
        let mut s = Sha1::new();
        s.update(&bytes);
        println!("G {} {},{},{} {}x{} {}", i, l, gw, cw, w, h, s.finalize().iter().take(5).map(|b| format!("{:02x}", b)).collect::<String>());
    }
    for (c, g) in f.chars_sorted().into_iter().step_by(97) {
        println!("C {} {}", c, g);
    }
}
