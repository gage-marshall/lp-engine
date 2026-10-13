//! Canonical text dump of an SDAT; diffed against tools/sdat_dump.py.   usage: sdat_dump SOUND_DATA.sdat
use lp_engine::sdat::*;
use sha1::{Digest, Sha1};

fn sha(b: &[u8]) -> String {
    let mut s = Sha1::new();
    s.update(b);
    s.finalize().iter().take(6).map(|x| format!("{:02x}", x)).collect()
}

fn region(r: &Region) -> String {
    format!("{},{},{},{},{},{},{},{},{}", r.kind, r.wave, r.wavearc, r.original_key, r.attack, r.decay, r.sustain, r.release, r.pan)
}

fn inst(i: &Inst) -> String {
    match i {
        Inst::None => "N".into(),
        Inst::Single(r) => format!("S {}", region(r)),
        Inst::DrumSet { min, max, regions } => format!("D {} {} [{}]", min, max, regions.iter().map(region).collect::<Vec<_>>().join(";")),
        Inst::KeySplit { keys, regions } => format!("K {} [{}]", keys.iter().map(|k| k.to_string()).collect::<Vec<_>>().join(","), regions.iter().map(region).collect::<Vec<_>>().join(";")),
        Inst::Other(t) => format!("O {}", t),
    }
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let s = Sdat::parse(std::fs::read(&a[1]).unwrap()).unwrap();
    for (i, f) in s.wavearcs.iter().enumerate() {
        if f.is_none() {
            continue;
        }
        let mut n = 0;
        while let Some(w) = s.wave(i, n) {
            let bytes: Vec<u8> = w.pcm.iter().flat_map(|x| x.to_le_bytes()).collect();
            println!("W {} {} fmt={} rate={} loop={} n={} {}", i, n, w.format, w.rate, match w.looped { Some((a, b)) => format!("({}, {})", a, b), None => "None".into() }, w.pcm.len(), sha(&bytes));
            n += 1;
        }
    }
    for (i, b) in s.banks.iter().enumerate() {
        if let Some((_, wa)) = b {
            let bank = match s.bank(i) { Some(b) => b, None => continue };
            let was: Vec<String> = wa.iter().map(|x| x.map_or("65535".to_string(), |v| v.to_string())).collect();
            println!("B {} [{}] {} {}", i, was.join(", "), bank.instruments.len(), sha(bank.instruments.iter().map(inst).collect::<Vec<_>>().join("\n").as_bytes()));
        }
    }
    for (i, q) in s.seqs.iter().enumerate() {
        if let Some((_, info)) = q {
            let sd = s.sequence(i).unwrap();
            println!("Q {} bank={} vol={} cpr={} ppr={} ply={} n={} {}", i, info.bank, info.volume, info.channel_priority, info.player_priority, info.player, sd.data.len(), sha(&sd.data));
        }
    }
    for (i, a) in s.seqarcs.iter().enumerate() {
        if a.is_none() {
            continue;
        }
        let n = s.seqarc_len(i).unwrap();
        println!("A {} count={}", i, n);
        for k in 0..n {
            let sd = s.archive_sequence(i, k).unwrap();
            println!(" a {} {} off={} bank={} vol={} cpr={} ppr={} ply={} {}", i, k, sd.start, sd.info.bank, sd.info.volume, sd.info.channel_priority, sd.info.player_priority, sd.info.player, sha(&sd.data));
        }
    }
    for (i, p) in s.players.iter().enumerate() {
        if let Some(p) = p {
            println!("P {} {} {}", i, p.max_seqs, p.channel_mask);
        }
    }
}
