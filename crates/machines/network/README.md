# cw-network

Pure serialized synthetic DNS, routing, HTTP delivery, streams, datagrams and
host authorization. This crate performs no IO and imports no host networking API.
`std::net::IpAddr` is used only to parse and compare address values.

`Network::with_seed(&world, seed)` translates generic topology and service placement
into listeners. `prepare_http(source_node, &request, tick)` resolves synthetic DNS,
checks reachability and policy, and returns a service identity and arrival time.
The kernel schedules service invocation. `finish_http` models the response route.
No service handler is embedded in the network crate.

Empty links deny communication between distinct nodes unless the world explicitly
sets `network.implicit_lan = true`. Links can be directed. DNS records can name a
resolver node; both directions of the DNS exchange must be reachable. CNAME chains
are bounded and cycle checked. TTLs and latency use logical microseconds.

Low-level clients can register process-owned `Listener`s, `connect`, `write_at`,
`advance` and `read`. Stream capacity includes pending bytes; no data arrives before
its scheduled delivery. `send_datagram` and `receive_datagram` preserve message
boundaries. Closing a process removes its listeners and closes owned streams.
These are semantic transports, not TCP packet emulation: there is no congestion
control, retransmission protocol or IP fragmentation.

`authorize_host` is a separate, explicit capability path. Its input addresses must
come from an owner-supplied resolver adapter. Every address must pass the policy;
IP literals must match the supplied addresses. The returned authorization contains
pinned addresses, a byte budget and a logical deadline. A `HostAdapter` must enforce
these constraints, avoid independent resolution, disable automatic redirects, and
return each redirect for fresh authorization. Ordinary synthetic HTTP never falls
back to this path. No host adapter is enabled by constructing a network.

All mutable state, including DNS caches, pending streams/datagrams and seeded loss
state, is serializable. `semantic_state()` borrows the state without diagnostic
history for efficient hashing. `traces()` exposes DNS, connection, HTTP and packet
activity to privileged consumers.
