# Performance and benchmark interpretation

Run release builds on an otherwise idle machine. Preserve compiler and dependency
versions, CPU/OS, engine revision, seed, world/action definitions, warmup/sample
counts, viewport, font and tracing mode with results. The benchmark package owns
its executable commands and raw results: [`benchmarks`](../benchmarks).

Measure persistent sessions. Process startup per action measures a transport
anti-pattern, not simulator step cost. Keep these operations separate:

- terminal and filesystem operations;
- DNS/routing/HTTP and service mutation;
- app input and browser navigation;
- structured layout/scene updates and optional rasterization;
- reset, in-memory snapshot/fork, and portable encode/decode;
- Wasm execution and language-boundary conversion.

Report p50/p95 of individual operations when collected; batch averages are not
individual-operation tail latency. State mutation and semantic assertions should
prevent benchmarks from accidentally timing no-ops. Keep profiling and timing
runs separate and record optimizations against a reproducible baseline.

For DOM comparisons, use matched content, dimensions and transitions. Compare
like metrics: browser screenshot PNG latency and native raw RGBA drawing are
incompatible measures of isolated raster cost. Browser update/layout timing and
native scene update timing can illuminate representation cost, with runtime
and workload differences disclosed. Preserve predecessor workflow measurements
separately from isolated rendering measurements. Do not call a partial rendering
comparison an end-to-end training speedup.

Snapshot sharing avoids repeated full copies for untouched state, but first writes
and portable serialization can be costly. Benchmark both. No fixed speedup claim
is part of the API contract; measured results and remaining gaps belong in the
benchmark report.
