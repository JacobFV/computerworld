# Layout parity against Chromium

The web engine in `crates/web` is measured against Chromium, which Playwright drives
through the same module the capture pipeline uses. For every fixture page Chromium dumps
each element's box and a fixed set of computed properties; the engine dumps the same at
the same viewport; the two are compared with a small tolerance, and a report says what
disagrees. This isolates the cascade and layout from rasterisation, which the fonts
would otherwise blur.

## Files

| Path | What |
|---|---|
| `crates/web/tests/parity/<name>.html` | A fixture: a self-contained page, CSS inline, no external requests, synthetic text |
| `crates/web/tests/parity/<name>.chromium.json` | Chromium's dump of it (checked in) |
| `crates/web/tests/parity/<name>.chromium.png` | Chromium's screenshot of the viewport |
| `crates/web/tests/parity/<name>.fonts.json` | Each distinct `font-family` list the page asks for and the platform face Chromium shaped it with |
| `crates/web/tests/parity/thresholds.json` | The pass rate each fixture must reach (0 to 1) |
| `crates/web/target-parity/<name>.engine.json` | The engine's dump, same shape (written by the Rust runner; not checked in) |
| `crates/web/target-parity/<name>.engine.png` | The engine's raster through `cw-render` |
| `crates/web/target-parity/<name>.report.md` | Pass count, mismatches by property, worst offenders with expected and got |
| `crates/web/target-parity/<name>.compare.png` | Chromium, the engine and their difference side by side (from `compare.mjs`) |
| `crates/web/tests/ref/<name>.html`, `<name>-ref.html` | Reftest pairs: two documents that must paint to the same scene |

Scripts: `dump.mjs` (Chromium side), `compare.mjs` (reports and pictures), `common.mjs`
(the property list and comparison rules, shared by both). The Rust side is
`crates/web/tests/parity.rs`, `crates/web/tests/reftest.rs` and their shared
`crates/web/tests/support/mod.rs`, which holds the same rules in Rust so `cargo test`
needs no Node.

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
   node scripts/web-parity/dump.mjs crates/web/tests/parity/google-1998.html
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
   ```

   The `pipeline` feature compiles the calls into `html::parse`, `css::parse_stylesheet`
   and `style::cascade`; without it those tests are ignored and only the dump format,
   thresholds and reftest file layout are checked. The runner writes the engine dump,
   PNG and `report.md` for every fixture into `crates/web/target-parity/` whether or not
   the threshold is met.

3. **Read the reports**: `crates/web/target-parity/<name>.report.md` lists the pass
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

## Reftests

`crates/web/tests/ref/<name>.html` and `<name>-ref.html` are two documents that must
paint identically: the runner parses, cascades, lays out and paints both at 1280×800
and compares scene digests (`Scene::stamp` after erasing node ids, since the two DOMs
have different node numbering). A failing pair leaves `ref-<name>.test.png` and
`ref-<name>.ref.png` in `target-parity/` to look at. The pairs cover Milestone 1:
margin collapsing, auto-margin centring, floats against a table, inline padding,
`<b>` against `font-weight`, `bgcolor`, `<center>`, `cellpadding`, list markers,
`<pre>`, `display: none`, `visibility: hidden`, entities, `<hr>`, `<br>`, nested inline
boxes, shorthands, font-size units, size attributes, colour syntaxes, cascade order and
the tree builder's implied elements.

## The fixtures

| Fixture | What it exercises |
|---|---|
| `google-1998` | The 1998 Google home page: centred table layout, `<font>`, `<center>`, `bgcolor`, the logo as styled text, two forms |
| `wikipedia-article` | An article: floated sidebar of link lists, header tabs as floats, an infobox table floated right with rowspans, headings with bottom borders, paragraphs with inline links and superscript references, a table of contents, a thumbnail figure, a wikitable, a references list |
| `docs-page` | A documentation page: floated fixed-width sidebar, `<pre><code>`, nested lists, a definition list, a blockquote, inline code, note and warning boxes with left borders, a parameter table |
| `hn-front` | Hacker News: the orange header table, the ranked story table with subtext rows and spacer rows, the footer links and search form |
| `acid1` | The W3C CSS1 test suite's `test5526c.htm` (Acid1), verbatim with the W3C licence notice, and its reference rendering `acid1-reference.gif` |
| `tables` | A torture page: colspan and rowspan, separate and collapsed borders, fixed layout, percentage widths, empty cells, nested tables, vertical alignment, presentational attributes, captions, thead and tfoot |
