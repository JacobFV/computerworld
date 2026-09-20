# Engine gaps found by the HTML service migration

Every gap below was hit while migrating a service from Page JSON to HTML (milestone 5 of
`docs/web-engine-plan.md`). The engine was being fixed in parallel while the migration ran, so
**every entry was re-verified against the working tree on 2026-09-20**, against a `cw-web` rebuilt
at the end of the sweep. Entries that no longer reproduce are recorded at the bottom rather than
deleted, so nobody re-opens them — in particular **all nine of the originally reported gaps are
now fixed**; everything in "Still open" was found during verification.

## Harness

Each reproduction is a standalone HTML document rendered through the whole pipeline
(`html::parse` → `css::parse_stylesheet` → `style::cascade` → `layout::layout` → `paint::paint`,
rasterised with `cw-render`). The quickest way to render one is `scratch_still` in
`services/discord/tests/still.rs`, which renders `$CW_STILL_HTML` to `$CW_STILL_OUT` at 640×240
(an absolute `CW_STILL_OUT` escapes `research/site-stills/`):

```sh
cargo test -p cw-service-discord --test still --no-run
CW_STILL_HTML=/tmp/repro.html CW_STILL_OUT=/tmp/out.png \
  ./target/debug/deps/still-<hash> scratch_still --ignored
```

Several of these are only decidable by measuring the PNG rather than looking at it — the
gutter-and-thumb question below was called wrongly by eye twice. When a gap is fixed, add a
regression test: a reftest pair under `crates/web/tests/ref/` whose two sides are visually
equivalent, or a paint/layout unit test asserting the box the node produces.

---

# Still open

## 1. A box sized only by `aspect-ratio` contributes no height to the block flow

The box paints at the right height, but its following sibling is laid out as if it were empty, so
the two overlap completely.

```html
<!DOCTYPE html><html><head><style>
body { margin: 0; background: #fff; padding: 10px; }
.ar    { width: 160px; aspect-ratio: 16 / 9; background: #fa3; }
.after { width: 60px; height: 20px; background: #0a0; }
</style></head><body>
<div class="ar"></div>
<div class="after"></div>
</body></html>
```

Measured from the PNG: the orange box occupies y 10–99 (90px, correct) and the green box also
starts at y = 10 — it should start at y = 100. Adding `display: flex`, `overflow: hidden`, any
border or padding, or any child content makes it behave, which is why most migrated sheets get
away with it (`services/media/src/video.css:89` already carries a comment about a related
symptom).

The same defect reads as "`aspect-ratio` does nothing" whenever the sibling is opaque, because the
sibling paints on top of the ratio box:

```html
<!DOCTYPE html><html><head><meta charset="utf-8"><title>g2</title><style>
.a { width: 60px; aspect-ratio: 2 / 1; background: #c36 }
.d { display: flex } .d .e { width: 60px; aspect-ratio: 1 / 1; background: #963; flex: 0 0 60px }
</style></head><body><div class="a"></div><div class="d"><div class="e"></div></div></body></html>
```

`.a` appears to paint nothing (the flex row is laid out at y = 0 and covers it); `.e`, a flex item,
is correct at 60×60.

Likely cause, in `crates/web/src/layout/block.rs`: `ratio_grows` (≈ line 980) deliberately sets
`own_height = None` so content can grow the box past the ratio height, and the final height at
line ≈1034 does take `.max(ratio_h)`; but the self-collapsing test at line ≈1085,
`let empty_box = contents.empty && own_height.is_none_or(|h| h <= Au::ZERO) && …`, still consults
`own_height`, so a childless ratio-sized box is reported to the BFC as an empty box that collapses
through. `empty_box` needs to consider `ratio_h` (or the computed `h`).

**Blocks:** every card grid that sizes a thumbnail by ratio —
`services/social/src/{facebook,mastodon,x,bsky,pinterest}.css`,
`services/media/src/{tiktok,video,ytmusic}.css`. They only work today because the element also
carries `display:flex` or `overflow:hidden`. `services/search/src/search.css:135` had to pin
`.shot .art { height: 128px }` instead of `aspect-ratio: 4 / 3`, so that image grid cannot be
fluid.

## 2. `text-shadow` paints above the glyphs instead of behind them

```html
<!doctype html><html><head><style>
body { margin: 0; background: #fff; font: 48px Arial, sans-serif; }
p { margin: 20px; text-shadow: 16px 16px 0 #d00; }
</style></head><body><p>Ag</p></body></html>
```

The red offset copy is drawn over the black text instead of under it.

**Blocks:** any raised/embossed/legibility shadow. Nothing in the migrated skins depends on it —
everyone who tried it backed out.

## 3. An outer `box-shadow` paints above the element's own background

