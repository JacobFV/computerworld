# ComputerWorld documentation

ComputerWorld is a computer world simulator: several machines with real filesystems,
shells, applications and a browser, joined by a synthetic internet of services, all
running inside one deterministic Rust engine. An agent gets a restricted handle to a
machine, sends actions, and reads back a scene or a rendered frame. The same engine runs
natively, from Python, and as WebAssembly in Node and the browser, and a run is a pure
function of (engine version, world, seed, action sequence, viewport): replay it anywhere
and you get the same bytes.

That property is the point. It makes computer-use episodes reproducible, checkpoints
cheap to fork, and every frame free labelled training data, because the scene tells you
what every pixel is before it is drawn.

## Install

```sh
pip install computerworld        # Python 3.9+
npm install computerworld        # Node and browsers, one package
```

```toml
# Rust: the workspace is not on crates.io yet, so depend on the tag
computerworld = { git = "https://github.com/JacobFV/computerworld.git", tag = "v0.2.0" }
```

Pin the version. Snapshots and state hashes name the engine that made them, and 0.x
releases may change APIs and world schemas. The [Python](python.md), [JavaScript](wasm.md)
and [Rust](native.md) guides cover platforms, building from source and version checks.

## A first episode

A world comes from a definition, a JSON document naming computers, their operating
systems, the network between them and the services on it. The repository ships one, the
reference company in `worlds/company-2026/world.json`, with five computers on three OS
profiles. Load it with a seed, grant an actor a machine and some action families, and
step.

```python
import json
from computerworld import World

with open("worlds/company-2026/world.json") as f:
    world = World(json.load(f), seed=7)

env = world.environment({
    "actor": "alice", "machines": ["alice-mac"],
    "actions": ["terminal.v1", "application.v1", "pointer.v1", "keyboard.v1"],
    "observations": ["semantic.v1"],
})

result = env.step([{"family": "terminal.v1", "op": "execute",
                    "machine": "alice-mac", "payload": {"command": "cat launch.txt"}}])
print(result["outcomes"][0])         # success, value or error, per action

scene = env.scene(1440, 900)         # every window, widget and text run, with bounds
frame = env.render(1440, 900)        # {"width", "height", "rgba": bytes}
```

```js
import init, { World } from "computerworld";
await init();

const world = new World(definition, 7n);
const env = world.environment({ actor: "alice", machines: ["alice-mac"],
  actions: ["pointer.v1", "keyboard.v1", "application.v1"], observations: ["semantic.v1"] });

env.step([{ family: "application.v1", op: "launch", machine: "alice-mac", payload: { kind: "code" } }]);
const scene = env.scene(1440, 900);
const frame = env.render(1440, 900);   // frame.rgba is a Uint8Array
```

```rust
use computerworld::{reference_world, ActionEnvelope, EnvironmentConfig, World};

let mut world = World::new(reference_world(), 7)?;
let session = world.environment(EnvironmentConfig::desktop("alice", "alice-mac"))?;
world.step(&session, vec![ActionEnvelope::new(
    "terminal.v1", "execute", "alice-mac", serde_json::json!({"command": "cat launch.txt"}),
)])?;
let frame = world.render(&session, 1440, 900)?;
```

Three things to know from the start:

- **Check each result.** `step` takes a batch and returns one outcome per action. A
  denied or failed action is not an exception; it is a result with an error.
- **The `Environment` is the agent's handle.** It can act on its machines and observe
  them, and nothing else. The `World` is the owner's: it inspects everything, edits the
  topology and takes snapshots. Give a model the environment, not the world.
- **Scenes before pixels.** `scene(w, h)` is the retained scene graph at that viewport:
  roles, names, text and geometry. Many agents never need `render`.

The [agent API](agent-api.md) explains grants, sessions, observations and evaluation;
[action families](action-families.md) lists every family, op and payload; and
[programmatic computer use](programmatic-computer-use.md) walks through pointer and
keyboard control with runnable demos in `examples/`.

## Checkpoints, forks and replay

```python
checkpoint = world.snapshot()         # copy-on-write, cheap
branch = world.fork(checkpoint)       # an independent world from that point
world.restore(checkpoint)             # back to it
world.state_hash()                    # the same on every platform for the same run
world.export_snapshot()               # JSON, restorable by the same engine version
world.trajectory()                    # every action so far, replayable
```

Two worlds forked from one checkpoint and fed the same actions produce the same state
hash and the same frames, on Linux, macOS, Windows and in a browser. The release
pipeline refuses to publish unless they do. [Determinism](determinism.md) says exactly
what is promised and how to use frames as labelled data.

