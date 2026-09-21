# Fork throughput and retained bytes

How cheap is it to branch a live world? Measured natively, in release, on the
`worlds/company-2026` world, forking a world that has already been driven through real
actor steps rather than one that has just booted.

## Headline

On a company-2026 world populated by **1,000 actor steps** (4,013 recorded events):

| Figure | Value | Spread |
|---|---:|---|
| **Forks per second** (one thread, serial) | **564.6 /s** | 551.0–566.7 across five runs |
| **Bytes retained per live fork** | **20,863 B** (5,975 B of heap + the 14,888 B `World` value) | the identical byte count in every run and at every level of accumulated state |
| Fork latency p50 / p95 / p99 | 1.740 / 2.015 / 2.150 ms | per-run p50 1.725–1.765 ms |

So: **milliseconds and kilobytes**, not microseconds and kilobytes. A fork of this world
costs about **1.7 ms of one core and about 20 KB**; 1,000 live forks of it were held at
once for 20.9 MB. The same world exports to 57.7 MB of portable JSON and its event log
alone occupies 248.8 MB live. The bytes are flat in the size of the world — exactly flat,
to the byte. The latency is dominated by validation rather than copying and grows mildly
with state. Two things a careful reader needs before quoting any of this are below:
"the first write in a branch" and "what these numbers do not support".

## Reproduce

```sh
export CARGO_BUILD_JOBS=4
cargo build --release -p cw-benchmarks --bin fork_throughput
taskset -c 19 target/release/fork_throughput
```

That writes [`results/fork-throughput.json`](results/fork-throughput.json) — per level and
per run, with every raw latency sample — and prints one line per level per run. It took 37
minutes on this host with the defaults below, most of it spent driving the world to 4,000
steps rather than forking, and peaks at a few GB of RSS at the 4,000-step level (the
parent's event log plus one diverging fork's copy of it).

Overrides, shown with the defaults these numbers were produced with: `BENCH_RUNS=5`,
`BENCH_SAMPLES=1000` measured forks per level per run, `BENCH_WARMUP=100`,
`BENCH_LIVE_FORKS=1000` simultaneously live forks for the memory window,
`BENCH_MUTATION_PROBES=5`, `BENCH_FORK_STEPS=0,250,1000,4000`,
`BENCH_EXPORT_MAX_STEPS=1000`, `BENCH_OUTPUT`. The headline level alone reproduces in a few
minutes: `BENCH_RUNS=2 BENCH_FORK_STEPS=1000 taskset -c 19 target/release/fork_throughput`.

Harness: [`runner/src/bin/fork_throughput.rs`](runner/src/bin/fork_throughput.rs).

## Machine

| | |
|---|---|
| CPU | ARM Cortex-X925 + Cortex-A725, 20 cores, 3.9 GHz max, frequency boost disabled |
| Pinned to | CPU 19, the same performance core the other benchmarks in this directory use |
| RAM | 121 GiB |
| OS | Ubuntu 24.04.4 LTS, Linux 6.17.0-1026-nvidia, aarch64, 4 KiB pages |
| Toolchain | rustc 1.97.1 (8bab26f4f 2026-07-14), cargo 1.97.1 |
| Profile | `release`; workspace settings are `lto = "thin"`, `codegen-units = 1` |
| Allocator | glibc malloc, wrapped by the harness's counting `GlobalAlloc` |
| Repository | `3ec0953`; world `worlds/company-2026/world.json`, 3,738,557 bytes, sha256 `ea44cac907209082ead2cbf3ab5533f82f4789c94978b5dded995f140e726bd3` |
| Host state | **Not an exclusive window.** Load average was 8–12 on this 20-core host while the benchmark ran; other agents were building and testing in the same repository. The measured process was pinned to CPU 19 and nothing else was pinned there, but the scheduler may still place other work on that core. The five-run spread is the honest bound on what that noise did. |

## The world being forked

`reference_world()` is the packaged `worlds/company-2026` world: a 3.7 MB definition
holding a whole simulated company and internet. The harness opens a desktop session for
`alice` on `alice-mac`, launches an editor, and drives the world through a repeating
five-action cycle before any fork is timed:

1. a shell command (`echo … | cat`),
2. a write to a **new** file each time, so the machine's inode table really grows,
3. an editor keystroke that stays in the document (focus is returned to the editor first,
   because the previous cycle ended in the browser),
4. a virtual HTTP request to `http://intranet.internal/`,
5. a browser navigation to the same host.

Every outcome is asserted successful. The 1,000-step world holds 4,013 event records and
exports to 57.7 MB of portable JSON, against 13.1 MB at boot; the 4,000-step world holds
16,019. (Event counts are a few above the nominal step count because each level leaves
behind three control writes, described under Method.) The 0-step level is in the table only
as the contrast: it is the "fork an empty world" number, and it is not the one to quote.

## What is actually shared and what is copied

