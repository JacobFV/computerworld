# Engine gaps found by the HTML service migration

Every gap below was hit while migrating a service from Page JSON to HTML (milestone 5 of
`docs/contracts/web-engine-plan.md`). The engine was being fixed in parallel while the migration ran, so
**every entry was re-verified against the working tree on 2026-09-20**, against a `cw-web` rebuilt
at the end of the sweep. Entries that no longer reproduce are recorded at the bottom rather than
deleted, so nobody re-opens them — in particular **all nine of the originally reported gaps are
now fixed**, and so is everything the verification sweep found bar the property catalogue. Each
fix carries a regression test in `crates/web/engine/src/paint/pipeline_tests.rs`.

## Harness

Each reproduction is a standalone HTML document rendered through the whole pipeline
(`html::parse` → `css::parse_stylesheet` → `style::cascade` → `layout::layout` → `paint::paint`,
rasterised with `cw-render`). The quickest way to render one is `scratch_still` in
`services/discord/tests/still.rs`, which renders `$CW_STILL_HTML` to `$CW_STILL_OUT` at 640×240
(an absolute `CW_STILL_OUT` escapes `research/studies/site-stills/`):

```sh
cargo test -p cw-service-discord --test still --no-run
CW_STILL_HTML=/tmp/repro.html CW_STILL_OUT=/tmp/out.png \
  ./target/debug/deps/still-<hash> scratch_still --ignored
```

Several of these are only decidable by measuring the PNG rather than looking at it — the
gutter-and-thumb question below was called wrongly by eye twice. When a gap is fixed, add a
regression test: a reftest pair under `crates/web/engine/tests/ref/` whose two sides are visually
equivalent, or a paint/layout unit test asserting the box the node produces.

---

# Still open

## 1. Properties the strict parser rejects

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
`filter`, and `resize: vertical` on textareas. `scrollbar-width` was on this list and is now
supported (see below). These three are in `crates/web/engine/src/style/properties/`, which the style
owner holds.

## 2. `cw_web::page::to_document` can emit a nested `<a>`, which duplicates the outer id

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
descendant, or the seed loader should reject it. Routed separately; not the engine's.

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
   back.** `crates/web/engine/src/layout/scroll.rs::attach_scroll_info` now tracks a non-positive
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
   tab-underline form (`inset 0 -3px 0 var(--accent)`) is correct. Offset insets were still wrong
   at the time of that sweep and are fixed in the second round below (12).
8. **`transform: scale()` appeared to have no effect.** `scale(0.5)` and `rotate(45deg)` are both
   correct, including `border-radius: 50% 50% 50% 0` + `rotate(-45deg)` (the map-pin teardrop
   `services/geo/src/{osm,gmaps}.css` rely on).
