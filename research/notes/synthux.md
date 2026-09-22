# SynthUX archaeology

Pinned source: `jacobfv/synthux` at `81f2f320012f4e2724ca469bebec99193d395e79` (2026-06-08); full git clone `/tmp/computerworld-sources/synthux`. MIT, copyright 2026 Jacob Valdez (`LICENSE`). No AGENTS.md found in clone or applicable workspace ancestors. This is research, not implementation.

## Provenance rows

| Subsystem | Best current implementation in SynthUX | Source path | Important alternatives | What should survive |
|---|---|---|---|---|
| Causal streams/scheduler | Logical-time scheduler and origin-tagged events | `src/synthux/scheduler/scheduler.py`, `world/{clock,channels,events,origin}.py` | Wall-clock threaded `runtime.py`; unified simulator's tick semantics | Explicit causal identity, seeded total ordering, delayed messages, deadlock diagnostics; remove grammar/node-tree coupling |
| Shared service state | Python world stores | `src/synthux/world/{mail,chat,calendar,git,apps_state}.py` | Independently hosted mock pages, which often have weaker/disconnected persistence | Recipient delivery, channel membership, reactions, calendar RSVP, git branches/PR review+merge, document comments/revisions, spreadsheet formula subset as optional packages |
| Virtual internet | Stateful API + mounted independent sites | `src/synthux/internet.py`, `internet_server.py`, `services.py` | Synthex's early VirtualInternet; unified InternetFabric | OS-independent service identity and request/response boundary, default deny, shared state visible across machines; replace host HTTP/proxy with deterministic internal network |
| Agent/visual execution | v2 Viewport + app-keyed dispatch | `src/synthux/viewport/{viewport,driver}.py`, `apps/base.py`, `resolver.py` | v1 `_compilers` and fallback workbench | Input-to-observation alignment, explicit unavailable action failure, app capability registry, hit-test metadata, active versus passive observations |
| Rich desktop appearance | Three pinned web desktop submodules | `sim/browser-os`, `sim/macos-web-next`, `sim/windows-web-next` | New custom scene renderer | Visual/interaction examples and fixture inspiration; no Chromium/DOM/selectors in canonical core |
| World ecosystem | Company/worker/device/service descriptors + bootstrap | `src/synthux/world/economy.py`, `economy/sample.py` | Generic unified serialized topology | Reproducible optional world blueprints; no Acme/company schema in kernel |

## Lineage reconstructed from code/history

Initial commit `08f0f35` (May 23) created SynthUX as synthetic visual trajectory generation, not as a fork identifiable by common git ancestry with the other requested projects. History search does not show explicit synthex/commandagi/synthetic-computer references. Similar names alone are not evidence of copied networking implementation.

- `16f4bb0` / `8af26ee` (May 24): target-trajectory executor and live web desktops.
- `444bde1` then `e6ce282`: low-level input events and validation.
- `49d22fe` (May 26): v2 foundation added schema, Fraction logical clocks, channels, origins, append-only event log; v1 deliberately preserved alongside.
- `dcd0f9e`: v2 stores/resolver/14 affordance surfaces.
- `148a6d0`: multi-viewport runtime; `ca0e91e`: desktop submodules.
- `091117f` (May 27): first Python virtual internet, mail/docs APIs and deny-by-default gateway. `f495032` adds chat routes; `892007f` injects client into devices; `07d13bc` autostarts a host HTTP server; `b0a7ae7` serves GitHub HTML.
- `0e0c48a`: standalone service reverse-proxy mounts; `a0c7b2c`: outbound fallback; `91ca7e9`: blocked hosts get plausible synthetic pages and universal URL rewriting.
- `dbf7adb`: hibernated actor path, discussed below. `f971995`: large economy scale tooling.
- `a85a79b` / final docs: rendering/fixture followups, not a new semantic kernel.