From `crates/kernel/src/lib.rs`: `Runtime` holds `state: Arc<State>` and `Snapshot` holds
the same `Arc<State>`, the `Arc<WorldDefinition>` and a small module-version map. Inside
`State`, computers are a `BTreeMap<String, Arc<Computer>>`, the network is `Arc<Network>`,
each service's state is an `Arc<Value>` and the event log is `Arc<Vec<EventRecord>>`. Below
that, `Vfs` keeps its inode table in an `Arc<BTreeMap<u64, Inode>>` and every file's bytes
in an `Arc<Vec<u8>>`.

- `Runtime::snapshot()` clones those `Arc`s plus the module map. It is a handle.
- `Runtime::fork()` **validates** the snapshot, then builds a runtime whose `state` is that
  same `Arc<State>`. No world state is copied.
- `Environment::fork()` wraps that, clones the `Arc`-backed session map, journal and
  registries, and then calls `restore()` — which validates the same snapshot a **second**
  time and re-clones the state handle.

That is why the retained bytes are constant: a fork allocates its own
`World`/`Environment` scaffolding and nothing belonging to the world.
`docs/determinism.md` says forks "share immutable backing with copy-on-write roots where
implemented". On this path the sharing is complete for kernel state; the "where
implemented" caveat does not show up in the fork, it shows up in the first write after it.

## Method

**Latency.** Per level and per run: 100 warm-up forks, then 1,000 measured forks, each
timed individually with `Instant` around `World::fork(&snapshot)` and then dropped, so
steady-state memory is flat and no sample pays for a growing heap. The snapshot is held
live across the loop, so the `Arc`s really are shared (refcount > 1) exactly as they would
be during a tree search. Quantiles describe individual forks. Table values are the median
across five runs of each run's own p50/p95/p99, and the spread column is the minimum and
maximum of the five per-run p50s. Throughput is `1 / arithmetic mean latency` within a run,
reported as the median of the five — the same convention as [`README.md`](README.md) and
`docs/performance.md`. Allocation accounting is switched off during this phase so timings
are not charged two relaxed atomics per allocation. After every level the parent's
`state_hash()` is asserted unchanged, so a fork that quietly mutated its parent could not
pass as a fast fork.

**Retained bytes, in one sentence.** The harness installs a counting `GlobalAlloc` around
the system allocator; it enables counting, reads live bytes, creates 1,000 forks and holds
them all at once, reads live bytes again, and divides the difference by 1,000 — an exact
live-heap delta, not a resident-set estimate.

Two details belong with that sentence. First, the `Vec` holding the 1,000 forks is reserved
*before* counting starts, so the 5,975 B figure excludes the inline `World` value itself;
`size_of::<World>()` is 14,888 B and has to live somewhere, so the honest per-fork total is
20,863 B. Second, when the forks are dropped the counter lands on exactly `-14,888,000`
relative to its baseline — that is the uncounted vector buffer being freed inside the
counted window, and it means all 5,975,000 counted bytes came back: the forks leak nothing.

**Resident-set cross-check.** The harness records the RSS delta over the same window. The
first clean window (0 steps, first run) gives 14,623 B per live fork, the same order as the
exact figure. Every later window reads 0, because by then the process has freed hundreds of
megabytes and glibc satisfies 6 MB of new forks from pages it already owns. That is a
limitation of RSS for an allocation this small inside a process this large, not evidence of
a free fork — which is why this report leads with the allocator accounting instead.

**Divergence, and its control.** With one fork alive at a time, the harness times the first
and second file write inside a fresh fork. It then drops the checkpoint and performs the
same write three times in the **parent**, where nothing is shared and no copy-on-write root
can split. Those control writes are what separates a fork's split cost from what a step
costs in a world this large; they are also why each level carries three steps of state
beyond its nominal count. Finally it measures the live size of one deep copy of the event
log, through the public `trajectory()`, to attribute the split.

## Results

Medians of five runs; 1,000 measured forks per level per run, 20,000 forks in total.

| Steps of accumulated state | Fork p50 | p95 | p99 | per-run p50 range | Forks/s | Forks/s range | Heap bytes per live fork |
|---:|---:|---:|---:|---|---:|---|---:|
| 0 (fresh boot, *not* the headline) | 1.635 ms | 1.911 ms | 2.009 ms | 1.597–1.677 ms | 595.7 | 586.5–614.5 | 5,975 |
| 250 | 1.659 ms | 1.919 ms | 2.029 ms | 1.637–1.679 ms | 589.7 | 579.3–595.8 | 5,975 |
| **1,000** | **1.740 ms** | **2.015 ms** | **2.150 ms** | 1.725–1.765 ms | **564.6** | 551.0–566.7 | **5,975** |
| 4,000 | 2.059 ms | 2.375 ms | 2.507 ms | 1.990–2.115 ms | 478.0 | 466.1–490.9 | 5,975 |

