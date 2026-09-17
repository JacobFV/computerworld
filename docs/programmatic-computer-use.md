# Programmatic computer interaction

Computerworld is a library, not an automation layer around the demo webpage.
Python embeds the canonical Rust runtime through PyO3; Node and browsers run that
runtime as Wasm. Keep one `World` and its actor `Environment` alive across steps.
No browser process or Node subprocess is required for Python, and browser Wasm
needs no simulation backend.

Start with [Python installation](python.md) or [JavaScript installation](wasm.md).
The executable examples are [Python](../examples/python/computer_interaction.py)
and [Node JavaScript](../examples/javascript/computer-interaction.mjs). They show
computer input, scene inspection, rendering and checkpoints using ordinary library
calls. The [interactive world console](../examples/browser/README.md) uses the same
runtime for its monitors and input.

```sh
node examples/javascript/computer-interaction.mjs --output target/javascript-demo
python examples/python/computer_interaction.py --output target/python-demo --compare target/javascript-demo
```

Both demos write `observation.json`, `scene.json`, `actions.json`, `trajectory.json`,
`summary.json`, a portable `snapshot.json`, raw `frame.rgba`, and a viewable
`frame.ppm`. They exercise launcher clicks, terminal typing, window drag/resize and
synthetic browser navigation. The Python `--compare` option checks the JavaScript
run's semantic/pixel hashes and portable checkpoint; omit it for an independent run.
Use `--help` for custom world, machine and output options.

## Grant a computer interface

The owner loads a serialized world definition and chooses capabilities. For a
keyboard/mouse desktop with synthetic browser access:

```python
import json
from computerworld import World

with open("worlds/company-2026/world.json") as source:
    definition = json.load(source)
definition.setdefault("metadata", {})["desktop_themes"] = {
    "alice-mac": "virtual-macos-golden-gate"
}
world = World(definition, seed=7)
env = world.environment({
    "actor": "alice", "machines": ["alice-mac"],
    "actions": ["application.v1", "keyboard.v1", "pointer.v1", "browser.v1"],
    "observations": ["semantic.v1"],
})

def act(family, op, payload):
    result = env.step([{
        "family": family, "op": op, "machine": "alice-mac", "payload": payload
    }])
    outcome = result["outcomes"][0]
    if not outcome["success"]:
        raise RuntimeError(outcome.get("error"))
    return outcome.get("value")

window = act("application.v1", "launch", {"kind": "terminal"})["window"]
act("keyboard.v1", "type", {"text": "echo hello from the agent"})
act("keyboard.v1", "key", {"key": "Enter"})
scene = env.scene(960, 640)  # semantic/layout data; no rasterization
```

For terminal-tool agents grant `terminal.v1` and call `execute` directly. For
filesystem tools grant `filesystem.v1`. Keyboard input to an installed terminal
is a separate permitted interface; omitting terminal-tool grants does not disable
the terminal application. Choose installed applications as well as grants to
restrict what an actor can do. Each action also names an allowed machine.

The JavaScript equivalents accept the same objects:

```js
const env = world.environment({
  actor: 'alice', machines: ['alice-mac'],
  actions: ['application.v1', 'keyboard.v1', 'pointer.v1', 'browser.v1'],
  observations: ['semantic.v1']
});
function act(family, op, payload) {
  const result = env.step([{family, op, machine: 'alice-mac', payload}]);
  const outcome = result.outcomes[0];
  if (!outcome.success) throw new Error(JSON.stringify(outcome.error));
  return outcome.value;
}
act('application.v1', 'launch', {kind: 'browser', argument: 'http://intranet.internal/'});
```

That URL resolves through simulated DNS, routing and HTTP to the service in the
world. It does not call host `fetch`. Browser content is a supported structured
page, not arbitrary HTML/JavaScript execution.

## Pointer coordinates, windows and gestures

`env.scene(width, height)` describes the currently focused machine in the session.
An action targeting an allowed machine selects that machine; use separate actor
sessions for independently controlled monitors when convenient. Match pointer
`width`/`height` to the scene or raster you used. CSS display scaling must be undone
before sending canvas coordinates.

Find actionable nodes by `interaction` and `semantic` fields. Desktop window
controls use targets such as `window:7:drag`, `window:7:resize:se`,
`window:7:maximize`, and `window:7:content:shell:address`. Actual content suffixes
come from the scene; do not invent them or retain node IDs across layout changes.

Node `bounds` are local. Its transform maps a local point to scene pixels:

