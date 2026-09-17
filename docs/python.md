# Python binding

Python uses a PyO3 extension containing the canonical Rust runtime. Node is not
required. Build a local wheel (registry publication is separate):

```sh
python3 -m pip install maturin
maturin build --release --manifest-path crates/python/Cargo.toml
python3 -m pip install target/wheels/computerworld*.whl
python3 examples/python/smoke.py
```

```python
import json
from computerworld import World

with open("worlds/company-2026/world.json") as source:
    definition = json.load(source)
world = World(definition, seed=7)
machine = definition["computers"][0]
env = world.environment({
    "actor": machine["user"], "machines": [machine["id"]],
    "actions": ["terminal.v1"], "observations": ["terminal.v1"]
})
result = env.step([{
    "family": "terminal.v1", "op": "execute", "machine": machine["id"],
    "payload": {"command": "pwd"}
}])
assert result["outcomes"][0]["success"]
observation = env.observe()
checkpoint = world.snapshot()
branch = world.fork(checkpoint)
world.restore(checkpoint)
```

`Environment` exposes `step`, `observe`, `scene` and `render`. `scene` returns
layout/interaction data without rasterization. `world.session(env.id)` reattaches
a restricted handle after a fork or portable checkpoint import. Rendering returns dimensions
and RGBA bytes. `World` owns reset, snapshots, portable export/import, definition,
trajectory, state hash and privileged inspection. Retain it in the harness;
provide only the environment to actor code. Python values cross a thin serialized
conversion boundary; there is no Python command/service implementation.

The privileged world owner also supports `world.add_computer(computer, node, links)`
and `world.remove_computer(id)`. These preserve other devices and service state;
`world.definition()` returns the current blueprint. Actor environments do not expose
these methods. See [device lifecycle and checkpoint rules](agent-api.md#owner-controlled-device-lifecycle).
