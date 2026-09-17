# TCN computer engine: provenance and downstream changes

Inspected 2026-09-17. Research only; no simulator implementation added.

Pinned repositories:

- `jacobfv/typed-crystallization-networks`: `018c9ce9286fa4961292fe1e024b49fcd7e2dd7f`.
- `jacobfv/synthetic-computer-environment`: `88c5bed768d6e3811a8e26d2dd2f117ec8b5103c`.

The TCN checkout's `AGENTS.md` was read. Its actor/evaluator separation and logical-time rules explain its adaptations, but its four-carrier model algebra is project-specific and should not become simulator API policy.

## Provenance matrix

| subsystem | best current implementation | source repo/path | important alternatives | what should survive |
|---|---|---|---|---|
| VFS, process, shell, package, git, fabric semantics | Unified August fidelity engine, also copied in TCN | SCE `packages/kernel/src/`; TCN `generators/computer/engine/packages/kernel/src/` | Earlier SCE implementations lack the August rewrites | Algorithms and behavioral tests; not Node host persistence |
| Seeded object identities | TCN addition | `generators/computer/engine/packages/kernel/src/determinism.ts` | SCE `node:crypto.randomUUID` | Explicit per-world deterministic identity allocator, versioned algorithm |
| Whole-episode logical time | TCN wrapper adaptation | `engine/bridge.ts:7`, `engine/session.ts:49` | SCE ambient Date throughout semantics | Injected clock in Rust; reset initialization clock; never monkey-patch globals |
| Persistent cross-language session | TCN optional session | `engine/session.ts`; `generator.py:58–107` | Default bridge launches Node and replays all events each transition | Persistent canonical runtime and append-only action stream |
| Actor/evaluator separation | TCN record projection and gated probes | `tcn/generation.py:73–100`; `computer/generator.py:230–280`; `engine/bridge.ts:37` | Direct snapshots expose whole world | Distinct actor observation and privileged evaluator interfaces, availability time, actor ownership |
| Replay/restore envelope | TCN generic Host | `tcn/generation.py:130–175` | SCE observational snapshot | Versioned state construction and action replay; stronger complete Rust checkpoint |
| Reference ecosystem | SCE, relocated unchanged by TCN | SCE `ecosystems/seed-2026`; TCN `engine/packages/seed-2026` | TCN wrapper hardcodes seed world and Ubuntu | External world package; configurable machine selection |
| Computer rendering | No reusable new renderer in computer wrapper | `computer/generator.py:263` calls `generators/raster_text` | GUI generator is separate from copied computer engine | Optional pixels derived from observations; avoid model-specific tensor representation |

## Code comparison and lineage

Compared all 62 TypeScript files under TCN `engine/packages` to SCE HEAD, normalizing only the package prefix `@seed/` to `@tcn-computer/`:

- 53 are byte-identical after normalization.
- Six differ only by importing `randomUUID` from the new local determinism module: `application.ts`, `collaboration.ts`, `vfs.ts`, `software.ts`, `network.ts`, `vfs.test.ts`.
- Three have no same-relative-path counterpart: `determinism.ts` and the two relocated seed ecosystem sources. Both relocated ecosystem sources are byte-identical after namespace normalization to SCE `ecosystems/seed-2026/src/{index,check}.ts`.

The entire vendored packages directory is unchanged between TCN's initial commit `7a30d6b5e41ff3fa8332d67b61b33002b287214f` (2026-09-08) and HEAD (`git diff` empty). This is not an independently evolved newer shell/network/VFS implementation. It includes SCE's August fidelity work, including shell grammar, content-addressed git, package resolver, inode/link VFS, rich fabric, and sandboxed JS application execution.

Walking SCE first-parent history and comparing all 59 shared TS paths after the two documented adaptations yields exact matches at `88c5bed`, `9bb6ec9`, `4c72e7e`, `b68912f`, and `8d2bc59c2ece3912bd414190249aec2883f0ed3f`. The next ancestor, `721aeebf44ce78884c9f3ea7495e1f5d3eba5542`, differs in four files. Thus `8d2bc59` is the earliest contiguous ancestor matching this inspected package subset; this does **not** prove that exact commit was the checkout used for copying. The fidelity acceptance tests copied alongside packages were added upstream later at `9bb6ec9`.

TCN history for actual wrapper evolution:

- `7a30d6b`: initial copy, seeded identity adaptation, one-shot bridge, generator.
- `e2151a7fdc796fc8001bcb8e180958b1dd0e9974`: gated evaluator filesystem/process/content probes, updated generator reward inspection.
- `0115056bbabe1600c7ace8ac977b3e7ec19a4a51`: persistent session transport and panel interface (commit subject concerns another concurrent track; file history is the evidence).
- `f67257659c5e7afac19dc17c1d26b8255f001db1`: reset clock when rebuilding a live session; tolerate scratch-directory cleanup failure; preserve shell action menu as a literal.

## Important downstream fixes

