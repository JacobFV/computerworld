# Performance

Measured on September 17, 2026, Linux ARM64, Cortex-X925 CPU 19 (3.9 GHz
maximum), Rust 1.97.1 release builds and Chrome 151. Native actor terminal steps
cost **3.79 µs p50**, dirty resets **1.44 µs**, and warm 1280×720 frames **0.522 ms**.

The renderer optimization improved native full-frame p50 **3.11×** and
100% patch rendering **7.21×**. Against a Chromium DOM, the format-matched custom
PNG capture pipeline achieved **2.70× p50 / 1.81× p95** improvement. The proposed
3× predecessor capture target was **not met**. The same Chromium screenshot API
on both representations produced essentially no speedup. We have not established
a general renderer-only speedup over Chromium.

[Raw samples and summary](../benchmarks/results/summary.json),
[machine and artifact hashes](../benchmarks/results/machine.json), and
[commands](../benchmarks/README.md) accompany the report. Final refreshed bindings
are measured, including the cached reset and exact integer conversion changes.
Device-console work added after measurement is not included. These are results
on one host, not universal hardware guarantees.

## Method

Persistent release runtimes, five runs, 100 warmups and 1,000 individual samples
per short workload. Raster loops use 200 samples/run, portable snapshot encoding
and decoding 200, and browser PNG captures 30 samples/run with three warmups.
Tables use the median of the five per-run p50 and p95 values. Raw samples and
per-run dispersion remain in `benchmarks/results/`; throughput uses the arithmetic
mean latency within a run. This does not treat batch averages as operation p95.

Native runs are pinned to CPU 19 on this ARM64 host. Machine/compiler/browser,
world, lockfile and font/bundle hashes are recorded in `results/machine.json`.
Initialization, boundary conversion, scene construction, rasterization and PNG
capture are distinct workloads. The native world runner measures the full actor
boundary including outcome construction, observation projection and journaling.
Neither structured observations nor scene generation requests rasterization.

## Native world results

All values are **microseconds**. Each short row has 5,000 measured operations;
portable snapshot encode/decode have 1,000. These use the rich company world and
full journaling. The initial snapshot is held during each workload. File, pipe
and HTTP results are asserted to prevent no-op success from counting as work.

| Workload | p50 | p95 |
|---|---:|---:|
| Terminal `pwd`, full actor step | 3.792 | 4.752 |
| Terminal parse + pipe | 4.448 | 4.976 |
| File write + read, two actor actions | 7.056 | 7.664 |
| Virtual HTTP, full actor step | 15.041 | 16.960 |
| Synthetic browser navigation through network | 23.280 | 26.560 |
| Editor type + backspace, two actions | 4.224 | 5.136 |
| Structured actor observation | 4.320 | 4.496 |
| Browser scene / layout, no raster | 1.936 | 2.016 |
| Already-clean same-seed reset | 0.096 | 0.112 |
| Snapshot handle | 0.448 | 0.528 |
| Fork from initial snapshot | 10.912 | 11.152 |
| Fork + first file mutation | 19.408 | 19.696 |
| Portable initial snapshot encode | 26.400 | 27.104 |
| Portable initial snapshot decode | 103.360 | 106.368 |

A dirty same-seed reset is **1.440 µs p50 / 1.504 µs p95**. Every iteration mutates a file before the timer,
then resets inside the timer and verifies the file disappeared after timing.
The 0.096 µs clean-reset row intentionally measures an already-reset world;
it should not be presented as the cost of discarding a populated episode.
Snapshot encode/decode rows use the initialized snapshot; populated trajectory
size naturally increases portable serialization cost. Fork + first write includes
both operations rather than claiming an isolated mutation latency.

## Native renderer results

A 100-text-node fixture, integer layout and the bundled DejaVuSansMono font.
Full render includes returning/dropping an owned RGBA copy. Incremental render
returns the retained framebuffer; each before/after pair uses the identical API.
Values are **microseconds**, with 1,000 samples per warmed raster row.

| Workload | Before p50 | After p50 | Before p95 | After p95 | Median improvement |
|---|---:|---:|---:|---:|---:|
| Full 640×480 | 772.59 | 354.51 | 781.36 | 368.02 | 2.18× |
| Full 1280×720 | 1623.70 | 522.06 | 1715.22 | 559.39 | 3.11× |
| Full 1920×1080 | 2586.03 | 792.43 | 2781.28 | 897.39 | 3.26× |
| Patch + raster 1% | 40.59 | 18.16 | 42.26 | 18.91 | 2.24× |
| Patch + raster 10% | 310.70 | 53.65 | 319.49 | 56.18 | 5.79× |
| Patch + raster 100% | 3075.46 | 426.45 | 3124.79 | 444.91 | 7.21× |

Cold renderer construction plus a 1280×720 frame costs **10.389 ms p50 /
10.739 ms p95** (150 samples). This includes parsing the bundled font; reuse a
persistent renderer for repeated frames. Scene construction is 10.560 µs,
hit testing is 1.120 µs, and scene-only 1%/10%/100% patches are
8.320 / 10.624 / 20.096 µs p50. None of those structured workloads rasterizes.

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

## Browser output and predecessor baseline

The next table is **milliseconds**, with 150 captures per row. Inputs and outputs
are the same 1280×720 primitive/text fixture; both final outputs are PNG returned
to the Node caller. The custom incremental path combines update, raster, canvas
upload, PNG encoding and transfer into one RPC. The DOM path needs the browser's
screenshot API. These are end-to-end capture costs, including their required
transport, not isolated compositor benchmarks.

