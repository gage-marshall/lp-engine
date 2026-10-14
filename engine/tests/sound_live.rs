//! The ARM7 sound driver (`runtime::snd7`) fed by the game itself: power on, run to the title (the title music and
//! the heroine's title call), then check that what the driver played is the game's data. Every bank it copied from
//! main RAM must have an SDAT bank with the same instruments and every wave those instruments use must decode to
//! the same samples as the SDAT's; the stream the game played on its own channels (its G.722.1 voice decoder
//! filling a ring buffer from the driver's alarms) must be a voice clip of the archive, every sample in order.
//! Needs ../content and the recorded session's battery save (with an empty save the title plays no voice within
//! 800 frames); skipped without them.
use lp_engine::rom::Rom;
use lp_engine::runtime::{Game, NoSubsystems};
use lp_engine::sdat::{Inst, Sdat};
use lp_engine::snd::Source;
use std::path::Path;

#[test]
fn the_driver_plays_the_games_own_sound_data() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    let Ok(save) = std::fs::read("../tools/emu_scripts/states/logs/backup-session-20261013-173034-f48509.sav") else { return };
    let rd = |p: &str| std::fs::read(format!("../content/{p}")).unwrap();
    let mut g = Game::new(rom, NoSubsystems);
    g.m.backup = save;
    g.m.rtc = [26, 10, 14, 3, 12, 0, 0];
    g.m.snd.tap = Some(Vec::new());
    assert!(g.power_on().is_empty());
    for _ in 0..800 {
        g.frame();
        assert!(g.m.faults.is_empty(), "{:?}", g.m.faults);
    }

    // banks and waves
    let sdat = Sdat::parse(rd("assets/sys/sound_data.sdat")).unwrap();
    let src = g.m.snd.src.clone();
    let banks = src.banks_held();
    assert!(!banks.is_empty(), "no sequence was started");
    let mut waves = 0;
    for (addr, bank, arcs) in banks {
        let id = (0..sdat.banks.len()).find(|&i| sdat.bank(i).is_some_and(|b| b.instruments == bank.instruments));
        let id = id.unwrap_or_else(|| panic!("bank at {addr:#x} is not an SDAT bank"));
        for inst in &bank.instruments {
            let regions = match inst {
                Inst::Single(r) => vec![*r],
                Inst::DrumSet { regions, .. } | Inst::KeySplit { regions, .. } => regions.clone(),
                _ => vec![],
            };
            for r in regions.iter().filter(|r| r.kind == 1 || r.kind == 4) {
                let k = r.wavearc as usize;
                let ram = arcs[k].and_then(|p| src.wave(p, r.wave as usize)).expect("a wave the bank uses is not loaded");
                let ext = sdat.bank_wavearc(id, k).and_then(|w| sdat.wave(w, r.wave as usize)).unwrap();
                assert!(*ram == *ext, "bank {addr:#x} (SDAT {id}): wave {} of slot {k} differs", r.wave);
                waves += 1;
            }
        }
    }
    assert!(waves > 0);

    // the stream
    let tap = g.m.snd.tap.take().unwrap();
    let first = tap.iter().position(|&v| v != 0).expect("the stream never sounded");
    assert!(first + 64 <= tap.len());
    let (vtb, spp) = (rd("assets/sys/Siren14Voice.vtb"), rd("assets/sys/Siren14Voice.spp"));
    let t = lp_engine::siren::Tables::from_pack(&rd("engine/g7221.pack")).unwrap();
    let probe = &tap[first..first + 64];
    let clip = (0..vtb.len() / 8).find_map(|c| {
        let e = &vtb[c * 8..c * 8 + 8];
        let (off, size) = (u32::from_le_bytes(e[0..4].try_into().unwrap()) as usize / 16, u32::from_le_bytes(e[4..8].try_into().unwrap()) as usize / 16);
        let (head, _) = lp_engine::siren::decode_clip(&t, spp.get(off..off + size.min(400))?, 16000, 14000);
        let at = head.windows(64).position(|w| w == probe)?;
        Some((c, off, size, first.checked_sub(at)?))
    });
    let (c, off, size, start) = clip.expect("the stream is no voice clip of the archive");
    let (pcm, _) = lp_engine::siren::decode_clip(&t, &spp[off..off + size], 16000, 14000);
    let n = pcm.len().min(tap.len() - start);
    assert!(n > 16000, "clip {c}: only {n} samples played");
    let bad = (0..n).filter(|&i| tap[start + i] != pcm[i]).count();
    assert_eq!(bad, 0, "clip {c}: {bad} of {n} samples differ");
}