```text
scene_x = floor((a * local_x + c * local_y) / 1024) + tx
scene_y = floor((b * local_x + d * local_y) / 1024) + ty
```

`clip`, when present, is already in scene coordinates. A transformed center can
be clipped or covered by another window. A robust target picker must select a
visible point, respect disabled semantics and rounded corners, and account for
higher `z` values (later insertion wins ties). Rust performs the authoritative hit
test again when input arrives. Re-read the scene after focus, navigation, resize
or other layout changes. Scene interaction strings identify controls; pointer
actions take coordinates, not a string to bypass hit testing.

For a known visible title-bar point `(x, y)`, drag with real pointer events:

```python
viewport = {"width": 960, "height": 640}
act("pointer.v1", "down", {**viewport, "x": x, "y": y})
act("pointer.v1", "move", {**viewport, "x": x + 90, "y": y + 45})
act("pointer.v1", "up", {**viewport, "x": x + 90, "y": y + 45})
```

The same sequence on a resize region resizes. `cancel` releases capture;
`double_click` on a title bar toggles maximize/restore. A simple `click` performs
one activation. When forwarding physical events, use down/move/up **or** a click
shortcut; adding a browser `click` handler after up would activate controls twice.
A move outcome may include a CSS-style `cursor` hint. Button `2` opens the desktop
context panel. Mobile down/move/up sequences also implement supported vertical
swipes; mobile apps use full-screen layouts rather than draggable windows.

Keyboard `type` supplies text; `key` supplies a key name such as `Enter`,
`Backspace`, `Alt+Tab`, or `Ctrl+s`. Key spelling/case matters. Input goes to the
focused app/control; focus it before typing. Application `focus` and `close` take
`{"window": window}`. Launch returns that window ID.

## Structured observations and pixels

`observe()` returns the granted observation channels. `scene()` returns compact
render primitives and semantics. Neither rasterizes. Scene/render access requires
`semantic.v1` or `pixels.v1`; granting pixels does not force rendering each step.
There is currently no PNG method on `Environment`: `render` returns RGBA.

Python can optionally encode a frame with Pillow (a consumer dependency):

```python
# python -m pip install Pillow
from PIL import Image
frame = env.render(960, 640)
Image.frombytes("RGBA", (frame["width"], frame["height"]), frame["rgba"]).save("desktop.png")
```

Browser JavaScript can draw it directly:

```js
const frame = env.render(960, 640);
try {
  canvas.width = frame.width;
  canvas.height = frame.height;
  const rgba = new Uint8ClampedArray(frame.rgba);
  canvas.getContext('2d').putImageData(new ImageData(rgba, frame.width, frame.height), 0, 0);
} finally {
  frame.free(); // release the Wasm-owned frame after copying its bytes
}
```

The generated Wasm objects expose `free()`. Release temporary frames in long
runs; keep the world/session alive until the episode finishes. Python bindings
return `bytes` and follow ordinary Python object lifetime. These handles are
synchronous and single-threaded; use independent runtimes/workers/processes for
parallel environments rather than concurrently sharing one handle.

## Owner checkpoints and evaluation

Only the harness should hold `World`. Agents receive `Environment`, whose API
contains `id`, `step`, `observe`, `scene`, and `render`; it has no world inspector,
checkpoint export or topology-edit methods.

| Operation | Python | JavaScript |
|---|---|---|
| In-memory checkpoint | `world.snapshot()` | `world.snapshot()` |
| Restore | `world.restore(checkpoint)` | `world.restore(checkpoint)` |
| Independent branch | `world.fork(checkpoint)` | `world.fork(checkpoint)` |
| Reattach actor in branch | `branch.session(env.id)` | `branch.session(env.id)` |
| Portable JSON checkpoint | `world.export_snapshot()` | `world.exportSnapshot()` |
| Import JSON checkpoint | `world.import_snapshot(text)` | `world.importSnapshot(text)` |
| Reset baseline | `world.reset(7)` | `world.reset(7)` |
| Owner diagnostics | `world.trajectory()`, `world.inspect()` | `world.trajectory()`, `world.inspect()` |

Portable imports require compatible runtime/schema and the matching baseline
world definition. A fork preserves sessions but needs its own returned actor
handle. Reset resets the world, not one actor; it restores the original blueprint
and clears episode state. Keep private rewards, goals and privileged inspection in
the evaluator. Restricted handles prevent accidental leakage; they are not a
sandbox for hostile code sharing the process. See [agent API](agent-api.md),
[security](security.md), and [determinism](determinism.md).
