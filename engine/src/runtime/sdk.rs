//! The hardware edge: the few SDK functions that would talk to hardware the engine does not have (the card, the
//! ARM7, interrupt-driven waits) answered from the engine. Everything above them, the SDK's own file system, heaps,
//! and the game, runs as the game's code or as native ports.
//!
//! The card's ROM is served from content/fs (written by the extractor's `fs` stage): the header, FNT, FAT and ARM9
//! overlay table at their ROM offsets (rom.json) and every NitroFS file at its FAT range (index.json).
use super::ext::{Entry, Kind::*, Ret::*};
use super::{Machine, Subsystems};
use crate::flow::Arg;
use std::path::{Path, PathBuf};

fn arg(a: &[Arg], i: usize) -> u32 {
    a.get(i).and_then(|x| x.int()).unwrap_or(0) as u32
}

/// the card ROM, assembled from the extracted content
#[derive(Default)]
pub struct VirtualRom {
    /// (start, end, file) sorted by start
    segs: Vec<(u32, u32, PathBuf)>,
    /// reads that fell outside every segment: (offset, length)
    pub misses: Vec<(u32, u32)>,
}

impl VirtualRom {
    pub fn open(content: &Path) -> VirtualRom {
        let dir = content.join("fs");
        let mut segs = Vec::new();
        if let Ok(b) = std::fs::read(dir.join("rom.json")) {
            if let Ok(serde_json::Value::Object(m)) = serde_json::from_slice::<serde_json::Value>(&b) {
                for (name, v) in m {
                    let (o, n) = (v[0].as_u64().unwrap_or(0) as u32, v[1].as_u64().unwrap_or(0) as u32);
                    segs.push((o, o + n, dir.join(name)));
                }
            }
        }
        if let Ok(b) = std::fs::read(dir.join("index.json")) {
            if let Ok(serde_json::Value::Array(v)) = serde_json::from_slice::<serde_json::Value>(&b) {
                for e in v {
                    let (path, s, t) = (e[1].as_str().unwrap_or(""), e[2].as_u64().unwrap_or(0) as u32, e[3].as_u64().unwrap_or(0) as u32);
                    segs.push((s, t, dir.join(path)));
                }
            }
        }
        segs.sort_by_key(|s| s.0);
        VirtualRom { segs, misses: Vec::new() }
    }

    /// `len` bytes of the card at `off` (zeros where nothing was extracted, recorded as a miss)
    pub fn read(&mut self, off: u32, len: u32) -> Vec<u8> {
        let mut out = vec![0u8; len as usize];
        let end = off + len;
        let mut covered = 0u32;
        let i = self.segs.partition_point(|s| s.1 <= off);
        for (s, e, path) in self.segs[i..].iter() {
            if *s >= end {
                break;
            }
            let (a, b) = ((*s).max(off), (*e).min(end));
            if a >= b {
                continue;
            }
            if let Ok(mut f) = std::fs::File::open(path) {
                use std::io::{Read, Seek, SeekFrom};
                if f.seek(SeekFrom::Start((a - s) as u64)).is_ok() {
                    let _ = f.read_exact(&mut out[(a - off) as usize..(b - off) as usize]);
                    covered += b - a;
                }
            }
        }
        if covered < len {
            self.misses.push((off, len));
        }
        out
    }
}

/// 0x2014b40 CARDi_ReadRom(dma, src, dst, len, callback, arg, is_async): the card read, then the completion callback
pub fn card_read_rom(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (src, dst, len) = (arg(a, 1), arg(a, 2), arg(a, 3));
    let (callback, cb_arg) = (arg(a, 4), arg(a, 5));
    let base = m.ram.u32(m.lit(0x2014cb0)); // the card's ROM base the function adds to src
    let data = m.ram.vrom.read(src.wrapping_add(base), len);
    m.vrom_reads.push((src.wrapping_add(base), len));
    m.ram.put(dst, &data);
    if callback != 0 {
        super::ext::call(m, s, callback & !1, &mut [Arg::I(cb_arg as i32)]);
    }
    1
}

