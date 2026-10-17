//! The microphone through the game's own SDK code: MIC_StartAutoSampling's request reaches the engine, samples (here
//! from a recording) land in the game's buffer in the format it asked for as time passes, MIC_GetLastSamplingAddress
//! follows them, MIC_DoSampling takes one, MIC_StopAutoSampling ends it. Needs ../content (skipped without it).
use lp_engine::flow::Arg;
use lp_engine::rom::Rom;
use lp_engine::runtime::{ext, sdk, Game, NoSubsystems, TICKS_PER_FRAME};
use std::path::Path;

const PARAM: u32 = 0x23a0000;
const BUF: u32 = 0x23a1000;
const SIZE: u32 = 0x400;
/// MIC_SAMPLING_RATE_8K: the interval in ARM7 cycles
const SPAN: u32 = 33_513_982 / 8000;

#[test]
fn auto_sampling_fills_the_games_buffer() {
    let Ok(rom) = Rom::open(Path::new("../content")) else { return };
    let mut g = Game::new(rom, NoSubsystems);
    let faults = g.power_on();
    assert!(faults.is_empty(), "{faults:?}");
    let ramp: Vec<i16> = (0..2000).map(|i| (i * 97 % 65536 - 32768) as i16).collect();
    for (ty, wide) in [(1u32, true), (0, false), (3, true), (2, false)] {
        g.m.mic.replay = Some(ramp.iter().copied().collect());
        g.m.ram.put(BUF, &vec![0u8; SIZE as usize]);
        // MICAutoParam: type, buffer, size, rate, loop_enable, full_callback, full_arg
        for (i, v) in [ty, BUF, SIZE, SPAN, 1, 0, 0].iter().enumerate() {
            g.m.ram.set_u32(PARAM + 4 * i as u32, *v);
        }
        let r = ext::call(&mut g.m, &mut g.subs, 0x20259f8, &mut [Arg::I(PARAM as i32)]); // MIC_StartAutoSampling
        assert_eq!(r, 0, "MIC_StartAutoSampling result");
        assert!(g.m.mic.sampling());
        // one frame: 8753 ticks of 64 cycles at an interval of 4189 cycles
        g.m.tick += TICKS_PER_FRAME;
        sdk::mic_advance(&mut g.m, &mut g.subs);
        let n = (TICKS_PER_FRAME * 64 / SPAN as u64) as u32;
        let bytes = if wide { 2 } else { 1 };
        for i in 0..n {
            let want = sdk::mic_format(ramp[i as usize], ty);
            let got = if wide { g.m.ram.u16(BUF + i * bytes) } else { g.m.ram.u8(BUF + i * bytes) as u16 };
            assert_eq!(got, want, "type {ty} sample {i}");
        }
        assert_eq!(g.m.ram.u16(BUF + n * bytes), 0, "type {ty}: no sample beyond the time passed");
        let last = ext::call(&mut g.m, &mut g.subs, 0x2025ac4, &mut []) as u32; // MIC_GetLastSamplingAddress
        assert_eq!(last, BUF + (n - 1) * bytes, "type {ty}: last address");
        println!("type {ty}: {n} samples, last at {last:#x}");
        let r = ext::call(&mut g.m, &mut g.subs, 0x2025a8c, &mut []); // MIC_StopAutoSampling
        assert_eq!(r, 0, "MIC_StopAutoSampling result");
        assert!(!g.m.mic.sampling());
    }
    // one sample: MIC_DoSampling(type, buf)
    g.m.mic.replay = Some([1234i16].into_iter().collect());
    let r = ext::call(&mut g.m, &mut g.subs, 0x2025890, &mut [Arg::I(1), Arg::I(BUF as i32)]);
    assert_eq!(r, 0, "MIC_DoSampling result");
    assert_eq!(g.m.ram.u16(BUF), sdk::mic_format(1234, 1));
    assert!(g.m.mic.wanted);
}
