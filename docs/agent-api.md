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
| `pointer.v1` | `click` at scene coordinates and viewport dimensions |

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