/// the SDK edge's registry entries (consulted before the game-level registry)
pub fn entry(a: u32) -> Option<Entry> {
    let e = |name, kind, ret| Some(Entry { name, kind, ret });
    match a {
        // waits for the display's line counter to reach 0 (the engine has no display timing)
        0x2021240 => e("OSi_WaitVCount0", Native(nop), Void),
        0x2014b40 => e("CARDi_ReadRom", Native(card_read_rom), Void),
        // the backup chip (the save), driven by the ARM7 on the DS: the engine's battery save
        0x2013de8 => e("CARDi_Request", Native(card_request), Value),
        // the card's protocol: page reads, the chip ID (the firmware recorded the same ID at boot, which the SDK's
        // pulled-out check compares), status and refresh
        0x20147d8 => e("CARDi_ReadRomWithCPU", Native(card_read_rom_cpu), Value),
        0x201466c => e("CARDi_ReadRomIDCore", Native(card_id), Value),
        0x20146c4 => e("CARDi_ReadRomStatusCore", Native(card_status), Value),
        0x201478c => e("CARDi_RefreshRomCore", Native(nop), Void),
        // the inter-processor FIFO: the ARM7's services are answered at their own APIs, so nothing waits on it
        // the real-time clock (the ARM7 reads it on the DS): the engine's clock, `Machine::rtc`
        0x2022d34 => e("RTC_Init", Native(nop), Void),
        0x2022e10 => e("RTC_GetDate", Native(rtc_get_date), Value),
        0x2022eb8 => e("RTC_GetTime", Native(rtc_get_time), Value),
        0x2022f6c => e("RTC_GetDateTime", Native(rtc_get_date_time), Value),
        0x2022da0 => e("RTC_GetDateAsync", Native(rtc_get_date_async), Value),
        0x2022e48 => e("RTC_GetTimeAsync", Native(rtc_get_time_async), Value),
        0x2022ef0 => e("RTC_GetDateTimeAsync", Native(rtc_get_date_time_async), Value),
        // DMA with a completion interrupt: the transfer, then the callback the interrupt would run
        0x201e2a8 => e("MIi_DmaCopy32Async", Native(dma_copy32_async), Void),
        0x201e1a0 => e("MIi_DmaFill32Async", Native(dma_fill32_async), Void),
        // the touch panel (sampled by the ARM7 on the DS): the engine's input, already in screen coordinates
        0x202514c => e("TP_Init", Native(nop), Void),
        0x20251c0 => e("TP_GetUserInfo", Native(one), Value),
        0x2025254 => e("TP_SetCalibrateParam", Native(nop), Void),
        0x2025344 => e("TP_RequestSamplingAsync", Native(nop), Void),
        0x20253e0 => e("TP_GetCalibratedResult", Native(tp_result), Value),
        0x202542c => e("TP_WaitCalibratedResult", Native(tp_result), Value),
        0x2025448 => e("TP_CalcCalibrateParam", Native(nop), Value),
        0x2025634 => e("TP_GetCalibratedPoint", Native(tp_point), Void),
        0x2025750 => e("TP_WaitBusy", Native(nop), Void),
        // the power manager (the ARM7's): commands succeed at once; backlights on, battery high
        0x202614c => e("PM_SendUtilityCommandAsync", Native(pm_command_async), Value),
        0x2026194 => e("PM_SendUtilityCommand", Native(pm_command), Value),
        0x2026420 => e("PM_GetBattery", Native(pm_get_battery), Value),
        0x2026460 => e("PM_GetBackLight", Native(pm_get_backlight), Value),
        0x2026258 => e("PM_SetBackLightAsync", Native(pm_async_ok_2), Value),
        0x20262e0 => e("PM_SetBackLight", Native(nop), Value),
        0x20261cc => e("PMi_SetLEDAsync", Native(pm_async_ok_1), Value),
        0x2026230 => e("PMi_SetLED", Native(nop), Value),
        0x20263bc => e("PM_SetAmp", Native(nop), Value),
        0x20263d4 => e("PMi_SetAmp", Native(nop), Value),
        0x2026404 => e("PM_SetAmpGain", Native(nop), Value),
        0x2026a6c => e("PM_SetLCDPower", Native(pm_set_lcd_power), Value),
        0x2026580 => e("PM_GoSleepMode", Native(nop), Void),
        0x2026380 => e("PM_ForceToPowerOff", Native(pm_power_off), Value),
        0x2025d68 => e("PMi_WaitBusy", Native(nop), Void),
        // the tick: the engine's clock
        0x2021d60 => e("OS_GetTick", Native(os_get_tick), Value),
        0x2022a44 => e("PXI_Init", Native(nop), Void),
        0x2022a50 => e("PXI_InitFifo", Native(nop), Void),
        0x2022b4c => e("PXI_SetFifoRecvCallback", Native(pxi_set_callback), Void),
        0x2022b98 => e("PXI_IsCallbackReady", Native(one), Value),
        0x2022bbc => e("PXI_SendWordByFifo", Native(pxi_send), Value),
        _ => None,
    }
}