**Does the cost stay flat as state accumulates?** The bytes do, exactly: 5,975 B per fork
at every level in every run, to the byte. Sixteen thousand events and three thousand extra
files move it by zero. The time does not quite: p50 rises 26% from an empty world to a
16,019-event one (1.635 → 2.059 ms), a 20% throughput loss over a 16,000-fold increase in
recorded state. That increase is in `validate_snapshot`, which walks the scheduler, the
service and computer instances and each computer's own `validate()`, not in copying state.

**Reproducibility.** An earlier build of the same harness, run end to end on the same host
before the control probe was added, gave 1.718 ms p50 and 565.8 forks/s at this level
(against 1.740 ms and 564.6 now), `heap_bytes_per_fork` identical to the byte, and
`first_write_heap_bytes` within 0.02%. A separate two-run invocation of the headline level
gave 1.699 and 1.770 ms.

## Where the time goes

Two probes in the same binary, on the same world:

| Probe | Median |
|---|---:|
| `WorldDefinition::validate()` on the 3.7 MB definition | 127 µs |
| Field-by-field `PartialEq` of the definition against the baseline | 715 µs |

`Environment::fork()` performs both twice — once in `Runtime::fork` and once in the
`restore` it calls — which is about **1.68 ms** against a 1.740 ms p50. The fork is
dominated by *defensive validation of the world definition*, not by state copying.
`docs/performance.md` recorded 265 µs for this operation when the definition was 432 KB;
it is now 3.7 MB, and the cost tracked it. Anyone who wants a faster fork should start
there: the state sharing is already free.

## The caveat: the first write in a branch copies the event log

This is the part that matters for tree search and it should not be dropped from any page
quoting the figures above. The fork is cheap; the *first mutation inside the fork* is not.
`Runtime::event()` does `Arc::make_mut(&mut self.state)` and then
`Arc::make_mut(&mut state.events)`, and the event log is a plain `Arc<Vec<EventRecord>>`,
so while parent and fork share it, the first side to record an event deep-copies the whole
log.

| Steps | Events | First write in a fresh fork | Second write in that fork | Same write in the parent, nothing shared | One deep copy of the event log |
|---:|---:|---:|---:|---:|---:|
| 0 | 1 | 89 KB, 66 µs | 10,143 B, 12 µs | 10,151 B, 13 µs | 2.1 KB |
| 250 | 1,007 | 67.4 MB, 21.0 ms | 51,232 B, 10.1 ms | 51,198 B, 10.3 ms | 62.2 MB |
| 1,000 | 4,013 | 269.2 MB, 82.7 ms | 51,232 B, 40.0 ms | 51,166 B, 43.9 ms | 248.8 MB |
| 4,000 | 16,019 | 1,076.6 MB, 533.1 ms | 51,233 B, 159.4 ms | 51,167 B, 179.7 ms | 995.3 MB |

Three readings, in order of importance:

1. **The split is real and it is O(accumulated trajectory).** The event log is 92% of it at
   every populated level; the rest is the action journal, the session state and the
   machine's inode table. It works out at about 270 KB of copy per parent step. There is no
   public API to trim or disable the trajectory.
2. **It is one-time, and afterwards a fork is not penalised at all.** The control column
   settles this: the second write inside the fork (40.0 ms at 1,000 steps) is no more
   expensive than the same write in an unshared parent (43.9 ms) — if anything slightly
   less. After the split a branch steps at exactly the parent's cost.
3. **The control column also shows where the real cost in a long episode is, and it is not
   forking.** A single actor step in this world costs 13 µs at boot, 43.9 ms at 4,013
   events and 179.7 ms at 16,019 — with no fork involved. The per-step cost of a populated
   world grows with its accumulated state. This benchmark did not attribute that (the
   allocation it retains is only 51 KB, so it is transient copying somewhere in the step
   path), and it is out of scope here; but it means branch cost is the small term. At the
   1,000-step level, forking and diverging a branch costs about 41 ms more than the step
   the branch was going to take anyway, against 43.9 ms for the step itself.

Practical reading: branch early and often from short episodes and it is 20 KB and 1.7 ms;
branch a world that has already recorded thousands of browser navigations and the first
write in each branch copies hundreds of megabytes.

## What these numbers do and do not support

They support: a fork of a realistic, populated world retains about 20 KB and takes about
1.7 ms on one core; the retained bytes do not grow with the world's state at all; a
thousand live branches of such a world were held simultaneously in 20.9 MB.

They do not support: any claim about containers or virtual machines. Nothing in this
directory measures a container or VM snapshot, and no such comparison was run on this host.
They are also one host, one pinned ARM64 core, one allocator and one world; they are a
serial single-thread result, not a parallel throughput figure; and the window was not free
of other load. The first-write cost above is part of the honest answer to "how cheap is it
to branch a world", not a footnote to it.
