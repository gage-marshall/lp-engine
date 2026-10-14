//! Scene inputs: what a heroine scene needs before it starts, resolved as the game resolves it from the event id, the save,
//! the game clock and content files. Pure functions; reference `sandbox/gx/scene_inputs.py` + `scene_data.py`, rules and
//! evidence in docs/RENDER_PRIMITIVES.md ("Scene state from game data").

use crate::rom::Rom;
use std::path::Path;

/// heroine slot (0 Manaka, 1 Rinko, 2 Nene) of an event id (top byte; `Model_SetByte0AndNotifyGlobals`)
pub fn heroine_of_event(event_id: u32) -> Option<u32> {
    match event_id >> 24 {
        0xa0 | 0xa4 | 0xa7 => Some(0),
        0x58 | 0x5c | 0x5f => Some(1),
        0x08 | 0x0c | 0x0f => Some(2),
        _ => None,
    }
}

pub const PREFIX: [&str; 3] = ["m", "r", "n"];

fn label_file(arc: &Path, label: u32) -> Result<Vec<u8>, String> {
    std::fs::read(arc.join(format!("tm_model_label.arc/{label:04}.bin"))).map_err(|e| format!("tm_model_label {label}: {e}"))
}

/// tm_model_label record: u16 base archive entry (`Mod1ffa934_Proc_1ffa9e0`), u16 motion model class (`MotionEntry_GetShort6`)
pub fn label_base(arc: &Path, label: u32) -> Result<u32, String> {
    let d = label_file(arc, label)?;
    Ok(u16::from_le_bytes([d[0], d[1]]) as u32)
}

pub fn label_class(arc: &Path, label: u32) -> Result<u32, String> {
    let d = label_file(arc, label)?;
    Ok(u16::from_le_bytes([d[2], d[3]]) as u32)
}

/// the wardrobe number a label stands for: the label's base entry is the part's shading bundle, named
/// `<h>_<00 hair | 01 outfit>_<NNN>_<state>.tmbsd`; NNN is the number the model and texture names carry
pub fn label_number(arc: &Path, heroine: u32, label: u32, hair: bool) -> Result<u32, String> {
    let base = label_base(arc, label)?;
    let file = arc.join(format!("{}_block3d_{:02}.arc/{base:04}.bin", PREFIX[heroine as usize], if hair { 0 } else { 1 }));
    let b = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    let ents = crate::heroine::bundle_entries(&b).ok_or("label base entry is not a bundle")?;
    let name = &ents.first().ok_or("empty bundle")?.0;
    name.split('_').nth(2).and_then(|n| n.parse().ok()).ok_or_else(|| format!("unexpected shading bundle name {name}"))
}

/// hair label (`SaveSlot_UpdateStateMetadata` 0x2060c7c -> FlowState +0x24 = `Overlay_UpdateMode` 0x20eee34): first entry of
/// fashion/fashion_hair_param_<slot>.bin (u32 n, n x {s16 label, s8 state, u8, u8 key, 3 u8}) with key = record +0xd and
/// state = the random-event state; none found and state != 0 -> retry with state 0
pub fn hair_label(content: &Path, slot: u32, key: u8, state: i32) -> Result<i32, String> {
    let p = content.join(format!("assets/fashion/fashion_hair_param_{slot}.bin"));
    let d = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let n = u32::from_le_bytes(d[0..4].try_into().unwrap()) as usize;
    for i in 0..n {
        let o = 4 + 8 * i;
        let (label, st, k) = (i16::from_le_bytes([d[o], d[o + 1]]) as i32, d[o + 2] as i8 as i32, d[o + 4]);
        if k == key && st == state {
            return Ok(label);
        }
    }
    if state != 0 { hair_label(content, slot, key, 0) } else { Ok(-1) }
}

/// random-event state FlowState +0x26 at event load: `RandomEvent_UpdateStateMachine(1)` 0x20ebe68 = 0 while GameState +0x78
/// is 0, else the active slot's record +0x107 (`SaveSlot_GetSlotPointerC` 0x2060c9c)
pub fn random_event_state(calendar_78: u8, record_107: i8) -> i32 {
    if calendar_78 == 0 { 0 } else { record_107 as i32 }
}

/// `Calendar_MonthToSeason`: first i with month <= table[i] (words at 0x2101cfc: 2, 5, 8, 11), else 0
pub fn season(rom: &Rom, month: u32) -> usize {
    (0..4).find(|&i| month as i32 <= rom.i32(0x2101cfc + 4 * i as u32)).unwrap_or(0)
}

/// `GameClock_RunTimeSequence` -> `GameClock_ClearTimeState` 0x205ec68: first i with hour < table[season][i] (bytes at
/// 0x2101d4e), else 0
fn clock_phase(rom: &Rom, month: u32, hour: u32) -> u32 {
    let s = season(rom, month) as u32;
    (0..3).find(|&i| hour < rom.u8(0x2101d4e + 3 * s + i) as u32).unwrap_or(0)
}

