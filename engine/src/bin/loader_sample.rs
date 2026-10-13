//! Proof of the text and NNS 2D loaders, straight from the layer-0 content tree (no exported data is read).
//! usage: loader_sample CONTENT_DIR OUT_DIR
//!   - prints lines of the system text resource,
//!   - renders a screen composite (NCGR+NCLR+NSCR) and an animated sprite strip (NCER cells ordered by an NANR sequence).
use lp_engine::nns2d::*;
use lp_engine::text::Txrc;
use std::fs;
use std::path::Path;

fn write_png(path: &Path, im: &Image) {
    let f = fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(f), im.w as u32, im.h as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&im.buf).unwrap();
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (content, out) = (Path::new(&a[1]), Path::new(&a[2]));
    fs::create_dir_all(out).unwrap();
    let assets = content.join("assets");

    // ---- text
    let t = Txrc::parse(&fs::read(assets.join("text/gd_textres_jpn.txrc")).unwrap()).unwrap();
    println!("gd_textres_jpn.txrc: {} groups, {} lines", t.groups.len(), t.line_count());
    for (g, s, l) in [(0, 0, 0), (0, 0, 1), (0, 0, 2), (1, 0, 0), (1, 1, 0), (1, 2, 3)] {
        println!("  [{g}][{s}][{l}] {}", t.line_str(g, s, l).unwrap_or_default());
    }

    // ---- screen composite: gift card background
    let card = assets.join("gift/card_bg_arc.bin");
    let ch = parse_ncgr(&fs::read(card.join("0120.ncgr")).unwrap()).unwrap();
    let pal = parse_nclr(&fs::read(card.join("0121.nclr")).unwrap()).unwrap();
    let sc = parse_nscr(&fs::read(card.join("0122.nscr")).unwrap()).unwrap();
    let screen = nscr_image(&sc, &ch, Some(&pal));
    write_png(&out.join("screen.png"), &screen);
    println!("screen.png: {}x{} from {} tiles, {}-bpp, {} colors", screen.w, screen.h, ch.raw.len() / if ch.bpp == 4 { 32 } else { 64 }, ch.bpp, pal.colors.len());

    // ---- animated sprite: mode-select menu cells in the order of the longest NANR sequence
    let sd = assets.join("arc/select_mode.bin");
    let ch = parse_ncgr(&fs::read(sd.join("0001.ncgr")).unwrap()).unwrap();
    let pal = parse_nclr(&fs::read(sd.join("0000.nclr")).unwrap()).unwrap();
    let seqs = parse_nanr(&fs::read(sd.join("0003.nanr")).unwrap()).unwrap();
    let ce = parse_ncer(&fs::read(sd.join("0002.ncer")).unwrap()).unwrap();
    let seq = seqs.iter().rev().max_by_key(|s| s.frames.len()).unwrap();   // first of the longest
    println!("sprite: {} cells, {} sequences; longest has {} frames ({:?})", ce.cells.len(), seqs.len(), seq.frames.len(), seq.frames.iter().take(6).map(|f| (f.index, f.frames)).collect::<Vec<_>>());
    let imgs: Vec<Option<(Image, (i32, i32))>> = seq.frames.iter().step_by((seq.frames.len() / 12).max(1)).take(12).map(|f| render_cell(&ce.cells[f.index as usize], ce.mapping, &ch, Some(&pal)).unwrap()).collect();
    let (cw, chh) = (128usize, 200usize);
    let mut strip = Image::new(cw * imgs.len(), chh);
    for (i, im) in imgs.iter().enumerate() {
        if let Some((im, (ox, oy))) = im {
            for y in 0..im.h {
                for x in 0..im.w {
                    let p = (y * im.w + x) * 4;
                    if im.buf[p + 3] != 0 {
                        strip.put((i * cw) as i64 + (64 + ox) as i64 + x as i64, (100 + oy) as i64 + y as i64, [im.buf[p], im.buf[p + 1], im.buf[p + 2]]);
                    }
                }
            }
        }
    }
    write_png(&out.join("sprite_strip.png"), &strip);
    println!("sprite_strip.png: {} frames", imgs.len());
}
