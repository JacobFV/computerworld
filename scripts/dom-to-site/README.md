# dom-to-site

A prototype that turns a real page's DOM into a computerworld site seed: the JSON a
`static-site` (or `press`) service is initialised with, in the shape of
`worlds/internet/sites/<id>.json`. Node 18+, ESM, no dependencies beyond the
Playwright module already on this machine for the capture step. The assessment behind it
is in [`research/notes/dom-to-site.md`](../../research/notes/dom-to-site.md).

```
capture.mjs   real page  ->  <name>.capture.json + <name>.capture.png   (Playwright)
convert.mjs   capture    ->  <name>.site.json + <name>.site.report.json  (pure Node)
render.mjs    site seed  ->  what the in-world browser shows, + semantic scene   (pkg/node Wasm)
compare.mjs   capture + renders -> fidelity score
```

## Usage

```sh
export PLAYWRIGHT_MODULE=/usr/lib/chatgpt/resources/cua_node/lib/node_modules/playwright/index.mjs
export CHROME_BIN=/usr/bin/google-chrome
cd scripts/dom-to-site

node capture.mjs fixtures/news.html fixtures/news.capture.json          # or a URL you own
node convert.mjs fixtures/news.capture.json meridian meridian.example fixtures/news.site.json
node render.mjs  fixtures/news.site.json fixtures/news.render.png       # needs pkg/node
node render.mjs  fixtures/news.site.json fixtures/news.render-scrolled.png --scroll 700
node compare.mjs fixtures/news.capture.json fixtures/news.render.scene.json fixtures/news.render-scrolled.scene.json
```

`convert.mjs --as press` emits a seed for the press service instead (headlines, deks,
bylines and sections synthesised from the capture; bodies are not copied), which
`render.mjs` also loads. The converted file can be dropped into
`worlds/internet/sites/` as is: it carries `place`, so `cw-world`
gives it a host, and stub pages for every same-site link so the `internet_links` crawl
finds nothing dead.

### capture.mjs

Opens the page at 1280x800, waits for network idle, and records for every visible
element: tag, id/class, ARIA role/label, page bounds, a fixed set of computed styles
(display, flex/grid, colours, borders, radius, padding, font size/weight/style/family,
alignment), its own text, `href`, form action/method, control type/name/value/placeholder
and `<label for>` text, and for images only bounds, alt, natural size and source host.
Nothing is downloaded and no image bytes are stored. It also saves a full-page screenshot
and Playwright's ARIA snapshot. An `http(s)` URL is checked against `robots.txt` first;
`--ignore-robots` exists for pages you own.

### convert.mjs

Mapping rules (all in one file, in this order):

- Drop: `display:none`, `visibility:hidden`, `aria-hidden`, `opacity:0`, iframes, and
  ids/classes matching ad/tracker/cookie-consent patterns. Every drop is listed in the
  report.
- Theme: body background and ink; accent = the most common link colour; surface = the
  most common card fill; muted = the second most common text colour; `content_width` =
  the tallest block that is narrower than the viewport (`<main>` wins).
- Layout is decided by geometry, not by CSS: a container's children are clustered into
  lines by vertical overlap. One line with several children is a `row` (gap = median
  horizontal gap; an outsize gap becomes a flexible `spacer`; `align` from
  `align-items`); several lines with the same column positions are a `grid` (equal CSS
  tracks) or a group of `row`s with `flex` shares (unequal tracks such as `2fr 1fr`);
  otherwise a vertical stack, with `spacer`s where the page left more than ~20 px of air.
- A container with a fill different from its parent's, or a full border, is a `card`
  (fill, border colour, radius, padding). A bottom-only border becomes a `divider` after
  the element. Other containers are `group`s.
- Text runs (an element whose subtree is inline) become `styled` with size, weight,
  colour, italic, alignment; a short filled rounded run is a `badge`; a run that is one
  link is a transparent `card` with a GET `action` around the `styled` text, so it keeps
  its typography and is still a click target. Ordered lists keep their numbers.
- `img`/`svg`/`picture` become `thumbnail`s with the alt text as label and the
  screenshot's mean colour over the image bounds as fill.
- Forms become `form` + `input`s (label from `<label for>`, placeholder, value) +
  `button`. The static-site service serves GET pages only and ignores the query string, so
  a form submits with GET to a stub page at its own path; a real backend needs `--as`.
- Widths: text measured in Chrome is widened for the renderer's DejaVu Sans by a ratio
  per font family (serif 1.28, sans 1.18, mono 1.05). Row children keep measured widths
  when they are short text, otherwise share the row by measured proportion (`flex`).
- Ids are slugs of text/class/tag, made unique; links to the same site become paths, other
  hosts stay absolute `http://` URLs.

`validate.mjs` mirrors `Page::validate`, `Style::validate`, `PageTheme::validate` and the
static-site `initialize` checks, so a conversion fails fast without a build. The
authoritative check is `render.mjs`, whose `World` constructor parses every page through
the Rust types.

### render.mjs

Builds a one-workstation world with the site as its only internet host, navigates the
in-world browser to it, and writes the 1280x800 frame, the page area alone
(`<out>.page.png`) and the semantic nodes (`<out>.scene.json`: role, label, bounds,
interaction target). `--scroll <px>` scrolls the page first.

### compare.mjs

- Structure: recall of the capture's ARIA names (links, headings, buttons, text boxes,
  images) among the render's semantic labels, role agreement, and the count of clickable
  targets on each side.
- Visual: each render viewport is aligned to the screenshot on the labels it shows (the
  converted page is never exactly the capture's height), then compared on a 64-column grid
  of mean colours and a grey-level SSIM. Coarse on purpose: the typeface differs, so pixel
  identity is not the goal.
- `fidelity` = 0.4 recall + 0.1 role agreement + 0.25 colour + 0.25 SSIM.

## Results on the fixture

`fixtures/news.html` is a small fictional newspaper front page written for this test
(masthead + nav, ticker, hero with image and briefs, three story cards, a stream with a
sidebar holding a newsletter form, most-read list and weather box, footer, plus an ad slot
and a hidden tracker div that must be dropped). Produced files are next to it. Score:

```
structure: 42/42 accessible names found (recall 1), role agreement 1, interactive targets 27 vs 25
visual @0:    colour similarity 0.936, grey SSIM 0.881
visual @700:  colour similarity 0.956, grey SSIM 0.562
visual @1400: colour similarity 0.853, grey SSIM 0.635
fidelity 0.902
```

## Limits

- Only what the page format can express: no absolute positioning, overlays, sticky
  headers, hover states, animation, custom fonts, gradients, background images, or per-side
  padding and borders; a full-bleed header is drawn inside the content column.
- Layout is inferred from one viewport; responsive behaviour comes from the renderer's own
  rules, not the site's breakpoints.
- Prose with links inside it loses the links (listed in the report as
  `inline_links_dropped`); every whole-run link is kept.
- One page per capture. Stub pages make links resolve but hold no content; a site of many
  captured pages needs one capture per page and a merge (not written).
- Images are colour blocks with the alt text; no pixels are copied. Icons are not detected.
- JavaScript behaviour (menus, tabs, infinite scroll, client-side search) is not captured;
  the `--as <kind>` path is the answer to that where an existing service has the behaviour.
- Fonts: the renderer uses its own families; widths are estimated by a ratio per family
  (`widthRatio` in `convert.mjs`), which is the main source of wrapping differences.
- Do not point `capture.mjs` at third-party production sites without permission; see the
  legal section of the research note.
