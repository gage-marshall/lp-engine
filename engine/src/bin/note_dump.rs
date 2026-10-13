//! Print the note commands a sequence executes in its first N ticks (diffed against tools/note_dump.py, the reference interpreter).
//!   note_dump SOUND_DATA.sdat SEQ_ID TICKS
use lp_engine::sdat::Sdat;
use lp_engine::snd::Driver;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let sdat = Arc::new(Sdat::parse(std::fs::read(&a[1]).unwrap()).unwrap());
    let id: usize = a[2].parse().unwrap();
    let ticks: u64 = a[3].parse().unwrap();
    let sd = sdat.sequence(id).unwrap();
    let mut drv = Driver::new(sdat, 48000);
    drv.note_log = Some(Vec::new());
    let h = drv.start(&sd, sd.info.player as usize, 0).unwrap();
    let mut buf = vec![0f32; 960];
    loop {
        match drv.seq_ticks(h) {
            Some(t) if t < ticks + 2 => drv.render(&mut buf),
            _ => break,
        }
    }
    let mut v = drv.note_log.take().unwrap();
    v.retain(|n| n.tick < ticks);
    v.sort_by_key(|n| (n.tick, n.track, n.key, n.vel, n.prg));
    for n in v {
        println!("{} {} {} {} {}", n.tick, n.track, n.key, n.vel, n.prg);
    }
}
