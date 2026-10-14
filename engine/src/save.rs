//! The game's save file (2 Mbit backup memory): card blocks, packed blocks, and the game structs they carry.
//! Format and proofs: docs/SAVE_FORMAT.md (derived and checked in sandbox/save/ against the game's own code under Unicorn).
//!
//! Card (`Save_WriteBlock` 0x20f0fb8): six block kinds, `slots` blocks per kind, each stored in two copies. A copy is a
//! 0x20-byte header {u32 magic 0x10190573, u32 stored size, u16 generation, u16 version 3, SHA-1 of the stored bytes}
//! followed by the stored bytes = `MI_CompressLZ` of the packed block, or the packed block when that does not shrink it.
//! A write goes to the other copy with generation + 1 (0 -> 1); the newest valid copy is the block.
//!
//! Packed block <-> game struct (`Ov00Init_PackSave<K>` / `Ov00Init_UnpackSave<K>`): a fixed bit permutation per kind
//! (data/save_maps.json, derived with the game's own packers and unpackers as the oracle), plus one rule the permutation
//! cannot express: GameState +0x3958..+0x39ff are booleans (`packed != 0`).

use crate::game::GameState;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::sync::OnceLock;

const MAGIC: u32 = 0x1019_0573;

/// block kind: the game struct a block carries
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A 'LDSD': settings record (0x1f8 at *(0x21544e8 + 4)): sound levels, text speed, ...
    Settings,
    /// B 'LDGD': GameState (0x3c58 at 0x2142c8c): names, calendar, flag storage, heroine records at +0x15c4
    Game,
    /// C 'LDOD': header block (0xf4 at *(0x21544e8 + 8))
    Header,
    /// D 'LDPD': event state (0x1002, `Schedule_GetEventStatePtr`)
    Events,
    /// E 'LDFD': schedule aux state (0xc4), 100 card slots
    Aux,
    /// F 'LDTG': slot record (0x14c8, SaveData slot table entry), 3 card slots
    Slot,
}

impl Kind {
    pub const ALL: [Kind; 6] = [Kind::Settings, Kind::Game, Kind::Header, Kind::Events, Kind::Aux, Kind::Slot];

    pub fn letter(self) -> &'static str {
        ["A", "B", "C", "D", "E", "F"][self as usize]
    }

    pub fn slots(self) -> usize {
        [1, 3, 1, 1, 100, 3][self as usize]
    }

    /// card offset of copy `copy` of block `slot`
    pub fn card_offset(self, slot: usize, copy: usize) -> usize {
        match self {
            Kind::Settings => (slot + copy) * 0xa20,
            Kind::Game => 0x1440 + (slot + 3 * copy) * 0x4020,
            Kind::Header => 0x19500 + (slot + copy) * 0x120,
            Kind::Events => 0x19740 + (slot + copy) * 0x2020,
            Kind::Aux => 0x1d780 + (slot + 100 * copy) * 0x120,
            Kind::Slot => 0x2b880 + (slot + 3 * copy) * 0x1520,
        }
    }

    pub fn struct_size(self) -> usize {
        maps().0[self as usize].struct_size
    }

    pub fn packed_size(self) -> usize {
        maps().0[self as usize].packed_size
    }
}

// ------------------------------------------------------------------ LZ10 (NitroSDK MI_UncompressLZ8 / MI_CompressLZ)

/// NitroSDK LZ77 (`MI_UncompressLZ8`, type 0x10)
pub fn lz10(d: &[u8]) -> Option<Vec<u8>> {
    if d.len() < 4 || d[0] != 0x10 {
        return None;
    }
    let n = d[1] as usize | (d[2] as usize) << 8 | (d[3] as usize) << 16;
    let mut out = Vec::with_capacity(n);
    let mut p = 4;
    while out.len() < n {
        let flags = *d.get(p)?;
        p += 1;
        for b in 0..8 {
            if out.len() >= n {
                break;
            }
            if flags & (0x80 >> b) != 0 {
                let x = (*d.get(p)? as usize) << 8 | *d.get(p + 1)? as usize;
                p += 2;
                let back = (x & 0xfff) + 1;
                if back > out.len() {
                    return None;
                }
                for _ in 0..(x >> 12) + 3 {
                    out.push(out[out.len() - back]);
                }
            } else {
                out.push(*d.get(p)?);
                p += 1;
            }
        }
    }
    Some(out)
}

