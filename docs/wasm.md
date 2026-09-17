# JavaScript and browser Wasm

Install the Rust Wasm target and a `wasm-bindgen-cli` version matching the locked
`wasm-bindgen` crate, then build both browser and Node wrappers:

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --locked
./scripts/build-wasm.sh
node scripts/smoke-node.cjs
```

The build creates `pkg/web` and `pkg/node`. Browser consumers import the generated
ES module and await its initializer before constructing `World`; Node consumers
load the Node wrapper. Both execute the same Wasm runtime, with object conversion
provided by wasm-bindgen rather than raw memory manipulation.

The [`examples/browser`](../examples/browser) demo packages static HTML/JS/CSS,
world data and the generated Wasm artifact. Its static asset server is only a
file server; no simulation backend is involved. Once assets are loaded, actions,
HTTP services, state, rendering and checkpoints execute client-side. The offline
verification blocks subsequent requests while driving the episode.

Owner `World` operations include environment creation, snapshots, reset/fork,
portable export/import and diagnostics. Pass its restricted `Environment` to
actor integrations; keep owner inspection out of policy scope. Raster output is
RGBA for a Canvas `ImageData`; semantic observations do not rasterize.

Do not enable a host fetch adapter just to make synthetic domains work. They
resolve inside the topology. A real-browser/HTML compatibility bridge is a
separate optional integration, not a prerequisite for the demo.

Minimal browser usage after generating `pkg/web`:

```js
import init, { World } from "../pkg/web/computerworld.js";
await init();
const world = new World(definition, 7);
const machine = definition.computers[0];
const env = world.environment({
  actor: machine.user, machines: [machine.id],
  actions: ["terminal.v1"], observations: ["terminal.v1", "semantic.v1"]
});
const result = env.step([{
  family: "terminal.v1", op: "execute", machine: machine.id,
  payload: { command: "pwd" }
}]);
console.assert(result.outcomes[0].success);
const scene = env.scene(1024, 768); // no rasterization
const frame = env.render(1024, 768);
const snapshot = world.snapshot();
const branch = world.fork(snapshot);
const branchEnv = branch.session(env.id);
```

For seeds outside JavaScript's exact integer range, pass a decimal string.
The owner uses `stateHash()`, `exportSnapshot()` and `importSnapshot(json)`.
See the [demo instructions](../examples/browser/README.md) for browser execution
and offline verification commands.
