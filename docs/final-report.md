# Implementation and verification report

Status: the canonical Rust implementation and live device lifecycle are implemented and
verified. This report records the measured results and remaining fidelity limits. It is
not a claim that every possible computer/browser behavior has been emulated.

> **Superseded in part, 2026-09-20.** The owner-only world console this report measured —
> its page, the Playwright suites that drove it and the `browser-demo` release archive —
> was removed; the project site's home page shows the live machines instead. The figures
> below are what those suites recorded on the dates given, and the artifacts they wrote
> are kept as that record, but nothing regenerates them now. The engine behaviour they
> covered is checked by `crates/computerworld/tests/site_world.rs`, `desktop.rs`,
> `phone_navigation.rs`, `native_apps.rs` and the window-management unit tests in
> `crates/applications/src/lib.rs`. See the changelog entry for the split.

## Delivered architecture

A modular Cargo workspace separates serializable contracts, deterministic
clock/RNG/scheduler, computer state, synthetic network, service/application SDKs,
scene construction/rasterization, actor interfaces, evaluator predicates and
trajectories. `WorldDefinition` is generic data; the company ecosystem is a separate
JSON package. Native Rust, wasm-bindgen and PyO3 consume the same runtime. The
Python package does not start Node. Wasm runs without ambient host capabilities.

The reference ecosystem exercises independent macOS, Windows and Ubuntu machines,
eight service nodes, internal/public-looking DNS, mail delivery, shared documents,
chat, calendars, Git transport, issues and network-served pages/assets. Kernel
state persists across steps. Actor sessions have explicit grants; privileged world
inspection belongs to the owner/evaluator interface, not actor observations.

## Reuse, redesign and compatibility

The [provenance matrix](provenance.md), [research reports](../research/) and
[pinned revisions](../research/sources.json) record code/history evidence from all
11 repositories. Reuse is primarily algorithms, causal behavior and regression
cases, reexpressed in Rust rather than wholesale source copying.

| Source | Preserved | Redesigned or dropped |
|---|---|---|
| synthetic-computer-environment | Generic topology, OS profiles, inode filesystem semantics, process ownership, package/Git invariants, DNS/routing/HTTP and egress authorization | In-memory serializable state instead of host disk; controlled scheduler; complete checkpoints; native app/page contract |
| typed-crystallization-networks | Persistent sessions, deterministic IDs/time, reset semantics, actor/evaluator boundary | No vendored engine copy, global clock patch, policy representation or subprocess-per-step bridge |
| synthux | Logical event causality, semantic service stores, input/visible-frame relationship | No threaded host HTTP world, disconnected browser fixtures or requested-action echoes |
| Six standalone service mocks | Independent service addresses and useful endpoint/view workflows | Native service transitions and scenes; fill actual state gaps such as recipient delivery and shared edits |
| synthex | Historical virtual-internet intent and later SynthUX integration | Early three-dictionary Internet stub contributes no working networking implementation |
| symbolic-ai-models | Explicit environment/action/perception boundary | No neural/model/training architecture |

The important downstream TCN change was not a newer complete computer substrate:
53 of 62 compared TypeScript files match SCE after namespace normalization; six
primarily substitute deterministic IDs. Persistent sessions, controlled time and
observation/evaluator wrappers provide the substantial downstream boundary fixes.
Those are explicit runtime/session contracts here.

The new schema, checkpoints and native service APIs are not drop-in predecessor
formats. JavaScript/Python extension implementations need a Rust port or explicit
adapter. The [migration notes](migration.md) document that boundary.

## Completed verification evidence

The raw evidence this section once linked — acceptance logs, console reports and
screenshots under `artifacts/` — is no longer kept. The runs were one-off, nothing
regenerated them, and the desktop screenshots among them had gone stale. The measured
results are below; the suites that produced them are still in the repository and can be
run again.

The final full `scripts/test-all.sh` acceptance pass recorded **178 passing test
executions** (160 workspace unit/integration/doc tests plus 3 optional native HTTP
adapter tests), zero failures. It completed formatting, pure-core dependency/host
boundary checks, strict workspace Clippy and a release `wasm32-unknown-unknown`
build, including the final device-lifecycle changes.

Behavior tests cover two-computer communication, DNS and HTTP, browser navigation
through the network, mutations visible to other clients, blocked outbound access,
filesystem/process isolation, seeded initialization, replay, portable checkpoints,
fork isolation, actor/evaluator separation, event/packet traces, package lifecycle,
service diagnosis, structured observations without rasterization, hit testing and
deterministic direct rendering. Unit/property tests cover filesystem and protocol
invariants, transport policy, scheduling, application dispatch and scene damage.

Node/Wasm and Python verification exercised
cross-language checkpoint/hash parity, reset,
fork and rendering against the same canonical runtime. Native/Wasm rendering
matched the golden RGBA hash
`01455c4eaa6c1eca6900b545f69bba35ad9428fb66275a41df86268ae3595ec6`.
The native company, custom-service and custom-application examples ran successfully.

