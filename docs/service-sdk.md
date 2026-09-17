# Service SDK

A service is an independent addressable instance of a registered Rust handler.
Its state is separate from every browser's cached response. Requests reach it only
after the kernel's DNS, topology and policy checks.

Implement `cw_sdk::Service`:

```rust,ignore
fn kind(&self) -> &str;
fn version(&self) -> u32; // defaults to 1
fn initialize(&self, initial: serde_json::Value,
              context: &cw_sdk::ServiceContext) -> cw_protocol::Result<serde_json::Value>;
fn handle(&self, state: &mut serde_json::Value,
          context: &cw_sdk::ServiceContext,
          request: &cw_protocol::HttpRequest) -> cw_protocol::Result<cw_protocol::HttpResponse>;
```

`ServiceContext` contains actor identity, source machine, logical tick, seed and
instance ID. It provides no global world inspector. `HttpRequest` contains method,
URL, headers and bytes. `HttpResponse` carries status, headers and bytes; `text`,
`json` and `page` constructors produce common content types. Header lookup is
case-insensitive.

Register executable code once with `Registry::register`, then reference its kind
from any number of `ServiceDefinition` instances with independent `initial_state`.
Duplicate kinds are errors. Increment `version` when changing state/semantics in a
way that invalidates replay or checkpoints. Registry code is not serialized.

For synthetic websites, return a `Page` with
`application/vnd.computerworld.page+json`. Headings, text, links, buttons, inputs,
forms, groups and image descriptions are data. A form's `PageAction` identifies a
method, URL and fields; the browser submits through networking. Do not construct
views by reading a service store directly from the browser.

Handlers are trusted in-process code. They should validate actor authorization,
request bodies and state transitions, mutate only their supplied state and avoid
host I/O. Rust's trait alone is not a sandbox against a malicious implementation.
Keep application-specific concepts in service packages, not kernel dispatch.
See [custom service](custom-service.md) for the executable example.

Services that initiate communication or timers override `handle_with_effects`,
returning a `ServiceTransition` with the HTTP response and explicit effects.
`ServiceEffect::Http` carries a request and reply token; `Schedule` carries a
logical delay, token and data; `Emit` records named data. `on_effect` receives the
HTTP/timer result and may return further effects. The kernel schedules these
continuations, so service-to-service work does not require host tasks or threads.
The default implementation delegates to `handle` and produces no effects.
