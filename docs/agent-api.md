# Agent environment API

An owner creates a `World` and grants an `EnvironmentConfig` to an actor. Config
names the actor, allowed machines, action families, observation channels and
maximum batch size. Session identifiers select those grants. The bindings expose
a restricted `Environment` object; owner operations remain on `World`.

Actions are extensible envelopes, not a universal model-specific token schema:

```json
{"family":"terminal.v1","op":"execute","machine":"workstation",
 "payload":{"command":"cat /home/alice/notes.txt"}}
```

`step` takes a batch and returns per-action success/value/error, an observation,
logical tick and pending count. Check the outcome, not just whether the call
returned. A denied or failed action is not success. Machine and family grants are
checked before dispatch. Batches are ordered sequences, not atomic transactions.

| Family | Representative operations |
|---|---|
| `terminal.v1` | `execute` with command |
| `filesystem.v1` | `read`, `write`, `list`, `stat` with path |
| `http.v1` | `request` with serialized `HttpRequest` |
| `browser.v1` | Navigation, history, fields and element interaction |
| `application.v1` | Launch/focus/close application windows and registered app events |
| `keyboard.v1` | `type` text and `key` input |
| `pointer.v1` | `click`, `down`, `move`, `up`, `cancel`, `double_click` at scene coordinates and viewport dimensions |

Native callers can register additional `ActionFamily` implementations and
`ObservationChannel` implementations through the environment owner API. Grants
select which families/channels an actor can use. These extensions are trusted
code receiving runtime access; their implementations must enforce appropriate
projection and access rules and must not leak privileged state. Their versions
are part of the checkpoint compatibility boundary.
`EnvironmentConfig::terminal` and `::desktop` are convenience presets, not
separate simulators.

Observations are a map of explicitly selected channels, keyed by machine within
each channel. Terminal results and semantic/native page views do not reveal the
kernel's complete state. `observe` does not rasterize. Request `render` explicitly
for pixels; use scene hit testing for pointer actions. The filesystem observation
contains the local working directory, not a world inspector. `browser.v1` includes
the page, URL and fields. `semantic.v1` or `pixels.v1` is required to request a
scene/render; selecting `pixels.v1` authorizes rendering but does not automatically
rasterize every step.

For complete Python and JavaScript input loops, see [programmatic computer use](programmatic-computer-use.md).
Pointer input uses the current scene viewport, transformed local bounds, world-space
clips and z-order; it does not bypass hit testing by accepting semantic target IDs.
Drag and resize are down/move/up sequences. Window controls are namespaced as
`window:<id>:<operation>`, and child content as `window:<id>:content:<target>`.
Scenes expose all retained nodes, including occluded ones; select a visible point
and refresh the scene after layout changes. Move outcomes may include a cursor hint.

`application.v1/launch` takes `{"kind":"terminal"}` (or another installed app),
optionally `argument`, and returns a window ID. `focus`/`close` take `{"window":id}`.
`keyboard.v1/type` takes `{"text":"..."}` and `key` takes `{"key":"Enter"}`.
Browser launch and browser controls require `browser.v1` as well as app access.
Installed apps are part of the capability surface: a GUI terminal can run commands
through keyboard input even when direct terminal-tool access is not granted.

Scene/render select the session's focused machine, initially the first granted
machine; actions select their target machine. Separate sessions can drive separate
monitors concurrently in an owner-controlled loop. The bindings are synchronous;
parallel workers should own independent runtime instances.

Only owners should call inspection, global trajectory, snapshot export or create
new grants. Keep task rewards/private predicates in the evaluator layer. Examples
should solve tasks using actor-visible actions rather than retrieving answers
through owner inspection.

## Owner-controlled device lifecycle

The world owner can add/remove computers while keeping existing machines and services alive:

```javascript
world.addComputer(computerDefinition, networkNode, links);
world.removeComputer('temporary-laptop');
const currentBlueprint = world.definition();
```

Rust uses `World::add_computer(computer, node, links)` and
`World::remove_computer(id)`; Python uses `world.add_computer(computer, node, links)`
and `world.remove_computer(id)`. These are privileged owner operations, absent from
actor environment handles. Definitions must refer to an existing OS profile and
unique machine/node/address; links must refer to valid nodes. A newly added computer
needs an explicitly granted actor session before agent access.

