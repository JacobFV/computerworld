# Layout parity against Chromium

The web engine in `crates/web/engine` is measured against Chromium, which Playwright drives
through the same module the capture pipeline uses. For every fixture page Chromium dumps
each element's box and a fixed set of computed properties; the engine dumps the same at
the same viewport; the two are compared with a small tolerance, and a report says what
disagrees. This isolates the cascade and layout from rasterisation, which the fonts
would otherwise blur.

## Files

| Path | What |
|---|---|
| `crates/web/engine/tests/parity/<name>.html` | A fixture: a self-contained page, CSS inline, no external requests, synthetic text |
| `crates/web/engine/tests/parity/<name>.chromium.json` | Chromium's dump of it (checked in) |
| `crates/web/engine/tests/parity/<name>.chromium.png` | Chromium's screenshot of the viewport |
| `crates/web/engine/tests/parity/<name>.fonts.json` | Each distinct `font-family` list the page asks for and the platform face Chromium shaped it with |
| `crates/web/engine/tests/parity/thresholds.json` | The pass rate each fixture must reach (0 to 1) |
| `crates/web/engine/target-parity/<name>.engine.json` | The engine's dump, same shape (written by the Rust runner; not checked in) |
| `crates/web/engine/target-parity/<name>.engine.png` | The engine's raster through `cw-render` |
| `crates/web/engine/target-parity/<name>.report.md` | Pass count, mismatches by property, worst offenders with expected and got |
| `crates/web/engine/target-parity/<name>.compare.png` | Chromium, the engine and their difference side by side (from `compare.mjs`) |
| `crates/web/engine/tests/ref/<name>.html`, `<name>-ref.html` | Reftest pairs: two documents that must paint to the same scene |
| `crates/web/engine/tests/modern_layout.rs` | Regression tests for the engine bugs the modern fixtures exposed: small documents through the whole pipeline, rects and computed values asserted by element id |
| `crates/web/engine/src/paint/pipeline_tests.rs` | Regression tests for the engine bugs the migrated services exposed: the smallest page that showed each one, through the whole pipeline, asserted on the scene or on the raster |
| `crates/web/engine/tests/wpt/` | The web-platform-tests reftest corpus (sparse, pinned; `manifest.json`, `expectations.json`, `README.md`, `LICENSE.md`), produced by `crates/web/engine/tools/fetch-wpt.py` |
| `crates/web/engine/target-parity/wpt-report.md` | Pass counts per WPT directory and the outcome of every pair (written by `crates/web/engine/tests/wpt.rs`) |

Scripts: `dump.mjs` (Chromium side), `compare.mjs` (reports and pictures), `common.mjs`
(the property list and comparison rules, shared by both). The Rust side is
`crates/web/engine/tests/parity.rs`, `crates/web/engine/tests/reftest.rs`, `crates/web/engine/tests/wpt.rs`
and their shared `crates/web/engine/tests/support/mod.rs`, which holds the same rules in Rust
so `cargo test` needs no Node.

## The dump

