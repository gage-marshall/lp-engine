# Save file format (backup memory)

The game saves to 2 Mbit (0x40000-byte) backup memory. This document covers how the save is laid out, how it maps to the game's structs, and how each claim was proven. Implementations:

- Python reader and writer: `sandbox/save/savefmt.py`.
- Rust engine: `engine/src/save.rs`.

Both use the same bit maps, `engine/data/save_maps.json`, which is a copy of `sandbox/save/bitmaps.json`.

## Layers

```
card image ── block copy {header, stored bytes} ── LZ10 ── packed block (sections of entries) ── bit map ── game struct
```

### 1. Card blocks (`Save_WriteBlock` 0x20f0fb8)

There are six block kinds. Kind *k* has `slots` blocks, and each block is stored in two copies (ping-pong).

| kind | tag | game struct (size) | slots | card offset of (slot *i*, copy *c*) |
|---|---|---|---|---|
| A | `LDSD` | settings record (0x1f8, `*(0x21544e8+4)`): sound levels +5..+7, text speed +0xc | 1 | (i+c)·0xa20 |
| B | `LDGD` | GameState (0x3c58 at 0x2142c8c): names, calendar, flag storage, heroine records | 1 used (3 allocated) | 0x1440 + (i+3c)·0x4020 |
| C | `LDOD` | header block (0xf4, `*(0x21544e8+8)`) | 1 | 0x19500 + (i+c)·0x120 |
| D | `LDPD` | event state (0x1002, `Schedule_GetEventStatePtr`) | 1 | 0x19740 + (i+c)·0x2020 |
| E | `LDFD` | schedule aux state (0xc4, `Schedule_FreeAuxState` getter) | 100 | 0x1d780 + (i+100c)·0x120 |
| F | `LDTG` | slot record (0x14c8, `SaveData_GetSlotTable()[i]`) | 3 | 0x2b880 + (i+3c)·0x1520 |

**Copy header** (0x20 bytes), followed by the stored bytes:

| offset | field |
|---|---|
| +0 | `u32` magic 0x10190573 |
| +4 | `u32` stored size |
| +8 | `u16` generation |
| +0xa | `u16` version 3 |
| +0xc | SHA-1 of the stored bytes (`Sys_CallFnPtr` hash) |

**Stored bytes**: `MI_CompressLZImpl(packed, size, dst, 0)` (LZ10). If compression does not make the block smaller, the packed block is stored raw, so stored size = packed size. LZ10 details that make the output byte-identical with the game:

- `SearchLZ` 0x201f3e0 searches the 0x1000 window farthest-first.
- It requires distance ≥ 2.
- Only a strictly longer match replaces the current best.
- The maximum match length is 18.
- The game pads its buffer to 4 bytes, but stores the unpadded length.

**Ping-pong.** The game keeps {current copy, generation} for every block: bytes at 0x2157fb0 and words at 0x2158020. The table index is 0 for A, 1 + slot for B, 4 for C, 5 for D, 6 + slot for E and 0x6a + slot for F (`0x20f0d64`).

- **Load:** the game takes the newest valid copy (magic, size and SHA-1 all check out).
- **Write:** the block goes to copy (copy + 1) mod 2 with generation + 1, and generation 0 becomes 1.
- **First write:** a block that was never loaded starts at {0, 0}, so its first write lands in copy 1 at generation 1. Every recorded card shows exactly this.

### 2. Packed block (`Ov00Init_PackSave<K>` / `Ov00Init_UnpackSave<K>`, overlay 0)

A packed block is a series of sections whose entries are laid out back to back from offset 0.

| | address (overlay 0 data) |
|---|---|
| entry *e* of section *s* | {u16 size, u16 count} at `*(descriptors + 4s) + 4e` |
| number of entries in section *s* | word at `counts + 4s` |

| kind | descriptors | counts | sections | packed size |
|---|---|---|---|---|
| A | 0x2165e80 | 0x2164f00 | 10 | 780 |
| B | 0x2165ea8 | 0x21657e0 | 73 | 17660 (padded to 4) |
| C | 0x2165fcc | 0x21659d0 | 16 | 486 |
| D | 0x2166a2c | 0x2165cc0 | 2 | 4118 |
| E | 0x2166a34 | 0x2165d50 | 2 | 210 |
| F | 0x2166a3c | 0x2165e10 | 2 | 3842 |

- **Entry 0 of every section** is that section's byte size.
- **Section 0** holds the tag, version and struct size (entries 1–3). In A and B it also holds a few struct fields; B's include GameState +0x77..+0x7b, +0x98 and +0x180.
- The unpacker checks the tag, version and section sizes (`CheckSectionSize*`).

`sandbox/save/savemap.json` (from `savemap.py`) records each entry's source. Every entry is one of:

- a direct copy of a struct field;
- a narrowed int (`PackBytes3/4`);
- computed by a named packer routine, listing the fields it read;
- unwritten (always 0).

### 3. Packed block ↔ game struct: bit maps

The packers rearrange fields: bitfields, narrowed ints, records split across sections, flag bytes. Rather than transcribing about 200 routines by hand, the transform was derived with the game's own code as the oracle (`sandbox/save/derive_maps.py`):