Edits are atomic and rejected while continuations are pending. Removing a computer
that hosts services is rejected rather than silently deleting service state. Removing
other computers removes their node/links, revokes their actor grants, and repairs
session focus. Existing files, services, process state and surviving transports are
preserved. Full topology changes are present in owner events.

Checkpoints include the original baseline and current topology. Restore/fork can
recover removed computers and sessions; reset returns to the original blueprint.
Portable snapshots remain constrained to the matching baseline. The browser console
additionally saves its device-shape visualization metadata alongside its in-memory
snapshot; these UI descriptors do not introduce another simulator implementation.

## Scene perception contract

`environment.scene(width, height)` returns a structured `Scene`. Everything below is
additive: existing `Scene`, `Node`, `Semantic` and `Observation` consumers are unchanged,
and every new field is omitted from the payload when it carries nothing. `SCENE_VERSION`
is `2`.

Every value is derived from world state. There is no wall clock, no RNG and no host I/O,
so the same state produces the same scene, the same node revisions and the same digests —
in this process, after a snapshot restore, in a fork, and in a fresh process.

### Windows (`Scene::windows`)

One `SceneWindow` per window the compositor knows about, bottom-to-top:

```rust
pub struct SceneWindow {
    pub id: u64, pub title: String, pub app: String,
    pub bounds: Rect,          // outer frame, including decoration
    pub content: Rect,         // client area, excluding decoration and browser chrome
    pub z: u32,                // 0 is bottom-most
    pub focused: bool, pub minimized: bool, pub maximized: bool,
    pub document: String,      // path, URL or document the window presents
    pub tabs: Vec<String>, pub active_tab: usize,
    pub occluded_by: Vec<u64>, // higher windows covering part of `bounds`
    pub exposed: Option<Rect>, // largest uncovered part; None when fully hidden
}
```

`Scene::window(id)` looks one up. `exposed` is a point you can actually click to raise a
partly covered window; spatial reasoning no longer has to be reconstructed from text.

### Roles, state and ownership

`Node` gains `window: Option<u64>`, `state: Option<NodeState>` and `revision: u64`.
`NodeState` carries `checked`, `selected` and `expanded` (each `Option<bool>`: `None`
means the role has no such state) plus `focused`. `Semantic` is unchanged — shells build
it with exhaustive struct literals, so it can never gain a field.

`Scene::accessibility() -> Vec<AxNode>` publishes the accessibility-shaped tree, one entry
per control, merging every node a shell paints for it. No inference required:

```rust
pub struct AxNode {
    pub id: String,             // the interaction id, which actions also address
    pub role: String, pub name: String, pub value: Option<String>,
    pub enabled: bool, pub focusable: bool, pub focused: bool,
    pub checked: Option<bool>, pub selected: Option<bool>, pub expanded: Option<bool>,
    pub window: Option<u64>,
    pub bounds: Rect,           // union of the merged nodes' painted bounds
    pub nodes: Vec<u64>, pub z: i32, pub order: usize,
    pub hit: Option<(i32, i32)>,// a point a click reaches it at; None when occluded
    pub revision: u64,
}
```

`window` is authoritative for controls, whose interactions are namespaced
`window:<id>:...`; other nodes are attributed from the compositor's per-window paint run.

### Focus and caret (`Scene::focus`)

```rust
pub struct Focus {
    pub window: Option<u64>, pub node: Option<u64>, pub interaction: Option<String>,
    pub role: String, pub label: String, pub value: Option<String>,
    pub caret: Option<Caret>, pub keyboard: Keyboard,
}
pub struct Caret { pub bounds: Rect, pub line: u32, pub column: u32, pub offset: u32 }
pub struct Keyboard {
    pub route: String,          // none|panel|application|address|page|terminal|editor|window
    pub window: Option<u64>, pub target: Option<String>, pub text_entry: bool,
}
```

`Keyboard::route` mirrors the `keyboard.v1` dispatch order exactly, so it answers "where
would this keystroke go" rather than describing it. `caret.bounds` is the character cell
the insertion point occupies, from the model's offset on the pane's own painted grid. Do
not look for a solid block in the pixels.