pub const TRAPPED: &[u32] = &[0x2013de8, 0x202614c, 0x2026194, 0x2026420, 0x2026460, 0x2026258, 0x20262e0, 0x20261cc, 0x2026230, 0x20263bc, 0x20263d4, 0x2026404, 0x2026a6c, 0x2026580, 0x2026380, 0x2025d68, 0x2021240, 0x202514c, 0x20251c0, 0x2025254, 0x2025344, 0x20253e0, 0x202542c, 0x2025448, 0x2025634, 0x2025750, 0x20147d8, 0x201466c, 0x20146c4, 0x201478c, 0x2021d60, 0x201e2a8, 0x201e1a0, 0x2014b40, 0x2022d34, 0x2022e10, 0x2022eb8, 0x2022f6c, 0x2022da0, 0x2022e48, 0x2022ef0, 0x2022a44, 0x2022a50, 0x2022b4c, 0x2022b98, 0x2022bbc];

fn nop(_: &mut Machine, _: &mut dyn Subsystems, _: &mut [Arg]) -> i32 {
    0
}
fn one(_: &mut Machine, _: &mut dyn Subsystems, _: &mut [Arg]) -> i32 {
    1
}
fn pxi_set_callback(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    m.pxi_callbacks.insert(arg(a, 0), arg(a, 1));
    0
}
/// a word sent to the ARM7: accepted (PXI_FIFO_SUCCESS); recorded so unanswered protocols show up. Tag 7 goes to the
/// sound driver (`snd7`)
fn pxi_send(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (tag, data) = (arg(a, 0), arg(a, 1));
    *m.pxi_sent.entry(tag).or_default() += 1;
    if tag == 7 {
        super::snd7::Snd7::receive(m, data);
    }
    0
}

// ---- the real-time clock: RTCDate {year 0..99, month, day, weekday 0 = Sunday}, RTCTime {hour, minute, second} --------

fn put_date(m: &mut Machine, p: u32) {
    if p != 0 {
        for (i, v) in m.rtc[..4].to_vec().into_iter().enumerate() {
            m.ram.set_u32(p + 4 * i as u32, v);
        }
    }
}
fn put_time(m: &mut Machine, p: u32) {
    if p != 0 {
        for (i, v) in m.rtc[4..].to_vec().into_iter().enumerate() {
            m.ram.set_u32(p + 4 * i as u32, v);
        }
    }
}
fn rtc_get_date(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    put_date(m, arg(a, 0));
    0
}
fn rtc_get_time(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    put_time(m, arg(a, 0));
    0
}
fn rtc_get_date_time(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    put_date(m, arg(a, 0));
    put_time(m, arg(a, 1));
    0
}
/// the async forms complete at once: callback(RTC_RESULT_SUCCESS, arg)
fn rtc_done(m: &mut Machine, s: &mut dyn Subsystems, cb: u32, cb_arg: u32) -> i32 {
    if cb != 0 {
        super::ext::call(m, s, cb & !1, &mut [Arg::I(0), Arg::I(cb_arg as i32)]);
    }
    0
}
fn rtc_get_date_async(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    put_date(m, arg(a, 0));
    rtc_done(m, s, arg(a, 1), arg(a, 2))
}
fn rtc_get_time_async(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    put_time(m, arg(a, 0));
    rtc_done(m, s, arg(a, 1), arg(a, 2))
}
fn rtc_get_date_time_async(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    put_date(m, arg(a, 0));
    put_time(m, arg(a, 1));
    rtc_done(m, s, arg(a, 2), arg(a, 3))
}