/// the 0 -> 2, 1 -> 0, 2 -> 1 remap shared by `MapCtl_RemapModeCode` 0x204ee30 and `ScheduleText_GetStateValue` 0x209e888
fn remap(phase: u32) -> u32 {
    [2, 0, 1][phase.min(2) as usize]
}

/// toon shading time mode (.tmbsd suffix _0/_1/_2): `MapCtl_RemapModeCode` of `Bg_ProcessTimedValue`; background kinds
/// 3 and 4 follow the clock phase, others use 1
pub fn time_mode(rom: &Rom, month: u32, hour: u32, bg_kind: u32) -> u32 {
    remap(if bg_kind == 3 || bg_kind == 4 { clock_phase(rom, month, hour) } else { 1 })
}

/// `TimePeriod_FromHour` 0x205ebf4: 5 below hour byte 0x2101d4d, else first i with hour < bytes 0x2101d48[i], else 5
pub fn time_period(rom: &Rom, hour: u32) -> u32 {
    if hour < rom.u8(0x2101d4d) as u32 {
        return 5;
    }
    (0..5).find(|&i| hour < rom.u8(0x2101d48 + i) as u32).unwrap_or(5)
}

/// backdrop time of day of a time period while GameState +0x78 is 0: `TextRes_SelectTimePeriodText` 0x209e8f4 (period 0-2
/// -> phase 1, 3 -> 2, 4-5 -> 0), then the shared remap
pub fn tod_of_period(period: u32) -> u32 {
    remap(match period {
        3 => 2,
        4 | 5 => 0,
        _ => 1,
    })
}

/// backdrop time of day (0 day, 1 evening, 2 night): `ScheduleText_UpdateState` 0x209dc34. Phase from
/// `TextRes_SelectTimePeriodText` (period 0-2 -> 1, 3 -> 2, 4-5 -> 0) while GameState +0x78 is 0 or flag 0xe0 is set, else
/// the clock phase; flag 0x104 turns phase 1 into 2, flag 0x105 forces 0; then the shared remap
pub fn backdrop_tod(rom: &Rom, month: u32, hour: u32, calendar_78: u8, flag_e0: bool, flag_104: bool, flag_105: bool) -> u32 {
    let mut phase = if calendar_78 == 0 || flag_e0 {
        match time_period(rom, hour) {
            3 => 2,
            4 | 5 => 0,
            _ => 1,
        }
    } else {
        clock_phase(rom, month, hour)
    };
    if flag_104 && phase == 1 {
        phase = 2;
    }
    if flag_105 {
        phase = 0;
    }
    remap(phase)
}

/// accessory archive entry (block3d.arc) of key id*100+variant: `Mod207b7a8` searches 0x93 records {u16 key, s16 entry} at the
/// table behind the literal 0x207b7f4; `kind` 0 = model, 1 = textures
pub fn accessory_entry(rom: &Rom, key: u32, kind: i32) -> Option<u32> {
    let t = rom.u32(0x207b7f4);
    (0..0x93u32).find(|&i| rom.u16(t + 4 * i) as u32 == key & 0xffff).map(|i| (rom.i16(t + 4 * i + 2) as i32 + kind) as u32)
}

/// accessory placement offset in the Head joint's frame for a heroine (`Model_ComputePartScaleOffsets`: block3d.arc/0202 =
/// i32 count, then 40-byte records {i32 id % 100, 3 heroines x (x, y, z) f32})
pub fn accessory_offset(arc: &Path, key: u32, heroine: u32) -> Option<[f64; 3]> {
    let d = std::fs::read(arc.join("block3d.arc/0202.bin")).ok()?;
    let n = i32::from_le_bytes(d.get(0..4)?.try_into().ok()?) as usize;
    let id = (key / 100) % 100;
    (0..n).find_map(|k| {
        let o = 4 + 40 * k;
        if i32::from_le_bytes(d.get(o..o + 4)?.try_into().ok()?) as u32 != id {
            return None;
        }
        let f = |i: usize| f32::from_le_bytes(d[o + 4 + 12 * heroine as usize + 4 * i..o + 8 + 12 * heroine as usize + 4 * i].try_into().unwrap()) as f64;
        Some([f(0), f(1), f(2)])
    })
}

/// typewriter interval table `TextControl_ProcessText` 0x208bf4c: 5 x 12.12 frames per character at 0x21274e0
pub fn char_intervals(rom: &Rom) -> [i32; 5] {
    let v = rom.i32s(0x21274e0, 5);
    [v[0], v[1], v[2], v[3], v[4]]
}

