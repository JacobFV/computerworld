use cw_render::Renderer;
use cw_scene::{Color, Node, PatchOp, Rect, Scene, ScenePatch};
use serde::Serialize;
use std::{hint::black_box, time::Instant};
#[derive(Serialize)]
struct Measurement {
    name: String,
    run: usize,
    count: usize,
    p50_ns: u64,
    p95_ns: u64,
    mean_ns: f64,
    samples_ns: Vec<u64>,
}
fn measure(
    results: &mut Vec<Measurement>,
    name: &str,
    count: usize,
    runs: usize,
    mut f: impl FnMut(usize),
) {
    for i in 0..100 {
        f(i)
    }
    for run in 0..runs {
        let mut samples = Vec::with_capacity(count);
        for i in 0..count {
            let t = Instant::now();
            f(i);
            samples.push(t.elapsed().as_nanos() as u64)
        }
        let mut sorted = samples.clone();
        sorted.sort_unstable();
        let mean = samples.iter().map(|x| *x as f64).sum::<f64>() / count as f64;
        results.push(Measurement {
            name: name.into(),
            run,
            count,
            p50_ns: sorted[count / 2],
            p95_ns: sorted[(count * 95 / 100).min(count - 1)],
            mean_ns: mean,
            samples_ns: samples,
        });
    }
}
fn fixture(w: u32, h: u32) -> Scene {
    let mut s = Scene::new(w, h);
    for i in 0..100 {
        let x = (i % 4) * ((w / 4) as i32);
        let y = (i / 4) * 22;
        s.nodes.push(
            Node::text(
                i as u64,
                Rect::new(x, y, w / 4, 22),
                format!("Computer row {i:03}"),
                16,
                Color::BLACK,
            )
            .interactive(format!("row.{i}"), "button", format!("Row {i}")),
        );
    }
    s
}
fn main() {
    let count = std::env::var("BENCH_SAMPLES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000);
    let runs = std::env::var("BENCH_RUNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(5);
    let mut results = Vec::new();
    measure(&mut results, "scene.build_100", count, runs, |_| {
        black_box(fixture(1280, 720));
    });
    let s = fixture(1280, 720);
    measure(&mut results, "scene.hit_test_100", count, runs, |i| {
        black_box(s.hit_test((i % 1280) as i32, (i % 550) as i32));
    });
    for percent in [1, 10, 100] {
        let mut s = fixture(1280, 720);
        measure(
            &mut results,
            &format!("scene.patch_{percent}_percent"),
            count,
            runs,
            |i| {
                let ops = (0..percent)
                    .map(|n| {
                        let mut node = s.nodes[n].clone();
                        node.primitive = cw_scene::Primitive::Text {
                            text: format!("Version {i:05}"),
                            color: Color::BLACK,
                            size: 16,
                        };
                        PatchOp::Upsert(node)
                    })
                    .collect();
                black_box(
                    s.patch(ScenePatch {
                        base_revision: s.revision,
                        revision: s.revision + 1,
                        operations: ops,
                    })
                    .unwrap(),
                );
            },
        );
    }
    for (w, h) in [(640, 480), (1280, 720), (1920, 1080)] {
        let mut renderer = Renderer::new();
        let s = fixture(w, h);
        measure(
            &mut results,
            &format!("raster.full_{w}x{h}"),
            count.min(200),
            runs,
            |_| {
                black_box(renderer.render(&s));
            },
        );
    }
    for percent in [1, 10, 100] {
        let mut renderer = Renderer::new();
        let mut s = fixture(1280, 720);
        renderer.render(&s);
        measure(
            &mut results,
            &format!("raster.patch_{percent}_percent"),
            count.min(200),
            runs,
            |i| {
                let ops = (0..percent)
                    .map(|n| {
                        let mut node = s.nodes[n].clone();
                        node.primitive = cw_scene::Primitive::Text {
                            text: format!("Version {i:05}"),
                            color: Color::BLACK,
                            size: 16,
                        };
                        PatchOp::Upsert(node)
                    })
                    .collect();
                let damage = s
                    .patch(ScenePatch {
                        base_revision: s.revision,
                        revision: s.revision + 1,
                        operations: ops,
                    })
                    .unwrap();
                black_box(renderer.render_incremental(&s, &damage));
            },
        );
    }
    let out =
        std::env::var("BENCH_OUTPUT").unwrap_or("benchmarks/results/native-render.json".into());
    let meta = serde_json::json!({"kind":"native-release","engine_version":env!("CARGO_PKG_VERSION"),"arch":std::env::consts::ARCH,"os":std::env::consts::OS,"note":"Individual operation latencies; 100 warmups. Raster: capped 200 samples/run. Fontdue0.9.3 with bundled DejaVuSansMono, 100 text nodes. All allocations and frame drops in timed region."});
    std::fs::write(
        &out,
        serde_json::to_vec_pretty(&serde_json::json!({"meta":meta,"results":results})).unwrap(),
    )
    .unwrap();
    for m in &results {
        println!(
            "{} run={} p50={}ns p95={}ns",
            m.name, m.run, m.p50_ns, m.p95_ns
        );
    }
}