The README says v1 still drives one dataset and contains fallback overlay language. Actual v2 `Viewport.execute` explicitly says no FallbackWorkbench and reports `app.native_unavailable`. Use code as authority; neither a clean v1 replacement nor one globally unified runtime exists at HEAD.

Desktop gitlinks: browser-os `36d4e90f3855ba9adb64f66b9ebb497132a10e8e`, macos-web-next `58f35ecde1f6c4ed079e3ef4ef99d54f29c6f41c`, windows-web-next `948203945a7dae4f54c65cf098fe3f5c9f34750b`. These external desktop repositories were not recursively cloned in this arm; orchestration and rendering costs are verified directly in SynthUX code.

Services agent verified all six service gitlinks exactly equal standalone HEADs, so there is no hidden vendored service divergence here. See `services.md` for fixture/license/API details.

## Actual architecture and surviving strengths

`World.fresh` constructs one world with named stores, a MultiStreamClock, ChannelBroker, EventLog, subscriptions, economy descriptors, and a seed. World mutations have Origin `{stream,node_id,t}`; `World.record_event` derives event IDs from origin/kind/log sequence. EventLog rejects time regressions within each stream. App-facing `Viewport` binds actor + machine + environment and accumulates observed trajectories separate from world events. This conceptual split is useful, even though implementation remains coupled to grammar execution and a live Playwright page.

The logical Scheduler chooses ready nodes by `(logical start, seed/path-derived tiebreak, sequence)`, expands grammar nodes, executes terminals, emits/consumes channels, and detects deadlock. Channel delay distributions are explicit Zero/Const/Uniform; multiplicity one consumes, many leaves a message available. Important correction: many does NOT track per-consumer acknowledgments; repeated consumes can return the same first message. `AfterAny` is named misleadingly: implementation takes max(all finish times), effectively all predecessors.

Strong services are Python stores, not necessarily their corresponding rendered pages. MailStore sends to recipient/cc inboxes and tracks per-actor folder/labels; ChatStore checks sender membership and offers reactions; CalendarStore stores attendees, recurrence descriptors and responses; GitServer has commits, branches, PRs, review-gated merge (simplified base-ref fast-forward); AppsStateStore has sheets, decks, paragraphs, comments and revisions. Preserve tested semantic subsets while adding consistent authorization. Existing authorization is incomplete: mail.send validates actor matches sender; label/archive don't consistently validate owner; calendar mutation methods and chat.react are weaker than creation/post rules. Origin tags are provenance, not a security capability.

VFS is only one global path→bytes dict using PurePosixPath; paths containing `../..` are not resolved, no per-computer mounts/process isolation/permissions are modeled here. The rich desktop apps have separate client-side stores. Do not port this VFS as the strongest implementation.

## Networking, persistence and host escape gaps

`VirtualInternet` exposes mail/chat/docs JSON routes and HTML surfaces. Native apps call injected `window.__synthuxInternet` via actual fetch to one host HTTP server. Registered websites are proxied by mount name, not independently resolved DNS nodes; no modeled interfaces, DNS, links, routes, transport connections or packets.

- `route_html` recognizes `/web/<name>` and GET-proxies configured service URLs with urllib. It does not dispatch synthetic network requests.
- Unrecognized dotted host names call outbound_get; default blocked requests become plausible HTML with HTTP 200. This obscures causal denial. Native network must preserve explicit errors unless the world has an actual synthetic site for that domain.
- Gateway allowlist checks only initial URL hostname before urllib follows redirects. Registered service URLs bypass gateway. These are not safe canonical capability boundaries.
- HTML reverse proxy only forwards GET; `do_POST` targets JSON routes rather than downstream HTML forms. Services agent confirmed form mutation gap.
- Inline docs dict, world application documents, and standalone docs page are separate states. GitHub HTML fixture is not automatically the Python GitServer's branches/PRs. Shared visual appearance does not establish shared semantic state.
- `actor_from_address` discards email domain, collapsing identities such as alice@company-a and alice@company-b.
- Internet origins use `Fraction(0)` rather than advancing world time.
- Host dependencies: Python ThreadingHTTPServer, sockets/free-port discovery, urllib, subprocess service startup, local simulator config paths, Playwright/browser processes, screenshot files, recorded video, real wall-clock timeout/sleeps. Keep these only in explicit compatibility/benchmark adapters.

