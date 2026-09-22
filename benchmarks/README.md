# Reproducible benchmarks

All timings use persistent runtime sessions. Run only after concurrent builds/tests
have stopped. Defaults: five runs, 100 warmups and 1,000 measured operations per
short workload; raw individual latency samples are retained. Expensive raster and
cold-start cases explicitly declare their smaller sample count.

Predecessor (checkout and install SCE from the commit in the provenance report):

```sh
SCE_SOURCE=/tmp/computerworld-sources/synthetic-computer-environment
benchmarks/run-predecessor.sh
```

`BENCH_SAMPLES`, `BENCH_RUNS`, and `BENCH_OUTPUT` override sample count, independent
runs and output location. Times are nanoseconds; JSON includes p50/p95, means and
raw samples. Quantiles describe individual operations, never a batched average.
Host-backed predecessor VFS and richer predecessor shell differ from the pure
in-memory Rust VFS; this is an operational comparison, not isolated language cost.

Browser comparisons separate DOM mutation/layout, PNG capture, CPU rasterization,
and encoding. Chrome screenshot latency is never described as isolated raster
cost or divided by raw RGBA time to claim a rendering speedup.

Native, bindings and browser runners:

```sh
cargo build --release -p cw-benchmarks
taskset -c 19 target/release/world
taskset -c 19 target/release/cw-benchmarks
taskset -c 19 target/release/many_worlds
taskset -c 19 target/release/fork_throughput   # 37 min; BENCH_RUNS=2 BENCH_FORK_STEPS=1000 for the headline level
node benchmarks/bindings.mjs
.venv/bin/python benchmarks/python_binding.py
node benchmarks/browser-render.mjs
```

Generate Wasm/Python packages first using the scripts documented in binding usage.
`PLAYWRIGHT_MODULE` and `CHROME` select installed automation and Chromium paths.
The browser runner serves local static assets (not environment semantics), then
executes the canonical Wasm runtime. Browser renderer tests use cross-origin
isolation for high-resolution timers. Expensive captures use 30 samples/run and
three warmups; renderer uses 200 samples/run and 100 warmups. Baseline mail uses
the actual predecessor `renderFolder('inbox')`, including its original CSS and
content. The migrated mail layout is a reconstruction, explicitly not a claim of
pixel-identical HTML/CSS compatibility.

`profiling/` preserves the temporary stage-timing instrumentation used because
this machine disallows `perf` (`perf_event_paranoid=4`). Copy the scene/render
crates and runner into a disposable workspace, instrument the renderer copy,
and use `profile.rs` as a binary. Instrumented timings identify stages; the
uninstrumented release runner supplies final p50/p95. Never ship the instrumented
renderer as the simulator.

The recorded pre-optimization renderer is commit `2c530b1`; its original runner is
in `c8396d6`. Rebuild it without reverting the workspace:

```sh
benchmarks/build-render-baseline.sh /tmp/computerworld-render-baseline
RENDER_BASELINE_BINARY=/tmp/computerworld-render-baseline/target/release/cw-benchmarks benchmarks/run-all.sh
```

`run-all.sh` assumes current release binaries and both binding packages are already
built, and must run in an exclusive measurement window. To measure a reset after
mutation (mutation and postcondition check outside the timed region):

```sh
BENCH_FILTER=dirty BENCH_OUTPUT=benchmarks/results/native-dirty-reset.json taskset -c 19 target/release/world
```

The clean-reset row intentionally resets an already-reset environment. Do not
substitute it for dirty-reset performance. `summarize.py` preserves per-run p50
ranges and operation samples. `machine.py` captures executable, world, lockfile,
font and Wasm artifact hashes. Linux RSS includes allocator-retained memory;
Wasm linear memory is a high-water allocation and does not shrink on handle free.

## Recorded results and local runs

`results/` contains selected, committed measurements supporting the performance
report; these are reference evidence, not a cache. Use
`BENCH_OUTPUT=benchmarks/results/local-<run>/result.json` for exploratory runs (the
`local-*` directories are ignored), or write to `target/benchmarks/`. Promote a
result deliberately with its workload, machine metadata and interpretation.
