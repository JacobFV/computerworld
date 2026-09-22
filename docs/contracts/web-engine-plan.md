# Plan: a real web engine for the in-world browser

Status: **M1, M2, M4 and M5 shipped in 0.2.0 (2026-09-20)**; M3 shipped with them, since
the frameworks in M4 cannot run without it. `cw-web` is the engine described below, and
every service serves HTML through it. What follows is the plan as written on 2026-09-19,
kept because the constraints, the architecture and the gates are still the contract the
engine is held to; read `crates/web/engine/DESIGN.md` for the interfaces as built, the 0.2.0
entry in `CHANGELOG.md` for what the gates actually returned, and
[html-migration.md](../html-migration.md) for the migration recipe. M0's remaining item —
deleting the old page renderer once nothing depends on it — is still open: `Page` is
behind `cw_web::page::to_document`.

## The problem

The simulated internet looks wrong while the desktop applications look right. The
desktop applications are widgets drawn by code written for them. The sites are
documents, and they are authored in `cw_protocol::Page`, a vocabulary that cannot say
what documents say: no margins, no per-side borders, no positioning, no inline rich text,
no shadows or gradients, a mandatory page gutter, links drawn as pills, one font family.
`research/notes/dom-to-site.md` §1 is the full list. Authoring the sites as Rust element trees
compounds it, because the author has no CSS prior and no feedback loop, but a better
author in the same format hits the same ceiling.

The fix is for the browser to interpret what the web is written in: HTML, CSS, and the
JavaScript that manipulates both. The hard part is not any one of those. It is that
style reaches an element through every mechanism the web has accumulated, and a page
looks right only when all of them agree:

| Era | How style arrives | What the engine must do |
|---|---|---|
| HTML 1.0 to 4 | Presentational attributes: `bgcolor`, `width`, `align`, `border`, `cellpadding`, `<font>`, `<center>`, `<b>`, table layout | Map attributes to presentational hints that sit below author CSS in the cascade, exactly as HTML 5 specifies; implement table layout, since these pages are tables |
| CSS 1 to 3 | `<style>`, `<link rel=stylesheet>`, `@import`, `@media`, `style=""`, the cascade (origin, `!important`, specificity, order), inheritance, initial values, the user-agent sheet, shorthands, custom properties, `calc()`, pseudo-classes, `::before`/`::after`, `@font-face` | A correct cascade with computed values per element, invalidated when the document or the matching state changes |
| Web 2.0 | `element.style.x = …`, `className`/`classList`, `setAttribute("style")`, `document.styleSheets[i].insertRule`, `getComputedStyle`, layout reads such as `getBoundingClientRect` and `offsetWidth`, `:hover`/`:focus`/`:active`, jQuery-style show/hide and animation | JS bindings to the DOM and CSSOM; layout on demand when script reads geometry; event-driven restyle |
| Frameworks | React and Preact (plain DOM operations behind a scheduler), Vue, Svelte; Tailwind (a large static sheet of utility classes); CSS modules (build-time, plain classes); CSS-in-JS such as styled-components and emotion (inject `<style>` elements and rules at runtime); `@keyframes` transitions and animations | Nothing beyond the DOM, CSSOM, timers and microtasks, done faithfully enough that unmodified production bundles run; animations resolved to their end state or stepped on the world clock |

Every row ends at one place: a computed style per element, then layout, then paint. The
plan is organised around that pipeline, and each milestone widens the set of mechanisms
that feed it.

## Constraints that shape the design

- **Determinism across platforms.** The release gate compares state and pixel hashes
  between five wheel platforms and Node/Wasm. Layout therefore uses fixed-point
  integers (1/64 px "app units", as Gecko and Servo do), never `f32`, and no
  transcendental functions. Timers, `requestAnimationFrame` and animations run on the
  world clock (`docs/determinism.md`), never the host's.
- **Snapshots.** The browser session is part of the checkpoint. A DOM, its stylesheets,
  the JS heap and pending timers must serialise. The JS VM already serialises its heap
  for snapshots (`crates/languages/jsvm`), so this is an extension, not a new capability.
- **Budget.** The Wasm build is 26 MB, mostly fonts and world data. The engine's code
  can be a few MB; a frame of a typical page must lay out and paint in the same
  order of time the current renderer takes, because rollouts render thousands of frames.