```html
<!doctype html><html><head><style>
body { margin: 0; background: #fff; }
div { margin: 20px; width: 160px; height: 80px; background: #ddd; box-shadow: 40px 40px 0 #0a0; }
</style></head><body><div></div></body></html>
```

The green shadow covers the bottom-right quadrant of the grey box; it should be entirely behind
it. Blurred and spread-only shadows (`0 0 0 8px`, `0 10px 12px …`) look right, so the symptom
needs a hard offset to show.

**Blocks:** correct card and sticky-header elevation. At the 1–2px offsets the migrated sheets use
it reads as a faint line rather than an obvious error (`services/forum/src/stackoverflow.css`
`.top` and `.sidebar .card`, `services/forum/src/reddit.css` header), but any larger offset is
visibly wrong.

## 4. An `inset box-shadow` with an offset paints a hairline instead of a band

```html
<!doctype html><html><head><style>
body { margin: 0; background: #fff; }
div { margin: 20px; width: 160px; height: 80px; background: #ddd; box-shadow: inset 0 40px 0 0 #07c; }
</style></head><body><div></div></body></html>
```

Expected a 40px `#07c` band across the top of the padding box; got a thin dark line at y = 40.
Spread-only insets (`inset 0 0 0 10px #c07`) are correct — those were fixed during this milestone
— so this is specifically the offset path.

**Blocks:** the inset top highlight on buttons (`services/forum/src/stackoverflow.css`
`.primary { box-shadow: inset 0 1px 0 0 rgba(255,255,255,.4) }` silently does nothing) and any
"coloured top rail" drawn with an inset shadow.

## 5. `border-style: dotted` and `dashed` ignore `border-radius`

```html
<!DOCTYPE html><html><head><meta charset="utf-8"><title>d</title><style>
.row { display: flex; gap: 24px; padding: 20px }
.a { width:34px; height:34px; border-radius:50%; border:5px dotted #ff8000 }
.b { width:34px; height:34px; border-radius:50%; border:5px solid  #ff8000 }
.c { width:34px; height:34px; border-radius:50%; border:5px dashed #ff8000 }
</style></head><body><div class="row"><span class="a"></span><span class="b"></span><span class="c"></span></div></body></html>
```

`solid` paints a ring; `dotted` and `dashed` lay their dots and dashes along the square border box
and paint a square.

**Blocks:** `services/press/src/reuters.css` `.logo` — the Reuters roundel reads as a dotted
square instead of a ring of dots.

## 6. `::placeholder { color }` is parsed but never applied

```html
<!DOCTYPE html><html><head><meta charset="utf-8"><title>g1</title><style>
input { width: 300px; border: 0 solid transparent; font: 16px Arial }
input::placeholder { color: #2e8b57 }
</style></head><body><input placeholder="wwwwwwwwwwwwwww"></body></html>
```

The darkest painted pixel is `(117, 117, 117)`, never `#2e8b57`. The UA sheet declares
`input::placeholder, textarea::placeholder { color: #757575 }`
(`crates/web/src/style/ua/forms.css:20`), but the text is painted with the hard-coded
`PLACEHOLDER_TEXT` constant (`crates/web/src/paint/replaced.rs:29`, used at lines 77, 191 and 208),
so no author rule can ever win.

**Blocks:** `services/assistant/src/base.css:67`
(`.composer input::placeholder { color: var(--muted) }`) is a dead rule — ChatGPT's dark composer
gets `#757575` where `--muted` `#9b9b9b` was wanted. The same applies to Google's search box and
the Messages composer.

## 7. Properties the strict parser rejects

Not bugs — a catalogue of what a skin cannot ask for, so the engine owners can prioritise. Each
reproduces as `Unsupported { kind: Property, name: "…", detail: "unknown property" }` from
`validate_strict`:

```html
<!doctype html><html><head><style>.p { mask-image: linear-gradient(#000, transparent) }</style></head><body><div class="p">x</div></body></html>
```

`mask-image`/`mask`, `filter`, `backdrop-filter`, `clip-path`, `mix-blend-mode`, `isolation`,
`columns`/`column-count`, `writing-mode`, `unicode-bidi`, `resize`, `hyphens`, `will-change`,
`content-visibility`, `scroll-behavior`, `overscroll-behavior`, `border-image`, `text-wrap`,
`font-feature-settings`, and the individual `translate`/`rotate`/`scale` transform properties.
`::first-line` is refused too, with a clear message.

Only three cost the migration anything: `mask-image` (Reddit and Quora fade a truncated post body
with a gradient mask — `-webkit-line-clamp` was used instead, and works, ellipsis included),
`filter`, and `resize: vertical` on textareas.

## 8. `cw_web::page::to_document` can emit a nested `<a>`, which duplicates the outer id

