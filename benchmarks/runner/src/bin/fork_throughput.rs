//! Fork throughput and retained bytes for a populated world.
//!
//! Forks a *populated* company-2026 world (driven through N real actor steps first),
//! never a fresh boot, and reports per-fork latency quantiles plus the marginal memory
//! a live fork retains. Memory is measured two ways at once: an exact heap accounting
//! wrapper around the system allocator, and the Linux RSS delta over the same window.
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};
use serde_json::{json, Value};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    hint::black_box,
    sync::atomic::{AtomicBool, AtomicI64, Ordering::Relaxed},
    time::Instant,
};

/// Exact live-heap accounting. Counting is enabled only around the memory phase, so the
/// latency samples are not charged two relaxed atomics per allocation. Inside a counting
/// window the program only builds new forks and frees its own temporaries, so the delta
/// is the bytes those forks retain.
struct Counting;
static LIVE: AtomicI64 = AtomicI64::new(0);
static COUNTING: AtomicBool = AtomicBool::new(false);
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() && COUNTING.load(Relaxed) {
            LIVE.fetch_add(layout.size() as i64, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if COUNTING.load(Relaxed) {
            LIVE.fetch_sub(layout.size() as i64, Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() && COUNTING.load(Relaxed) {
            LIVE.fetch_add(new_size as i64 - layout.size() as i64, Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() && COUNTING.load(Relaxed) {
            LIVE.fetch_add(layout.size() as i64, Relaxed);
        }
        p
    }
}
#[global_allocator]
static ALLOC: Counting = Counting;

fn rss() -> i64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap_or_default()
        .lines()
        .find(|s| s.starts_with("VmRSS:"))
        .and_then(|s| s.split_whitespace().nth(1))
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0)
        * 1024
}
fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}
fn quantile(sorted: &[u64], numerator: usize, denominator: usize) -> u64 {
    sorted[(sorted.len() * numerator / denominator).min(sorted.len() - 1)]
}

/// One realistic mixed action cycle: shell, a new file (the inode table really grows),
/// an editor keystroke, a virtual HTTP request and a browser navigation.
fn drive(world: &mut World, session: &str, editor_window: u64, from: usize, to: usize) {
    let act = |family: &str, op: &str, p: Value| ActionEnvelope::new(family, op, "alice-mac", p);
    for i in from..to {
        let actions = match i % 5 {
            0 => vec![act(
                "terminal.v1",
                "execute",
                json!({"command": format!("echo bench-{i} | cat")}),
            )],
            1 => vec![act(
                "filesystem.v1",
                "write",
                json!({"path": format!("/tmp/bench-{i}.txt"), "content": format!("populated-{i}")}),
            )],
            2 => vec![
                // The previous cycle ended in the browser, so the editor is focused again
                // before typing; the typed text stays, so the document really grows.
                act("application.v1", "focus", json!({"window": editor_window})),
                act("keyboard.v1", "type", json!({"text": "x"})),
            ],
            3 => vec![act(
                "http.v1",
                "request",
                json!({"method":"GET","url":"http://intranet.internal/"}),
            )],
            _ => vec![act(
                "browser.v1",
                "navigate",
                json!({"url":"http://intranet.internal/"}),
            )],
        };
        let result = world.step(session, actions).unwrap();
        assert!(
            result.outcomes.iter().all(|o| o.success),
            "populating step {i} failed: {:?}",
            result.outcomes
        );
    }
}

// Reject misleading debug timings while still allowing cargo test to compile the runner.
#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(
        !cfg!(debug_assertions),
        "Benchmark binaries require --release"
    );
    let samples = env_usize("BENCH_SAMPLES", 1000);
    let warmup = env_usize("BENCH_WARMUP", 100);
    let runs = env_usize("BENCH_RUNS", 5);
    let live_forks = env_usize("BENCH_LIVE_FORKS", 1000);
    let mutations = env_usize("BENCH_MUTATION_PROBES", 5);
    // A portable export of a heavily populated world is gigabytes of JSON; only the
    // smaller levels are exported, and the field is null above this cutoff.
    let export_max = env_usize("BENCH_EXPORT_MAX_STEPS", 1000);
    let levels: Vec<usize> = std::env::var("BENCH_FORK_STEPS")
        .unwrap_or_else(|_| "0,250,1000,4000".into())
        .split(',')
        .map(|s| {
            s.trim()
                .parse()
                .expect("BENCH_FORK_STEPS is a step count list")
        })
        .collect();
    let mut results = vec![];
    for run in 0..runs {
        let mut world = World::new(reference_world(), 2026).unwrap();
        let session = world
            .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
            .unwrap();
        // The editor keystrokes need a window; launched once, outside every timed region.
        let launch = world
            .step(
                &session,
                vec![ActionEnvelope::new(
                    "application.v1",
                    "launch",
                    "alice-mac",
                    json!({"kind":"editor"}),
                )],
            )
            .unwrap();
        assert!(launch.outcomes.iter().all(|o| o.success));
        let editor_window = *launch.outcomes[0]
            .effect
            .as_ref()
            .and_then(|e| e.windows_opened.first())
            .expect("editor launch opens a window");
        let mut driven = 0usize;
        for &steps in &levels {
            drive(&mut world, &session, editor_window, driven, steps);
            driven = steps;
            let snapshot = world.snapshot();
            let hash = world.state_hash().unwrap();

            // Latency: each fork is built and dropped, so steady-state memory is flat.
            for _ in 0..warmup {
                black_box(world.fork(&snapshot).unwrap());
            }
            let mut latency = Vec::with_capacity(samples);
            for _ in 0..samples {
                let start = Instant::now();
                let fork = world.fork(&snapshot).unwrap();
                latency.push(start.elapsed().as_nanos() as u64);
                black_box(fork);
            }
            let mut sorted = latency.clone();
            sorted.sort_unstable();
            let mean = latency.iter().map(|v| *v as f64).sum::<f64>() / samples as f64;

            // Where a fork's time goes: `fork` validates the checkpoint and the `restore`
            // inside it validates the same checkpoint again. Each validation walks the whole
            // world definition and compares it field by field against the baseline, so these
            // two probes bound the part of fork latency that is not state copying.
            let mut validate_ns = vec![];
            let mut compare_ns = vec![];
            for _ in 0..20 {
                let definition = world.definition();
                let start = Instant::now();
                definition.validate().unwrap();
                validate_ns.push(start.elapsed().as_nanos() as u64);
                let start = Instant::now();
                black_box(definition == world.definition());
                compare_ns.push(start.elapsed().as_nanos() as u64);
            }

            // Retained bytes: hold `live_forks` forks at once. The vector is reserved
            // before the baseline reading so its own spine is not charged to the forks.
            let mut held: Vec<World> = Vec::with_capacity(live_forks);
            let rss_before = rss();
            COUNTING.store(true, Relaxed);
            let heap_before = LIVE.load(Relaxed);
            for _ in 0..live_forks {
                held.push(world.fork(&snapshot).unwrap());
            }
            let heap_live = LIVE.load(Relaxed) - heap_before;
            let rss_live = rss() - rss_before;
            drop(held);
            let heap_after_drop = LIVE.load(Relaxed) - heap_before;
            let rss_after_drop = rss() - rss_before;

            // Diverging a branch: the first write into a fork splits every copy-on-write
            // root it touches; the second write into the same fork owns them already.
            // One fork is alive at a time, so peak memory stays bounded.
            let mut first_write = vec![];
            let mut second_write = vec![];
            let mut first_write_ns = vec![];
            let mut second_write_ns = vec![];
            for probe in 0..mutations {
                let mut fork = world.fork(&snapshot).unwrap();
                for (round, bytes, times) in [
                    (0, &mut first_write, &mut first_write_ns),
                    (1, &mut second_write, &mut second_write_ns),
                ] {
                    let before = LIVE.load(Relaxed);
                    let start = Instant::now();
                    let result = fork
                        .step(
                            &session,
                            vec![ActionEnvelope::new(
                                "filesystem.v1",
                                "write",
                                "alice-mac",
                                json!({"path": format!("/tmp/branch-{round}.txt"),
                                       "content": format!("branch-{probe}-{round}")}),
                            )],
                        )
                        .unwrap();
                    times.push(start.elapsed().as_nanos() as u64);
                    bytes.push(LIVE.load(Relaxed) - before);
                    assert!(result.outcomes.iter().all(|o| o.success));
                }
            }

            // What the fork shares rather than copies: `trajectory()` deep-copies the
            // event log, so its cost is that log's live size for this level of state.
            let before = LIVE.load(Relaxed);
            let events = world.trajectory();
            let trajectory_bytes = LIVE.load(Relaxed) - before;
            let event_count = events.len();
            drop(events);
            COUNTING.store(false, Relaxed);
            assert_eq!(
                hash,
                world.state_hash().unwrap(),
                "forking or mutating a fork must not disturb the parent"
            );
            let median = |v: &Vec<i64>| {
                let mut v = v.clone();
                v.sort_unstable();
                v[v.len() / 2]
            };
            let median_ns = |v: &Vec<u64>| {
                let mut v = v.clone();
                v.sort_unstable();
                v[v.len() / 2]
            };
            let portable = (steps <= export_max).then(|| world.export_snapshot().unwrap().len());

            // The control for the two rows above: the same write in the *parent*, with the
            // checkpoint dropped so nothing is shared and no copy-on-write root can split.
            // Whatever this costs is the price of a step in a world this large, not a fork
            // tax. It leaves three extra steps of state behind before the next level.
            drop(snapshot);
            let mut unshared_ns = vec![];
            let mut unshared_bytes = vec![];
            COUNTING.store(true, Relaxed);
            for round in 0..3 {
                let before = LIVE.load(Relaxed);
                let start = Instant::now();
                let result = world
                    .step(
                        &session,
                        vec![ActionEnvelope::new(
                            "filesystem.v1",
                            "write",
                            "alice-mac",
                            json!({"path": format!("/tmp/unshared-{round}.txt"),
                                   "content": format!("unshared-{round}")}),
                        )],
                    )
                    .unwrap();
                unshared_ns.push(start.elapsed().as_nanos() as u64);
                unshared_bytes.push(LIVE.load(Relaxed) - before);
                assert!(result.outcomes.iter().all(|o| o.success));
            }
            COUNTING.store(false, Relaxed);

            let row = json!({
                "steps": steps,
                "run": run,
                "count": samples,
                "warmup": warmup,
                "p50_ns": quantile(&sorted, 1, 2),
                "p95_ns": quantile(&sorted, 95, 100),
                "p99_ns": quantile(&sorted, 99, 100),
                "min_ns": sorted[0],
                "max_ns": sorted[sorted.len() - 1],
                "mean_ns": mean,
                "forks_per_second": 1e9 / mean,
                "live_forks": live_forks,
                "heap_bytes_per_fork": heap_live as f64 / live_forks as f64,
                "rss_bytes_per_fork": rss_live as f64 / live_forks as f64,
                "heap_bytes_returned_on_drop": heap_after_drop,
                "rss_bytes_retained_after_drop": rss_after_drop,
                "mutation_probes": mutations,
                "first_write_heap_bytes": median(&first_write),
                "first_write_ns": median_ns(&first_write_ns),
                "second_write_heap_bytes": median(&second_write),
                "second_write_ns": median_ns(&second_write_ns),
                "first_write_heap_bytes_samples": first_write,
                "second_write_heap_bytes_samples": second_write,
                "trajectory_clone_heap_bytes": trajectory_bytes,
                "unshared_write_ns": median_ns(&unshared_ns),
                "unshared_write_heap_bytes": median(&unshared_bytes),
                "unshared_write_ns_samples": unshared_ns,
                "definition_validate_ns": median_ns(&validate_ns),
                "definition_compare_ns": median_ns(&compare_ns),
                "definition_json_bytes": include_str!("../../../../worlds/company-2026/world.json").len(),
                "world_struct_bytes": std::mem::size_of::<World>(),
                "portable_snapshot_bytes": portable,
                "events": event_count,
                "state_hash": hash,
                "samples_ns": latency,
            });
            println!(
                "steps={steps} run={run} p50={}ns p99={}ns forks/s={:.0} heap/fork={:.0}B rss/fork={:.0}B first_write={}B/{}ns second_write={}B/{}ns unshared_write={}B/{}ns trajectory={}B",
                quantile(&sorted, 1, 2),
                quantile(&sorted, 99, 100),
                1e9 / mean,
                heap_live as f64 / live_forks as f64,
                rss_live as f64 / live_forks as f64,
                median(&first_write),
                median_ns(&first_write_ns),
                median(&second_write),
                median_ns(&second_write_ns),
                median(&unshared_bytes),
                median_ns(&unshared_ns),
                trajectory_bytes
            );
            results.push(row);
        }
    }
    let out = std::env::var("BENCH_OUTPUT")
        .unwrap_or_else(|_| "benchmarks/results/fork-throughput.json".into());
    std::fs::write(
        out,
        serde_json::to_vec_pretty(&json!({
            "meta": {
                "seed": 2026,
                "world": "company-2026",
                "mode": "release",
                "note": "Each fork is of a world already driven through `steps` real actor steps \
                         (shell, a new file, an editor keystroke, virtual HTTP, browser navigation), \
                         never a fresh boot. Latency samples build and drop one fork each. Retained \
                         bytes hold `live_forks` forks at once: heap_bytes_per_fork is an exact \
                         live-allocation delta from a counting global allocator divided by the number \
                         of live forks, rss_bytes_per_fork the Linux resident-set delta over the same \
                         window divided the same way. first_write/second_write are the heap and wall \
                         cost of the first and second file write inside one fresh fork, median of \
                         `mutation_probes`, with one fork alive at a time. trajectory_clone_heap_bytes \
                         is the live size of one deep copy of the event log at this level of state. \
                         unshared_write is the control: the same write in the parent after the \
                         checkpoint is dropped, so nothing is shared, which separates a fork's \
                         copy-on-write split from what a step costs in a world this large. \
                         Allocation counting is off during the latency phase. The parent state hash \
                         is asserted unchanged before the control writes, which add three steps of \
                         state beyond each nominal level."
            },
            "results": results
        }))
        .unwrap(),
    )
    .unwrap();
}
