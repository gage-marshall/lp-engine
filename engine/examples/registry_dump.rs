//! Lists the runtime's registry: every game function address the engine knows, and whether it answers it
//! natively (a port or an SDK/hardware native), routes it to a subsystem, or leaves it pending (game code).
//! usage: cargo run --release --example registry_dump
use lp_engine::runtime::ext::{entry, Kind};

fn main() {
    let mut addrs: Vec<u32> = lp_engine::flow::addr::ALL.to_vec();
    addrs.extend(lp_engine::runtime::sdk::TRAPPED);
    addrs.extend(lp_engine::runtime::ext::extra());
    addrs.sort();
    addrs.dedup();
    for a in addrs {
        if let Some(e) = entry(a) {
            let kind = match e.kind {
                Kind::Native(_) => if lp_engine::runtime::sdk::entry(a).is_some() { "sdk-native" } else { "port" },
                Kind::Subsystem(_) => "subsystem",
                Kind::Pending => "pending",
            };
            println!("{a:08x},{},{kind}", e.name);
        } else {
            println!("{a:08x},?,unregistered");
        }
    }
}