/// voice stream attenuation (cB) for a voice level 0..15: `GameSnd_ApplyChannelVolume` 0x20724f8 slot 3 -> value =
/// trunc(500 (level + 15) / 30), level 0 -> 0; `GameSnd_ConvertVolumeToAttenuation` 0x2071970 -> trunc((500 - value) (-640)
/// / 500); `GameSnd_StreamSetup` 0x2077e70 / `StreamStartIfReady` 0x2077f18 clamp <= -640 to -1024
pub fn voice_cb(level: u8) -> i32 {
    let value = if level == 0 { 0 } else { 500 * (level as i32 + 15) / 30 };
    let cb = (500 - value.clamp(0, 500)) * -640 / 500;
    if cb <= -640 { -1024 } else { cb }
}

/// sequence slot volume (extVolume): `GameSnd_ApplyChannelVolume` music / sound slots: 105 level / 15, at most 127
pub fn slot_volume(level: u8) -> u8 {
    (105 * level as u32 / 15).min(127) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn voice_curve_matches_the_traced_values() {
        assert_eq!(voice_cb(15), 0);
        assert_eq!(voice_cb(5), -213);
        assert_eq!(voice_cb(1), -299);
        assert_eq!(voice_cb(0), -1024, "level 0: value 0 -> -640 -> clamped to -1024");
        assert_eq!(slot_volume(15), 105);
    }

    #[test]
    fn event_heroines() {
        assert_eq!(heroine_of_event(0x0814_3402), Some(2));
        assert_eq!(heroine_of_event(0xa000_0000), Some(0));
        assert_eq!(random_event_state(0, 5), 0);
        assert_eq!(random_event_state(1, 5), 5);
    }
}

/// The name text variables, as `TextVar_InitAllNameVars` 0x2074168 sets them. Strings come from text bank 7
/// (`gd_textres07`, group 0 = the game's 0x1c0 = bank 7 * 0x40 + group 0); `game` is the GameState image of the save
/// (`save::Kind::Game`), `None`: a fresh state (every index 0, no name). `slot` = the current heroine.
///   v0 / 0x1c: name at +0x18; 0x1b: name at +0x8
///   v1: sub 0, line = +0x71 (0..1, else 0) - the player's pronoun (俺 / 僕)
///   v2 / v4 / v6 (heroine h): sub 1, line = byte table 0x2102994[2h] - how she refers to herself
///   v3 / v5 / v7 (heroine h): sub 2 + h, line = byte table (0x21029c2, 0x21029ef, 0x2102a1c)[0] - what the player calls her;
///     with +0x78 != 0, the current heroine's (and var 0x22) uses index +0x72 (below 0x22, Nene 0x26, else 0)
///   0x1a: +0x70 = 0: name +0x18, 1: name +0x8, each + "くん" unless the current heroine is Rinko; other kinds come from a
///     string the game keeps elsewhere (`UiText_UpdateLines`) and stay unset
pub fn name_vars(content: &Path, rom: &Rom, game: Option<&[u8]>, slot: u32) -> Result<std::collections::HashMap<u16, String>, String> {
    let raw = std::fs::read(content.join("assets/text/gd_textres07_jpn.txrc")).map_err(|e| format!("gd_textres07: {e}"))?;
    let t = crate::text::Txrc::parse(&raw)?;
    let line = |sub: usize, l: usize| t.line_str(0, sub, l).unwrap_or_default();
    let byte = |o: usize| game.map_or(0, |g| g[o]);
    let name = |o: usize| game.map_or(String::new(), |g| {
        let b = &g[o..o + 0x10];
        String::from_utf8_lossy(&b[..b.iter().position(|&c| c == 0).unwrap_or(16)]).into_owned()
    });
    let mut v = std::collections::HashMap::new();
    v.insert(0u16, name(0x18));
    v.insert(0x1c, name(0x18));
    v.insert(0x1b, name(0x8));
    let p = byte(0x71) as i8;
    v.insert(1, line(0, if (0..2).contains(&p) { p as usize } else { 0 }));
    const NICK: [(u32, usize); 3] = [(0x21029c2, 0x22), (0x21029ef, 0x22), (0x2102a1c, 0x26)];
    for h in 0..3usize {
        v.insert(2 + 2 * h as u16, line(1, rom.u8(0x2102994 + 2 * h as u32) as usize));
        v.insert(3 + 2 * h as u16, line(2 + h, rom.u8(NICK[h].0) as usize));
    }
    if byte(0x78) != 0 {
        let h = slot as usize % 3;
        let i = byte(0x72) as i8 as i32;
        let i = if (0..NICK[h].1 as i32).contains(&i) { i as u32 } else { 0 };
        let s = line(2 + h, rom.u8(NICK[h].0 + i) as usize);
        v.insert(3 + 2 * h as u16, s.clone());
        v.insert(0x22, s);
    }
    let suffix = if slot == 1 { "" } else { "くん" };
    match byte(0x70) as i8 {
        0 => { v.insert(0x1a, format!("{}{suffix}", name(0x18))); }
        1 => { v.insert(0x1a, format!("{}{suffix}", name(0x8))); }
        _ => {}
    }
    v.retain(|_, s| !s.is_empty());
    Ok(v)
}
