use computerworld::{reference_world, World};
use serde_json::json;
use std::time::Instant;
fn rss() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|s| s.starts_with("VmRSS:"))
        .and_then(|s| s.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0)
        * 1024
}
fn main() {
    let definition = reference_world();
    let mut rows = vec![];
    for count in [1, 100, 1000] {
        let before = rss();
        let start = Instant::now();
        let worlds: Vec<_> = (0..count)
            .map(|seed| World::new(definition.clone(), seed).unwrap())
            .collect();
        rows.push(json!({"count":count,"create_total_ns":start.elapsed().as_nanos() as u64,"rss_before":before,"rss_live":rss(),"checksum":worlds[0].state_hash().unwrap()}));
        drop(worlds);
        rows.last_mut().unwrap()["rss_after_drop"] = json!(rss());
    }
    std::fs::write("benchmarks/results/many-worlds.json",serde_json::to_vec_pretty(&json!({"rows":rows,"note":"RSS is Linux process resident memory, allocator retained pages included. Serial creation; no renderer allocation."})).unwrap()).unwrap();
}