The browser console report recorded seven
computers, forty-seven services, 343 events and **zero host network requests during
the episode** after static assets were loaded. It was written by the browser suite that
has since been removed with the console, so it is a dated record rather than a regenerated
one, and the report itself is no longer kept. Real browser pointer/keyboard input,
terminal execution, reset, checkpoint/fork, deterministic rendering and actor
restrictions were exercised. A phone is added, browses and writes a file; removal,
topology checkpoint restoration and fork preserve state; a headless server runs
terminal commands and observes blocked network access; phone text entry and Enter
route to the simulator. The console shows network links, live monitor previews
and device peripherals.

## Performance

The [performance report](performance.md) contains p50/p95, raw samples, artifact
hashes, machine details and reproduction instructions. Final native measurements:
terminal step 3.840/4.752 µs, synthetic HTTP 14.880/16.928 µs, dirty same-seed reset
1.520/1.568 µs, snapshot handle 0.464/0.528 µs, fork 16.545/16.896 µs. Structured
scene generation is 2.016/2.080 µs and requests no pixel work.

Profiling and optimization improved the Rust renderer's full 1280×720 p50 by
3.11× and 100% incremental patch by 7.21× relative to its initial implementation.
The matched browser capture pipeline is 2.74× faster at p50 using incremental
canvas PNG export than DOM plus screenshot. The same screenshot API is slower for
canvas in the final run, and the approximate migrated mail scene is slower. This
does not establish
a universal renderer-only advantage over Chromium; the report retains contrary
results and distinguishes layout, raster, encoding and capture costs.

The earlier pre-overhaul browser binding and topology build was 5,490,584 bytes raw / 1,664,137 gzip;
its Python wheel was 2,945,242 bytes. Boundary performance was refreshed against this build.
Node/Python parity after dynamic topology changes passes with baseline state hash
`bef75176ee710983e5605fda2cbe590727ece1d609b947c33411126debb035dd`.

## Current fidelity limits and next extensions

The simulator executes documented shell/process/package subsets, not arbitrary
host binaries or a full POSIX/Windows kernel. Transport is a causal synthetic
stream/datagram/HTTP model, not a full TCP/IP stack. Git uses synthetic SHA-256 JSON
object transport; local binary files are supported but remote transfer currently
rejects unsupported binary content. OS profiles do not emulate native desktop APIs. Phone devices use the same
synthetic application/runtime contracts and a touch-sized viewport; they do not
emulate Android or iOS. Peripheral visualization represents input/display
affordances rather than arbitrary hardware drivers. Removal of a computer that
hosts service definitions is rejected to prevent orphaned service placement;
dynamic service migration/removal is not implemented.

Pages are structured native descriptions. Arbitrary HTML/CSS/JavaScript execution
and a real-browser compatibility backend are not implemented. Text uses bundled fixed-cell terminal and proportional UI fonts without full bidi/script shaping. The scene renderer
supports useful deterministic primitives rather than full browser typography and
compositing. Native extension handlers are trusted code, not a sandbox for untrusted
plugins. The explicit native HTTP adapter is optional and is outside pure core.

Useful next extensions are incremental scene/binding transfer profiling, selective
Wasm service bundles, binary remote Git objects, richer typography, independently
packaged service crates and optional real-browser/task-harness adapters. Public
package-registry publication and stable cross-version checkpoint migration are
separate release work; this workspace does not claim those guarantees.

## OS desktop follow-up

Five native desktop/mobile shells now render and interact through the canonical Rust scene pipeline. The [desktop GUI notes](desktop-gui.md) describe profiles, controls, rendering and fidelity limits; [source archaeology](../research/desktop-visuals.md) identifies the recovered predecessor patterns. All five passed offline browser verification. The most recent acceptance suite passes 178 tests, with zero failures. Cross-language desktop checkpoint restoration reproduces identical semantic state and pixels. The performance table above remains the previously pinned benchmark run, not a new measurement of desktop-shell workloads.

## Desktop overhaul and consumer examples

Desktop windows now retain individual geometry, stacking, minimized/maximized/snap state, independent browser content and pointer capture. Dragging, eight resize handles, snap, restore and mobile navigation run inside Rust and survive snapshots. Five platform shells use shared decoded image assets, cached soft shadows and proportional UI typography. Structured service pages now provide native Mail/Documents/Chat/Calendar layouts while retaining their actual network state and actions.

The new [Python and JavaScript walkthrough](programmatic-computer-use.md) runs equivalent persistent-session actions, exports observations/scenes/pixels/trajectories, restores snapshots, forks and verifies replay. Cross-language checks compare exact scene and pixel output. Repository publication is at https://github.com/JacobFV/computerworld; PyPI/npm releases remain separate work.

The photographic assets materially increase the Wasm download; the measured sizes are two paragraphs below. The performance table above measures the earlier pinned workloads, not the new asset-rich shells. Desktop shells remain visual approximations: typography, mobile system panels, native settings and application breadth do not reproduce every real OS behavior.

The final overhaul native verification passed **203 tests** and strict workspace Clippy. Python/Node examples reproduce identical state, scenes and pixels after 11 input actions. Current Wasm is 17,165,529 bytes raw / 12,254,195 gzip; the locally built Python wheel is 13,683,421 bytes. Asset/font copyright notices ship with both bindings. These are local verification results, not a claim of completed remote CI or published package registries.

Browser verification at the time also passed all five OS profiles: 55 overhaul interaction checks, the existing desktop suite and the world-console lifecycle suite, with zero outbound requests after boot. A visual review recorded remaining fidelity gaps separately from behavior checks.
