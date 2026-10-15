//! Profiles of the game's code (`Machine::profile`, `--profile` in `game` and `game_probe`): the instructions run
//! per code address, attributed to functions by the labels (`fid/labels_all.csv`: module, address, name, ...) with
//! the overlays that were loaded at the time.

/// the profile attributed to functions: per (module, function) the instructions run, from the labels'
/// start addresses (a function runs to the next label of its module); written as CSV, summarised on stdout
pub fn write(epochs: &[(Vec<(u32, u32, u32)>, Vec<(u32, u32)>)], labels_csv: &std::path::Path, out: &str) {
    use std::collections::{BTreeMap, HashMap};
    // labels by module: sorted (address, name, source)
    let mut labels: HashMap<String, Vec<(u32, String, String)>> = HashMap::new();
    if let Ok(t) = std::fs::read_to_string(labels_csv) {
        for line in t.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() >= 5 {
                if let Ok(a) = u32::from_str_radix(f[1], 16) {
                    labels.entry(f[0].to_string()).or_default().push((a, f[2].to_string(), f[4].to_string()));
                }
            }
        }
    }
    for v in labels.values_mut() {
        v.sort_by_key(|x| x.0);
    }
    // LP_PROFILE_RAW=<path>: the counts per address too (an entry address's count is the function's calls)
    if let Some(p) = std::env::var_os("LP_PROFILE_RAW") {
        let mut raw: BTreeMap<u32, u64> = BTreeMap::new();
        for (_, counts) in epochs {
            for &(pc, n) in counts {
                *raw.entry(pc).or_default() += n as u64;
            }
        }
        let t: String = raw.iter().map(|(a, n)| format!("{a:08x},{n}\n")).collect();
        std::fs::write(p, t).expect("raw profile");
    }
    let mut per: BTreeMap<(String, u32), (String, String, u64)> = BTreeMap::new();
    for (ovls, counts) in epochs {
        for &(pc, n) in counts {
            let module = ovls.iter().find(|&&(_, b, e)| pc >= b && pc < e).map_or("static".to_string(), |o| format!("ov{:02}", o.0));
            let (addr, name, src) = labels
                .get(&module)
                .and_then(|v| {
                    let i = v.partition_point(|x| x.0 <= pc);
                    (i > 0).then(|| v[i - 1].clone())
                })
                .unwrap_or((pc, format!("unlabelled_{pc:08x}"), "?".into()));
            let e = per.entry((module, addr)).or_insert((name, src, 0));
            e.2 += n as u64;
        }
    }
    let mut rows: Vec<_> = per.into_iter().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1 .2));
    let mut csv = String::from("module,addr,name,source,instructions\n");
    for ((m, a), (n, src, c)) in &rows {
        csv += &format!("{m},{a:08x},{n},{src},{c}\n");
    }
    std::fs::write(out, csv).expect("profile");
    let total: u64 = rows.iter().map(|r| r.1 .2).sum();
    println!("profile: {} functions ran as the game's code, {total} instructions -> {out}", rows.len());
    let mut by: BTreeMap<(String, String), (usize, u64)> = BTreeMap::new();
    for ((m, _), (_, src, c)) in &rows {
        let k = (if m == "static" { "static".into() } else { "overlays".into() }, src.clone());
        let e = by.entry(k).or_default();
        e.0 += 1;
        e.1 += c;
    }
    for ((m, src), (n, c)) in by {
        println!("  {m:8} {src:22} {n:5} functions {:6.2}% of instructions", 100.0 * c as f64 / total.max(1) as f64);
    }
}
