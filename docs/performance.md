# Performance

Measured on September 17, 2026, Linux ARM64, Cortex-X925 CPU 19 (3.9 GHz
maximum), Rust 1.97.1 release builds and Chrome 151. Native actor terminal steps
cost **3.84 µs p50**, dirty resets **1.52 µs**, and warm 1280×720 frames **0.522 ms**.

The renderer optimization improved native full-frame p50 **3.11×** and
100% patch rendering **7.21×**. Against a Chromium DOM, the format-matched custom
PNG capture pipeline achieved **2.74× p50 / 2.29× p95** improvement. The proposed
3× predecessor capture target was **not met**. The same Chromium screenshot API
on both representations was slower for canvas in this run. We have not established
a general renderer-only speedup over Chromium.

[Raw samples and summary](../benchmarks/results/summary.json),
[final machine and artifact hashes](../benchmarks/results/machine.json),
[initial renderer/native baseline hashes](../benchmarks/results/machine-baseline.json), and
[commands](../benchmarks/README.md) accompany the report. Final refreshed bindings
are measured, including the cached reset and exact integer conversion changes.
The final device-lifecycle runtime is included; rendering the multi-device console
itself is not a measured workload. These are results on one host, not universal
hardware guarantees.

## Method

Persistent release runtimes, five runs, 100 warmups and 1,000 individual samples
per short workload. Raster loops use 200 samples/run, portable snapshot encoding
and decoding 200, and browser PNG captures 30 samples/run with three warmups.
Tables use the median of the five per-run p50 and p95 values. Raw samples and
per-run dispersion remain in `benchmarks/results/`; throughput uses the arithmetic
mean latency within a run. This does not treat batch averages as operation p95.

Native runs are pinned to CPU 19 on this ARM64 host. Machine/compiler/browser,
world, lockfile and font/bundle hashes are recorded in `results/machine.json`.
`machine-baseline.json` retains the earlier measurement metadata for the native
renderer before/after runs; the final lifecycle-runtime runs refresh native world,
capacity, Node, Python and browser results. `native-dirty-reset.json` is the initial
standalone reset experiment; the report uses the newer `native-world.json` row.
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
| Terminal `pwd`, full actor step | 3.840 | 4.752 |
| Terminal parse + pipe | 4.448 | 4.912 |
| File write + read, two actor actions | 7.168 | 7.744 |
| Virtual HTTP, full actor step | 14.880 | 16.928 |
| Synthetic browser navigation through network | 23.568 | 26.256 |
| Editor type + backspace, two actions | 4.256 | 5.184 |
| Structured actor observation | 4.448 | 4.624 |
| Browser scene / layout, no raster | 2.016 | 2.080 |
| Already-clean same-seed reset | 0.144 | 0.144 |
| Snapshot handle | 0.464 | 0.528 |
| Fork from initial snapshot | 16.545 | 16.896 |
| Fork + first file mutation | 25.729 | 26.225 |
| Portable initial snapshot encode | 32.800 | 34.529 |
| Portable initial snapshot decode | 141.986 | 145.634 |

A dirty same-seed reset is **1.520 µs p50 / 1.568 µs p95**. Every iteration mutates a file before the timer,
then resets inside the timer and verifies the file disappeared after timing.
The 0.144 µs clean-reset row intentionally measures an already-reset world;
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
| DOM update + Chrome PNG capture | 35.004 | 52.579 |
| Wasm full update + same Chrome PNG capture | 44.671 | 53.737 |
| Wasm full update + canvas PNG export, two RPCs | 15.195 | 26.865 |
| Wasm incremental update + PNG export, one RPC | 12.786 | 22.980 |

DOM mutation plus forced layout costs 5 / 40 / 355 µs p50 for 1% / 10% / 100%
changes. Wasm patch + raster + full RGBA boundary copy costs 1.675 / 1.800 / 2.560 ms.
Those are different operations and are **not divided into a speedup ratio**.
Browser timers are quantized; the smallest DOM result is near timer resolution.
The RGBA-copy and canvas/export boundary is a remaining optimization target.

The real predecessor mail visual fixture captures in 46.434 / 63.092 ms p50/p95.
Its migrated geometry reconstruction captures in 50.915 / 74.317 ms. The latter
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
| Node Wasm terminal step | 13.408 | 42.817 |
| Node Wasm ten-action batch | 67.425 | 77.184 |
| Node Wasm observe | 1.472 | 3.696 |
| Node Wasm already-clean reset | 0.352 | 0.384 |
| Node Wasm snapshot handle | 0.960 | 1.233 |
| Node Wasm fork | 26.352 | 28.752 |

| Workload | p50 | p95 |
|---|---:|---:|
| Python terminal step | 7.936 | 8.480 |
| Python ten-action batch | 42.849 | 59.136 |
| Python observe | 1.280 | 1.360 |
| Python already-clean reset | 0.256 | 0.288 |
| Python snapshot handle | 0.672 | 0.720 |
| Python fork | 16.896 | 17.296 |

Actual Chrome Wasm terminal execution is **15 µs p50 / 35 µs p95** over 5,000
steps. One cold Node module load/instantiation took **16.600 ms**; this is one
observation, not a p50. Browser world construction warmed from 25.705 ms on the
first run to 1.020–3.330 ms on later runs; five points do not establish a robust
cold-start percentile.

| Packaged artifact | Raw bytes | Gzip bytes |
|---|---:|---:|
| Browser Wasm, standard services + renderer/font | 5,490,584 | 1,664,137 |
| Browser JS glue | 32,456 | 6,121 |
| Bundled font, already embedded in Wasm | 343,140 | 203,155 |

The font row is an accounting breakdown, not an additional required demo fetch.
Browser linear memory grew from **19 MiB to 20.56 MiB** across five
create/1,100-step/free cycles; Wasm memory retains its high-water allocation after
handles are freed. That is a bounded sample, not proof of zero growth over all
workloads. Native 1 / 100 / 1,000 independent, unrendered company worlds occupied
3.42 / 27.34 / 244.68 MiB RSS and took 2.73 / 27.79 / 272.25 ms total to create in
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
