# Python API

The `computerworld` module is a PyO3 extension holding the canonical Rust engine. Every
value that crosses into Python is plain data: definitions, configs, actions, results,
observations and scenes are `dict`s and `list`s with the same shape as the JSON the
[agent API](agent-api.md) and [action families](action-families.md) document. Errors
raise `RuntimeError` with the engine's message. Objects are safe to share between
threads; calls on one world are serialized.

```python
import computerworld
computerworld.__version__      # the package version, PEP 440 ("0.1.0", "0.1.0a3")
computerworld.engine_version   # the engine version, Cargo style ("0.1.0", "0.1.0-alpha.3")
```

## `World`

The owner's handle. It sees and changes everything; give agents an `Environment`.

| Member | Description |
|---|---|
| `World(definition, seed=0)` | Build a world from a definition (`dict`, as loaded from a world JSON) and a seed. Everything downstream is a function of the two. |
| `environment(config) -> Environment` | Grant a session. `config` names `actor`, `machines`, `actions` (families), `observations` (channels) and optionally `action_budget`, the largest batch a step may carry. |
| `session(id) -> Environment` | Reconnect to a session by id, for instance one stored in a snapshot that was restored or forked. |
| `snapshot() -> Snapshot` | A copy-on-write checkpoint of the whole world. Cheap; take them freely. |
| `restore(snapshot)` | Return this world to a checkpoint. |
| `fork(snapshot) -> World` | A new, independent world starting from a checkpoint. |
| `reset(seed=0)` | Rebuild from the definition with a seed. |
| `export_snapshot() -> str` | The current state as JSON. Only the same engine version can import it. |
| `import_snapshot(json)` | Replace the state with an exported snapshot. |
| `state_hash() -> str` | A hash of the whole state, identical across platforms for the same run. It covers the engine version. |
| `trajectory() -> dict` | Every action stepped so far, replayable. |
| `definition() -> dict` | The definition this world was built from. |
| `inspect() -> dict` | Privileged inspection: machines, sessions, network and service state. |
| `add_computer(computer, node, links)` | Add a machine to the topology: its computer definition, its network node, and the links that wire it. |
| `remove_computer(id)` | Remove a machine. |

## `Environment`

An actor's restricted handle: only the machines, action families and observation
channels its grant names.

| Member | Description |
|---|---|
| `id` | The session id. |
| `step(actions) -> dict` | Run a batch of action envelopes in order. Returns one result per action (success, value or error), the observation, the logical tick and the pending count. A batch is a sequence, not a transaction. |
| `observe() -> dict` | The current observation on the granted channels without acting. |
| `scene(width=1024, height=768) -> dict` | The retained scene at that viewport: windows, widgets, text runs and their geometry. No rasterization. |
| `render(width=1024, height=768) -> dict` | `{"width", "height", "rgba": bytes}`, the canonical frame. Bytes are identical on every platform. |

An action envelope:

```python
{"family": "terminal.v1", "op": "execute", "machine": "alice-mac",
 "payload": {"command": "cat launch.txt"}}
```

## `Snapshot`

Opaque. Made by `World.snapshot()`, consumed by `restore` and `fork`. Use
`export_snapshot` for a portable form.

## Example

```python
import json
from computerworld import World

world = World(json.load(open("worlds/company-2026/world.json")), seed=7)
env = world.environment({"actor": "alice", "machines": ["alice-mac"],
                         "actions": ["terminal.v1"], "observations": ["terminal.v1"]})
before = world.snapshot()
out = env.step([{"family": "terminal.v1", "op": "execute", "machine": "alice-mac",
                 "payload": {"command": "echo hello > note.txt"}}])
assert out["outcomes"][0]["success"]
branch = world.fork(before)          # a world where the file was never written
world.restore(before)
assert world.state_hash() == branch.state_hash()
```

See the [Python guide](python.md) for installation and version pinning, and
[programmatic computer use](programmatic-computer-use.md) for pointer and keyboard work.
