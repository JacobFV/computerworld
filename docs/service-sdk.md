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
`application/vnd.computerworld.page+json`. The seventeen element kinds below are all
data; the original eight are headings, text, links, buttons, inputs, forms, groups
and image descriptions. A form's `PageAction` identifies a
method, URL and fields; the browser submits through networking. Do not construct
views by reading a service store directly from the browser.

Pages that need real site structure add `Row`, `Grid`, `Card`, `Styled`,
`Thumbnail`, `Icon`, `Badge`, `Divider` and `Spacer`, plus an optional page `theme`
(`accent`, `background`, `surface`, `ink`, `muted`, `content_width`). `Style`
carries `size`, `weight`, `color`, `background`, `border`, `radius`, `padding`,
`align`, `width`, `height`, `flex`, `one_line`, `scroll_x` and `pin`; colours are `#rrggbb` or
`#rrggbbaa` and radius/padding are capped at 64 by `Page::validate`. `pin: "bottom"` on
a top-level element keeps it on the viewport's bottom edge while the page scrolls under
it (a music site's player bar); the page beneath is clipped so a click on the bar never
reaches what it covers. A `Thumbnail` shorter than 12 px (a progress bar's segment) has
no minimum width and paints no caption, so a row of them can be a seek bar whose
segments each keep an accessible name. Inside a `Row`,
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

An `Icon` is a real glyph rather than a character that happens to look like one: it
names one of `cw_protocol::PAGE_ICONS` (`play`, `pause`, `skip-next`, `shuffle`,
`heart-fill`, `thumb-up`, `cast`, `volume`, …, each a symbol every renderer bundles,
checked by `Page::validate` and by `crates/render/tests/page_icons.rs`), and carries a
`label`, which is required and is its accessible name. `Style::size` is the glyph's
size in pixels (20 by default), `color` tints it, and `padding`, `background`, `border`
and `radius` make the box around it; with a `PageAction` that box is one click target
(role `button`, or `link` for a GET), and without one it is a picture (role `img`).
`cw_service_common::icon` and `icon_action` build them.

A `Row` with `Style::scroll_x` lays its children out on one line at their own widths
and scrolls sideways when they overflow, instead of wrapping: a shelf of album covers.
The browser publishes it as a horizontal `ScrollArea` (`pane:row:<row id>`), which a
wheel's `delta_x` (or `delta_y` with Shift), a sideways swipe on a phone, or
`browser.v1 scroll` with `{"row": id, "x": n}` moves.

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

Pictures are served, not described: an `Image` element names a same-origin URL, and
the browser fetches it and expects `application/vnd.computerworld.rgba+json`
(`cw_protocol::RGBA_MEDIA_TYPE`), a JSON `{width, height, rgba}` straight-alpha RGBA8
image, cached per URL. The music sites serve their covers that way from
`GET /art/<key>?size=&radius=`, drawn by `cw_artwork` from the key alone, so the same
album has the same cover on the site and in the native players. An `Image` keeps its
declared proportions when a narrow column shrinks it.

A page whose content moves with the world clock — a music site's player bar, whose
position and lit lyric line advance as the clock does — answers with a `refresh:
<seconds>; url=<same-origin path>` header. The browser re-requests that URL once that
much world time has passed since the page was shown, replacing it in place (what was
typed, the focus, the scroll positions and cached pictures all stay), and marks the
request with `x-computerworld-refresh: 1` (`cw_protocol::REFRESH_HEADER`) so the site
can tell a page keeping itself current from a person visiting it. Intervals are clamped
to 1 second..1 hour, and a page that names another origin is ignored.

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
