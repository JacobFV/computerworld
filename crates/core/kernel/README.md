# cw-kernel

The canonical persistent simulation runtime. It consumes a validated
`WorldDefinition` and caller-supplied `cw_sdk::Registry`; no world, service catalog,
application, task, reward or language binding is hardcoded here.

`Runtime::new(definition, seed, registry)` initializes computers, seeded service
instances and synthetic networking. `execute`, `read_file`, `write_file` and `http`
are owner APIs. Actor grants belong to `cw-environment`, above this crate.

## Scheduling

Logical time is integer microseconds. `submit_http` resolves synthetic DNS and
routing and schedules delivery. `advance` processes `(due, phase, sequence)` order,
then `take_response` consumes a completed response. `http` is the synchronous
convenience wrapper over those same continuations. Requests and responses each
traverse network timing/policy. Process deadlines are drained before same-tick
network work, so splitting an advance does not change a sleep's completion time.

Services mutate a private candidate state. Failed handlers do not commit it.
Successful handlers can emit events, timers and HTTP effects; peer calls traverse
the network and return through `on_effect`. The scheduler limits each advance to
100,000 continuations and each transition to 1,024 effects. Budget exhaustion
returns an error and leaves remaining work resumable.

Services placed on a computer have synthetic processes owning their listeners.
Stopping those processes removes reachability; restarting the registered service
recreates the listener while retaining its persistent store.

## Checkpoints and isolation

Snapshots share an `Arc` state root; computers, VFS blobs, network state and service
stores detach on mutation. Same-seed reset reuses the initialized root. Different
seeds construct a new deterministic baseline. Portable JSON includes pending
requests, response deliveries, timers, RNG positions and events. Restore validates
engine/module versions, definition identity, computers and scheduler invariants.
It validates before publishing state. Environment checkpoints additionally contain
actor sessions and application/browser state; a kernel checkpoint alone is not a
complete UI checkpoint.

`state_hash` excludes diagnostic journal/packet payload history, includes their
semantic counters, and uses canonical JSON plus SHA-256. Events include causal
network records; full packet traces are also available through `network()`.

Default execution has no host I/O. `begin_external` requires explicit policy and
owner-supplied resolved addresses; it returns an authorization for an external
adapter to enforce. `complete_external` records either a response or failure;
byte/deadline violations become failed responses. Live external operations make
`try_snapshot`, portable export, restore of that snapshot, and fork fail. Restoring
an earlier checkpoint or resetting invalidates outstanding tokens by a separate
host generation. Actual outbound effects cannot be undone. Trusted plugins and
owner adapters are not a native-code security sandbox.
