//! The save port against the game's own pack / unpack code (docs/SAVE_FORMAT.md).
//! Fixtures: tests/data/save/oracle.json (derived numbers only: SHA-1 of the game's unpack of every block of the recorded
//! backups, run under Unicorn by `sandbox/save/export_fixtures.py`). Skipped when the backups (local, not committed) are
//! absent.
use lp_engine::save::{lz10, lz10_compress, pack, unpack, Card, Kind, Save, SaveFacts};
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};

fn oracle() -> Value {
    serde_json::from_slice(&std::fs::read("tests/data/save/oracle.json").unwrap()).unwrap()
}

fn backups() -> Vec<(PathBuf, Value)> {
    let dir = Path::new("../tools/emu_scripts/states/logs");
    let o = oracle();
    o.as_object().unwrap().iter().map(|(name, v)| (dir.join(name), v.clone())).filter(|(p, _)| p.is_file()).collect()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn kind(letter: &str) -> Kind {
    *Kind::ALL.iter().find(|k| k.letter() == letter).unwrap()
}

#[test]
fn blocks_unpack_to_the_games_structs_and_pack_back_byte_identical() {
    let bs = backups();
    eprintln!("{} recorded backups", bs.len());
    for (path, o) in bs {
        let card = Card::open(&path).unwrap();
        for (letter, slot) in [("A", 0), ("B", 0), ("C", 0), ("B1", 1), ("B2", 2)] {
            let e = &o[letter];
            if e.is_null() {
                continue;
            }
            let k = kind(&letter[..1]);
            let cur = card.current(k, slot).unwrap();
            assert_eq!(cur.copy as u64, e["copy"].as_u64().unwrap());
            assert_eq!(cur.generation as u64, e["generation"].as_u64().unwrap());
            let packed = card.read(k, slot).unwrap();
            assert_eq!(hex(&Sha1::digest(&packed)), e["block_sha1"].as_str().unwrap(), "{letter} block");
            let img = unpack(k, &packed).unwrap();
            assert_eq!(hex(&Sha1::digest(&img)), e["image_sha1"].as_str().unwrap(), "{letter}: unpack == the game's unpack");
            assert_eq!(pack(k, &img).unwrap(), packed, "{letter}: pack(unpack(block)) == block");
            // the stored bytes are what the game's compressor produced
            let body = &card.bytes()[cur.offset + 0x20..cur.offset + 0x20 + cur.stored_size];
            assert_eq!(lz10_compress(&packed).as_deref(), Some(body), "{letter}: LZ byte-identical with the game");
            assert_eq!(lz10(body).unwrap(), packed);
        }
    }
}

#[test]
fn facts_names_and_records() {
    for (path, o) in backups() {
        let save = Save::open(&path).unwrap();
        let f = &o["fields"];
        assert_eq!(save.player_name().unwrap(), f["name_v0"].as_str().unwrap());
        assert_eq!(save.name_1b().unwrap(), f["name_1b"].as_str().unwrap());
        for slot in 0..3 {
            assert_eq!(save.record(slot).unwrap()[6] as u64, f["affection"][slot].as_u64().unwrap(), "affection slot {slot}");
        }
        let facts = SaveFacts::read(&save).unwrap();
        assert_eq!(facts.levels, [15, 15, 15]);
        assert_eq!(facts.text_speed, 1);
        let g = save.game_state(1, 2).unwrap();
        assert_eq!(g.player_name, f["name_v0"].as_str().unwrap());
        for slot in 0..3u32 {
            let want = f["status"][slot as usize].as_u64().unwrap() as u32;
            assert_eq!(g.slot_status(slot), want, "slot {slot}: the game's SaveSlot_ProcessStorageStatus");
        }
    }
}

#[test]
fn an_edit_survives_write_and_reload() {
    let Some((path, _)) = backups().into_iter().next() else { return };
    let mut card = Card::open(&path).unwrap();
    let mut save = Save::load(&card).unwrap();
    let before = card.current(Kind::Game, 0).unwrap();
    let rec = lp_engine::save::game_state::RECORDS + 2 * lp_engine::save::game_state::RECORD_SIZE;
    let old = save.image(Kind::Game, 0).unwrap()[rec + 6];
    save.image_mut(Kind::Game, 0).unwrap()[rec + 6] = old.wrapping_add(37);
    let w = save.store(&mut card, Kind::Game, 0).unwrap().unwrap();
    assert_eq!(w.copy, 1 - before.copy, "the other copy");
    assert_eq!(w.generation, before.generation + 1);
    let again = Save::load(&Card::from_bytes(card.bytes().to_vec())).unwrap();
    assert_eq!(again.record(2).unwrap()[6], old.wrapping_add(37));
    assert_eq!(again.image(Kind::Game, 0), save.image(Kind::Game, 0), "the whole GameState round-trips");
    assert_eq!(again.image(Kind::Settings, 0), save.image(Kind::Settings, 0), "other blocks untouched");
}