/// `MI_CompressLZImpl` 0x201f20c with `SearchLZ` 0x201f3e0: farthest match first, distance >= 2, only a strictly longer
/// match replaces the best; None when the result would not be smaller than the input (the game then stores the raw
/// block). The game pads its buffer to 4 bytes but stores the unpadded length. Byte-identical with every recorded block.
pub fn lz10_compress(src: &[u8]) -> Option<Vec<u8>> {
    let n = src.len();
    let mut out = vec![0x10, n as u8, (n >> 8) as u8, (n >> 16) as u8];
    let mut i = 0;
    while i < n {
        let fpos = out.len();
        out.push(0);
        let mut flags = 0u8;
        for b in 0..8 {
            if i >= n {
                break;
            }
            let (mut best_len, mut best_off) = (0usize, 0usize);
            let maxlen = 18.min(n - i);
            for j in i.saturating_sub(0x1000)..i.saturating_sub(1) {
                if src[j] != src[i] {
                    continue;
                }
                let mut l = 1;
                while l < maxlen && src[j + l] == src[i + l] {
                    l += 1;
                }
                if l > best_len {
                    best_len = l;
                    best_off = i - j;
                    if l == maxlen {
                        break;
                    }
                }
            }
            if best_len >= 3 {
                flags |= 0x80 >> b;
                let x = ((best_len - 3) << 12) | (best_off - 1);
                out.push((x >> 8) as u8);
                out.push(x as u8);
                i += best_len;
            } else {
                out.push(src[i]);
                i += 1;
            }
        }
        out[fpos] = flags;
        if out.len() >= n {
            return None;
        }
    }
    Some(out)
}

// ------------------------------------------------------------------ card

/// a valid copy of a block on the card
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Copy {
    pub copy: usize,
    pub offset: usize,
    pub generation: u16,
    pub stored_size: usize,
}

/// the backup memory image
#[derive(Clone)]
pub struct Card {
    image: Vec<u8>,
}

impl Card {
    /// an erased 2 Mbit card
    pub fn blank() -> Card {
        Card { image: vec![0xff; 0x40000] }
    }

    pub fn from_bytes(image: Vec<u8>) -> Card {
        Card { image }
    }

    pub fn open(path: &std::path::Path) -> Result<Card, String> {
        Ok(Card::from_bytes(std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?))
    }

    pub fn bytes(&self) -> &[u8] {
        &self.image
    }

    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        std::fs::write(path, &self.image).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn u32_at(&self, o: usize) -> Option<u32> {
        Some(u32::from_le_bytes(self.image.get(o..o + 4)?.try_into().ok()?))
    }

    /// the copies of a block whose header and SHA-1 are valid
    pub fn copies(&self, kind: Kind, slot: usize) -> Vec<Copy> {
        let mut out = Vec::new();
        for copy in 0..2 {
            let o = kind.card_offset(slot, copy);
            let (Some(magic), Some(size)) = (self.u32_at(o), self.u32_at(o + 4)) else { continue };
            let size = size as usize;
            if magic != MAGIC || size == 0 || size > 0x40000 {
                continue;
            }
            let Some(body) = self.image.get(o + 0x20..o + 0x20 + size) else { continue };
            if Sha1::digest(body).as_slice() != &self.image[o + 12..o + 32] {
                continue;
            }
            let generation = u16::from_le_bytes([self.image[o + 8], self.image[o + 9]]);
            out.push(Copy { copy, offset: o, generation, stored_size: size });
        }
        out
    }

    /// the newest valid copy (generations compare modulo 2^16)
    pub fn current(&self, kind: Kind, slot: usize) -> Option<Copy> {
        let cs = self.copies(kind, slot);
        let mut best = *cs.first()?;
        for c in &cs[1..] {
            if c.generation != best.generation && c.generation.wrapping_sub(best.generation) < 0x8000 {
                best = *c;
            }
        }
        Some(best)
    }

