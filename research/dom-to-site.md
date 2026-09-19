# Converting real websites into computerworld sites

An assessment of whether real pages can be turned automatically into this repository's
deterministic, performance-optimised site format, and a prototype that does it for one
class of page. Prototype: [`scripts/dom-to-site/`](../scripts/dom-to-site/README.md).

The short answer: yes for structure, layout, colour, links and forms; no for pixels,
fonts, JavaScript behaviour and anything positioned outside normal flow. The
hand-authored sites in `worlds/company-2026/sites/` are already "structure and synthetic
content" rather than copies, so the honest target for a converter is the same thing: take
the *layout and affordances* of a real page, put the text through a synthesiser, and let
an existing service supply the behaviour.

## 1. What the target format can and cannot express

The format is `cw_protocol::Page` (`crates/protocol/src/lib.rs`, from the `Page` struct
near line 430 through `PageAction`): a title, an optional `PageTheme`, an optional `lang`,
and a tree of `PageElement`s. The browser lays it out in
`crates/browser/src/page_scene.rs` (`Layout::place`, `row`, `grid`, `layout_scrolled`).

### Elements

| Kind | What it draws | Notes for a converter |
|---|---|---|
| `heading`, `text`, `link`, `button`, `input`, `form`, `group` | The plain vocabulary: fixed sizes (headings 18/15 px, text 13 px), a link is an accent pill 38 px tall, a button is an accent box | `link` looks like a control, not a hyperlink in prose; use a `card` with a GET `action` when the original was styled text |
| `styled` | Text with `Style` (size 6–96, weight, colour, italic, align, one_line, fill, border, radius, padding, width, height) | The workhorse for headings, paragraphs, captions |
| `row` | Children left to right; fixed `width` children take it, the rest split by `flex`; wraps like `flex-wrap` when too narrow; `align` start/center/end/stretch; `scroll_x` for a shelf | Flex children never shrink below min-content (longest word) |
| `grid` | `columns` (1–12) equal columns, row-major, drops columns when cells would be narrower than content or a phone width | Equal columns only: `2fr 1fr` must become a `row` with `flex` |
| `card` | Filled, optionally bordered, padded container; with `action` the whole card is one click target | Default padding 14, radius 10, fill = theme surface; `#00000000` is a valid transparent fill |
| `thumbnail` | Flat colour block with a centred label, optional `action` | The stand-in for every photo, avatar and logo |
| `badge`, `divider`, `icon`, `spacer` | Pill, 1 px rule, one of ~65 bundled symbols (`PAGE_ICONS`), vertical gap | A `spacer` inside a `row` is a flexible push (no style → flex 1) |
| `image` | RGBA pixels served by the same origin as `application/vnd.computerworld.rgba+json` (`{width, height, rgba}`) | Same-origin only, ≤ 4 MiB decoded, 16 MiB browser cache (`crates/browser/src/lib.rs` `MAX_IMAGE_BYTES`, `MAX_CACHE_BYTES`) |

`PageTheme` gives accent, background, surface, ink, muted and `content_width`; a themed
page gets no drawn title and centres its column. Forms post `PageAction {method, url,
fields}`; the renderer names a form's purpose from its id (`form_purpose`: search, compose,
subscribe, comment, ...) and puts inputs in a bordered card. Validation limits:
`MAX_STYLE_SPAN` 64 (padding), `MAX_STYLE_RADIUS` 512, `MAX_PAGE_GAP` 128, extent 8192,
100 000 elements, 64 levels, unique non-empty ids, colours as `#rrggbb`/`#rrggbbaa` only.

The working tree (after release 0.1.1) adds `Style.mono` and `pin: "top"`; the
prototype's validator accepts both, the released Wasm build used for the renders has
neither.

### What it cannot express

- Arbitrary CSS: no per-side padding or borders, no margins (only `spacer`s and `gap`),
  no gradients, shadows, background images, opacity, transforms, `position:
  absolute/fixed/sticky` (only the top/bottom `pin` on top-level elements), no overlays,
  overflow clipping except `scroll_x`, no z-order, no line-height or letter-spacing, no
  text decoration except what a link pill implies.
- Typography: the bundled families only (`crates/scene/src/metrics.rs`: DejaVu, the Noto
  packs for scripts, the terminal mono). No web fonts, so widths differ from the capture.
- Behaviour: no JavaScript, hover, focus styles, menus, tabs, carousels, infinite scroll or
  client-side search. Every state change is a GET/POST to a service.
- Full-bleed regions: the layout owns a 16 px page margin and an optional centred column;
  a header that spans the viewport is drawn inside the column.