- **Precedent on dependencies.** The repository writes its own regex, zlib, time zone,
  JavaScript and Python engines for determinism and control. The plan keeps that:
  parser, cascade and layout are written here in fixed point. `html5ever`, `cssparser`
  and `taffy` are the alternatives and would cut months; they are rejected because
  `taffy` lays out in `f32` and the other two would be the only foreign parsers in an
  otherwise self-owned pipeline. This is a decision to revisit at Milestone 2 if the
  hand-written layout is behind schedule.
- **Two strictness modes.** Sites authored in this repository are validated strictly:
  an unsupported property, selector or element fails the build by name, so that no one
  writes CSS that will not render. Pages captured from the real web render leniently,
  as browsers do, ignoring what is not understood and logging it.

## Architecture

New crate `crates/web/engine` with these modules, each with its own conformance tests. The
existing `crates/web/browser` becomes the chrome (tabs, address bar, history, cursor) around
it, and `cw_protocol::Page` becomes one more input format, converted to HTML plus an
internal stylesheet, so nothing existing breaks during migration.

1. **`html`**: an HTML 5 tokenizer and tree builder, including the error-recovery
   rules that real pages depend on (unclosed `<p>` and `<li>`, misnested formatting
   elements, implied `<tbody>`, `<table>` foster parenting). Character references,
   `<script>`/`<style>` raw text, `<template>`. No XML, no SVG at first (an SVG element
   becomes a box with its width and height).
2. **`dom`**: an arena of nodes with stable, deterministic ids; element, text, comment,
   document, document fragment; attributes; a `classList`; the `id`/`class`/tag indexes
   that selector matching needs. Every mutation records what changed so restyle and
   relayout are incremental. The arena is the snapshot representation.
3. **`css`**: a CSS Syntax Level 3 tokenizer and parser (any input, spec error
   recovery), selectors (Level 3 complete; Level 4 `:is`, `:where`, `:not`, `:has` with
   a subject restriction, `:nth-child(of)`), at-rules (`@media` with width, height,
   `prefers-color-scheme`, `hover` and `pointer`; `@import`; `@font-face`; `@keyframes`;
   `@supports` evaluated against the supported set; `@layer`), declarations with
   `!important`, and a typed value grammar per property with shorthand expansion
   (`margin`, `padding`, `border*`, `background`, `font`, `flex`, `grid-*`, `inset`,
   `gap`, `place-*`, `transition`, `animation`). Property table generated from one
   source listing each property's syntax, initial value, inheritance and computed form.
4. **`style`**: the cascade. Inputs in cascade order: the user-agent sheet (a real one,
   modelled on the HTML spec's rendering section, including `<table>` defaults,
   `<button>` and `<input>` appearance, and quirks-mode differences), presentational
   hints mapped from attributes, author sheets in document order, inline `style`, JS
   changes (which are inline style or sheet mutations, so they need no separate path).
   Then specified to computed values: inheritance, `initial`/`inherit`/`unset`/`revert`,
   custom properties with fallback and cycle detection, `calc()` and `min/max/clamp`,
   relative units (`em`, `rem`, `ex`, `ch`, `%`, `vw`, `vh`, `vmin`, `vmax`) and colour
   syntaxes (`#hex`, `rgb`, `hsl`, `hwb`, named, `currentColor`, `color-mix` later).
   Pseudo-elements `::before`, `::after`, `::placeholder`, `::marker`, `::selection`.
   Matching state: `:hover`, `:active`, `:focus`, `:focus-visible`, `:focus-within`,
   `:checked`, `:disabled`, `:visited` (always unvisited, for determinism), `:target`,
   `:empty`, `:link`. Invalidation is by the standard set of dependencies (a rule's
   rightmost compound selects what to restyle; class, id, attribute and state changes
   are looked up in a rule index) so a `classList.toggle` restyles a subtree, not the
   document.