    /// the packed block (decompressed), if the block has a valid copy
    pub fn read(&self, kind: Kind, slot: usize) -> Option<Vec<u8>> {
        let c = self.current(kind, slot)?;
        let body = &self.image[c.offset + 0x20..c.offset + 0x20 + c.stored_size];
        let packed = if body.len() < kind.packed_size() { lz10(body)? } else { body.to_vec() };
        (packed.len() == kind.packed_size()).then_some(packed)
    }

    /// store a packed block as `Save_WriteBlock` does: the game keeps {copy, generation} per block from the load (bytes at
    /// 0x2157fb0, words at 0x2158020); a write goes to copy (copy + 1) % 2 with generation + 1 (0 -> 1). A block with no valid
    /// copy starts at {0, 0}, so its first write lands in copy 1 (as on every recorded card). Returns the copy written.
    pub fn write(&mut self, kind: Kind, slot: usize, packed: &[u8]) -> Copy {
        let (copy, gen) = self.current(kind, slot).map_or((0, 0), |c| (c.copy, c.generation));
        let copy = (copy + 1) % 2;
        let generation = match gen.wrapping_add(1) {
            0 => 1,
            g => g,
        };
        let stored = lz10_compress(packed).unwrap_or_else(|| packed.to_vec());
        let o = kind.card_offset(slot, copy);
        let mut blk = Vec::with_capacity(0x20 + stored.len());
        blk.extend_from_slice(&MAGIC.to_le_bytes());
        blk.extend_from_slice(&(stored.len() as u32).to_le_bytes());
        blk.extend_from_slice(&generation.to_le_bytes());
        blk.extend_from_slice(&3u16.to_le_bytes());
        blk.extend_from_slice(Sha1::digest(&stored).as_slice());
        blk.extend_from_slice(&stored);
        if self.image.len() < o + blk.len() {
            self.image.resize(o + blk.len(), 0xff);
        }
        self.image[o..o + blk.len()].copy_from_slice(&blk);
        Copy { copy, offset: o, generation, stored_size: stored.len() }
    }
}

// ------------------------------------------------------------------ packed block <-> game struct

struct KindMap {
    struct_size: usize,
    packed_size: usize,
    /// (struct bit, packed bit, length)
    unpack: Vec<[usize; 3]>,
    /// (struct byte, packed byte, length): struct byte = (packed byte != 0)
    unpack_bools: Vec<[usize; 3]>,
    unpack_const: Vec<u8>,
    /// (packed bit, struct bit, length)
    pack: Vec<[usize; 3]>,
    pack_const: Vec<u8>,
}

struct Maps([KindMap; 6]);

fn maps() -> &'static Maps {
    static M: OnceLock<Maps> = OnceLock::new();
    M.get_or_init(|| {
        let v: Value = serde_json::from_str(include_str!("../data/save_maps.json")).expect("data/save_maps.json");
        let runs = |x: &Value| -> Vec<[usize; 3]> {
            x.as_array().unwrap().iter().map(|r| {
                let r = r.as_array().unwrap();
                [0, 1, 2].map(|i| r[i].as_u64().unwrap() as usize)
            }).collect()
        };
        let hex = |x: &Value| -> Vec<u8> {
            let s = x.as_str().unwrap();
            (0..s.len() / 2).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect()
        };
        Maps(Kind::ALL.map(|k| {
            let m = &v[k.letter()];
            KindMap {
                struct_size: m["struct_size"].as_u64().unwrap() as usize,
                packed_size: m["packed_size"].as_u64().unwrap() as usize,
                unpack: runs(&m["unpack"]),
                unpack_bools: runs(&m["unpack_bools"]),
                unpack_const: hex(&m["unpack_const"]),
                pack: runs(&m["pack"]),
                pack_const: hex(&m["pack_const"]),
            }
        }))
    })
}

fn copy_bits(dst: &mut [u8], dst_bit: usize, src: &[u8], src_bit: usize, n: usize) {
    for k in 0..n {
        let (s, d) = (src_bit + k, dst_bit + k);
        let bit = src[s >> 3] >> (s & 7) & 1;
        dst[d >> 3] = dst[d >> 3] & !(1 << (d & 7)) | bit << (d & 7);
    }
}

