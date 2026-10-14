//! Power-on runs the game's own start-up code (crt0, 0x2000800) on the ARM9 program as the card holds it
//! (content/fs/arm9.bin): it unpacks the program in place, copies the ITCM and DTCM blocks and clears the bss. What it
//! leaves in main RAM must be the program the extractor unpacked (content/engine/static.bin), apart from the bss it
//! cleared; ITCM and DTCM must hold the autoload blocks. Needs ../content (skipped without it).
use lp_engine::rom::Rom;
use lp_engine::runtime::{Game, NoSubsystems, TICKS_PER_FRAME};
use std::path::Path;

#[test]
fn crt0_unpacks_the_program_the_extractor_unpacked() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    let (base, img) = rom.static_image();
    let img = img.to_vec();
    let mut g = Game::new(rom, NoSubsystems);
    let Some(entry) = g.m.ram.boot_entry else { return }; // content without fs/arm9.bin
    g.run_crt0(entry);
    assert!(g.m.faults.is_empty(), "{:?}", g.m.faults);
    // the module parameters (0xdec00621 marker) give the bss bounds
    let k = img.windows(4).position(|x| x == 0xdec00621u32.to_le_bytes()).unwrap();
    let w = |o: usize| u32::from_le_bytes(img[o..o + 4].try_into().unwrap());
    let (bss_start, bss_end) = (w(k - 0x10), w(k - 0xc));
    let mut differ = Vec::new();
    for (i, &b) in img.iter().enumerate() {
        let a = base + i as u32;
        if (bss_start..bss_end).contains(&a) {
            continue;
        }
        if g.m.ram.u8(a) != b {
            differ.push(a);
        }
    }
    assert!(differ.is_empty(), "{} bytes differ from the unpacked program, first at {:#x}", differ.len(), differ[0]);
    assert!((bss_start..bss_end).all(|a| g.m.ram.u8(a) == 0), "bss not cleared");
    eprintln!("crt0 ran {} instructions, {:.1} frames of game time", g.m.steps, g.m.tick as f64 / TICKS_PER_FRAME as f64);
}
