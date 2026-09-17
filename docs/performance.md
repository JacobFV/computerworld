# Performance

This report is populated from the reproducible runners in `benchmarks/`. Final
measurements are pending the coordinated quiet measurement window; no smoke-run
numbers are release performance claims.

## Method

Persistent release runtimes, five runs, 100 warmups and 1,000 individual samples
per short workload. Raster loops use 200 samples/run, portable snapshot encoding
and decoding 200, and browser PNG captures 30 samples/run with three warmups.
Tables use the median of the five per-run p50 and p95 values. Raw samples and
per-run dispersion remain in `benchmarks/results/`; throughput uses the arithmetic
mean latency within a run. This does not treat batch averages as operation p95.

Native runs are pinned to CPU 0 on this ARM64 host. Machine/compiler/browser,
world, lockfile and font/bundle hashes are recorded in `results/machine.json`.
Initialization, boundary conversion, scene construction, rasterization and PNG
capture are distinct workloads. The native world runner measures the full actor
boundary including outcome construction, observation projection and journaling.
Neither structured observations nor scene generation requests rasterization.

## Renderer profiling and optimization

Linux `perf` is blocked by `perf_event_paranoid=4`; we did not change host policy.
Temporary instrumentation on a disposable renderer copy measured sorting,
clearing, text preparation and node raster stages. The source of that instrumentation
and its results are included. Before optimization, node raster consumed about 95%
of full-frame wall time. Identical old/new patch damage also painted the same area
twice, making a 100% incremental update slower than full rendering.

The resulting optimization caches nonzero text spans, specializes identity text
and opaque rectangle drawing, and coalesces redundant damage. Renderer semantic
contracts and golden frames remain unchanged. Cache limits account for retained
span storage. Authoritative before/after p50/p95 use uninstrumented binaries.

## Chromium comparisons

Three distinct measures avoid misleading ratios:

- DOM mutation plus forced layout: no claim that this includes rasterization.
- Wasm patch, software raster and RGBA copy: no ratio against DOM layout alone.
- PNG delivered to the automation caller: both paths produce PNG, including
  encoding and transfer. We measure both the same Chrome screenshot API on DOM
  and canvas and the custom canvas export path (`toDataURL`) versus DOM screenshot.
  The latter is capture-pipeline savings, not isolated renderer speedup.

The primitive fixture uses identical content, dimensions and bundled font source;
Chrome and fontdue differ in glyph antialiasing. The real predecessor mail fixture
is `synthux-mail-mock`'s inbox, normalized to that font. An additional migrated
scene captures its per-character text geometry and background boxes. It omits CSS
rounded corners, shadows and emoji fallback, so it is an approximate workload,
not a pixel-identical HTML compatibility or matched-render speedup result. Review
images are included beside the raw results.

## Reproduction

See [benchmark commands](../benchmarks/README.md). Do not run performance captures
concurrently with builds, tests or other benchmark processes. All data is local;
Chrome only serves static package assets, and the simulator uses its synthetic
network. Python and Node bindings use the same Rust runtime, without a simulation
subprocess per step.
