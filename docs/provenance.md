# Provenance matrix — before architecture

Investigation date: 2026-09-17. Exact revisions are recorded in
[sources.json](../research/sources.json). “Best” means strongest observed semantics,
not a claim that an entire predecessor should be ported. Research reports contain
source paths, history, defects and verification details.

Abbreviations: **SCE** = `jacobfv/synthetic-computer-environment`; **TCN** =
`jacobfv/typed-crystallization-networks`; **SynthUX** = `jacobfv/synthux`.

| subsystem | best current implementation | source repo/path | important alternatives | what should survive |
|---|---|---|---|---|
| Early virtual internet | No working implementation in early Synthex | `commandagi/synthex/synthetic_worlds/domains/internet/__init__.py` | SynthUX host HTTP façade; SCE network fabric | Historical intent only; do not port the three-dictionary stub |
| World/topology composition | SCE serialized topology and separate ecosystem/OS packages | `packages/protocol/src/index.ts`, `ecosystems/seed-2026`, `packages/os-*` | TCN identical relocated ecosystem; SynthUX World stores | Generic world input, machine/service placement, data-defined OS profiles; remove catalog and fixture imports from kernel |
| VFS | SCE August inode-graph rewrite, also present in TCN | `packages/kernel/src/vfs.ts`, `vfs.test.ts` | Earlier path/file façade; TCN deterministic UUID substitution | Binary bytes, inode identity, links, rename, ownership, OS path rules, regression cases; replace host disk backing |
| Processes/shell | SCE August process model and grammar | `packages/kernel/src/{processes,shell-grammar,shell,programs}.ts` | SynthUX actor/thread machinery | Per-machine PID/fd ownership, exit cleanup, signals, parsing/pipes/redirection and explicit dialect subsets |
| Packages/Git | SCE dependency resolver and object-based Git | `packages/kernel/src/packages/`, `git/`, `software.ts` | SynthUX Git review workflow; GitHub mock has no Git object transport | Resolver/transaction invariants; content-addressed commits/trees/refs and network push/fetch; keep workflow services optional |
| DNS/routing/HTTP/transport | SCE InternetFabric | `packages/kernel/src/network.ts` and tests | SynthUX `internet.py` uses host sockets/proxy; early Synthex stub | Per-node identity, DNS, loopback, routes/listeners, causal request/response and traces; selective transport fidelity |
| Host gateway policy | SCE egress authorization/pinned transport | `packages/kernel/src/network.ts` | SynthUX allowlist checks initial URL but host redirects escape that check | Default deny, every-address/redirect authorization; explicit adapter outside pure simulator |
| Causal scheduler/events | SynthUX clock/channel/event contracts | `src/synthux/world/{clock,channels,events,origin}.py`, `scheduler/scheduler.py` | SCE network timing; Synthex event cursor has reproduced ordering defect | Logical time, causal origin, delayed delivery/deadlock semantics; replace threaded execution and sorted-list cursor |
| Deterministic sessions | TCN wrapper + local UUID helper | `generators/computer/engine/{bridge.ts,session.ts}`, `engine/packages/kernel/src/determinism.ts` | SCE only seeds network RNG; SynthUX mixes logical and host time | Explicit clock/RNG/IDs and persistent runtime; no global Date patch or prefix replay per normal step |
| Checkpoints/replay | No predecessor has complete portable world checkpoints | SCE `simulation.ts`, `trajectory.ts`; TCN wrapper | Synthex deep-copy resources/re-run-from-seed; SynthUX component snapshots | New complete checkpoint contract; preserve traces separately from filtered observations |
| Application boundary | SCE scoped syscalls + SynthUX semantic stores | `packages/app-sdk`, kernel `application.ts`, `permissions.ts`, `syscalls.ts`; SynthUX `world/apps_state.py` | Worker/vm execution; product-specific operation tables | Versioned app state/events, narrow capabilities, lifecycle/resource ownership; no required JS executor |
| Mail/chat/calendar/docs semantics | SynthUX inline domain stores | `src/synthux/world/{mail,chat,calendar,apps_state}.py`, `internet.py` | Standalone mail only writes Sent; Slack discards POST; docs read-only | Recipient delivery, membership checks, RSVP/document transitions where implemented; isolate instance state and connect all clients to it |
| Git hosting/issues/reviews | SCE Git transport + SynthUX workflow + latest standalone GitHub mock | SCE `git/`; SynthUX `world/git.py`; `synthux-github-mock/src/{state,server}.mjs` | Jira mock only creates issues; mock branch/diff displays are often static | Shared causal Git objects, issue/comment/review transitions and views; never report mock counters as actual state |
| Independent sites | Six standalone mock service boundaries and view schemas | `synthux-*-mock/src/{server,state,render}.mjs` | SynthUX `sim/services` pins identical commits | OS-independent endpoints, document blocks, board/mail/calendar views; reexpress scenes and seed consistent reference data |
| Browser/visual interaction | SynthUX input/frame provenance scaffolding + SCE network-backed navigation | SynthUX `viewport/{viewport,driver}.py`; SCE client `apps/BrowserApp.tsx` | Actual Playwright/React/iframe paths; SynthUX native observation can echo requested arguments | URL/history/storage/input semantics; derive observations from actual visible state, never action intent; keep real-browser bridge optional |
| Fast synthetic renderer | None of the examined predecessors | Existing browser/DOM paths provide comparison workloads | Canvas/native shell layers still rely on browser execution | New semantic/layout/scene/raster separation; benchmark rather than claim speedup |
| Agent/evaluator separation | TCN actor projection and gated evaluator probes | `generators/computer/`, adapter/frame tests | symbolic-ai-models typed action/perception/state boundaries; Synthex task predicates | Explicit actor grants and evaluator handle; public objective separate from private predicates/answers |
| Agent/model decoupling | symbolic-ai-models environment-facing contracts | See `research/notes/lineage-boundaries.md` for exact paths | TCN representation and SynthUX grammar coupling | Typed actions, local observation and effect accounting; exclude model architecture/training machinery |

