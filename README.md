# ComputerWorld

**A deterministic computer world for training and evaluating computer-use agents.**
Several machines with real filesystems, shells, applications and a browser, joined by a
synthetic internet of services, inside one Rust engine that runs natively, from Python,
and as WebAssembly in Node and the browser. A run is a pure function of (engine, world,
seed, action sequence, viewport): replay it on any platform and get the same bytes.

[Site](https://jacobfv.github.io/computerworld/) ·
[Documentation](https://jacobfv.github.io/computerworld/docs/) ·
[PyPI](https://pypi.org/project/computerworld/) ·
[npm](https://www.npmjs.com/package/computerworld) ·
[Releases](https://github.com/JacobFV/computerworld/releases)

## Why

- **Verification, not vibes.** Every frame and every state hash is reproducible across
  Linux, macOS, Windows and the browser. An episode, a benchmark result or a bug report
  is a world, a seed and an action list, and anyone can replay it exactly.
- **Cheap branching.** Snapshots are copy-on-write. Fork a world at any step, try
  several futures, keep the best: tree search, counterfactuals and RL rollouts without
  virtual machines.
- **Free labels.** `scene(w, h)` gives every text run, widget and window with its bounds
  before rasterization, so a rendered frame comes with its own ground truth. One
  consumer trained an OCR model this way and moved held-out-font accuracy from 0.668 to
  0.794 ([determinism](docs/determinism.md#rendered-frames-as-labelled-data)).
- **Nothing escapes.** The engine has no host filesystem, socket, clock, entropy or
  network access unless the owner wires an adapter in. Run thousands of agents in one
  process.
- **One engine, three languages.** Python, JavaScript/TypeScript and Rust call the same
  code and produce identical state hashes; the release pipeline refuses to publish if
  they do not.

## Install

```sh
pip install computerworld        # Python 3.9+ (Linux x86-64 and arm64, macOS arm64 and x86-64, Windows x64)
npm install computerworld        # Node and browsers, one package
```

```toml
# Rust: not on crates.io yet, so depend on the tag
computerworld = { git = "https://github.com/JacobFV/computerworld.git", tag = "v0.2.0" }
```

Pin the version. This is 0.x: APIs, world schemas and snapshots may change between
minor versions, and snapshots and state hashes name the engine that made them. The same
wheels and package are on each [GitHub release](https://github.com/JacobFV/computerworld/releases)
with checksums. Guides: [Python](docs/python.md), [JavaScript](docs/wasm.md),
[Rust](docs/native.md).

## Sixty seconds

```python
import json
from computerworld import World

world = World(json.load(open("worlds/company-2026/world.json")), seed=7)

env = world.environment({
    "actor": "alice", "machines": ["alice-mac"],
    "actions": ["terminal.v1", "application.v1", "pointer.v1", "keyboard.v1"],
    "observations": ["semantic.v1"],
})

result = env.step([{"family": "terminal.v1", "op": "execute",
                    "machine": "alice-mac", "payload": {"command": "cat launch.txt"}}])
print(result["outcomes"][0])          # success, value or error, per action

scene = env.scene(1440, 900)          # every window, widget and text run, with bounds
frame = env.render(1440, 900)         # {"width", "height", "rgba": bytes}

checkpoint = world.snapshot()         # copy-on-write
branch = world.fork(checkpoint)       # an independent world from here
world.state_hash()                    # identical on every platform for this run
```

The `Environment` is the agent's handle: its machines, action families and observation
channels, nothing else. The `World` is the owner's: inspection, topology, snapshots.
Give a model the environment. The same episode in
[JavaScript and Rust](https://jacobfv.github.io/computerworld/docs/#a-first-episode),
and runnable demos in [`examples/`](examples/).

## What is in a world

- **Machines.** Inode filesystems with owners and modes, processes, users, packages,
  and a shell running documented subsets of POSIX and PowerShell. Real Python and
  JavaScript run through embedded interpreters, and a program in the world can be
  debugged from Visual Studio Code.
- **Desktops.** Window managers in five styles (macOS, Windows 11, Ubuntu 24, iOS 18,
  Android 12) with native applications: terminal, file manager, editor, browser,
  spreadsheet, image and video editors, and professional tools on exact engines.
- **A synthetic internet.** DNS, routes, links with latency and HTTP between machines
  and services: mail, chat, texting (iMessage/SMS), Slack, Discord, documents, drive,
  calendar, Git remotes, issues, search, wiki, forum, social, press, media, shop, bank,
  maps and an assistant, each with its own state, plus a browsable web of independent
  sites. The browser renders received
  pages natively; no DOM or Chromium.
- **Time and chance under control.** Deterministic scheduling, controlled RNG and IDs,
  persistent sessions, portable snapshots and recorded-action replay.

The reference world, `worlds/company-2026/world.json`, is five computers on three OS
profiles with the services and sites above. Worlds are data, and [`worlds/`](worlds/)
holds three of them — the company, [one desktop for one agent](worlds/agent-desktop), and
[a machine with no network at all](worlds/unrelated-lab). Write your own with the
[world schema](docs/world-schema.md), and add applications and services through small
Rust SDKs ([custom app](docs/custom-application.md), [custom service](docs/custom-service.md)).

## Documentation

The [documentation site](https://jacobfv.github.io/computerworld/docs/) has the guides in
reading order and the Python, JavaScript and Rust API references. The sources are in
[`docs/`](docs/):

| To | Read |
|---|---|
| Drive a machine: grants, `step`, observations, evaluation | [Agent API](docs/agent-api.md), [action families](docs/action-families.md), [programmatic computer use](docs/programmatic-computer-use.md) |
| Understand what the desktop and shell can do | [Desktop GUI](docs/desktop-gui.md), [shell](docs/shell.md), [debugging](docs/debugging.md) |
| Build a world, application or service | [Schema](docs/world-schema.md), [custom world](docs/custom-world.md), [app SDK](docs/application-sdk.md), [service SDK](docs/service-sdk.md) |
| Know exactly what is promised | [Determinism](docs/determinism.md), [rendering](docs/rendering.md), [security](docs/security.md), [performance](docs/performance.md) |
| See how it is put together | [Architecture](docs/architecture.md), [computers](docs/computers.md), [networking](docs/networking.md) |
| Upgrade or release | [Migration](docs/migration.md), [releasing](docs/releasing.md), [changelog](CHANGELOG.md) |

## Developing

Rust 1.88 or newer (verified with 1.97.1). The workspace is about fifty crates;
[architecture](docs/architecture.md) maps them.

```sh
git clone https://github.com/JacobFV/computerworld.git && cd computerworld
cargo run --release --example company        # a native episode in the reference world
cargo test --workspace

bash scripts/test-all.sh                     # native tests, boundary checks, lint, Wasm build
bash scripts/smoke-bindings.sh               # Node and Python agree on checkpoints and hashes
```

Python from source (`pip install .`, Rust required) and the Wasm build
(`bash scripts/build-wasm.sh`) are in the [Python](docs/python.md) and
[JavaScript](docs/wasm.md) guides. The site, including the docs, is in
[`site/`](site/README.md). Issues and pull requests are welcome.

## Scope

Fidelity is bounded on purpose and documented per subsystem: POSIX and PowerShell
subsets, synthetic Git over HTTP rather than packfile compatibility, native pages rather
than arbitrary HTML and JavaScript, bundled deterministic fonts rather than full browser
typography. Unsupported operations fail explicitly rather than approximately. Native
extensions are trusted code; restricted handles prevent accidental state leakage, not
hostile memory access in the same process.

The engine is an independent implementation informed by
[eleven predecessor repositories](research/sources.json). Font and asset attribution is in
[the render asset notices](crates/render/assets/README.md). MIT licensed.
