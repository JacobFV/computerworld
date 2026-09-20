# JavaScript and browser Wasm

Node and browser clients run the same canonical Rust runtime compiled to Wasm.
They receive generated JavaScript classes and ordinary objects, not raw-memory
management APIs. There is no separate JavaScript simulator.

## Install from npm

```sh
npm install computerworld@0.1.2
```

One package serves both: Node resolves it to the CommonJS binding, which loads the Wasm
beside it, and bundlers and browsers to the ES module, whose default export
initializes it.

```js
// Node (require, or import from an ES module)
const { World, engineVersion } = require('computerworld');

// Browser, through a bundler or an import map
import init, { World } from 'computerworld';
await init();
```

The package holds the Wasm once, the TypeScript declarations for each entry, the
reference world (`computerworld/worlds/company-2026/world.json`), the font pack and the
third-party notices. It is published by the release workflow from the
`computerworld-0.1.2.tgz` asset of the GitHub release, after checking it against that
release's `SHA256SUMS`.

## Install a pinned bundle

Download from [v0.1.2](https://github.com/JacobFV/computerworld/releases/tag/v0.1.2)
and verify the accompanying `SHA256SUMS`. Choose:

- `computerworld-0.1.2-wasm-web.tar.gz`: browser ES module, Wasm and TypeScript declarations.
- `computerworld-0.1.2-wasm-node.tar.gz`: Node CommonJS module, Wasm, declarations and runnable Node demo.

Each archive has a top-level directory matching its filename without the archive
extension. Runtime bundles include `worlds/`, example code, `release.json` with
source/version metadata, and required notices. Keep the module, `.wasm` file and
notices together when copying them into an application. No Rust, wasm-bindgen or
npm installation is needed to use the downloaded bundle.

For Node:

```sh
tar -xzf computerworld-0.1.2-wasm-node.tar.gz
cd computerworld-0.1.2-wasm-node
node -e "console.log(require('./computerworld.js').engineVersion())"
# Expected: 0.1.2
node examples/javascript/computer-interaction.mjs --output ./demo-output
```

For a browser, unpack the web bundle and import its root module:

```js
import init, {World, engineVersion} from './computerworld.js';
await init();
console.log(engineVersion()); // 0.1.2
// Supply your own world definition, or load a bundled worlds/ JSON file.
const world = new World(definition, 7);
```

Serve these files through a static HTTP server rather than opening `file://`. The
server serves static assets only. After bootstrap the simulated episode needs no
network backend. APIs and checkpoint formats may change between alpha releases;
do not mix wrapper/Wasm files from different versions.

## Build and run from source

```sh
git clone --branch v0.1.2 https://github.com/JacobFV/computerworld.git
cd computerworld
rustup target add wasm32-unknown-unknown
# Match Cargo.lock; currently 0.2.128.
cargo install wasm-bindgen-cli --version 0.2.128 --locked
bash scripts/build-wasm.sh
node scripts/smoke-node.cjs
node examples/javascript/computer-interaction.mjs
```

The build creates browser ES modules/types under `pkg/web` and the Node CommonJS
wrapper/types under `pkg/node`. Copy the corresponding directory, including its
`.wasm` file and `notices/`, into your consuming project. Node ES modules can load
the generated Node package with `createRequire`:

```js
import {createRequire} from 'node:module';
import {readFileSync} from 'node:fs';
const require = createRequire(import.meta.url);
const {World} = require('./pkg/node/computerworld.js');
const definition = JSON.parse(readFileSync('./worlds/company-2026/world.json', 'utf8'));
const world = new World(definition, 7);
```

Paths above assume a script at the checkout root. Node initialization is
synchronous. For a browser module at the checkout root:

```js
import init, {World} from './pkg/web/computerworld.js';
await init();
const definition = await (await fetch('./worlds/company-2026/world.json')).json();
const world = new World(definition, 7);
const machine = definition.computers[0];
const env = world.environment({
  actor: machine.user, machines: [machine.id],
  actions: ['terminal.v1'], observations: ['terminal.v1', 'semantic.v1']
});
const result = env.step([{
  family: 'terminal.v1', op: 'execute', machine: machine.id,
  payload: {command: 'pwd'}
}]);
if (!result.outcomes[0].success) throw new Error(JSON.stringify(result.outcomes[0].error));
const scene = env.scene(1024, 768); // no rasterization
const frame = env.render(1024, 768);
const pixels = new Uint8ClampedArray(frame.rgba); // copy before freeing
frame.free();
const checkpoint = world.snapshot();
const branch = world.fork(checkpoint);
const branchEnv = branch.session(env.id);
```

That one `fetch` loads a static blueprint, not simulated traffic. You can bundle
or embed the definition instead. Generated initialization loads the Wasm asset;
after bootstrap, synthetic DNS/HTTP, applications, rendering and checkpoints need
no external requests. A static file server supplies correct JavaScript/Wasm MIME
types; it runs no simulation backend.

## Computer input in a page

[Programmatic computer use](programmatic-computer-use.md) describes keyboard,
pointer drag/resize, app/window focus, scene transforms and Canvas output. The
[Node computer demo](../examples/javascript/computer-interaction.mjs) uses those
same APIs without a browser. API errors throw; failed individual actions are
reported in `outcomes` even when `step` itself returns normally.

The [project site](https://jacobfv.github.io/computerworld/) is the same module in a
page: each screen is a `<canvas>` that forwards coordinates and keyboard events to
Rust and paints the frame Rust returns (`site/live.js`, about two hundred lines). It
does not implement an alternate desktop state machine. Run it locally with

```sh
bash scripts/build-wasm.sh && cp -r pkg/web site/pkg
node scripts/serve-site.mjs 8000
```

Do not install a host-fetch adapter just to make synthetic domains work. A real
HTML/browser compatibility bridge is optional and separate.

## Ownership and lifetime

Pass restricted `Environment` handles to actors. Keep `World` in the trusted
harness for snapshots/reset/fork, `exportSnapshot()`/`importSnapshot(text)`,
`stateHash()`, inspection, topology edits and trajectory access. A fork reattaches
existing grants through `branch.session(env.id)`.

Use a decimal string for a seed beyond JavaScript's safe integer range. Other
serialized integral fields outside that range use `BigInt`; results may contain
`BigInt`, so plain `JSON.stringify` is not a general checkpoint serializer. Use
`world.exportSnapshot()` for lossless portable state. Input must consist of plain
objects/arrays and supported scalar values; functions, undefined and non-finite
numbers are rejected.

Wasm objects expose `free()`: release temporary frames, snapshots and completed
sessions/worlds in long-running consumers. `frame.rgba` returns a copied Uint8Array.
Methods are synchronous; use independent Web Workers/runtime instances for
parallel worlds or to keep large renders off your UI thread. Sharing one handle
across workers is not supported.

Owner-only `addComputer(computer, node, links)` and `removeComputer(id)` change the
canonical runtime; `definition()` reads its blueprint. See [owner lifecycle](agent-api.md#owner-controlled-device-lifecycle).