Because state is hashable, the transition relation is deterministic and backtracking is
a fork, the same world can be *searched* rather than sampled.
[Bounded model checking](checking.md) enumerates every state reachable within `k`
actions of a start state and returns either a certificate for that bound or a literal
action sequence that breaks a policy, replayed from a fresh world before it is written
down.

## What is inside a machine

Each computer has an inode filesystem with owners and modes, processes, users, installed
packages, and a shell that runs a documented subset of POSIX or PowerShell. Machines run
real Python and JavaScript through embedded interpreters, and a program in the world can
be debugged from Visual Studio Code. On top sits a window manager in one of five styles
(macOS, Windows 11, Ubuntu 24, iOS 18, Android 12) with native applications: a terminal,
file manager, editor, browser, spreadsheet, and professional tools on exact engines.

<ul class="cards">
<li><a href="computers.md">Computers</a><span>Filesystems, processes, users, packages.</span></li>
<li><a href="shell.md">Shell</a><span>The POSIX and PowerShell subsets.</span></li>
<li><a href="desktop-gui.md">Desktop GUI</a><span>Shells, windows, launchers, targets.</span></li>
<li><a href="debugging.md">Debugging</a><span>Breakpoints inside the world from VS Code.</span></li>
<li><a href="networking.md">Networking</a><span>DNS, routes, transports, HTTP.</span></li>
<li><a href="rendering.md">Rendering</a><span>Scenes, text and the frame contract.</span></li>
<li><a href="checking.md">Bounded checking</a><span>Certificates and counterexamples.</span></li>
</ul>

## The synthetic internet

Machines talk to services over a modelled network: DNS, routes, links with latency, and
HTTP. Services are independent state machines with their own storage: mail, chat
(Slack and Discord), phone messages, documents, drive, calendar, Git remotes, issues,
search, wiki, forum, social, press, media and speakers, shop, bank, maps, static sites
and an assistant. The reference world hosts eighty-seven of their sites.

Every one of them serves HTML, and the browser is a web engine: an HTML 5 parser, a CSS
cascade over 124 longhands, block, inline, table, flex and grid layout in fixed point,
paint, and the page's own JavaScript on a DOM, CSSOM and event loop driven by the world
clock. React, Vue, Svelte, Tailwind and jQuery bundles run unmodified. It is measured
element by element against Chromium on eleven fixture pages and against 1,586 Web
Platform Tests reftests; Acid1 and Acid2 render pixel for pixel. No Chromium process is
involved and nothing leaves the world.

Nothing reaches the host. The engine has no filesystem, socket, clock, entropy or
network access unless the owner wires an adapter in explicitly.
[Security](security.md) describes the isolation model.

## Building your own world

A world definition is data. Add computers and OS profiles, wire a network, choose which
services run and seed their state. Applications and services are Rust crates behind
small SDKs, and the guides include a worked example of each.

<ul class="cards">
<li><a href="world-schema.md">World schema</a><span>The definition document.</span></li>
<li><a href="custom-world.md">Custom world</a><span>Writing one from scratch.</span></li>
<li><a href="application-sdk.md">Application SDK</a><span>What an application is to the kernel.</span></li>
<li><a href="custom-application.md">Custom application</a><span>A native app, end to end.</span></li>
<li><a href="service-sdk.md">Service SDK</a><span>What a service is to the network.</span></li>
<li><a href="custom-service.md">Custom service</a><span>A service, end to end.</span></li>
</ul>

## API reference

<ul class="cards">
<li><a href="api-python.md">Python</a><span>World, Environment, Snapshot.</span></li>
<li><a href="api-javascript.html">JavaScript</a><span>The npm package's declarations.</span></li>
<li><a href="api/rust/computerworld/index.html">Rust</a><span>The crate, from rustdoc.</span></li>
</ul>

## Working on ComputerWorld

The engine is a Cargo workspace of about fifty crates; [architecture](architecture.md)
maps them and the boundaries between them. `bash scripts/test-all.sh` runs the native
tests, boundary checks and lints and builds the Wasm; `bash scripts/smoke-bindings.sh`
proves Node and Python agree on checkpoints and hashes. [Releasing](releasing.md)
describes how a version is built on every platform, compared, and published to PyPI and
npm from one GitHub release. Issues and pull requests are welcome at
[github.com/JacobFV/computerworld](https://github.com/JacobFV/computerworld).