1. **Deterministic IDs.** Hash `${seed}:${counter++}` with SHA-256 and format a UUID-shaped string. Preserve deterministic allocation, but use world-local state: TCN's module-global seed/counter would couple multiple worlds sharing one JS realm.
2. **Logical initialization and action time.** Replace global Date before dynamically importing the kernel; epoch is 2000-01-01 and event time is rounded to milliseconds. `session.ts:55` resets epoch on every boot. Previously rebuilding inside a warm process inherited the previous episode's clock and moved inode times, process times, uptime and trajectory stamps.
3. **Host-path suppression.** Replace the temporary state root in serialized payloads with `<episode-storage>`. Rust should never insert such paths into semantic state in the first place.
4. **Non-mutating privileged probes.** Direct VFS listing/reads avoid recording evaluator reads as actor trajectory events; results are sorted deterministically and missing file content is represented as null. This also removes a second subprocess invocation for reward checking, and fixes create-file objectives failing before the file existed.
5. **Persistent session.** Reuse runtime only when seed matches and requested log extends the previously applied prefix; apply suffix only. A changed prefix/seed rebuilds. This is a useful compatibility transport, but still compares/resends the entire log and returns ~227 KB snapshots for tiny read actions in the measured smoke test.
6. **Cleanup is not semantics.** Session teardown retries `rm` and ultimately tolerates failure; one-shot bridge still lets `finally` cleanup errors fail the transition.
7. **Stable interface menus.** Explicit `SHELL_MENU` avoids silently changing existing actor-visible actions when adding panel verbs to the shared generator schema.

## Actor boundary: useful, but not an adversarial sandbox

`StepRecord` physically separates observations, latents, probes, transitions, rewards and metadata. `actor_view` exports only observations whose ownership is the actor/public (or unprefixed) and whose `available_at <= time`, plus actor action names, time and **the supplied objective unchanged**. `Host.view()` supplies `self.objective`. `computer.observe()` exposes terminal/pixels, while snapshot counts and privileged filesystem/process state go to probes/latents.

Tested directly with a constructed record: other-actor and future observations were removed; probe/latent attributes were absent; a secret printed into terminal output remained visible; `{'content': 'objective secret'}` remained visible through the objective. Therefore:

- It prevents accidentally handing record probe fields to the policy; it does not redact arbitrary terminal/service content.
- Objective answers must be separated from public task instructions in the new design. Blindly adopting `Host.view()` can disclose an evaluator's expected file content.
- The Node transport always returns full snapshots to its trusted Python owner. It is not safe to hand that owner/transport directly to an untrusted actor.
- `Generator.validate_actions` and the computer wrapper constrain verb/type/interface, but the computer wrapper does not implement a general actor-to-machine authorization model.
- Probe gating is a trusted configuration convention, not authentication. Use separate Rust capability handles and separately serialized actor/evaluator responses.

## Remaining determinism and portability liabilities

The seeded generator and global Date patch do not establish whole-system determinism. The application sandbox starts workers and VM contexts (`kernel/src/sandbox.ts`), which do not inherit the main realm's patched Date. The worker execution budget still relies on actual timers, while parent budget accounting calls the patched Date. Network/runtime code retains host adapters; VFS persists to host files; global identity state cannot isolate concurrent worlds. These are code observations; adversarial sandbox determinism was not tested here.

`Host.snapshot()` deep-copies state, records and complete input history. `restore()` restores the Python wrapper and checks source fingerprint/generator version; it does not materialize a native kernel checkpoint. A later bridge request reconstructs from events; the session either happens to match or rebuilds. This is reproducible construction, not cheap engine snapshot/fork. Source fingerprint covers the whole `tcn` and `generators` trees, over-coupling computer replay compatibility to unrelated model/generator edits.

Do not carry forward Node subprocess semantics, global Date/ID patches, fixed Ubuntu selection, seed-world imports in bindings, full-history replay per step, automatic full snapshots per observation, objective answers in actor views, model algebra, policy parameter counts, training reward logic, or repository-specific absolute paths (including those in research scripts).

## Verification performed in this investigation

Fresh `npm ci --ignore-scripts --no-audit --no-fund`, then `npm test` in vendored engine, Node `v24.21.0`:

- **372 / 372 tests passed**, 10 test files, **6.45 s** reported wall duration.
- Includes git 60, software 81, network 54, shell 33, VFS 33, application 39, processes 22, fidelity acceptance 27, integration 15, catalog 8.
- Integration tests exercise shared git smart-HTTP, separate collaboration services, package state, bundles, cloud app operations; passing is evidence of these asserted behaviors, not blanket fidelity/wasm support.

Additional Python-stdlib driver invoked `bridge.ts` and persistent `session.ts` with three identical requests and compared decoded **entire response payloads**, including snapshots and trajectories:

| case | entire payload equal | bridge seconds | session seconds |
|---|---:|---:|---:|
| boot seed 7 + write | yes | 0.8465 | 0.6414 |
| same seed + appended read | yes | 0.4877 | 0.00317 |
| changed seed 8 + empty log | yes | 0.8512 | 0.3647 |

These are single smoke-test samples in the shared investigation environment, not p50/p95 benchmark results. The warm-read difference demonstrates the transport pathology without claiming stable throughput. Full response size was approximately 227 KB.

Historical results, **not reproduced here**: `research/credit-assignment/RESULTS.md:209–245` reports 0.48 s persistent versus about 8 s one-shot horizon-6 panel episode; 64/64 StepRecord equivalence combinations; the reset-clock bug; and cleanup `ENOTEMPTY` failures under load. `research/computer-capability/RESULTS.md:71` reports 1.96/3.13/2.77/3.88 s read/write/wait/command steps and explains the second reward subprocess. Those are workload/machine-specific reports, not interchangeable with this smoke test.

## Migration consequences

Use SCE as the shared fidelity mechanism provenance and explicitly credit TCN for deterministic identity/time adaptation, persistent-session experiments, actor/probe projection, and regression lessons. Port causal tests rather than JS mechanisms. Design Rust world-local clock/RNG/ID state and complete checkpoints, actor-owned observation contracts, public task descriptions separate from evaluator answers, side-effect-free privileged inspection, and persistent thin bindings from the outset.