### Hit testing, z-order and occlusion

`Scene::hit_test(x, y)` is unchanged. Added:

- `Scene::hit_stack(x, y) -> Vec<Hit>` — every node covering the point, topmost first,
  the `elementFromPoint` equivalent. Non-interactive nodes are included, each with
  `interactive` and `opaque`, so occlusion is legible and not merely answerable.
- `Scene::hit_reaches(node, x, y) -> bool` — "is this point actually this element".
- `Scene::reachable_point(node) -> Option<(i32, i32)>` — a point a click reaches the node
  at, or `None` when it is fully covered, disabled or inert. Sampling is a fixed grid, so
  the answer is deterministic.
- `Rect::subtract`, `Rect::union` and `cw_scene::exposed(bounds, covers)` expose the same
  geometry windows use.

### Deltas (`Scene::digest`, `Node::revision`)

`Scene::stamp()` fills `Node::revision` with a digest of what the node paints and
announces, and `Scene::digest` with a digest of the whole scene. `environment.scene`
stamps before returning, so both are always populated; `0` means unstamped.

A revision is a **content digest, never a counter**. There is no mutable sequence a
snapshot restore could desynchronise: the same state hashes the same, so restoring a
checkpoint reproduces the ids exactly and a scene taken before and after a restore
compares equal.

`new.diff(&old) -> SceneDelta` gives `changed`, `added`, `removed`, `updated`, `windows`,
`background`, `resized`, `focus` and a `damage` rect list. `changed == false` is the cheap
"nothing happened" answer: it short-circuits on the scene digest and touches no nodes.

### Text lines and pane buffers

Panes hard-wrap at the character cell, so joining painted lines naively corrupts text
(`initial commi` + `t`). Every painted line now carries its provenance:

```rust
pub struct TextLine {
    pub logical: u32,           // index in the owning pane's buffer
    pub continuation: bool,     // continues the previous visual line: join with no separator
    pub offset: u32,            // character offset within the logical line
    pub wrapped_from: Option<u64>, // node id of the logical line's first fragment
    pub pane: Option<String>,   // handle into Scene::buffers
}
```

`Scene::buffers` carries each text pane's whole buffer, not only the visible region:

```rust
pub struct TextBuffer {
    pub handle: String, pub window: Option<u64>, pub kind: String, // "terminal" | "editor"
    pub lines: Vec<String>,     // every logical line, unwrapped; `logical` indexes this
    pub first_visible: u32, pub visible: u32,
    pub truncated: bool,        // older lines dropped at MAX_BUFFER_LINES/CHARS
}
```

Terminal panes publish the scrollback the transcript holds; editors publish the whole
document. `cw_scene::reflow(logical, visual)` and `wrap_text_lines` are the same
attachment available standalone.

### Action results (`ActionOutcome::effect`)

`step` outcomes report the envelope. `ActionOutcome::effect` reports the app-level
consequence, derived by comparing an actor-visible projection of the target machine
before and after dispatch:

```rust
pub struct ActionEffect {
    pub changed: Vec<String>,       // sorted `cw_protocol::effect::*` tags
    pub windows_opened: Vec<u64>, pub windows_closed: Vec<u64>,
    pub focused_window: Option<u64>, pub url: Option<String>,
    pub state: u64,                 // digest of the machine's actor-visible state after
}
```

Tags are `window.opened`, `window.closed`, `window.moved`, `window.focused`,
`window.title`, `content`, `document`, `navigate`, `focus`, `terminal` and `application`.
An empty `changed` (`ActionEffect::is_noop`) means the action was accepted and changed
nothing observable — a real answer, distinct from failure. A failed action can still carry
an effect when it mutated state before failing, which is the case worth seeing. `effect`
is `None` only when the action was refused by its grants and never reached a machine.
`state` is content-derived like the scene digest, so equal values mean equal observable
state across restores.

### Cost

Stamping adds one digest pass over the composed scene. `environment.scene` remains a
sub-millisecond structured call; nothing here rasterizes, and `diff` between two stamped
scenes is a digest comparison plus a map walk.