- **Method:** run `UnpackSave<K>` and `PackSave<K>` under Unicorn on index-coded patterns. Pattern *k* sets input bit *j* if bit *k* of *j* is set, and each pattern is paired with its complement. An output bit that follows input *j* in every pattern is mapped to *j*.
- **Result:** every kind is a pure bit permutation, plus constants (tag, sizes, version) and one exception. GameState +0x3958..+0x39fe (section 60, entries 1–5, 167 bytes) are booleans: unpack stores `packed byte != 0` and pack stores `struct byte == 1`.
- **Pack map:** on every image the game can hold (booleans 0/1, bitfields within their width), pack inverts unpack. The pack map is the inverse of the unpack map, plus derived sources for the packed bits the unpacker ignores, which are duplicates. With out-of-range struct values, the game's packer ORs a field into its neighbours. A writer must therefore store in-range values, as the game itself does.
- **Bytes the unpacker never writes** (runtime fields, kept across a load): C +0xf0..+0xf3. Two B fields hold −1 in RAM but are not saved: +0x38f8 and +0x3ab4. A fresh load gives 0.

## GameState fields used by the engine

Offsets are into the B struct image, which equals RAM at 0x2142c8c. Each was verified with the game's own accessors under Unicorn.

| offset | meaning |
|---|---|
| +0x8 | name, text var 0x1b (`GameState_GetPtr_Ofs0x08`): 16 bytes, NUL-terminated UTF-8 |
| +0x18 | player name `{v0}` / var 0x1c (`GameState_GetPtr_Ofs0x18`) |
| +0x78 | calendar / lovers state (`calendar_78`, docs/CALENDAR.md) |
| +0x1224 + 0x688·(s−1) | scoped flag block of scope *s* = 1..3 (Manaka, Rinko, Nene) |
| +0x15c4 + 0x688·slot | heroine record (docs/SAVE_RECORD.md); affection at +6 |

Each heroine has one 0x688-byte block (`GameState_GetPointer(k)` = 0x2143eb0 + 0x688·k, k = scope − 1 = heroine slot):

| offset in block | contents |
|---|---|
| idx/8 | flag bit *idx* (`GameFlags_TestBit` / `SetBit` on the storage pointer) |
| 800 (decimal) + idx | scoped byte *idx* (counters) |
| 0x360 + 4·idx | scoped word *idx* |
| 0x3a0 | the heroine record (= GameState +0x15c4 + 0x688·k) |

Flag ids normalise as in docs/CONDITIONS.md. There is no global block:

- A global (scope 0) flag set writes the bit in all three blocks (`GameFlags_SetScopedBit` loops `SetScope(0..2)`).
- A global flag read reads block 0, Manaka's (`GameFlags_EnterScopeForRead` → `GameFlags_SyncStoragePointer(0)` → `GameState_GetPointer(0)`), whatever the current heroine is.
- A global scoped byte is read and written in block 0 (traced: `SetScopedByte(0x0001)` writes GameState +0x1545).

`game::Flags` and `game::byte_key` follow these rules, and `Save::game_state` loads the three blocks as they are.

Settings record (A): music, sound and voice levels at +5, +6 and +7 (0..15); text speed at +0xc.

## Proofs (all recorded backups, `tools/emu_scripts/states/logs/backup-*.sav`)

| check | where | result |
|---|---|---|
| The game's packers reproduce the A, B and C blocks from RAM | `packemu.py` | byte-identical |
| LZ10 compressor | `savefmt.lz10_compress`, `save::lz10_compress` | byte-identical on all 12 blocks |
| The game's unpackers restore RAM from our blocks | `unpack_all.py` | A 504/504, C 240/244 (4 runtime bytes), B all except two runtime fields |
| Bit maps vs the game's unpacker and packer | `verify_maps.py` | exact on every backup's real blocks, and on 8 random canonical images per kind per backup |
| D, E, F maps | `derive_maps.py` | exact vs the game's code on random data (none of the recorded cards holds D, E or F) |
| The game's own `Save_WriteBlock` vs our card writer, every kind A–F, several slots, copy/generation states incl. wrap | `writeblock_check.py` | 30 / 30 byte-identical card writes (offset, header, LZ, SHA-1) |
| Second save file (B slot 1, slot = GameState +0x7b) from live play | `unpack_all` method on session 155533 | exact vs the game's unpack; equals RAM except the 2 runtime fields |
| Python writer → boot | `write_check.py` | the game loads the edited GameState: 15448/15448 bytes equal |
| Rust writer → boot | `examples/save_edit.rs` + boot | the game loads it: 15448/15448 bytes equal |
| Rust vs the game's unpack | `tests/save_content.rs` (oracle digests) | SHA-1 of every image equal |

## Open points

- **D and F with real data.** Both are proven against the game's own code: struct maps on generated data, card writes byte for byte. No recorded card holds them yet. F (`LDTG`, one block per heroine) is most likely the trip (旅行 / 旅の思い出ギャラリー) data, and D is written by the save menu after `ItemData_AddRandomEntry`. Both belong to the lovers phase, which no available save reaches.
- **E** is written only by the communication and wireless screens (`Ov11MsgDialog`, `Ov16CardExchange`, `Ov26AdhocComm`), which are out of scope for the port. Its format is proven against the game's code like D and F.
- **B's third slot (save file 3)** follows the same code path as file 2, which was recorded and verified.
