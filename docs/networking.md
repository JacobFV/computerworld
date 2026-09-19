# Synthetic networking

The path is application → URL → source-aware DNS → route and gateway checks →
listener/service → HTTP response → browser page. A browser cannot bypass this path
by consulting backing service state. A second machine sees a mutation after its
own request; cached pages remain snapshots of received content.

Each computer has its own node identity and address. Network definitions contain
links, routes, DNS records, placement and gateway policy. Service instances bind
at their configured node/port and own domain names; unless a service sets
`tls: false` it also listens on 443, so `https://` reaches the same handler as
`http://` and the browser shows whichever scheme was asked for (the first
service placed on a node owns that node's 443). The network distinguishes
local, synthetic internet and host zones. DNS cache expiry and link latency use
logical microseconds; configured loss uses seeded simulation state. Links are
required unless `network.implicit_lan` is explicitly true (default false). DNS
records can carry IP literals or CNAME targets; an explicit resolver must be
reachable from the source.

`cw-network` supplies DNS resolution, reachability/policy decisions, listeners,
HTTP preparation/response traces, stream connection buffers and datagrams.
Timed stream writes use `write_at`/`advance`/`read`; datagrams use explicit
send/receive operations. Transport is semantic rather than packet-level TCP emulation. Listener ownership
allows process cleanup. Network traces expose request/resolution/response and
failure information without host packet capture.

The kernel drives requests and service dispatch in the deterministic scheduler.
Synchronous convenience HTTP uses that same synthetic path. Use queued APIs when
an episode needs an in-flight checkpoint or separately advanced communication.

Default host egress is denied. Unknown names never fall through to host DNS.
Explicit host authorization checks policy and resolved addresses before producing
adapter authorization; the default runtime does not install a real network
adapter. An allow flag alone cannot open a socket. Any future adapter must keep
redirect and address checks inside the capability boundary.

See [world schema](world-schema.md), [service SDK](service-sdk.md), and
[host isolation](security.md). A synthetic public-looking domain does not imply
traffic reaches the real public site with that name.

The native owner's `begin_external` creates an authorized external-effect record
from an explicit address set. A separately implemented adapter performs the I/O
and returns it through `complete_external`. Generation, deadline and response-size
checks reject stale/oversized completions. These APIs do not perform DNS or fetch
on the host by themselves. Live external effects cannot be resumed as deterministic
synthetic work; complete them before creating a portable resumable checkpoint.

The optional `cw-host-adapters` crate supplies two implementations. `RecordedAdapter`
consumes exact request/response exchanges in order, rejects mismatches without
consuming them, and never falls back to live traffic. `NativeHttpAdapter` is
available only with feature `native-http` on native targets. Its reqwest client
pins authorized addresses, disables proxy discovery and automatic redirects,
rejects caller Host overrides and enforces timeout/response-byte limits. The
owner must resolve candidate addresses and authorize **every** address before
calling it; the adapter does not bypass `begin_external`/`complete_external`.
Reauthorize redirects as new effects instead of following them automatically.

Build and test the opt-in adapter separately:

```sh
cargo test -p cw-host-adapters --features native-http
```

Live HTTP results are external input, so reproducible replay must supply recorded
exchanges. `Runtime::try_snapshot` rejects unresolved live effects. The default
simulation and browser Wasm artifacts do not depend on reqwest.
