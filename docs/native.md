# Native Rust usage

Depend on the facade crate `computerworld`, or select lower-level workspace crates
when implementing an embedding. Registry publication is separate from building
this checkout. Run the packaged examples with Cargo; their target names are in
[`crates/computerworld/Cargo.toml`](../crates/computerworld/Cargo.toml).

```rust,ignore
use computerworld::{ActionEnvelope, EnvironmentConfig, World, WorldDefinition};
use serde_json::json;

let definition = WorldDefinition::from_json(world_json)?;
let machine = definition.computers[0].id.clone();
let actor = definition.computers[0].user.clone();
let mut world = World::new(definition, 7)?;
let session = world.environment(EnvironmentConfig::terminal(actor, &machine))?;
let before = world.snapshot();
let result = world.step(&session, vec![ActionEnvelope::new(
    "terminal.v1", "execute", &machine, json!({"command": "pwd"}),
)])?;
assert!(result.outcomes[0].success);
let observation = world.observe(&session)?;
let branch = world.fork(&before)?;
world.restore(&before)?;
```

The default constructor composes standard services. Use `World::with_registry`
for your own service registry. Register agent-driven custom applications with
`World::register_application`. `World::from_json` combines parsing
and construction. `scene` requests structured layout, `render` requests RGBA when
the render feature is enabled. The renderer cache is disposable and excluded
from semantic state; snapshots preserve runtime/session state.

`runtime()` / `runtime_mut()` and `interfaces()` / `interfaces_mut()` are privileged
embedding hooks, not actor capabilities. Use them to register custom interfaces,
queue network work or perform evaluator diagnostics. When exposing an agent API,
route requests through its granted session rather than forwarding arbitrary owner
methods.

## Persistent owner JSON-lines transport

`cargo run -p computerworld --bin cw` starts an optional persistent stdin/stdout
JSON-lines process. It holds one canonical Rust `World`; keep it alive across
steps rather than launching a process per action. Each input line is an object
with `op`; replies are `{ "ok": true, "value": ... }` or
`{ "ok": false, "error": ... }`.

Start with `{"op":"create","definition":{...},"seed":7}`. Omitting the
definition explicitly selects the packaged reference example. Supported owner
operations include `environment` (`config`), `step` (`session`, `actions`),
`observe`, `scene`, `render`, `export_snapshot`, `import_snapshot`, `reset`,
`trajectory`, `inspect` and `state_hash`. The `render` response contains frame
size, byte count and SHA-256 rather than sending RGBA in JSON.

This CLI is a **privileged owner transport**, not a restricted agent endpoint.
Do not expose it directly to an untrusted policy: it can create grants, inspect
state and import checkpoints. A harness should forward only validated actor
operations for a fixed session. The executable's host stdio is deliberately
outside the pure library boundary and does not change simulator semantics.

For what a session can then do, see the [agent API](agent-api.md) and
[action families](action-families.md); for building the crate into something else, the
[architecture](architecture.md) map and the crate boundaries it describes. The Python and
JavaScript bindings wrap this same facade: [Python](python.md), [JavaScript](wasm.md).
