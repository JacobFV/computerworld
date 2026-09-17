# Application SDK

`cw_sdk::Application` separates persistent state, input handling and presentation.
An application has `kind`, `version`, `initialize`, `event` and `page` methods.
`event` receives mutable JSON state, `AppContext` and an `AppEvent`, returning
explicit `AppEffect` values. `page` receives immutable state and returns a native
`Page`; observation should never change application state.

`AppContext` identifies the actor, machine, logical tick, seed and instance.
`AppEvent` has a kind, optional target and JSON data. Effects are explicit HTTP
requests, file reads/writes, application launch or named emitted data. The
embedding environment mediates them; returning an effect is not a capability to
access arbitrary host resources.

Use stable element IDs so semantic targeting, keyboard focus and pointer hit
regions refer to the same control across updates. Buttons/forms describe actions,
not JavaScript callbacks. A custom application should render state after actual
operations complete, rather than echoing requested input as proof of success.

For an agent-driven application, register with `World::register_application`
(or `Environment::register_application`). `Registry::register_application` is the
lower-level code registry operation. Multiple instances can share the
implementation while keeping separate state. Version changes participate in the
runtime's module identity. See [custom application](custom-application.md) and
[rendering](rendering.md).

Launch a registered application with family `application.v1`, op `launch`, and
payload `{ "kind": "your.kind", "instance": "optional-id", "initial": {} }`.
Send op `event` with `{ "instance": "optional-id", "event": { "kind": "activate",
"target": "control-id", "data": null } }`. Effects return through an `effect_result`
event whose data identifies the completed operation, such as HTTP or file access.
Actor observations expose its page, not the private application state.