/// packed block -> game struct image (`Ov00Init_UnpackSave<K>`; bit-exact with the game's unpacker)
pub fn unpack(kind: Kind, packed: &[u8]) -> Result<Vec<u8>, String> {
    let m = &maps().0[kind as usize];
    if packed.len() != m.packed_size {
        return Err(format!("save: {} block of {} bytes, expected {}", kind.letter(), packed.len(), m.packed_size));
    }
    let mut out = m.unpack_const.clone();
    for &[t, i, n] in &m.unpack {
        copy_bits(&mut out, t, packed, i, n);
    }
    for &[so, po, n] in &m.unpack_bools {
        for j in 0..n {
            out[so + j] = (packed[po + j] != 0) as u8;
        }
    }
    Ok(out)
}

/// game struct image -> packed block (`Ov00Init_PackSave<K>`; bit-exact with the game's packer on every image the game
/// can hold: booleans 0 / 1, bitfields within their width)
pub fn pack(kind: Kind, image: &[u8]) -> Result<Vec<u8>, String> {
    let m = &maps().0[kind as usize];
    if image.len() != m.struct_size {
        return Err(format!("save: {} struct of {} bytes, expected {}", kind.letter(), image.len(), m.struct_size));
    }
    let mut out = m.pack_const.clone();
    for &[t, i, n] in &m.pack {
        copy_bits(&mut out, t, image, i, n);
    }
    Ok(out)
}

// ------------------------------------------------------------------ the loaded save

/// GameState layout (offsets into the Kind::Game image), verified with the game's own accessors under Unicorn
pub mod game_state {
    /// heroine records (`GameState_GetSlotPtr` 0x2063618; docs/SAVE_RECORD.md): 0x688 bytes per slot
    pub const RECORDS: usize = 0x15c4;
    pub const RECORD_SIZE: usize = 0x688;
    /// player name ({v0}, var 0x1c: `GameState_GetPtr_Ofs0x18`) and var 0x1b (`GameState_GetPtr_Ofs0x08`): NUL-terminated
    /// UTF-8 in 16 bytes
    pub const NAME_V0: usize = 0x18;
    pub const NAME_1B: usize = 0x8;
    pub const NAME_LEN: usize = 0x10;
    /// scoped block of scope s = 1..=3 (`GameState_GetPointer(s - 1)` = 0x2143eb0 + 0x688 (s - 1)): flag bit idx at +idx/8,
    /// scoped bytes at +800 + idx (`GameFlags_GetByteAtOffset800`), words at +0x360 + 4 idx, then heroine record s - 1 at +0x3a0
    pub const FLAGS: usize = 0x1224;
    pub const FLAGS_STRIDE: usize = 0x688;
    pub const FLAG_BITS: usize = 800 * 8;
    pub const SCOPED_BYTES: usize = 800;
    /// scoped bytes before the word table at +0x360
    pub const SCOPED_BYTE_COUNT: usize = 0x360 - 800;
}

/// every block of a card as the game struct it carries (None: no valid copy on the card)
#[derive(Clone)]
pub struct Save {
    images: Vec<Vec<Option<Vec<u8>>>>,
}

impl Save {
    /// load every block (newest valid copy) the way the game does
    pub fn load(card: &Card) -> Result<Save, String> {
        let mut images = Vec::new();
        for k in Kind::ALL {
            let mut v = Vec::new();
            for slot in 0..k.slots() {
                v.push(match card.read(k, slot) {
                    Some(p) => Some(unpack(k, &p)?),
                    None => None,
                });
            }
            images.push(v);
        }
        Ok(Save { images })
    }

    pub fn from_bytes(sav: &[u8]) -> Result<Save, String> {
        Save::load(&Card::from_bytes(sav.to_vec()))
    }

    pub fn open(path: &std::path::Path) -> Result<Save, String> {
        Save::load(&Card::open(path)?)
    }

    pub fn image(&self, kind: Kind, slot: usize) -> Option<&[u8]> {
        self.images[kind as usize].get(slot)?.as_deref()
    }

