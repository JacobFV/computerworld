# Host isolation and actor boundaries

The default runtime has no ambient host networking. Synthetic local nodes,
synthetic internet nodes and host destinations are distinct policy classes.
An unknown domain is an error, not permission to consult the operating system's
DNS resolver. A host allowlist is not itself an implementation of outbound access:
real traffic requires a separately supplied capability adapter as well as policy.

Pure simulation crates may parse IP addresses using `std::net` value types but
must not open sockets, spawn processes, read host files/time, or draw host random
bytes. [`scripts/check-boundaries.py`](../scripts/check-boundaries.py) checks
for forbidden dependencies and common imports. It is a development guard, not a
formal whole-program capability proof. The core also compiles for ordinary
browser Wasm.

Owner and actor access are different APIs. Owners construct worlds, select grants,
inspect state, export checkpoints and examine complete trajectories. Actors step
only their session and receive its selected observations. Keep private evaluator
answers and predicates out of actor payloads. A terminal grant gives access to
that synthetic machine's commands; narrow the machine set and action families to
match the task.

Registered Rust service/application handlers are trusted code in the same process.
They are not a safe way to execute untrusted native plugins. When embedding the
simulator in a training worker, expose only the restricted actor wrapper/transport
to the policy and retain owner objects in the harness. Wasm host imports, optional
adapters and JavaScript page hosting are integration responsibilities; adding an
adapter must not silently broaden the default core's authority.

Resource budgets and validation reduce accidental misuse, but this release is
not a hostile multi-tenant OS sandbox. Simulated users, processes and files are
world semantics, not host security principals.

The optional native HTTP adapter lives in `cw-host-adapters`, behind its explicit
`native-http` feature. It is excluded from pure-crate dependency policy. The
persistent JSON-lines `cw` executable similarly owns host stdio outside the pure
facade library; the boundary checker skips `src/bin` and `main.rs`, while still
checking library modules. Neither exception installs ambient capabilities in the
simulation. See [network adapters](networking.md) and [owner CLI](native.md).
