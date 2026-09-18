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
`application/vnd.computerworld.page+json`. The sixteen element kinds below are all
data; the original eight are headings, text, links, buttons, inputs, forms, groups
and image descriptions. A form's `PageAction` identifies a
method, URL and fields; the browser submits through networking. Do not construct
views by reading a service store directly from the browser.

Pages that need real site structure add `Row`, `Grid`, `Card`, `Styled`,
`Thumbnail`, `Badge`, `Divider` and `Spacer`, plus an optional page `theme`
(`accent`, `background`, `surface`, `ink`, `muted`, `content_width`). `Style`
carries `size`, `weight`, `color`, `background`, `border`, `radius`, `padding`,
`align`, `width`, `height`, `flex` and `one_line`; colours are `#rrggbb` or
`#rrggbbaa` and radius/padding are capped at 64 by `Page::validate`. Inside a `Row`,
children with `Style::width` keep it and the rest divide the remainder by `flex`,
never below their min-content width (the longest word, a button's label, a link's
longest word). A row whose children cannot all fit that way wraps onto more lines,
like `flex-wrap: wrap`; on a viewport under 600 px a child holding a column of
reading matter asks for three fifths of the screen, so sidebars stack under the
content as a mobile breakpoint would make them. A `Grid` of cards or tiles drops
columns until each cell fits its content (and, under 600 px, is at least 150 px);
a grid of short labels such as calendar days keeps its columns. For a column of
stacked children with a flex share, use `column` (a one-column `Grid` with a style),
not a `Row`.

A `Badge` with no `background`, `border` or `color` is an accent pill; one that sets
only its text `color` is a plain label, one with a `border` is an outline, and an
empty badge holds its place and draws nothing. `Link` text wraps and its box grows.
A `Form` with no `Input` renders as its bare buttons (a "Message bob" entry), and
its card title and a generic `Submit` label are read from the form id: `search`
gets a Search button and no title, `compose` a "New message" title and Send,
`reply` Reply, `rsvp` RSVP, `playlist` "New playlist" and Create, and so on.
`Card` and `Thumbnail` accept an optional `PageAction`: with one the whole box is a
single real click target, and without one it is inert decoration that carries no
interaction and no focusable semantics. `cw_service_common` exposes `row`, `grid`,
`card`, `card_action`, `column`, `styled`, `thumbnail`, `thumbnail_action`, `badge`,
`divider`, `spacer`, `visit`, `style` and `themed_page` for building these tersely.
All fields are optional and default, so pages written against the original eight
element kinds keep working unchanged.

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