5. **`layout`**: fixed-point box tree from the styled DOM. Formatting contexts in this
   order of implementation: block (margins, collapsing, `width: auto`, `min/max`),
   inline (line boxes, inline boxes with borders and padding, `vertical-align`,
   `white-space` modes, `text-overflow`, `word-break`, soft hyphens, bidi through the
   existing `unicode-bidi` tables), replaced elements (`<img>`, `<input>`, `<button>`,
   `<select>`, `<textarea>`, `<canvas>` as a box), tables (automatic and fixed
   algorithms, spans, `border-collapse`, captions), floats and clearance, positioned
   boxes (`relative`, `absolute`, `fixed`, `sticky`) with containing blocks, flex (the
   full algorithm including `flex-wrap`, `order`, `align-content`, `min-content`
   auto-minimums), grid (explicit and implicit tracks, `fr`, `minmax`, `auto-fill`,
   line names, areas, `auto-placement`; subgrid and masonry excluded), `overflow`
   scroll containers, `display: none`/`contents`/`inline-block`/`list-item`, columns
   excluded. Intrinsic sizing (`min-content`, `max-content`, `fit-content`) is cached
   per box and invalidated with layout. Fragmentation and printing excluded.
6. **`paint`**: box tree to the existing `cw_scene::Scene` primitives (`Box`,
   `RoundedBox`, `Text`, `Image`, `Shadow`, `Path`, clips). Stacking contexts and
   `z-index` order, backgrounds (colour, `linear-gradient`, `radial-gradient`, images
   with `background-size`/`position`/`repeat`), per-side borders and styles (solid,
   dashed, dotted, double; the rest drawn solid), `border-radius` with per-corner
   radii, `box-shadow` and `text-shadow`, `opacity` (a group), `transform` for 2-D
   translate, scale and rotate (rotate needs a scene transform; the `Transform` type
   exists), `filter` excluded except `opacity`. Text decoration (underline, overline,
   line-through, style and colour). The scene's semantic layer (`Semantic`, `AxNode`)
   is filled from the DOM's roles and ARIA attributes, so the agent-facing accessibility
   tree gets better, not worse.
7. **`fonts`**: the missing piece the current sites suffer from most. Bundle a set of
   openly licensed web faces with metric-compatible stand-ins for the families pages
   ask for: Arimo for Arial and Helvetica, Tinos for Times New Roman, Cousine for
   Courier New, Gelasio for Georgia, Carlito for Calibri, Caladea for Cambria, plus
   Inter, Roboto, Open Sans, Lato, Source Sans and Source Serif, Poppins, Montserrat,
   Playfair Display and JetBrains Mono as themselves. `font-family` fallback lists
   resolve through a table of aliases and generic families (`sans-serif`, `serif`,
   `monospace`, `system-ui` per theme). `@font-face` with a same-origin `src` is
   accepted only for faces in the bundle (matched by name), so no page can ship bytes.
   Text shaping through the `rustybuzz` and `ttf-parser` already in the lock file, with
   advances quantised to app units. Weights 400 and 700 for every face, 300, 500, 600
   where the family has them; synthetic bold and oblique otherwise.
