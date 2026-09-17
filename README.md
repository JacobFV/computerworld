# computerworld

A reusable synthetic computing world with **one deterministic Rust runtime** for
native Rust, browser/Node WebAssembly, and Python. Define a world, grant an agent
an interface, and run persistent episodes without a host OS or browser process.

The default simulator has no host filesystem, socket, subprocess, clock, entropy,
or network access. The browser demo runs entirely client-side after loading its
static assets. Python calls the Rust runtime directly; Node is not required.

Requires Rust 1.88 or newer to build (verified with Rust 1.97.1).

## Start here

```sh
git clone https://github.com/JacobFV/computerworld.git
cd computerworld

# Native examples and tests
cargo run --release --example company
cargo run --example custom-service
cargo run --example custom-app
cargo test --workspace
```

```rust
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};
use serde_json::json;

let mut world = World::new(reference_world(), 7)?;
let session = world.environment(EnvironmentConfig::desktop("alice", "alice-mac"))?;
let checkpoint = world.snapshot();
let result = world.step(&session, vec![ActionEnvelope::new(
    "terminal.v1", "execute", "alice-mac",
    json!({"command": "cat launch.txt"}),
)])?;
let scene = world.scene(&session, 960, 560)?; // no rasterization
let frame = world.render(&session, 960, 560)?; // canonical RGBA
let branch = world.fork(&checkpoint)?;
world.restore(&checkpoint)?;
```

Supply your own `WorldDefinition` or use `World::from_json`. The reference world
is an explicitly selected example; the kernel contains no company or service
fixtures. Give a native agent `world.actor(&session)?`, a restricted interface,
rather than the privileged owner object.

### Python

```sh
python3 -m venv .venv
. .venv/bin/activate
pip install .                 # build native extension; Rust toolchain required
python examples/python/smoke.py
python examples/python/computer_interaction.py
```

Alternatively install a built wheel from `target/wheels/`; wheel consumers need
neither Rust nor Node. Registry publication is separate; these instructions use
this checkout's package.

```python
import json
from computerworld import World

with open("worlds/company-2026/world.json") as f:
    world = World(json.load(f), seed=7)
env = world.environment(dict(
    actor="alice", machines=["alice-mac"], actions=["terminal.v1"],
    observations=["terminal.v1"],
))
result = env.step([dict(family="terminal.v1", op="execute",
    machine="alice-mac", payload={"command": "cat launch.txt"})])
checkpoint = world.snapshot()
branch = world.fork(checkpoint)
```

### Browser / JavaScript

```sh
rustup target add wasm32-unknown-unknown
# Install the wasm-bindgen-cli version matching Cargo.lock (currently 0.2.128).
cargo install wasm-bindgen-cli --version 0.2.128 --locked
bash scripts/build-wasm.sh
node examples/javascript/computer-interaction.mjs
node examples/browser/build.mjs
python3 -m http.server 8000
```

Try the [private hosted world console](https://computerworld-console.jacobfv123.chatgpt.site), or open `http://localhost:8000/examples/browser/` locally. The server only supplies static assets; it executes
no simulation. The world console shows all seven device screens/consoles and actual network links, including distinct macOS, Windows 11, Ubuntu 24, iOS 18 and Android 12-style shells. Add/remove devices, interact with their applications and peripherals, and save/restore/fork the live topology. [Browser demo instructions](examples/browser/README.md) include
offline verification. The build also emits a Node package under `pkg/node`.

```javascript
import init, { createWorld } from './pkg/web/computerworld.js';
await init();
const world = createWorld(definition, '7');
const env = world.environment({actor: 'alice', machines: ['alice-mac'],
  actions: ['browser.v1'], observations: ['semantic.v1']});
const result = env.step([{family: 'browser.v1', op: 'navigate',
  machine: 'alice-mac', payload: {url: 'http://intranet.internal/'}}]);
const frame = env.render(960, 560); // width, height, Uint8Array rgba
```

## What it models

- Multiple machines and data-defined OS profiles; inode filesystems, permissions,
  links, processes, shell commands, installed packages and applications.
- Source-aware DNS, routes, links, loopback, listeners, timed transports and HTTP;
  independent service state and inspectable causal/network events.
- Mail delivery, shared chat, document revisions, calendars, Git objects/remotes,
  issues/reviews and native websites in optional service crates.
- A synthetic browser with received-page state, history, cookies/storage, forms
  and network-loaded images. No DOM or Chromium is needed.
- Compact scenes, semantic observations, hit testing, keyboard/pointer interaction,
  cached deterministic text and incremental CPU rasterization.
- Controlled time/RNG/IDs, deterministic scheduling, persistent sessions, cheap
  COW checkpoints/forks, portable restore and recorded-action replay.
- Separate actor grants, owner/evaluator inspection and optional task/reward logic.

The reference company has five computers, three OS profiles and eight services.
Its examples exercise local file work, cross-machine Git, mail/document work,
shared chat, browser discovery and service debugging.

## Contracts and extension guides

| Topic | Guide |
|---|---|
| Architecture and crate boundaries | [Architecture](docs/architecture.md) |
| World/topology schema and custom worlds | [Schema](docs/world-schema.md), [custom world](docs/custom-world.md) |
| Native / Python / Wasm interfaces | [Rust](docs/native.md), [Python](docs/python.md), [Wasm](docs/wasm.md) |
| Programmatic computer interaction | [Python/JavaScript guide and demos](docs/programmatic-computer-use.md) |
| Agent actions, observations and evaluation | [Agent API](docs/agent-api.md) |
| Application and service extensions | [App SDK](docs/application-sdk.md), [service SDK](docs/service-sdk.md) |
| Authoring examples | [Custom app](docs/custom-application.md), [custom service](docs/custom-service.md) |
| Semantics and isolation | [Computers](docs/computers.md), [networking](docs/networking.md), [security](docs/security.md) |
| Reproducibility and rendering | [Determinism](docs/determinism.md), [rendering](docs/rendering.md) |
| Measurements and migration | [Performance](docs/performance.md), [provenance](docs/provenance.md), [migration](docs/migration.md) |

Build with `--no-default-features` to omit rasterization from the facade. Host
networking is opt-in through `cw-host-adapters/native-http`, explicit policy and
an owner-supplied adapter; recorded host results can be consumed offline. The
optional `cw` binary is a persistent privileged JSON-lines transport.

## Verification and scope

```sh
bash scripts/test-all.sh          # native, boundaries, lint, optional host adapter, Wasm build
bash scripts/smoke-bindings.sh    # actual Node Wasm + Python checkpoint/hash parity
node scripts/test-browser.mjs     # actual Chromium, episode networking blocked
```

Semantic fidelity is intentionally bounded: documented POSIX/PowerShell subsets,
synthetic Git HTTP rather than packfile compatibility, native pages rather than
arbitrary HTML/JS, bundled deterministic UI fonts rather than full browser typography. Unsupported
operations fail explicitly. Native extensions are trusted code; restricted API
handles prevent accidental state leakage, not hostile memory access in the same
process. Detailed supported behavior and limitations are documented per subsystem.

The [investigation reports](research/sources.json) pin all eleven predecessor
repositories. Their strongest semantics and regression lessons informed this
independently implemented Rust workspace; no predecessor runtime is required.
Bundled asset and font attribution is in [the render asset notices](crates/render/assets/README.md).

The [native desktop GUI](docs/desktop-gui.md) documents OS profiles, functional window/app controls, deterministic rendering and current fidelity limits. [Source research](research/desktop-visuals.md) traces the recovered visual patterns.
