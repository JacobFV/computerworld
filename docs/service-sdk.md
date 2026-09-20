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
(`accent`, `background`, `surface`, `ink`, `muted`, `content_width`, `font`). `font` is a
CSS `font-family` list (`"Roboto, sans-serif"`, `"Times New Roman"`, at most 256
characters) resolved by `cw_scene::fonts::resolve_family` to a bundled face — Arimo for
Arial and Helvetica, Tinos for Times New Roman, Cousine for Courier New, Gelasio for
Georgia, Carlito for Calibri, Caladea for Cambria, and Lato, Source Sans 3, Source Serif
4, Poppins, Montserrat, Playfair Display and JetBrains Mono as themselves, plus the
generic families — in which every text on the page is set and measured; without it the
page uses the platform's UI face. `Style`
carries `size`, `weight`, `color`, `background`, `border`, `radius`, `padding`,
`align`, `width`, `height`, `flex`, `one_line`, `scroll_x`, `pin`, `justify` and `mono`;
colours are `#rrggbb` or `#rrggbbaa` and radius/padding are capped at 64 by
`Page::validate`. `pin: "bottom"` on a top-level element keeps it on the viewport's
bottom edge while the page scrolls under it (a music site's player bar), and
`pin: "top"` holds a sticky header on the top edge with the page flowing below it; the
page beneath either is clipped so a click on the bar never reaches what it covers.
`mono: true` sets the text in the bundled monospace face (a commit hash, a code span),
measured with `Typeface::Mono` on the same grid the terminal paints on.

`Link` and `Button` take an optional `style` of their own. A bare link is
accent-coloured text at its own width (no slab); with a style it takes the size,
weight, colour, `background`, `border`, `radius`, `padding` and `width` given, so it can
be a nav item, a tab or a bordered button (`cw_service_common::styled_link`,
`inline_link`). A bare button keeps the accent pill at its label's width; a styled one
recolours and resizes it (`styled_button`). An `Image` takes an optional `style`
(`radius` rounds it into an avatar, `border` frames it, `width`/`height` override its
declared size) and an optional `action`, which makes the picture one click target named
by its `alt`. `chip`, `avatar` and `pills`/`rest` in `cw_service_common` build the
tags, initials-avatars and chip rows every skin needs. A `Thumbnail` shorter than 12 px (a progress bar's segment) has
no minimum width and paints no caption, so a row of them can be a seek bar whose
segments each keep an accessible name. Inside a `Row`,
children with `Style::width` keep it and the rest divide the remainder by `flex`,
never below their min-content width (the longest word, a button's label, a link's
longest word). Once any child names a `flex` (or the row sets `justify`), the
children that name none sit at their natural width instead, so a chip beside a `rest`
spacer stays a chip; `justify` (`start`, `center`, `end`, `space-between`) places what
is left over. A row whose children cannot all fit that way wraps onto more lines,
like `flex-wrap: wrap`; on a viewport under 600 px a child holding a column of
reading matter asks for three fifths of the screen, so sidebars stack under the
content as a mobile breakpoint would make them. A `Grid` of cards or tiles drops
columns until each cell fits its content (and, under 600 px, is at least 150 px);
a grid of short labels such as calendar days keeps its columns. For a column of
stacked children with a flex share, use `column` (a one-column `Grid` with a style),
not a `Row`.

An `Icon` is a real glyph rather than a character that happens to look like one: it
names one of `cw_protocol::PAGE_ICONS` (`play`, `pause`, `skip-next`, `shuffle`,
`heart-fill`, `thumb-up`, `cast`, `volume`, and the developer set `branch`, `fork`,
`commit`, `merge`, `pull-request`, `code`, `file`, `folder`, `issue-open`,
`issue-closed`, `check`, `x-circle`, `comment`, `hash`, `lock`, `bell`, `at`, `emoji`,
`paperclip`, `bold`, `italic`, `send`, `thread`, `more`, `chevron-down`,
`chevron-right`, `star-filled`, …: the list is exactly the symbol set every renderer
bundles, checked by `Page::validate` and by `crates/render/tests/icon_assets.rs`), and carries a
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
declared proportions when a narrow column shrinks it. The maps sites do the same with
`cw_map`: `GET /map.rgba?w=&h=&center=&zoom=&route=&sel=` draws a street map from the
places alone (a grid in micro-degrees, arterials named by the places' addresses, water
west of the places' world), in integers, so the native Maps app draws the identical
streets from the same geometry. `cw_service_common::image` builds the element and
`rgba_response` the reply.

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

## HTML services

A service answers with `text/html` through `cw_service_common::html`, and the browser
renders it with the `cw-web` engine (see [html-migration.md](html-migration.md) for the
recipe that moves a `Page` service over, with the search service as the worked example).

**The layer.** `Html` is a node: `el("div").id("x").class("card").attr("title", ..)
.child(..).text(..)`, with `fragment`, `empty`, `when(cond, |n| ..)`, `maybe(Option)` and
`each(iter, |item| ..)` for conditional and repeated children. Text is escaped for
element content, attribute values for attributes, and `style`/`script` bodies are
emitted verbatim with `</` defused; void elements (`input`, `img`, `meta`, ...) take no
closing tag. Helpers read like the markup: `link(id, href, text)`, `a(href)`,
`form(id, action, "get"|"post")`, `text_input(id, name, value)`, `hidden(name, value)`,
`button(id, label)`, `label(for, text)`, `div(class)`, `span(class)`, and
`href(path, &[("q", value)])` builds a form-encoded query string. `raw(markup)` exists
for strings the service itself wrote; never pass it request or seed text.

**Documents and responses.** `Document::new(title).lang("en").stylesheet(css)
.root_style("--accent: #1a73e8").body_class("skin-google").body([..])` renders
`<!DOCTYPE html>`, `<html lang style>`, a `<head>` with the title and one `<style>`,
and the body. `HtmlResponse::ok(&doc)` (or `html::page(&doc)` for a `Result`) is a
200 with `text/html; charset=utf-8`; `.header("refresh", ..)` adds what a page needs.
Page responses carry only their content type, so HTML responses do too.

**The CSS file convention.** Each service keeps its stylesheet as a real `.css` file
next to its source (`services/search/src/search.css`), pulled in with `include_str!`
and passed to `Document::stylesheet`, which emits it as one inline `<style>`: one
request per page, nothing for the browser to fetch, and the strict validator sees
the whole page in one string. The sheet is static; what varies per instance (a
seeded palette) goes on `<html style>` as custom properties (`--accent`, `--ink`),
which the sheet reads with `var()`. A skin is a class on `<body>`.

**Ids for the agent API.** The browser's `click`, `fill`, `key` and `submit` address
elements by `id`, and the semantic observation lists links, buttons, inputs and forms
by the same id. Every control a person could use carries one, ids are unique per
page, and a service keeps its ids across a redesign so an agent's script keeps
working. A block-level `<a id>` wrapping a card makes the whole card one link with
that id, the way a `Card` with an action was.

**The strict validator.** `html::validate_strict(&html)` (feature `validate`, enabled
from a service's `[dev-dependencies]` on `cw-service-common`) parses the page, parses
every `<style>` with `Strictness::Strict` and runs the strict cascade, so an unknown
property, value, selector or at-rule, a repeated id or a linked stylesheet fails the
test that calls it. Every service's tests run every page through it.

**Static sites.** The `static-site` service serves `pages` seeds as HTML through
`cw_web::page::to_document` by default, so every existing site renders through the
engine with its current look and ids; a site that must stay JSON sets
`"format": "page"`. The `files` map serves authored HTML, CSS, JavaScript and pictures
with their media types.