## Determinism and performance limitations

Logical schedule construction is substantially deterministic, but production runtime is not the same execution model. `runtime.py` expands with a no-op terminal callback, then groups nodes into per-actor threads. Each rendered worker launches its own Chromium and browser context with video recording, navigates a real server, waits 700 ms at startup and 400 ms at end, then invokes UI handlers. ThreadSafeBroker uses wall-clock Condition deadlines; runtime origins frequently remain t=0. Shared mutation/event ordering follows actual thread arrival, making the event sequence/hash depend on scheduling. World.fresh creates ChannelBroker without seeded RNG, so custom Uniform delays need explicit wiring.

`_service_stream_worker` for hibernated actors records a successful terminal event/observation but does not execute application mutation. It does process emit/wait nodes. The scale report claiming 288 workers, 9 rendered and 279 service-executed over 181.5 seconds (`docs/economy-scale-qc.md`) is not evidence for 288 semantically complete simulated computers. Preserve optional rendering, but it must never change the semantic execution path.

ExecutionDriver captures screenshots via `page.screenshot`, hashes PNGs, writes files; hit tests via elementFromPoint/getBoundingClientRect; cursor movement calls JS/Playwright; text executes events per character (screenshots at first/last/every 16th). App handlers use repeated selector probing and real waits. New benchmark must separate browser startup, intentional idle time, action dispatch, DOM/layout, screenshot/PNG and file/video I/O. Comparing all of this to a Rust no-op scene update would not quantify renderer speedup fairly.

## Verification and reproducible baseline plan

Executed without installing dependencies:

```
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_internet.py' -v
# 6 tests passed, 0.013 seconds
PYTHONPATH=src python3 -m unittest discover -s tests -p 'test_v2*' -q
# 89 tests passed, 0.042 seconds
```

These test direct stores/logical scheduling/stub viewports, not browser fidelity or host isolation. No rendering speed measurements have been made in this research phase.

A credible later benchmark can pin this checkout + desktop/service submodule revisions, warm one Chromium context, serve an existing SynthUX HTML route as fixed fixture, execute the same visible action sequence and viewport dimensions as the new scene renderer, and capture: (1) semantic update only, (2) update+layout/hit-test, (3) RGBA raster readback, (4) PNG separately. Fix font assets/DPR, disable networking and animations, report fresh/cold lifecycle separately; never silently remove work from one side. Record p50/p95 plus throughput and peak memory over multiple samples. A second full-run measurement should retain normal old orchestration and label it end-to-end, including its sleeps/video cost.

## Carry forward / do not carry forward

Keep causal event origins and independent service identity; preserve pure store behavior in optional Rust service packages with one canonical backing state for API + browser + direct app; keep optional actor-bound views and trajectory alignment. Reuse v2 tests as semantic acceptance examples with corrections noted above.

Drop grammar trees/goals/dataset schemas from simulator core, Chromium-first representation, selector compilers, fallback fabricated success, hidden hibernation no-ops, host HTTP as internal networking, global shared filesystem, arbitrary wall-clock sleeps, and global mutable app/production registries. Do not promise old screenshot parity; promise documented semantic compatibility and explicit visual fixtures where selected.

Additional actor-boundary issue: `apps/_native_base.py:209` default `observe(page, action, ctx)` never reads `page`; it returns ok=true and `visibleText` extracted from requested action arguments. This is intended-action echo, not an independently observed result. The Rust actor interface must derive observations from canonical visible state and must not equate dispatch completion with successful state mutation. The same file's docstring promises launch like a real user, while macOS launch actually first invokes `window.__synthuxLaunchApp` via script_eval before UI fallback. Preserve explicit action modality tags rather than trusting broad low_level_input labels.