| Workload | p50 | p95 |
|---|---:|---:|
| DOM update + Chrome PNG capture | 33.518 | 51.437 |
| Wasm full update + same Chrome PNG capture | 33.820 | 52.491 |
| Wasm full update + canvas PNG export, two RPCs | 14.798 | 28.856 |
| Wasm incremental update + PNG export, one RPC | 12.422 | 28.431 |

DOM mutation plus forced layout costs 5 / 40 / 345 µs p50 for 1% / 10% / 100%
changes. Wasm patch + raster + full RGBA boundary copy costs 1.595 / 1.660 / 2.450 ms.
Those are different operations and are **not divided into a speedup ratio**.
Browser timers are quantized; the smallest DOM result is near timer resolution.
The RGBA-copy and canvas/export boundary is a remaining optimization target.

The real predecessor mail visual fixture captures in 36.461 / 63.180 ms p50/p95.
Its migrated geometry reconstruction captures in 49.975 / 54.612 ms. The latter
uses per-character nodes to preserve predecessor positions, omits decorations and
emoji fallback, and is not a faithful same-pixel screen. It does **not** establish
a renderer speedup. Reviewed [DOM](../benchmarks/results/predecessor-mail.png) and
[reconstructed scene](../benchmarks/results/migrated-mail.png) images are retained.
These are visual-state fixtures, not a service mutation end-to-end benchmark.

Persistent SCE runtime operational baselines (microseconds):

| Workload | p50 | p95 |
|---|---:|---:|
| Terminal pwd | 9.408 | 11.216 |
| Terminal pipe | 18.816 | 23.008 |
| Shell file write + read | 150.337 | 212.337 |
| Virtual HTTP | 45.280 | 52.064 |

SCE's host-backed VFS, reference topology, shell projection and tracing differ from
Computerworld. These are useful operational reference points, not isolated Rust
versus JavaScript language comparisons. SCE initialization is recorded separately
in its raw result. No per-step subprocess is used in either benchmark.

## Bindings and memory

Binding values are **microseconds**. Node executes the generated Wasm module;
Python executes the installed PyO3 wheel. These terminal-only configurations have
a smaller observation projection than the native desktop fixture, so cross-table
ratios would not isolate binding overhead.

| Workload | p50 | p95 |
|---|---:|---:|
| Node Wasm terminal step | 12.816 | 15.376 |
| Node Wasm ten-action batch | 67.456 | 99.697 |
| Node Wasm observe | 1.344 | 1.568 |
| Node Wasm already-clean reset | 0.256 | 0.288 |
| Node Wasm snapshot handle | 1.008 | 1.296 |
| Node Wasm fork | 18.336 | 19.200 |

| Workload | p50 | p95 |
|---|---:|---:|
| Python terminal step | 8.096 | 8.640 |
| Python ten-action batch | 43.825 | 48.336 |
| Python observe | 1.280 | 1.344 |
| Python already-clean reset | 0.240 | 0.256 |
| Python snapshot handle | 0.656 | 0.704 |
| Python fork | 11.424 | 11.712 |

Actual Chrome Wasm terminal execution is **15 µs p50 / 20 µs p95** over 5,000
steps. One cold Node module load/instantiation took **17.264 ms**; this is one
observation, not a p50. Browser world construction warmed from 24.395 ms on the
first run to 1.495–10.450 ms on later runs; five points do not establish a robust
cold-start percentile.

| Packaged artifact | Raw bytes | Gzip bytes |
|---|---:|---:|
| Browser Wasm, standard services + renderer/font | 5,444,132 | 1,651,674 |
| Browser JS glue | 31,703 | 6,008 |
| Bundled font, already embedded in Wasm | 343,140 | 203,155 |

The font row is an accounting breakdown, not an additional required demo fetch.
Browser linear memory grew from **19.0 MiB to 20.5625 MiB** across five
create/1,100-step/free cycles; Wasm memory retains its high-water allocation after
handles are freed. That is a bounded sample, not proof of zero growth over all
workloads. Native 1 / 100 / 1,000 independent, unrendered company worlds occupied
3.37 / 21.97 / 190.88 MiB RSS and took 1.01 / 28.71 / 246.05 ms total to create in
one serial trial each. RSS after dropping them retained allocator pages; no claim
of OS page reclamation is made.

## Limits and next measurements

This is a single ARM64 host and single pinned performance core. No release target
or claim applies universally to x86, mobile or other browser engines. Quiet
windows remove project builds/tests, not every unrelated host process. Per-run
min/max p50 and all raw samples are in the summary; median tables do not hide
run-zero warmup variability in the source data. No allocator event count, hardware
counter profile, multithread throughput or full CSS/browser fidelity is claimed.

The next useful work is reducing full-frame Wasm boundary copies, sharing parsed
immutable font data across renderers, measuring large populated snapshots and
traces, and extending the fair migrated-screen corpus. The 3× predecessor capture
goal remains open; it must not be substituted with the separate 3.11× native
before/after improvement.

## Reproduction

See [benchmark commands](../benchmarks/README.md). Do not run performance captures
concurrently with builds, tests or other benchmark processes. All data is local;
Chrome only serves static package assets, and the simulator uses its synthetic
network. Python and Node bindings use the same Rust runtime, without a simulation
subprocess per step.