/// the RTC fields for a time given as seconds since 1970-01-01 in the player's local time
pub fn rtc_from_local_seconds(t: i64) -> [u32; 7] {
    let days = t.div_euclid(86400);
    let secs = t.rem_euclid(86400) as u32;
    // civil-from-days (H. Hinnant)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (if mo <= 2 { y + 1 } else { y }) as u32;
    let weekday = (days + 4).rem_euclid(7) as u32; // 1970-01-01 was a Thursday
    [y % 100, mo, d, weekday, secs / 3600, secs / 60 % 60, secs % 60]
}

/// the host's local time as seconds since the epoch (the offset from `date +%z`, or UTC)
/// local seconds since 1970 for a calendar date and time (days-from-civil, H. Hinnant)
pub fn local_seconds(y: i64, mo: i64, d: i64, h: i64, mi: i64, sec: i64) -> i64 {
    let y = if mo <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if mo > 2 { mo - 3 } else { mo + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + h * 3600 + mi * 60 + sec
}

pub fn host_local_seconds() -> i64 {
    let utc = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let off = std::process::Command::new("date")
        .arg("+%z")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|z| {
            let z = z.trim();
            let sign = if z.starts_with('-') { -1 } else { 1 };
            let (h, mi) = (z.get(1..3)?.parse::<i64>().ok()?, z.get(3..5)?.parse::<i64>().ok()?);
            Some(sign * (h * 3600 + mi * 60))
        })
        .unwrap_or(0);
    utc + off
}

/// MIi_DmaCopy32Async(dma, src, dst, size, callback, arg)
fn dma_copy32_async(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (src, dst, n) = (arg(a, 1), arg(a, 2), arg(a, 3));
    for i in (0..n).step_by(4) {
        let v = m.ram.u32(src + i);
        m.ram.set_u32(dst + i, v);
    }
    let (cb, cb_arg) = (arg(a, 4), arg(a, 5));
    if cb != 0 {
        super::ext::call(m, s, cb & !1, &mut [Arg::I(cb_arg as i32)]);
    }
    0
}
/// MIi_DmaFill32Async(dma, dst, value, size, callback, arg)
fn dma_fill32_async(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (dst, v, n) = (arg(a, 1), arg(a, 2), arg(a, 3));
    for i in (0..n).step_by(4) {
        m.ram.set_u32(dst + i, v);
    }
    let (cb, cb_arg) = (arg(a, 4), arg(a, 5));
    if cb != 0 {
        super::ext::call(m, s, cb & !1, &mut [Arg::I(cb_arg as i32)]);
    }
    0
}

/// OS_GetTick: 64-bit (r1:r0), the OS tick timer (timer 0 at bus / 64, started by OS_InitTick)
fn os_get_tick(m: &mut Machine, _: &mut dyn Subsystems, _: &mut [Arg]) -> i32 {
    let t = os_tick(m);
    m.ret_hi = Some((t >> 32) as u32);
    t as u32 as i32
}
/// the OS tick now (0 before OS_InitTick)
pub fn os_tick(m: &Machine) -> u64 {
    m.tick.saturating_sub(m.tick_base.unwrap_or(m.tick))
}

/// the chip ID a mask ROM of the header's capacity reports (maker 0xc2; size byte(s) = capacity in MB - 1, the
/// capacity being 128 KB << header byte 0x14), as the DS's card reports it and the emulators derive it; power_on
/// writes it where the firmware keeps the boot card's ID
pub fn card_id_from_header(h: &[u8]) -> u32 {
    let cap = h.get(0x14).copied().unwrap_or(0) as u32;
    0xc2 | ((((128u32 << cap) / 1024).saturating_sub(1)) << 8)
}

