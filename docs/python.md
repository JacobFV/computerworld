# Python binding

Python uses a PyO3 extension containing the canonical Rust runtime. No Node,
Chromium or simulation server is required. CPython 3.9+ is supported by the abi3
binding. A compatible prebuilt wheel needs no Rust toolchain.

## Install the prerelease wheel

Download the wheel matching your operating system/architecture from
[v0.1.0-alpha.1](https://github.com/JacobFV/computerworld/releases/tag/v0.1.0-alpha.1).
The listed assets determine available platforms. Python's package version is
`0.1.0a1`; this is the same engine release. Verify its checksum using the accompanying
`SHA256SUMS` before installation. There is no PyPI publication yet.

```sh
python -m venv .venv
# Linux/macOS: source .venv/bin/activate
# Windows PowerShell: .venv\Scripts\Activate.ps1
python -m pip install ./computerworld-0.1.0a1-<matching-wheel-tags>.whl
python -c "import computerworld; print(computerworld.__version__, computerworld.engine_version)"
# Expected: 0.1.0a1 0.1.0-alpha.1
```

Replace the example wheel filename with the actual downloaded filename. Wheels
contain the runtime, not an implicit company world. Download the tagged source
archive for example world definitions and runnable Python demos, or supply your
own definition. API and checkpoint compatibility may change between alpha releases;
keep the exact version with your episode records.

## Build from source

A Rust toolchain is required for this path. Use the release tag for a reproducible
checkout (omit `--branch` to work on current development instead):

```sh
git clone --branch v0.1.0-alpha.1 https://github.com/JacobFV/computerworld.git
cd computerworld
python3 -m venv .venv
. .venv/bin/activate
python -m pip install .
python examples/python/smoke.py
python examples/python/computer_interaction.py
```

To build distributable wheels instead:

```sh
python -m pip install 'maturin>=1.7,<2'
maturin build --release --manifest-path crates/python/Cargo.toml
python -m pip install target/wheels/computerworld*.whl
```

Run source examples from the checkout; example world JSON is not an implicit kernel
resource. Your application supplies its own JSON-compatible definition:

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
assert result["outcomes"][0]["success"], result["outcomes"][0]
observation = env.observe()
checkpoint = world.snapshot()
branch = world.fork(checkpoint)
branch_env = branch.session(env.id)
world.restore(checkpoint)
```

Keep `world` and `env` alive for an episode; do not construct a process or runtime
per action. Configuration/actions/results are Python dictionaries and lists.
Python seeds are unsigned 64-bit integers. Invalid binding calls raise `ValueError`;
individual failed actions appear in `result["outcomes"]` and must also be checked.
The handles are synchronous and not thread-shareable; independent processes can
own independent worlds.

`Environment` exposes `id`, `step`, `observe`, `scene(width=1024, height=768)` and
`render(width=1024, height=768)`. Scene access requires a `semantic.v1` or `pixels.v1`
observation grant. `scene` returns layout/interaction data without rasterization.
`render` returns `{"width": int, "height": int, "rgba": bytes}`. Use an image library
to encode those bytes as PNG; no host image library participates in simulation.

See [programmatic computer use](programmatic-computer-use.md) for desktop launch,
keyboard input, pointer dragging, transformed hit targets, PNG output and owner
checkpoint examples. The runnable [computer interaction demo](../examples/python/computer_interaction.py)
is intended as a starting point for an agent integration.

`World` owns reset, snapshots, portable `export_snapshot()`/`import_snapshot(text)`,
`definition()`, `trajectory()`, `state_hash()` and privileged `inspect()`. Retain it
in the harness; provide only the environment to actor code. `world.session(env.id)`
reattaches a restricted handle after fork/import. Python only converts values; it
contains no separate command, service or desktop semantics.

The owner also supports `world.add_computer(computer, node, links)` and
`world.remove_computer(id)`. These preserve other devices and service state.
See [device lifecycle and checkpoint rules](agent-api.md#owner-controlled-device-lifecycle).