- Rich text: one `styled` is one run; inline links, bold words and code spans inside a
  paragraph are flattened.
- Icons beyond `PAGE_ICONS`; SVG art; video; canvas.

## 2. Proposed pipeline

Five stages, each with an inspectable artifact, so a bad site is caught at the stage that
lost it.

### 2.1 Capture (Playwright on the real page)

Record, for each visible element: tag, id/class, ARIA role and label, bounding box in page
coordinates, a fixed set of computed styles (display and flex/grid properties, colours,
border widths/colours/style, radius, padding, margins, font size/weight/style/family,
text-align, white-space/text-overflow, cursor), own text nodes, `href`, form
action/method, control type/name/value/placeholder/label, and for images only bounds,
alt, natural size and source host. Save a full-page screenshot and the ARIA snapshot.
Capture at more than one viewport (1280 and 390 wide) to learn which containers reflow.
Do not download images. Check `robots.txt` before opening a URL. Prototype:
`capture.mjs`.

### 2.2 Normalise (semantic segmentation)

Drop `display:none`, `aria-hidden`, zero-opacity, iframes, and ad/tracker/consent
patterns; record every drop. Label regions from landmarks first (`header`, `nav`, `main`,
`aside`, `footer`, `article`, `form`, `role=`), then from geometry: a full-width band at
the top with a wordmark and a row of links is the header, a narrow column beside a wide
one is a sidebar, repeated siblings with the same box structure are cards or list rows.
Collapse wrappers that draw nothing. Flatten inline runs into text with their links kept
aside. Prototype: the `prune`, `isTextLeaf`, `lines` and `container` functions in
`convert.mjs`.

### 2.3 Map (box tree → page elements)

- Layout from geometry, not CSS: cluster children into lines by vertical overlap; one line
  = `row`, several equal lines = `grid` (or `row`s with `flex` when tracks are unequal),
  otherwise a stack with `spacer`s for air. A row's outsize gap becomes a flexible
  `spacer` (the format's `margin-left: auto`).
- Boxes: fill or full border → `card`; bottom border → trailing `divider`; padding →
  the vertical padding for a single-line box (height matters), the horizontal inset for
  a stack (alignment matters).
- Text: `styled` with size/weight/colour/italic/align; short filled rounded → `badge`;
  a run that is one link → transparent `card` + `action` around a `styled` (keeps
  typography and clickability); ordered lists keep their numbers.
- Colours: `rgb()`/`rgba()` → hex; theme from body colours, the dominant link colour, the
  dominant card fill, the second text colour; `content_width` from the main column.
- Images: `thumbnail` with alt as label and the screenshot's mean colour over the box as
  fill (a tint, not the picture). Optionally `image` elements backed by *generated*
  artwork served as RGBA assets by the static-site service (`assets` in its state), never
  copied pixels.
- Forms: `form` + `input`s + `button`; the static-site service serves GET pages only and
  drops the query, so a form submits by GET to a stub confirmation page; a real backend
  comes from 2.4.
- Widths: text measured in Chrome is widened by a per-family ratio for the renderer's
  DejaVu (serif ~1.28, sans ~1.18, mono ~1.05). This is the weakest link; see 5.

### 2.4 Backend synthesis (site kind → service seed)

Detect the site's kind from signals (`<article>` counts, bylines, "ago" timestamps →
press; currency and "add to cart" → shop; "reply"/"upvote"/"points" → forum; `<pre>`
counts and "getting started" → docs; "inbox"/"compose" → mail; "follow"/"repost" →
social). With a confident kind, emit a seed for the matching existing service (`press`,
`shop`, `forum`, `wiki`, `mail`, `social`, `media`, `search`; shapes in
`worlds/company-2026/sites/*.json` and `services/*/src/lib.rs`) instead of a static tree:
the front page then comes from the service's own layout (`layout: wire|magazine|blog`
for press) and every interaction (save, follow, comment, cart, reply) really mutates
state and survives snapshots. The capture contributes the *records* (headlines, sections,
product names, thread titles) and the theme; bodies and prices are synthesised. Otherwise
emit a static-site tree with stub pages for every same-site link so the
`internet_links` crawl passes. Prototype: `detectKind` and `--as press` in
`convert.mjs`.

### 2.5 Verification and fidelity score

Load the seed into the Wasm build (`pkg/node`): the `World` constructor runs every
service's `initialize`, which parses each page through the Rust types, so the seed is
validated without cargo. Navigate the in-world browser, render at the capture's viewport
and several scroll offsets, and save the frame and the semantic scene. Compare:

- Structure: recall of the ARIA names (links, headings, buttons, text boxes, images)
  among the render's semantic labels; role agreement; clickable target counts.
- Visual: align each render viewport to the screenshot on the labels it shows, then
  compare a 64-column grid of mean colours and a grey SSIM. Coarse by design: the
  typeface differs, so pixel identity is not the goal.
- Links: crawl every `action`/`url` in the emitted pages against the seed (the repo's
  `internet_links` test does this for the shipped world).
- Score: `0.4 recall + 0.1 role agreement + 0.25 colour + 0.25 SSIM`, with the report of
  what was dropped alongside, so a reviewer sees what a high score still lost.

Prototype: `render.mjs`, `compare.mjs`, `validate.mjs`.

## 3. Legal and ethical constraints

- **robots.txt and ToS.** Fetching a page for conversion is crawling. Honour
  `robots.txt` (the capture script refuses disallowed paths unless told the page is
  yours) and site terms; many large sites forbid automated access or reuse in their
  terms regardless of robots.
- **Copyright in text and images.** Article bodies, product descriptions, photographs and
  logos are copyrighted. Shipping them inside `world.json` (which goes into the npm
  package and every release) would redistribute them. The capture stores no image bytes,
  and the converter copies visible text only into a *static* tree; the `--as` path keeps
  headlines and standfirsts and synthesises bodies. For anything published, run the text
  through a synthesiser (same length, same register, fictional facts) as the existing
  sites already do by hand.
- **Trademarks.** Domain names and brand names in the reference world are used as the
  names of fictional services on documentation-range addresses; keep that convention and
  never copy wordmark artwork.
- **Recommended policy.** Capture *structure and layout*; synthesise *content*; restrict
  raw-text conversion to sites you own, permissively licensed sites (CC-BY wikis with
  attribution kept in the seed), and fixtures written for the purpose. Keep a provenance
  record per converted site (source URL, capture date, licence, what was synthesised),
  as `research/sources.json` does for other inputs.
- **Personal data.** Comments, usernames and avatars on forums and social sites are
  personal data; never carry them over. Generate members as the existing forum and
  social seeds do.

## 4. Determinism constraints

- The simulator has no host access at run time (`docs/determinism.md`,
  `docs/networking.md`): every byte a page shows must be in the world definition or
  generated from the seed by a service. A converted site is a service `initial_state`;
  nothing is fetched from the real site later.
- Assets: images must be served by the site's own service as RGBA JSON assets
  (same-origin policy in the browser), each ≤ 4 MiB decoded, with a 16 MiB browser cache
  evicted lexically. Colour blocks (`thumbnail`) cost nothing; a 640×260 RGBA asset is
  ~665 KB raw and JSON-encoded is larger, so a site should carry at most a handful, or
  generate them procedurally in the service from the seed.
- Size: `worlds/company-2026/world.json` ships in the npm package and the release
  archives. The fixture conversion is 78 KB pretty-printed for one page with 19 stubs;
  a ten-page site is under 1 MB. Keep stubs minimal and prefer service seeds (records) to
  static trees (rendered elements) for anything with many pages.
- Ids must be unique per page and stable across runs (slugs of text, made unique in
  document order), so actions replay and semantic diffs are meaningful.
- No `Date`, `Math.random` or host fonts in anything the converter emits; the press seed
  uses fixed `tick` values in place of timestamps.

## 5. Achievable fidelity and what would raise the ceiling

Estimates of the `compare.mjs` score, after the mapping rules above, per site class:

| Site class | Structure recall | Visual | Notes |
|---|---|---|---|
| Documentation, blogs, news front pages, marketing pages | 0.9–1.0 | 0.8–0.9 | Flow layouts, cards, lists; the fixture scores 0.90 overall |
| Wikis, forums (thread lists, threads) | 0.9 | 0.7–0.85 | Tables and nested comments flatten to stacks; inline links in prose are lost |
| Shops (listing, product) | 0.85 | 0.7–0.8 | Grids map well; image-heavy; carousels, filters and variant pickers are JS |
| Search results, mail, calendars, dashboards | 0.6–0.8 | 0.5–0.7 | App-like: fixed sidebars, sticky headers, dense controls, overlays; the `--as` path with an existing service is far better than a static tree |
| Social feeds, video sites | 0.7 | 0.5–0.7 | Infinite scroll and media; use the `social`/`media` services |
| Anything canvas/WebGL, maps, editors | n/a | n/a | Out of scope; hand-built applications exist for these |

Renderer features that would raise the ceiling, roughly in order of payoff:

1. **Text metrics for the converter.** Export `cw_scene::metrics::text_width` through
   the Wasm (`SceneRenderer.measure(text, size, bold)`) so widths are measured, not
   estimated by a per-family ratio; this removes most wrapping differences.
2. **Per-side padding and a `margin`/`gap` on stacks**, or at least
   `padding: [v, h]`, so a box keeps both its height and its inset.
3. **`pin: top`** (landing in the working tree) for sticky headers, and a per-child
   `align` in rows for vertically centred wordmarks beside tall navs.
4. **Unequal grid tracks** (`columns: [2, 1]`) or `flex` on grid children, so the
   common `2fr 1fr` hero is one element instead of rows.
5. **Rich text runs**: a `styled` whose `text` carries a few inline spans with their own
   colour/weight/link, so prose keeps its links.
6. **Monospace** (`mono`, landing) and a wider icon set, and `thumbnail` gradients or
   two-tone fills so hero art does not look like a flat block.
7. **Full-bleed bands**: a top-level `bleed: true` that lets a row ignore the content
   column, so headers and footers span the viewport.
8. **Tables** (`table` with column widths) for wikis, docs and shops.
9. **Line height and letter spacing** on `styled`, for headline blocks.

Effort: the prototype (capture, convert, validate, render, compare, fixture) was about a
day; taking it to a tool that converts a class of sites reliably is roughly 2–3 weeks:
one week for the mapping rules across ten diverse fixtures with the compare loop, one
week for kind detection and seed emitters for press/shop/forum/docs/wiki with synthesised
content, and a few days for multi-page capture and merging, provenance records and a
`build-content.sh` hook. Each renderer feature above is a separate change to
`page_scene.rs` of a day or two with its tests.

## 6. Prototype results and next steps

Prototype: `scripts/dom-to-site/{capture,convert,validate,render,compare,png}.mjs`,
1 079 lines, no dependencies beyond the Playwright module on this machine. Tested on
`fixtures/news.html`, a fictional newspaper front page written for the purpose (masthead
and nav, ticker, hero with image and briefs, three story cards, a stream with a sidebar
holding a newsletter form, a numbered most-read list and a weather box, a footer, an ad
slot and a hidden tracker that must be dropped). No third-party site was fetched.

Results (files under `scripts/dom-to-site/fixtures/`):

- `capture.mjs`: 111 boxes, ARIA snapshot, 1280×1567 screenshot.
- `convert.mjs`: 138 elements on `/`, 19 stub pages, validation clean; the ad slot
  dropped; no inline links lost. Kind detected as press with confidence 0.96.
- `render.mjs`: the seed loads in the Wasm world (Rust serde validation passes), the
  browser navigates and renders; three viewports at scroll 0, 700 and 1400
  (`news.render*.png`). The masthead, ticker, hero, briefs with rules, story cards with
  badges, stream with dividers, sidebar cards, form, numbered list, weather row and footer
  are all present and clickable where they should be.
- `compare.mjs` (`news.compare.json`): structure recall 42/42, role agreement 1.0,
  27 clickable targets vs 25 in the capture (the form target and the current-nav link
  are extra); colour similarity 0.94/0.96/0.85 and grey SSIM 0.88/0.56/0.64 across the
  three viewports; fidelity 0.902.
- `--as press`: a press-service seed with 5 sections and 7 articles (title, dek, byline,
  section from the URL, synthesised body note) that loads and renders the same headlines
  through the press service's magazine layout (`news.press-render.png`), with follow,
  reading list and article pages working from the existing backend.

What still differs on the fixture: the renderer's DejaVu is larger than Georgia, so
lines wrap earlier and the page is ~6% taller; the header is inside the content column
rather than full-bleed; the `<img>` alt is drawn as a caption in a tinted block; the
form gets the renderer's bordered card; stub pages hold a title and a back link only.

Next steps, in order:

1. Ten more fixtures across the classes in section 5 (docs page, shop listing, forum
   thread, wiki article, marketing page, dashboard) with the compare loop in a small test
   (`node --test`) so mapping regressions show as score drops.
2. Expose text metrics from the Wasm and replace `widthRatio`.
3. Multi-page capture (`capture.mjs --crawl 2`) and a merge that writes one site with
   real pages instead of stubs, keyed by path.
4. Seed emitters for shop, forum, docs and wiki with a content synthesiser (length- and
   register-preserving, fictional facts) and a provenance record per site.
5. Capture at a phone viewport and use the two layouts to decide which grids reflow,
   rather than leaving it to the renderer's rule.
6. The renderer features in section 5, starting with per-side padding and `pin: top`.