For every element in document order (skipping `<head>`): a stable path
(`html>body>div:nth-child(2)>p:nth-child(1)`; `:nth-child` counts element siblings of
any tag; `html` and `body` carry no index), tag, id, `getBoundingClientRect` rounded to
1/64 px, and these computed properties as strings: display, position, float, width,
height, margin-\*, padding-\*, border-\*-width, font-family, font-size, font-weight,
line-height, color, background-color, text-align, white-space, vertical-align,
overflow-x, overflow-y, z-index, box-sizing. For every non-blank text node: its path
(`…>#text:nth(i)`, counting the parent's non-blank text children), its data and the
client rect of each line (`Range.getClientRects`).

Chromium reports used values for the box of a rendered element (`width` is the content
box in px, `auto` margins are resolved) and computed values for one that generates no
box; the engine's dump does the same from its fragment tree.

## Comparison rules

- Every rect edge (x, y, width, height) within **1 px** of Chromium's.
- Widths and heights of **text-dependent** boxes within **2 px**: inline boxes, table
  parts, floats, absolutely positioned boxes and shrink-to-fit list items, because the
  two engines shape text with different faces and quantise advances differently. Text
  node line rects get the same 2 px.
- A `<br>` generates no box in the engine, so its rect is not compared, only its
  computed values.
- Length properties within the same tolerances; every other property equal as a string
  after normalisation (`-webkit-center` is `center`, `bold` is `700`, colours are
  `rgb(r, g, b)` or `rgba(r, g, b, a)` with the alpha rounded to two places).
- `font-family` is reported, never compared. The report's font table shows what Chromium
  shaped each family list with (through CDP `CSS.getPlatformFontsForNode`) and which
  bundled face the engine resolved it to (`cw_scene::fonts::resolve_family`).
- The engine runs with `Media::fonts = FontEnvironment::LinuxBaseline`: the dumps were
  made on a stock Linux desktop where only the Liberation and DejaVu families exist
  (each fixture's `fonts.json` shows it: `Inter`, `Poppins`, `Roboto`, `Georgia` and
  `JetBrains Mono` all fell through to Liberation Sans, Serif or Mono), so a family the
  engine bundles but that machine lacks falls through to the list's generic here too.
  The world's browser keeps the default, `Bundled`.
- Rects are client rects: an element under a `transform` (its own or an ancestor's)
  reports the bounds of its transformed border box, as `getBoundingClientRect` and
  `Range.getClientRects` do; computed `width` and `height` stay untransformed.
- An `auto` margin reports the used value layout recorded on the fragment
  (`Fragment::used_margin`: block centring, the free space a flex or grid item absorbed).
- A node passes when it has no mismatch. The fixture's pass rate is passing nodes over
  Chromium's node count (elements and text nodes). The Rust test fails when it is below
  the fixture's threshold.

## Workflow

Environment for the Node side:

```sh
export PLAYWRIGHT_MODULE=/usr/lib/chatgpt/resources/cua_node/lib/node_modules/playwright/index.mjs
export CHROME_BIN=/usr/bin/google-chrome
```

1. **Regenerate a Chromium dump** after editing a fixture (or adding one):

   ```sh
   node scripts/web-parity/dump.mjs crates/web/engine/tests/parity/google-1998.html
   ```

   Defaults: 1280×800, device pixel ratio 1, screenshot of the viewport (`--full` for the
   whole page; `--width`, `--height`, `--dpr` to change the viewport, which the Rust
   runner must then match). Check the JSON, PNG and fonts.json in. Add the fixture's name
   to `thresholds.json` and a `parity_fixture!` line in `tests/parity.rs`.

2. **Run the Rust tests**:

   ```sh
   cargo test -p cw-web --test parity --test reftest            # format checks only
   cargo test -p cw-web --features pipeline --test parity        # engine vs Chromium
   cargo test -p cw-web --features pipeline --test reftest       # reftest pairs
   cargo test -p cw-web --features pipeline --test wpt           # web-platform-tests reftests
   ```

   The `pipeline` feature compiles the calls into `html::parse`, `css::parse_stylesheet`
   and `style::cascade`; without it those tests are ignored and only the dump format,
   thresholds and reftest file layout are checked. The runner writes the engine dump,
   PNG and `report.md` for every fixture into `crates/web/engine/target-parity/` whether or not
   the threshold is met.

3. **Read the reports**: `crates/web/engine/target-parity/<name>.report.md` lists the pass
   count, mismatches per property (which tells you whether it is the cascade, the
   block layout or the text that is off) and the forty worst nodes with expected and got
   values. For pictures:

   ```sh
   node scripts/web-parity/compare.mjs --all
   ```

   writes `<name>.compare.png` (Chromium | engine | difference in red) next to a report
   with the same numbers as the Rust runner, plus Chromium's platform fonts.

4. **Raise thresholds**: when a fixture's pass rate goes up and stays up, raise its entry
   in `thresholds.json` to just under the achieved rate, so a regression fails the build.
   Thresholds start at 0 for every fixture; the integration step raises them as the
   modules land.

## The environment both sides share

- **Scrollbars.** Playwright launches headless Chromium with `--hide-scrollbars`, so a
  scroll container in the dump gives none of its width to a scrollbar. The harness lays
  out with `LayoutCache::overlay_scrollbars` for the same reason; the engine's default
  (what the browser shell uses) reserves 15 px like a desktop Chromium.
- **Fonts.** The cascade resolves families as a stock Linux desktop does
  (`FontEnvironment::LinuxBaseline`), which is the machine the dumps were taken on.
- **`data:` URLs.** The harness decodes every `data:` image a page names
  (`cw_web::paint::ImageMap::from_document`) and reads `data:` stylesheets from
  non-alternate `<link rel=stylesheet>` elements. There is no other fetching: fixtures
  are self-contained.
- **HTTP statuses.** A `file:` URL has none, so `dump.mjs` answers a request carrying
  web-platform-tests' `?pipe=status(N)` the way the WPT server would: 4xx and 5xx fail
  the load (Acid2's `<object>` fallback depends on a 404).

## Reftests

A pair listed in `NAVIGATE` in `tests/reftest.rs` is compared after a fragment navigation
(both documents scrolled to the named id, as Acid2 asks to be viewed at `#top`); a pair
listed in `TOLERANCE` may differ by the stated number of pixels for the stated reason
(none does today). `tests/pending/` holds pairs the engine does not pass yet.


`crates/web/engine/tests/ref/<name>.html` and `<name>-ref.html` are two documents that must
paint identically: the runner parses, cascades, lays out and paints both at 1280×800
and compares scene digests (`Scene::stamp` after erasing node ids, since the two DOMs
have different node numbering). A failing pair leaves `ref-<name>.test.png` and
`ref-<name>.ref.png` in `target-parity/` to look at. The pairs cover Milestone 1:
margin collapsing, auto-margin centring, floats against a table, inline padding,
`<b>` against `font-weight`, `bgcolor`, `<center>`, `cellpadding`, list markers,
`<pre>`, `display: none`, `visibility: hidden`, entities, `<hr>`, `<br>`, nested inline
boxes, shorthands, font-size units, size attributes, colour syntaxes, cascade order and
the tree builder's implied elements.

## Web Platform Tests

`crates/web/engine/tests/wpt/` is a sparse, pinned copy of web-platform-tests (commit and
tarball SHA-256 in `crates/web/engine/tools/fetch-wpt.py`; `python3 crates/web/engine/tools/fetch-wpt.py`
regenerates it, `--check` verifies the manifest). It holds reftests only, whole leaf
directories in the plan's order under a budget of about 1,500 pairs, excluding tests that
need script, SVG, vertical writing modes, `reftest-wait`, print media, nested documents
or real web fonts; `crates/web/engine/tests/wpt/README.md` lists the directories, the pairs per
directory and the exclusion counts. The runner `crates/web/engine/tests/wpt.rs` renders each
pair at 800×600 (the WPT default), resolving `<link rel=stylesheet>` and `@import` from
the tree and mapping `font-family: Ahem` to the bundled JetBrains Mono on both sides,
compares the rasters (`rel=match` must be identical, `rel=mismatch` must differ), writes
`target-parity/wpt-report.md` grouped by directory, and fails only when a directory's pass
count drops below `crates/web/engine/tests/wpt/expectations.json` (0 everywhere to start; raise an
entry to just under the achieved count once it holds).

## The fixtures

| Fixture | What it exercises |
|---|---|
| `google-1998` | The 1998 Google home page: centred table layout, `<font>`, `<center>`, `bgcolor`, the logo as styled text, two forms |
| `wikipedia-article` | An article: floated sidebar of link lists, header tabs as floats, an infobox table floated right with rowspans, headings with bottom borders, paragraphs with inline links and superscript references, a table of contents, a thumbnail figure, a wikitable, a references list |
| `docs-page` | A documentation page: floated fixed-width sidebar, `<pre><code>`, nested lists, a definition list, a blockquote, inline code, note and warning boxes with left borders, a parameter table |
| `hn-front` | Hacker News: the orange header table, the ranked story table with subtext rows and spacer rows, the footer links and search form |
| `acid1` | The W3C CSS1 test suite's `test5526c.htm` (Acid1), verbatim with the W3C licence notice, and its reference rendering `acid1-reference.gif` |
| `tables` | A torture page: colspan and rowspan, separate and collapsed borders, fixed layout, percentage widths, empty cells, nested tables, vertical alignment, presentational attributes, captions, thead and tfoot |
| `google-modern` | The current Google home page: full-viewport flex column, header packed right with a grid-of-dots icon, logo as coloured text, pill search box with pseudo-element icons and a shadow, two buttons, full-bleed grey footer with two `space-between` link rows; custom properties, `calc()`, a media query |
| `github-repo` | A repository page: dark top nav, repo header with pill counters, a tab row with an `::after` underline, a `position: sticky` sub-header, a `minmax(0, 1fr) 296px` grid with the latest-commit bar, the file table, the README in a bordered box (headings, `<pre><code>` in JetBrains Mono, lists, a table) and an About column (chips with `flex-wrap`, avatar circles, a stacked percentage bar) |
| `stripe-marketing` | A marketing page: a hero on a skewed `linear-gradient` band, `clamp()` fluid type, pill CTA buttons with shadows, a flex bar chart, a three-column feature grid with gradient-circle icons, a pricing grid with an absolutely positioned badge and `::before` check marks, a Georgia testimonial, a logo row in six families, a dark footer with a `2fr repeat(4, 1fr)` grid |
| `amazon-grid` | A search results page: dark flex header with a growing search bar and an absolutely positioned cart badge, a nav strip, a 240px filter sidebar (star rows, checkboxes, a price form), a `repeat(auto-fill, minmax(220px, 1fr))` grid of twelve cards (`aspect-ratio` thumbnails, line-clamped titles, `<sup>` cents, chips, corner ribbons, `margin-top: auto` buttons), pagination, a footer |
| `slack-shell` | An app shell: `100vh` flex column with `overflow: hidden`, a top bar, a rail, a purple sidebar with sections and unread pill badges, a main column with a `position: sticky` channel header, a `flex: 1; overflow: auto` message list with grouped messages, a blockquote, an attachment card and code in JetBrains Mono, a composer pinned at the bottom, and a right details panel that also scrolls |
| `acid2` | The Second Acid Test from web-platform-tests' `acid/acid2/` (WPT licence), with its subresources under `parity/acid2/` and a README on how it is viewed and where its `data:` images, `<object>` fallback, painting order and negative clearance are implemented; also the reftest pair `ref/acid2` against the pixel-for-pixel reference, compared at `#top` |

## Pages a framework renders

`crates/web/engine/tests/framework-parity/` holds apps drawn by the unmodified production
framework builds in `tests/vendor/`: `react18-tasks` (React 18, `createElement`, no build
step) and `vue3-tasks` (Vue 3's global build compiling a template at runtime). Both render
the same task tracker (a header with tabs, a filter sidebar, stat cards, an add form, a
list with badges and avatars, and a three-column board), and Chromium lays the two out
identically node for node. `<name>.steps.json` names states and the steps that reach
each one from a fresh load: `click` (a selector), `type` (text to the focused element),
`press` (a key).

The Chromium side is `dump.mjs` driven through those steps, one state at a time:

```sh
D=crates/web/engine/tests/framework-parity
for f in react18-tasks vue3-tasks; do for s in initial added filtered board; do
  node scripts/web-parity/dump.mjs $D/$f.html --state $D/$f.steps.json --state-name $s \
    --out $D/$f.$s.chromium.json
done; done
```

The engine side, `crates/web/engine/tests/framework_parity.rs`, runs the page on the
script layer (`Realm`) with the same bundles served at `/vendor/`, dispatches each step as
Playwright performs it (a click moves the pointer onto the element's centre and clicks
there), and dumps the realm's live document, styles and fragment tree. It writes
`target-parity/<name>.<state>.{engine.json,engine.png,report.md}` and fails when a
state's pass rate is below its entry in `framework-parity/thresholds.json`:

```sh
cargo test -p cw-web --features pipeline --test framework_parity -- --nocapture
```

### Real apps served as sites

The `oss-*` fixtures are real open-source apps as `worlds/oss-web` serves them
(docs/oss-webapps.md). TodoMVC and JSON Server's home load from a page like the others;
Conduit (React and Vue) and react-admin cannot (ES modules, chunks loaded from
root-relative paths, an API on another host), so each is a **site**:
`<name>.site.json` names the URL to open, the node-app package whose `public/` each
static host serves (with its SPA fallback), the API hosts, `<name>.api.json` (the
API's answers) and, for an app with its own timers, `settle_ms`:

```sh
D=crates/web/engine/tests/framework-parity
node scripts/web-parity/dump.mjs $D/oss-conduit-react.site.json \
  --state $D/oss-conduit-react.steps.json --state-name tag --out $D/oss-conduit-react.tag.chromium.json
```

dump.mjs answers every request from the package and the recording (anything else fails,
as offline in the world), waits for the network to go quiet and `settle_ms` after load
and each step, and runs Chromium's clock in UTC, as the world's is. An API request that
is not in the recording is listed and fails the dump: add it to `<name>.api.json`'s
list and record the answers from the world's own services (a fresh `worlds/oss-web`
world, the app's backend on the in-world VM):

```sh
cargo test -p computerworld --features oss-web --test oss_parity_record -- --ignored
```

`framework_parity.rs` serves the same files and recording to the realm and advances
`settle_ms` of virtual time. Development aids: `PARITY_HTML=<file>` makes dump.mjs write
the page's DOM after the steps (for choosing selectors), and `PARITY_EVAL=<js>` /
`CW_PARITY_EVAL=<js>` print what an expression evaluates to after the steps in Chromium
and in the engine, the quickest way to compare a computed style or a probe of intrinsic
sizes on both sides.

`--baseline-fonts` runs Chromium with a fontconfig that knows only the Liberation and
DejaVu directories (and includes the system's `conf.d` for hinting and aliases). JSON
Server's home asks for a system stack that reaches Ubuntu on the dump machine, a face
no stock desktop and not the engine has; its dump is taken with this flag.

### What still differs

Every framework-parity state matches Chromium node for node except two: JSON Server's
home (84/85: a text node with Arabic letters, which Chromium reports as one rect per
bidi run and the engine as one) and react-admin's posts list (940/941: one hidden
popover's input background). All 12 static fixtures are at 100%. What still differs is
visible only in the pictures (`gallery.mjs`, `compare.mjs`), and it is the
environment, not layout:

- **Glyph rasterisation.** Chromium draws text through Skia with FreeType hinting and
  subpixel positioning; `cw-render` rasterises the same faces its own way. Every run of
  text shows as a thin red outline in a difference picture even where its box, line and
  advance widths agree to the 1/64 px. This is most of each state's 0.3-7% of differing
  pixels (text-dense states such as app-inbox and app-chat the most).
- **Edge antialiasing.** Rounded borders, avatar rings and 1 px hairlines land on the
  same pixels but with different coverage, so they outline in red too.
- **Rounded shadows on transparent boxes.** The scene has no clip-out, so an outer
  `box-shadow` on a box whose background is not opaque is drawn in the four bands around
  the border box; on a rounded box the small corner pieces inside the rect but outside
  the curve are left unshadowed where Chromium shades them.

Chromium defaults of the machine the dumps were taken on that the engine reproduces
(the framework harness runs every realm with `FontEnvironment::LinuxBaseline`, like
the static runner),
and that would differ on another machine:

- **Fonts.** fontconfig there has only Liberation and DejaVu: `Arial`/`Helvetica`/
  `sans-serif` shape with Liberation Sans, `Times New Roman`/`serif` with Liberation
  Serif, `Courier New` with Liberation Mono, and the generic `monospace` (and Consolas)
  with DejaVu Sans Mono (`CSS.getPlatformFontsForNode` names it). Web fonts the page
  loads (Inter in the React apps) are the same file on both sides.
- **Text inputs' `size`.** A single-line control's width comes from the face's OS/2
  average and bounding-box widths as Linux Chromium's hinted metrics report them (the
  average goes up to a whole pixel when its fraction is a half or more; the box width
  rounds), so `layout::boxes::text_control_width` holds the values of those faces.
- **Closed `<select>` keys.** The steps press ArrowDown on focused selects; Chromium on
  Linux changes a menu list's selection on the arrows, PageUp/PageDown, Home/End and
  type-ahead and fires `input` then `change` at once (on macOS the arrows open the
  popup instead). The engine does what Linux Chromium does.
- **Scrollbars** are hidden (`--hide-scrollbars`), as described above.