    pub fn image_mut(&mut self, kind: Kind, slot: usize) -> Option<&mut Vec<u8>> {
        self.images[kind as usize].get_mut(slot)?.as_mut()
    }

    pub fn set_image(&mut self, kind: Kind, slot: usize, image: Vec<u8>) -> Result<(), String> {
        if image.len() != kind.struct_size() {
            return Err(format!("save: {} struct of {} bytes, expected {}", kind.letter(), image.len(), kind.struct_size()));
        }
        self.images[kind as usize][slot] = Some(image);
        Ok(())
    }

    /// write one block to the card (pack, compress, hash, other copy, generation + 1)
    pub fn store(&self, card: &mut Card, kind: Kind, slot: usize) -> Result<Option<Copy>, String> {
        match self.image(kind, slot) {
            Some(img) => Ok(Some(card.write(kind, slot, &pack(kind, img)?))),
            None => Ok(None),
        }
    }

    /// write every block this save holds
    pub fn store_all(&self, card: &mut Card) -> Result<(), String> {
        for k in Kind::ALL {
            for slot in 0..k.slots() {
                self.store(card, k, slot)?;
            }
        }
        Ok(())
    }

    fn game(&self) -> Result<&[u8], String> {
        self.image(Kind::Game, 0).ok_or_else(|| "save: no game state block (B 'LDGD')".to_string())
    }

    /// heroine record of slot 0 Manaka / 1 Rinko / 2 Nene (docs/SAVE_RECORD.md)
    pub fn record(&self, slot: usize) -> Result<&[u8], String> {
        let o = game_state::RECORDS + game_state::RECORD_SIZE * slot;
        Ok(&self.game()?[o..o + game_state::RECORD_SIZE])
    }

    fn name_at(&self, o: usize) -> Result<String, String> {
        let b = &self.game()?[o..o + game_state::NAME_LEN];
        Ok(String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]).into_owned())
    }

    /// the player's name (text var {v0})
    pub fn player_name(&self) -> Result<String, String> {
        self.name_at(game_state::NAME_V0)
    }

    /// text var 0x1b
    pub fn name_1b(&self) -> Result<String, String> {
        self.name_at(game_state::NAME_1B)
    }

    fn flag_block(&self, scope: usize) -> Result<&[u8], String> {
        let o = game_state::FLAGS + game_state::FLAGS_STRIDE * (scope - 1);
        Ok(&self.game()?[o..o + game_state::SCOPED_BYTES + game_state::SCOPED_BYTE_COUNT])
    }

    /// scoped flag bit (scope 1..=3 = heroine slot + 1, idx = normalised bit index, `game::flag_key`)
    pub fn flag(&self, scope: usize, idx: usize) -> Result<bool, String> {
        Ok(idx < game_state::FLAG_BITS && self.flag_block(scope)?[idx >> 3] >> (idx & 7) & 1 != 0)
    }

    /// scoped byte (counter) of scope 1..=3
    pub fn scoped_byte(&self, scope: usize, idx: usize) -> Result<u8, String> {
        Ok(*self.flag_block(scope)?.get(game_state::SCOPED_BYTES + idx).ok_or("save: scoped byte index out of range")?)
    }

    /// the script-visible game state of this save: player name, the three scoped blocks (flag bits and bytes; global ids
    /// resolve to them as the game does, `game::Flags`), heroine stats
    pub fn game_state(&self, seed: u64, current_slot: u32) -> Result<GameState, String> {
        let mut g = GameState::new(seed);
        g.current_slot = current_slot;
        g.player_name = self.player_name()?;
        let id = |scope: usize, idx: usize| (scope as u32) << 12 | (idx as u32 & 0xfff) | ((idx as u32 & 0x1000) << 2);
        for scope in 1..=3 {
            let blk = self.flag_block(scope)?;
            for idx in 0..game_state::FLAG_BITS {
                if blk[idx >> 3] >> (idx & 7) & 1 != 0 {
                    g.flags.set(id(scope, idx), true);
                }
            }
            for idx in 0..game_state::SCOPED_BYTE_COUNT {
                let v = blk[game_state::SCOPED_BYTES + idx];
                if v != 0 {
                    g.counters.insert((scope as u32, idx as u32), v as u32);
                }
            }
        }
        for slot in 0..3 {
            let r = self.record(slot)?;
            let h = &mut g.heroines[slot];
            h.stat_a = r[0x180] as u32;
            h.stat_b = r[0x181] as u32;
            h.record_flags = u32::from_le_bytes(r[0..4].try_into().unwrap());
            h.phase = r[4] as i8 as i32;
            h.heart = r[5] as u32;
        }
        Ok(g)
    }
}