/// CARDi_ReadRomWithCPU(userdata, buffer, offset, length): the bytes; returns length
fn card_read_rom_cpu(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (dst, off, len) = (arg(a, 1), arg(a, 2), arg(a, 3));
    let data = m.ram.vrom.read(off, len);
    m.vrom_reads.push((off, len));
    m.ram.put(dst, &data);
    len as i32
}
fn card_id(m: &mut Machine, _: &mut dyn Subsystems, _: &mut [Arg]) -> i32 {
    m.ram.card.card_id = m.card_id;
    m.card_id as i32
}
fn card_status(_: &mut Machine, _: &mut dyn Subsystems, _: &mut [Arg]) -> i32 {
    0x20
}

/// TPData {u16 x, u16 y, u16 touch, u16 validity}: the engine's touch (validity 0: both axes valid). A pen-up
/// sample keeps the last pressed position, as the hardware's does: the game's release hit tests read it
/// (Input_SwitchMode copies x/y whenever validity is 0)
fn tp_result(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let p = arg(a, 0);
    let (x, y, t) = match m.input.touch {
        Some((x, y)) => {
            m.tp_last = (x, y);
            (x, y, 1)
        }
        None => (m.tp_last.0, m.tp_last.1, 0),
    };
    if p != 0 {
        m.ram.set_u16(p, x);
        m.ram.set_u16(p + 2, y);
        m.ram.set_u16(p + 4, t);
        m.ram.set_u16(p + 6, 0);
    }
    0
}
/// TP_GetCalibratedPoint(dst, src): the engine's samples are calibrated already
fn tp_point(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (d, s) = (arg(a, 0), arg(a, 1));
    if d != 0 && s != 0 {
        let v = m.ram.bytes(s, 8);
        m.ram.put(d, &v);
    }
    0
}

/// the pad as the hardware shows it: KEYINPUT (0x4000130, A B Select Start Right Left Up Down R L, active low) and
/// the ARM7's extended keys at 0x27fffa8 (X Y in bits 10..11 of the engine's mask, the hinge open), active low
pub fn write_keys(m: &mut Machine) {
    let k = m.input.keys;
    m.ram.set_u16(0x4000130, !k & 0x3ff);
    let ext = (!(k >> 10) & 3) | 0x7c; // X, Y; debug and pen-down bits idle; hinge open (bit 7 clear)
    m.ram.set_u16(0x27fffa8, ext as u16);
}

// ---- the power manager ---------------------------------------------------------------------------------------------

/// PM_SendUtilityCommandAsync(number, parameter, retValue*, callback, arg): PM_RESULT_SUCCESS, callback(0, arg)
fn pm_command_async(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let ret = arg(a, 2);
    if ret != 0 {
        m.ram.set_u16(ret, 0);
    }
    let (cb, cb_arg) = (arg(a, 3), arg(a, 4));
    if cb != 0 {
        super::ext::call(m, s, cb & !1, &mut [Arg::I(0), Arg::I(cb_arg as i32)]);
    }
    0
}
/// PM_SendUtilityCommand(number, parameter, retValue*)
fn pm_command(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let ret = arg(a, 2);
    if ret != 0 {
        m.ram.set_u16(ret, 0);
    }
    0
}
/// PM_GetBattery(*battery): PM_BATTERY_HIGH
fn pm_get_battery(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let p = arg(a, 0);
    if p != 0 {
        m.ram.set_u32(p, 0);
    }
    0
}
/// PM_GetBackLight(*top, *bottom): both on
fn pm_get_backlight(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    for i in 0..2 {
        let p = arg(a, i);
        if p != 0 {
            m.ram.set_u32(p, 1);
        }
    }
    0
}
/// async PM calls whose callback / arg are the arguments after `n` leading ones
fn pm_async_ok(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg], n: usize) -> i32 {
    let (cb, cb_arg) = (arg(a, n), arg(a, n + 1));
    if cb != 0 {
        super::ext::call(m, s, cb & !1, &mut [Arg::I(0), Arg::I(cb_arg as i32)]);
    }
    0
}
fn pm_async_ok_1(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    pm_async_ok(m, s, a, 1)
}
fn pm_async_ok_2(m: &mut Machine, s: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    pm_async_ok(m, s, a, 2)
}
/// PM_SetLCDPower(on): the LCDs' power bit of POWCNT (0x4000304 bit 0); TRUE
fn pm_set_lcd_power(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let v = m.ram.u16(0x4000304);
    m.ram.set_u16(0x4000304, if arg(a, 0) != 0 { v | 1 } else { v & !1 });
    1
}
/// PM_ForceToPowerOff: the game wants to switch the DS off
fn pm_power_off(m: &mut Machine, _: &mut dyn Subsystems, _: &mut [Arg]) -> i32 {
    m.not_ported.push("PM_ForceToPowerOff: the game switched the console off".into());
    0
}