## Established lineage

This is a branching history, not a sequence of complete replacements:

1. SynthUX develops actual desktop/viewport and host-backed virtual-internet machinery.
   Its six mock services were split into submodules; current gitlinks match all six
   standalone HEADs. Inline Python service stores remain separate from those HTML
   mock stores. They are not one causally consistent backend.
2. Synthex introduces its Internet stub on May 31 and later integrates SynthUX
   through a Playwright bridge. That is evidence of consumption, not proof that
   SynthUX networking evolved from the stub.
3. SCE starts with a July imported baseline and later adds topology composition.
   August 11 replaces important filesystem, shell, process, Git, package and network
   mechanics. Shared product vocabulary alone does not prove file ancestry from SynthUX.
4. TCN vendors the current SCE mechanics: 53 of 62 TypeScript files match after
   namespace normalization; six differ by deterministic UUID substitution; three
   are the new helper and two relocated ecosystem files. Its additional advances
   live mainly in session, time, observation and evaluator wrappers.
5. symbolic-ai-models supplies boundary ideas rather than a replacement computer engine.

## Migration implications

Preserve algorithms, verified behaviors and regression cases at subsystem level.
Do not copy a repository wholesale, assume display controls mutate state, or infer
complete determinism from a seed parameter. No complete restorable world state or
DOM-free canonical renderer was found. Those are new work.

Provenance reports record licenses/attributions as found. In particular, SCE and the
standalone mocks do not declare a repository-wide license; some SCE security files
explicitly attribute browser-os. Retain attribution and track any literal reuse;
do not invent license grants for predecessor code/assets.

What the redesign kept, dropped and made incompatible is summarised in
[migration](migration.md), which also covers moving between 0.x releases of this project.
The design those decisions produced is [architecture](architecture.md).