8. **`script`**: bindings from `cw_jsvm` to the DOM and CSSOM. The VM already has
   Node's timers and microtasks on the world clock; the browser realm replaces the Node
   globals with `window`, `document`, `navigator`, `location`, `history`,
   `localStorage`/`sessionStorage` (snapshot state), `fetch` and `XMLHttpRequest` to
   in-world services over the existing transport, `requestAnimationFrame` tied to
   frames, `MessageChannel` (React's scheduler uses it), `MutationObserver`,
   `ResizeObserver` and `IntersectionObserver` (delivered after layout), `matchMedia`,
   `CustomEvent`, `URL`, `TextEncoder`, `structuredClone`, `performance.now` on the
   world clock. DOM: the `Node`/`Element`/`HTMLElement` hierarchy with the members
   frameworks touch (`createElement`, `createElementNS` for SVG stubs,
   `createTextNode`, `appendChild`, `insertBefore`, `removeChild`, `replaceChild`,
   `cloneNode`, `textContent`, `innerHTML`/`outerHTML` through the parser,
   `insertAdjacentHTML`, `setAttribute`/`getAttribute`/`removeAttribute`/`hasAttribute`,
   `dataset`, `className`, `classList`, `id`, `style` as a live `CSSStyleDeclaration`,
   `querySelector`/`querySelectorAll`/`closest`/`matches`, `getElementById`,
   `getElementsBy*` as live collections, `children`, `childNodes`, `parentNode`,
   `nextSibling`, `firstChild`, `contains`, `focus`/`blur`, `click`, form element
   values and `checked`, `scrollTop`/`scrollIntoView`, `getBoundingClientRect`,
   `offsetWidth/Height/Top/Left`, `clientWidth/Height`, `getComputedStyle`). CSSOM:
   `document.styleSheets`, `CSSStyleSheet.insertRule`/`deleteRule`/`cssRules`,
   `CSSStyleRule.style`, `adoptedStyleSheets`, `CSSStyleDeclaration.setProperty` with
   priority. Events: capture, target and bubble phases, `preventDefault`,
   `stopPropagation`, `addEventListener` options, the events frameworks synthesise
   from (`click`, `dblclick`, `mousedown/up/move/enter/leave/over/out`, `pointer*`
   aliases, `keydown/keyup/keypress`, `input`, `change`, `submit`, `focus`/`blur`,
   `focusin/out`, `scroll`, `resize`, `load`, `DOMContentLoaded`, `transitionend`,
   `animationend`, `wheel`). Layout reads flush pending style and layout, as in a real
   engine, so `offsetWidth` after a class change is correct.
9. **`session`**: the document lifecycle. Fetch the resource, parse progressively,
   fetch subresources (stylesheets block rendering, `<script>` runs in order,
   `defer`/`async`/`type=module` with the VM's module loader), fire `DOMContentLoaded`
   and `load`, run the event loop until quiescent or the frame budget is spent, then
   style, lay out and paint. Navigation, `history.pushState`, hash targets, form
   submission with the HTML encoding rules, `<meta name=viewport>` on phones,
   `<iframe>` (same-origin only, laid out as a nested document; cross-origin drawn as a
   box). Memory and time caps per document, enforced by the VM's existing fuel counter.

## Interaction model

The agent's actions do not change. A click at a point hit-tests the box tree, dispatches
the mouse and click events through the DOM, then performs the default action if not
prevented: follow the link, submit the form, toggle the checkbox, focus the input. Typing
goes to the focused editable element and fires `input`. Hover moves, which already exist
for the cursor, set `:hover` and dispatch `mouseover`/`mouseenter`, so menus that open
on hover work. Scrolling scrolls the nearest scroll container. The `cursor` hint comes
from computed style, replacing the current hard-coded `CursorKind` for pages. Text
selection and drag remain as they are.

## Frameworks: what must run, in order

Each is a fixture site in `worlds/` served by the static-site service, with the
unmodified production bundle, and a test that drives it through the agent API:

1. Vanilla DOM and a jQuery 3 page (menus, tabs, accordion, AJAX to a service).
2. Preact with hooks: small, plain DOM operations, the first framework gate.
3. React 18 with `react-dom` (production): the scheduler on `MessageChannel`, synthetic
   events on a root listener, controlled inputs, `useEffect` timing after paint.
4. Vue 3 (runtime build with templates precompiled) and Svelte 4 compiled output.
5. Tailwind: a generated stylesheet of several thousand utility rules, a test that
   selector matching stays fast with a large rule index.
6. styled-components and emotion: runtime `insertRule` into a constructed sheet,
   restyle correctness when rules arrive after first paint.
7. Next-style static export: hydration of server-rendered HTML with a client bundle.

Excluded and named as such: WebGL, Web Audio, video decoding, Web Workers in the page
(the VM has workers; a page worker is a later addition), Service Workers, WebSockets
(a later addition over the existing transport), `contenteditable` beyond plain text,
cross-origin iframes, CSS Houdini, `@container` queries at first, print.

## Serving HTML

Services keep their state and routes and change their responses from Page JSON to
`text/html`. `services/common` gets a small HTML template layer (typed builders that
escape correctly, and a Rust-side stylesheet per site kept in a `.css` file next to the
service so it is written as CSS, not as Rust). Each site's HTML and CSS live in files an
author can open in a real browser during development, and the conformance harness checks
that the in-world render matches that browser. The static-site service serves authored
directories of HTML, CSS, JS and images. The `Page` format remains supported for
applications that use it through a converter, and can be retired when nothing does.

The capture pipeline in `scripts/dom-to-site/` inverts: instead of compressing a real
DOM into the element vocabulary, it records the DOM with its computed styles into a
self-contained HTML document with synthesised text and generated images, which the
engine can now render nearly as captured. `research/notes/dom-to-site.md` §5 lists the losses;
most of them disappear.

## Verification

The engine is measured against Chromium, which the toolchain already drives through
Playwright:

- **Layout parity**: for every fixture page, Chromium dumps each element's
  `getBoundingClientRect` and a chosen set of computed properties; the engine dumps the
  same; the test compares with a tolerance of one pixel for positions and none for
  computed values that are not lengths. This is the primary gate because it isolates
  cascade and layout from rasterisation.
- **Pixel parity**: the existing SSIM and colour-grid comparison, coarse by design,
  because the fonts differ in hinting and anti-aliasing.
- **Web Platform Tests**: a curated subset of the CSS reftests (two documents that must
  render identically) run through the engine alone, no Chromium needed. Reftests suit a
  deterministic engine perfectly. Start with `css/CSS2` block and inline, then
  `css-flexbox`, `css-grid`, `css-tables`, `css-position`, `selectors`, `css-cascade`,
  `css-variables`, `css-values`. Track the pass count per milestone.
- **Acid1** at Milestone 1, **Acid2** at Milestone 2 (excluding its data-URI and
  `object` fallback parts if those are out of scope, and saying so).
- **Framework fixtures** as above, driven through the agent API, checked by state and
  by screenshot.
- **Determinism**: every fixture rendered on all release platforms with identical
  pixel and scene hashes, added to the existing parity gate.
- **Performance**: a budget per fixture for parse, style, layout and paint, measured in
  CI on the same machine class, failing on regression.

## Milestones

Each milestone ships behind a feature flag on the browser and is usable on its own.

**M0: fonts and the ceiling test.** Bundle the web faces and alias table and use them in
the current renderer. Hand-author the Google homepage in the current `Page` format with
maximum care, next to an HTML mock rendered in Chromium, and record the gap in
`research/`. This is a week of work and settles the diagnosis in writing.

**M1: documents.** `html`, `dom`, `css`, `style`, block and inline layout, tables,
presentational hints, the user-agent sheet, paint, no script. Gates: Acid1; the 1998
Google homepage, a Wikipedia article, a plain-HTML documentation page and the Hacker
News front page each within layout parity. The `Page` converter, so every existing site
renders through the new engine and the old renderer can be compared frame for frame.

**M2: modern CSS.** Floats, positioned boxes, flex, grid, overflow and scrolling,
`z-index`, gradients, shadows, radii, transforms, media queries, custom properties,
`calc`, pseudo-elements, `@font-face` to the bundle, transitions and animations resolved
on the world clock. Gates: Acid2; the current Google homepage, a GitHub repository page,
a Stripe-style marketing page, an Amazon-style product grid, a Slack-style app shell,
each within layout parity; the WPT subsets above at agreed pass rates.

**M3: script.** DOM and CSSOM bindings, events, timers, `fetch`, storage, the document
lifecycle, layout reads, `:hover`/`:focus`/`:active` through real events. Gates: the
vanilla and jQuery fixtures; snapshot and restore of a page mid-interaction; determinism
across platforms.

**M4: frameworks.** Preact, React, Vue, Svelte, Tailwind, styled-components, emotion,
hydration. Gates: the fixture list above, unmodified bundles, each driven end to end
through the agent API.

**M5: migration.** Every service serves HTML; the sites are rewritten as HTML and CSS
with the strict validator on; the capture pipeline emits HTML; `Page` is behind a
converter with a deprecation note; the old page renderer is deleted when the frame
comparison from M1 shows nothing depends on it.

## Risks

- **Scope of CSS.** The property table is where the work hides. The plan bounds it by
  generating the table from one source and by the strict validator, so the supported set
  is explicit and authored sites never depend on what is missing.
- **Performance of a general layout engine** against a renderer that was fast because
  its format was tiny. Mitigations: incremental restyle and relayout, intrinsic size
  caching, a frame budget, and the performance gate from M1 onward.
- **Framework compatibility is a long tail.** Production bundles touch obscure APIs
  (`Node.compareDocumentPosition`, `Range`, `Selection`, `document.implementation`). Each
  is added when a fixture fails, with the fixture as the regression test.
- **Determinism of text.** Shaping and rasterisation must produce identical output on
  every platform; the current engine already achieves this, and the new one uses the
  same rasteriser and quantised advances.
- **Hand-written versus crates.** Revisited at the end of M1 with the layout parity
  numbers in hand.