// ---- the backup chip ------------------------------------------------------------------------------------------------

/// CARDi_Request(common, request, retries): the request the ARM7 would carry out on the backup chip, done on
/// `Machine::backup`. The shared command block (*common): +0 result, +0xc source, +0x10 destination, +0x14 length.
/// Requests (SDK CARDRequest): 0 init, 1 ack, 2 identify, 3 read ID, 6 read, 7 write, 8 program, 9 verify,
/// 10/11/12/15 erase page / sector / chip / subsector, 13 read status, 14 write status.
fn card_request(m: &mut Machine, _: &mut dyn Subsystems, a: &mut [Arg]) -> i32 {
    let (common, req) = (arg(a, 0), arg(a, 1));
    let flags = m.ram.u32(common + 4);
    m.ram.set_u32(common + 4, (flags | 2) & !0x20);
    let cmd = m.ram.u32(common);
    let (src, dst, len) = (m.ram.u32(cmd + 0xc), m.ram.u32(cmd + 0x10), m.ram.u32(cmd + 0x14));
    let size = m.backup.len() as u32;
    let mut result = 0u32;
    match req {
        6 => {
            // read: chip -> RAM
            let d: Vec<u8> = (0..len).map(|i| m.backup.get(((src + i) % size.max(1)) as usize).copied().unwrap_or(0xff)).collect();
            m.ram.put(dst, &d);
        }
        7 | 8 => {
            // write / program: RAM -> chip
            let d = m.ram.bytes(src, len as usize);
            for (i, b) in d.into_iter().enumerate() {
                let o = (dst as usize + i) % m.backup.len().max(1);
                if let Some(x) = m.backup.get_mut(o) {
                    *x = b;
                }
            }
            m.backup_dirty = true;
        }
        9 => {
            // verify: RAM == chip
            let d = m.ram.bytes(src, len as usize);
            let same = d.iter().enumerate().all(|(i, &b)| m.backup.get((dst as usize + i) % m.backup.len().max(1)) == Some(&b));
            if !same {
                result = 1; // CARD_RESULT_FAILURE
            }
        }
        10 | 11 | 12 | 15 => {
            let (at, n) = if req == 12 { (0, size) } else { (dst, len) };
            for i in 0..n {
                let o = ((at + i) % size.max(1)) as usize;
                if let Some(x) = m.backup.get_mut(o) {
                    *x = 0xff;
                }
            }
            m.backup_dirty = true;
        }
        13 => {
            if dst != 0 {
                m.ram.set_u8(dst, 0); // status: ready, not write-enabled
            }
        }
        _ => {}
    }
    m.ram.set_u32(cmd, result);
    *m.card_requests.entry(req).or_default() += 1;
    (result == 0) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_seconds_round_trip() {
        // 2026-10-14 is a Wednesday (weekday 3)
        let t = local_seconds(2026, 10, 14, 12, 34, 56);
        assert_eq!(rtc_from_local_seconds(t), [26, 10, 14, 3, 12, 34, 56]);
        for &(y, mo, d) in &[(2000, 2, 29), (2009, 9, 3), (2099, 12, 31), (2024, 3, 1)] {
            let r = rtc_from_local_seconds(local_seconds(y, mo, d, 0, 0, 0));
            assert_eq!((r[0], r[1], r[2]), ((y % 100) as u32, mo as u32, d as u32));
        }
    }
}