Not an engine bug — the HTML parser is doing exactly what the spec's adoption agency algorithm
says — but a trap the Page→HTML converter walks into silently.

```html
<!DOCTYPE html><html><head><meta charset="utf-8"><title>n</title></head><body>
<a id="card" href="/a">
<div id="row">head</div>
<p id="body">Text</p>
<div id="line"><a id="more" href="/a">More</a></div>
</a>
</body></html>
```

`doc.by_id("card")` returns **2** nodes and `validate_strict` fails with
`id "card" appears more than once`, so the agent API can no longer address the card. A direct
inner `<a>` with no intervening block element does not trigger it. This is how
`worlds/company-2026/sites/northstar-status.json` broke: an action-bearing `card` containing a
`link`. Either `to_document` should drop the card's anchor wrapper when the card contains a link
descendant, or the seed loader should reject it.

---

# Fixed since the gap list was first written

All nine originally reported gaps were re-checked on 2026-09-20 with their own reproductions and
now render correctly. They are listed so the fixes get regression tests rather than being
forgotten.

1. **Non-uniform `border-radius` painted nothing.** `border-radius: 0 8px 8px 0` now paints with
   square left corners and round right ones; a 200×120 box with `0 60px 60px 0` tracks the ideal
   circle to within 1px on every scanline, with no interior seam.
2. **A border with one transparent side, or unequal side widths, plus any radius, painted
   nothing.** `border:3px solid #fff;border-top-color:transparent;border-radius:0 0 12px 12px` and
   `border:3px solid #fff;border-bottom-width:10px;border-radius:4px` both paint.
3. **The CSS triangle painted nothing.** `width:0;height:0;border-top:12px solid transparent;
   border-bottom:12px solid transparent;border-right:18px solid #fff` paints a left-pointing
   triangle. `services/wiki/src/archive.css:25` still carries a comment saying this is
   unavailable; that workaround can be revisited.
4. **`position:absolute` on a `display:inline-flex` element** painted its background twice and lost
   its text. It now paints one circle at the requested offset with the text inside — the
   avatar/presence-dot pattern discord, slack and social all use.
5. **`flex-direction: column-reverse` in an `overflow-y: auto` container could not be scrolled
   back.** `crates/web/src/layout/scroll.rs::attach_scroll_info` now tracks a non-positive
   `origin_x`/`origin_y` over the children's overflow and shifts the caller's offset by it, so the
   start-overflow is reachable. `justify-content: flex-end` on an overflowing column behaves the
   same way, so bottom-anchored transcripts work either way (`margin-top: auto` on the first child
   remains the pattern the sheets use).
6. **An emoji run measured narrower than it painted.** Both halves are fixed: a row of
   `<button style="display:inline-flex">👍 2</button>` pills lays out and paints cleanly, and
   `<p><span>👍A</span><span>BBBB</span></p>` — which mid-sweep still lost the `A` under the emoji
   glyph and overlapped the two spans' backgrounds — now renders `👍ABBBB` correctly.
7. **`box-shadow: inset` on a rounded box** painted an offset square. A spread-only inset
   (`inset 0 0 0 12px`) now paints a ring indistinguishable from the equivalent `border`, and the
   tab-underline form (`inset 0 -3px 0 var(--accent)`) is correct. Offset insets are still wrong —
   see open gap 4.
8. **`transform: scale()` appeared to have no effect.** `scale(0.5)` and `rotate(45deg)` are both
   correct, including `border-radius: 50% 50% 50% 0` + `rotate(-45deg)` (the map-pin teardrop
   `services/geo/src/{osm,gmaps}.css` rely on).
9. **Inner scroll containers reserved a gutter and painted no scrollbar.** They now paint both
   parts. Measured on
   `<div style="width:160px;height:80px;overflow-y:auto;background:#ff0"><div style="height:300px;background:#f0f"></div></div>`:
   content stops at x = 144, the 15px gutter carries `SCROLLBAR_TRACK` (#f1f1f1) and a
   `SCROLLBAR_THUMB` (#c1c1c1) rounded thumb 21px long at the top, moving as the offset changes
   (`crates/web/src/paint/display_list.rs:384-387`). With `LayoutCache::overlay_scrollbars` the
   container reserves nothing, as Chromium's overlay bars do. This one reads as "a blank strip" at
   thumbnail size — measure the pixels before re-opening it.

# Reported but not reproducible

- **Hairline seam where a rounded corner meets a straight edge.** Reported by the geo/wiki/bank
  sweep. Re-rendered `border-radius: 0 60px 60px 0` and `border-radius: 60px` on 200×120 white
  boxes over black and scanned every scanline: no interior non-white pixel in either box, and the
  corner arcs track the ideal circle to within 1px. Nothing to fix unless it can be reproduced at
  some other size.
