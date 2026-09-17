use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};
use serde_json::{json, Value};
use std::{hint::black_box, time::Instant};
// Reject misleading debug timings while still allowing cargo test to compile the runner.
#[allow(clippy::assertions_on_constants)]
fn main() {
    assert!(
        !cfg!(debug_assertions),
        "Benchmark binaries require --release"
    );
    let count = std::env::var("BENCH_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let runs = std::env::var("BENCH_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let mut results = vec![];
    for name in [
        "terminal.pwd",
        "terminal.pipe",
        "filesystem.write_read",
        "network.http",
        "browser.navigate",
        "application.edit",
        "observe.structured",
        "scene.structured",
        "reset.same_seed",
        "reset.dirty_same_seed",
        "snapshot.handle",
        "snapshot.fork",
        "snapshot.first_write",
        "snapshot.encode",
        "snapshot.decode",
    ] {
        if let Ok(filter) = std::env::var("BENCH_FILTER") {
            if !name.contains(&filter) {
                continue;
            }
        }
        for run in 0..runs {
            let mut world = World::new(reference_world(), 2026).unwrap();
            let session = world
                .environment(EnvironmentConfig::desktop("alice", "alice-mac"))
                .unwrap();
            let act =
                |family: &str, op: &str, p: Value| ActionEnvelope::new(family, op, "alice-mac", p);
            let step = |world: &mut World, actions: Vec<ActionEnvelope>| {
                let x = world.step(&session, actions).unwrap();
                assert!(
                    x.outcomes.iter().all(|x| x.success),
                    "{name}: {:?}",
                    x.outcomes
                );
                if name == "filesystem.write_read" {
                    let value = &x.outcomes.last().unwrap().value;
                    assert!(value["content"].as_str().unwrap().starts_with("value-"));
                }
                if name == "terminal.pipe" {
                    assert!(x.outcomes[0].value["stdout"]
                        .as_str()
                        .unwrap()
                        .contains("benchmark"));
                }
                if name == "network.http" {
                    assert_eq!(x.outcomes[0].value["status"], 200);
                }
                black_box(x);
            };
            if name == "application.edit" {
                step(
                    &mut world,
                    vec![act("application.v1", "launch", json!({"kind":"editor"}))],
                );
            }
            if ["observe.structured", "scene.structured"].contains(&name) {
                step(
                    &mut world,
                    vec![act(
                        "browser.v1",
                        "navigate",
                        json!({"url":"http://intranet.internal/"}),
                    )],
                );
            }
            let snapshot = world.snapshot();
            let encoded = world.export_snapshot().unwrap();
            let n = if name.starts_with("snapshot.en") || name.starts_with("snapshot.de") {
                count.min(200)
            } else {
                count
            };
            let mut samples = Vec::with_capacity(n);
            for i in 0..n + 100 {
                if name == "reset.dirty_same_seed" {
                    step(
                        &mut world,
                        vec![act(
                            "filesystem.v1",
                            "write",
                            json!({"path":"/tmp/dirty-reset", "content":format!("mutation-{i}")}),
                        )],
                    );
                }
                let start = Instant::now();
                match name {
                    "terminal.pwd" => step(
                        &mut world,
                        vec![act("terminal.v1", "execute", json!({"command":"pwd"}))],
                    ),
                    "terminal.pipe" => step(
                        &mut world,
                        vec![act(
                            "terminal.v1",
                            "execute",
                            json!({"command":"echo benchmark | cat"}),
                        )],
                    ),
                    "filesystem.write_read" => step(
                        &mut world,
                        vec![
                            act(
                                "filesystem.v1",
                                "write",
                                json!({"path":"/tmp/bench.txt","content":format!("value-{i}")}),
                            ),
                            act("filesystem.v1", "read", json!({"path":"/tmp/bench.txt"})),
                        ],
                    ),
                    "network.http" => step(
                        &mut world,
                        vec![act(
                            "http.v1",
                            "request",
                            json!({"method":"GET","url":"http://intranet.internal/"}),
                        )],
                    ),
                    "browser.navigate" => step(
                        &mut world,
                        vec![act(
                            "browser.v1",
                            "navigate",
                            json!({"url":"http://intranet.internal/"}),
                        )],
                    ),
                    "application.edit" => {
                        step(
                            &mut world,
                            vec![
                                act("keyboard.v1", "type", json!({"text":"x"})),
                                act("keyboard.v1", "key", json!({"key":"Backspace"})),
                            ],
                        );
                    }
                    "observe.structured" => {
                        black_box(world.observe(&session).unwrap());
                    }
                    "scene.structured" => {
                        black_box(world.scene(&session, 1280, 720).unwrap());
                    }
                    "reset.same_seed" | "reset.dirty_same_seed" => world.reset(2026).unwrap(),
                    "snapshot.handle" => {
                        black_box(world.snapshot());
                    }
                    "snapshot.fork" => {
                        black_box(world.fork(&snapshot).unwrap());
                    }
                    "snapshot.first_write" => {
                        let mut branch = world.fork(&snapshot).unwrap();
                        step(
                            &mut branch,
                            vec![act(
                                "filesystem.v1",
                                "write",
                                json!({"path":"/tmp/bench.txt","content":"branch"}),
                            )],
                        );
                    }
                    "snapshot.encode" => {
                        black_box(world.export_snapshot().unwrap());
                    }
                    "snapshot.decode" => world.import_snapshot(&encoded).unwrap(),
                    _ => unreachable!(),
                };
                let elapsed = start.elapsed().as_nanos() as u64;
                if name == "reset.dirty_same_seed" {
                    assert!(world
                        .runtime()
                        .read_file("alice-mac", "/tmp/dirty-reset")
                        .is_err());
                }
                if i >= 100 {
                    samples.push(elapsed)
                }
            }
            let mut sorted = samples.clone();
            sorted.sort_unstable();
            let mean = samples.iter().map(|v| *v as f64).sum::<f64>() / n as f64;
            let row = json!({"name":name,"run":run,"count":n,"p50_ns":sorted[n/2],"p95_ns":sorted[n*95/100],"mean_ns":mean,"samples_ns":samples});
            println!("{name}: p50={} p95={}", sorted[n / 2], sorted[n * 95 / 100]);
            results.push(row);
        }
    }
    let out =
        std::env::var("BENCH_OUTPUT").unwrap_or("benchmarks/results/native-world.json".into());
    std::fs::write(out,serde_json::to_vec_pretty(&json!({"meta":{"seed":2026,"world":"company-2026","warmup":100,"mode":"release","note":"Full actor step includes projection, outcomes, trace/journal. Persistent sessions across samples. Snapshot held during operations to measure real copy-on-write. Rasterization never requested."},"results":results})).unwrap()).unwrap();
}
