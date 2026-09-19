# Python binding

Python uses a PyO3 extension containing the canonical Rust runtime. No Node,
Chromium or simulation server is required. CPython 3.9+ is supported by the abi3
binding. A compatible prebuilt wheel needs no Rust toolchain.

## Install

```sh
python -m venv .venv
# Linux/macOS: source .venv/bin/activate
# Windows PowerShell: .venv\Scripts\Activate.ps1
python -m pip install computerworld==0.1.0
python -c "import computerworld; print(computerworld.__version__, computerworld.engine_version)"
# Expected: 0.1.0 0.1.0
```

PyPI carries wheels for Linux x86-64 (manylinux2014), macOS arm64 and Windows x64, and
no source distribution: on any other platform, build from source as below. The same
wheels are assets of
[v0.1.0](https://github.com/JacobFV/computerworld/releases/tag/v0.1.0) with a
`SHA256SUMS`; the release workflow publishes to PyPI the files it downloaded from that
release and checked against it, so the two are byte-identical. Wheels
contain the runtime, not an implicit company world. Download the tagged source
archive for example world definitions and runnable Python demos, or supply your
own definition. API and checkpoint compatibility may change between 0.x releases;
keep the exact version with your episode records.

## Build from source

A Rust toolchain is required for this path. Use the release tag for a reproducible
checkout (omit `--branch` to work on current development instead):

```sh
git clone --branch v0.1.0 https://github.com/JacobFV/computerworld.git
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
python -m pip install --force-reinstall target/wheels/computerworld-0.1.0-*.whl
python -c "import computerworld; print(computerworld.__version__, computerworld.engine_version)"
# Expected: 0.1.0 0.1.0
```

Install by exact version, not `computerworld*.whl`. `target/wheels/` is a build
directory: it accumulates wheels from every revision you have built, and a glob can
silently install an older one. If `engine_version` does not match the source you
built, you installed a stale wheel — that is the usual cause of an example failing
with missing interactions or `application module versions differ`. CI builds
release candidates into `target/release-wheels/` instead, and
`scripts/smoke-bindings.sh` uses `target/python-wheel/` with an exact pin.

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
individual failed actions appear in `result["outcomes"]` and must also be checked. Codes
and refusal reasons are in [Errors and refusals](agent-api.md#errors-and-refusals).

## Threading

`World`, `Environment` and `Snapshot` may be passed to other threads and called from
them. A `World` and every `Environment` minted from it share one lock around the Rust
world, so calls from several threads are **serialized**, not concurrent: a call that
arrives while another is running waits for it. Nothing is silently dropped and nothing
races.

```python
import threading
frame = {}
t = threading.Thread(target=lambda: frame.update(env.render(320, 240)))
t.start(); t.join()          # works: the render happens on that thread
```

What threads do *not* buy you is parallel simulation. One `World` is one state machine,
and the order its actions land in is the order the threads took the lock — which is not
deterministic across runs. **Keep an episode on one thread**, or give each thread its own
`World` (`World(definition, seed)`, or `world.fork(world.snapshot())`, whose result is an
independent world). Threads are for keeping a UI responsive or for driving independent
worlds side by side, not for splitting one episode.

If a call panics inside the simulation, the lock is poisoned and every later call on that
world raises `RuntimeError` naming the situation. That is a deliberate change from
earlier alpha builds, where the objects were marked `unsendable` and a call from a second
thread **aborted the interpreter** instead of raising. Nothing here aborts the host
process. `crates/python/tests/test_threading.py` asserts both halves: a call from a
second thread succeeds, and a poisoned world raises rather than dying.

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