9. **Inner scroll containers reserved a gutter and painted no scrollbar.** They now paint both
   parts. Measured on
   `<div style="width:160px;height:80px;overflow-y:auto;background:#ff0"><div style="height:300px;background:#f0f"></div></div>`:
   content stops at x = 144, the 15px gutter carries `SCROLLBAR_TRACK` (#f1f1f1) and a
   `SCROLLBAR_THUMB` (#c1c1c1) rounded thumb 21px long at the top, moving as the offset changes
   (`crates/web/engine/src/paint/display_list.rs:384-387`). With `LayoutCache::overlay_scrollbars` the
   container reserves nothing, as Chromium's overlay bars do. This one reads as "a blank strip" at
   thumbnail size — measure the pixels before re-opening it.

# Behaviour change worth knowing about

**The Page projector now breaks an id-bearing inline element out of the text run around it.**
`Projector::walk`/`flush_as` in `crates/web/browser/src/web_document.rs` splits
`<span id=a><span id=b>147</span> points by <a id=c>tomw</a> 31t ago</span>` into four projected
elements: `b` = "147", an anonymous `text:N` = "points by", the link `c`, and `a` carrying only the
trailing residue. Each id became individually addressable, which is the point of the change and a
win for agents; the cost is that connective words end up in anonymous fragments, so **no single
element's text spans them**.

Any browser-level test that asserts a multi-word phrase straddling an id'd child will fail. It
took out `forum_sites::hacker_news_is_read_voted_on_and_commented_on`
(`assert!(says(&home, "147 points by"))`), which was rewritten to address `t-9001-score` and
`t-9001-author` by id instead — the stronger assertion, and how docs/html-migration.md says agents
should read a page. Expect the same in other suites; fix them the same way rather than folding the
connective words back into the id'd span.

# Reported but not reproducible

- **Hairline seam where a rounded corner meets a straight edge.** Reported by the geo/wiki/bank
  sweep. Re-rendered `border-radius: 0 60px 60px 0` and `border-radius: 60px` on 200×120 white
  boxes over black and scanned every scanline: no interior non-white pixel in either box, and the
  corner arcs track the ideal circle to within 1px. Nothing to fix unless it can be reproduced at
  some other size.

# Fixed in the second round (the verification sweep's findings)

10. **A box sized only by `aspect-ratio` contributed no height to the flow.** Two places read it
    as empty: `compute_empty_block` in `layout/block.rs` classed a childless `height: auto` box as
    an empty block before layout ran, and the self-collapsing test at the end of `layout_block_box`
    consulted `own_height`, which `ratio_grows` deliberately leaves empty. Both now account for the
    ratio, so `.ar { width: 160px; aspect-ratio: 16/9 }` is 90px tall and the next block starts at
    y = 90 (`a_box_sized_only_by_aspect_ratio_takes_its_height_in_the_flow`). The sheets that
    worked around it with `display: flex`, `overflow: hidden` or a pinned height can drop that.
11. **`::placeholder { color }` never applied.** The pseudo-element's rules were dropped by the
    cascade (`style/cascade.rs` skipped `PseudoElement::Placeholder`) and the hint was painted in a
    hard-coded grey. `StyleSet` now carries a `placeholder` style per text control, cascaded like
    `::marker`, and `paint/replaced.rs` paints the hint in its colour; the UA sheet's `#757575`
    is what an unstyled control still gets
    (`a_placeholder_takes_the_colour_its_pseudo_element_was_given`).
12. **An `inset box-shadow` with an offset painted a hairline.** The four strips were a fixed
    `blur + spread` thick wherever the offset put them. They are now the bands between the padding
    box and the inner rect (the box moved by the offset and pulled in by the spread), so
    `inset 0 40px 0` is a 40px band and `inset 0 1px 0 rgba(255,255,255,.4)` is the 1px top
    highlight it was meant to be (`an_inset_shadow_with_an_offset_is_a_band_not_a_hairline`).
13. **`dotted` and `dashed` borders ignored `border-radius`.** They are now laid along the same
    rounded centre line the solid sides use, walked by arc length: dots are round boxes every other
    `w`, dashes are `3w` strokes every `4w` (`a_dotted_border_follows_the_corner_radius`). The
    Reuters roundel is a ring of dots again.
14. **`overflow-wrap: break-word` disabled `text-overflow: ellipsis`.** CSS Text §5 only offers
    mid-word break opportunities where the line may wrap, so `white-space: nowrap` makes
    `overflow-wrap` and `word-break` inert; the engine took them anyway and wrapped a clamped
    one-line title onto a second line the box then clipped mid-glyph
    (`overflow_wrap_does_not_disable_the_ellipsis`).
15. **No way to hide a scroll container's scrollbar.** `scrollbar-width: auto | thin | none` is now
    a supported property: `none` reserves no gutter and paints no bar, `thin` takes half of one.
    A horizontal chip rail asks for `scrollbar-width: none`
    (`scrollbar_width_none_takes_no_gutter_and_paints_no_bar`). `::-webkit-scrollbar` is still
    unsupported and is not planned — `scrollbar-width` is the standard property.
16. **Reserving one bar could ask for the other.** `auto_bars` was handed the already-narrowed
    content width as the visible width and subtracted the gutter from it a second time, so any
    vertically scrolling box also claimed a horizontal bar and lost 15px of height. The scrollport
    width is passed now and the decision is re-made until it settles. This is what made
    `clientHeight` 85 where Chromium says 100.

# Checked in the second round and not reproducible

- **`text-shadow` painting above the glyphs** and **an outer `box-shadow` painting above its own
  background.** Both are emitted before what they sit behind and the scene paints in emission
  order. Measured: with `text-shadow: 0 0 0 #d00` not one red pixel survives the glyphs, and a
  `box-shadow: 20px 20px 0` is covered wherever the background reaches it and shows only past its
  corner (`shadows_paint_behind_what_they_belong_to` pins both).
- **`box-shadow` with a large blur rendering faintly.** `box-shadow: 0 26px 50px rgba(0,0,0,.55)`
  over a `#888` background darkens it to 69/255 — within a few counts of the 61 the colour's own
  alpha allows, so the blur keeps its energy. A black shadow over a genuinely dark background is
  invisible because it is black on black, which is what Chromium does too; a ground shadow under a
  dark hero needs a lighter colour or a larger spread, not an engine change.
