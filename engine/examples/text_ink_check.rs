//! Whether the patch overlay turns on proportional text, and the ink extents it measured (LC12, a few letters).
//!     cargo run --release --example text_ink_check -- ../content
use lp_engine::rom::Rom;
use std::path::Path;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let rom = Rom::open(Path::new(&a[1])).expect("content");
    println!("content dir {:?}", rom.content_dir());
    match lp_engine::runtime::natives::proportional_text_ink(&rom) {
        None => println!("proportional text: off"),
        Some(ink) => {
            for c in "AiWm. ".chars() {
                println!("{c:?}: {:?}", ink[0][c as usize - 0x20]);
            }
        }
    }
}