/// what the scene drivers read from the save
#[derive(Clone, Debug, PartialEq)]
pub struct SaveFacts {
    /// settings record +5..+7: music, sound, voice levels 0..15
    pub levels: [u8; 3],
    /// settings record +0xc: text speed index into the typewriter table 0x21274e0
    pub text_speed: i8,
    /// GameState +0x78 (calendar / lovers state)
    pub calendar_78: u8,
    /// heroine records +0xd (hair key) and +0x107 (random-event state), per slot
    pub record_0d: [u8; 3],
    pub record_107: [i8; 3],
}

impl SaveFacts {
    pub fn read(save: &Save) -> Result<SaveFacts, String> {
        let a = save.image(Kind::Settings, 0).ok_or("save: no settings block (A 'LDSD')")?;
        let b = save.game()?;
        let rec = |slot: usize, f: usize| -> Result<u8, String> { Ok(save.record(slot)?[f]) };
        Ok(SaveFacts {
            levels: [a[5], a[6], a[7]],
            text_speed: a[0xc] as i8,
            calendar_78: b[0x78],
            record_0d: [rec(0, 0xd)?, rec(1, 0xd)?, rec(2, 0xd)?],
            record_107: [rec(0, 0x107)? as i8, rec(1, 0x107)? as i8, rec(2, 0x107)? as i8],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lz10_round_trip_of_a_known_stream() {
        // literal "abc", then a back-reference of length 3 at distance 3 -> "abcabc"
        let d = [0x10, 6, 0, 0, 0b0001_0000, b'a', b'b', b'c', 0x00, 0x02];
        assert_eq!(lz10(&d).unwrap(), b"abcabc");
    }

    #[test]
    fn lz10_compress_round_trips_and_never_uses_distance_1() {
        let src: Vec<u8> = (0..3000u32).map(|i| if i % 7 < 4 { 0 } else { (i * 31 % 251) as u8 }).collect();
        let c = lz10_compress(&src).unwrap();
        assert_eq!(lz10(&c).unwrap(), src);
        assert!(lz10_compress(&[1, 2, 3, 4, 5]).is_none(), "incompressible input is stored raw");
        // a run of zeros: distance-1 matches are not allowed (SearchLZ), so the first match is at distance 2
        let c = lz10_compress(&[0u8; 64]).unwrap();
        assert_eq!(&c[4..9], &[0b0011_1100, 0, 0, 0xf0, 0x01], "two literals, then length 18 at distance 2");
    }

    #[test]
    fn maps_cover_every_kind() {
        for k in Kind::ALL {
            let blank = vec![0u8; k.struct_size()];
            let p = pack(k, &blank).unwrap();
            assert_eq!(p.len(), k.packed_size());
            assert_eq!(&p[4..6], b"LD", "{} tag", k.letter());
            assert_eq!(unpack(k, &p).unwrap().len(), k.struct_size());
        }
    }

    #[test]
    fn writes_alternate_copies_and_bump_the_generation() {
        let mut card = Card::blank();
        let mut img = vec![0u8; Kind::Settings.struct_size()];
        img[5] = 7;
        let p = pack(Kind::Settings, &img).unwrap();
        let w1 = card.write(Kind::Settings, 0, &p);
        assert_eq!((w1.copy, w1.generation), (1, 1), "first write lands in copy 1");
        img[5] = 9;
        let w2 = card.write(Kind::Settings, 0, &pack(Kind::Settings, &img).unwrap());
        assert_eq!((w2.copy, w2.generation), (0, 2));
        let s = Save::load(&card).unwrap();
        assert_eq!(s.image(Kind::Settings, 0).unwrap()[5], 9, "the newest generation wins");
        assert!(s.image(Kind::Game, 0).is_none());
    }
}
