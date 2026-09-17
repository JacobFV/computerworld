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
taskset -c 0 target/release/world
taskset -c 0 target/release/cw-benchmarks
taskset -c 0 target/release/many_worlds
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
