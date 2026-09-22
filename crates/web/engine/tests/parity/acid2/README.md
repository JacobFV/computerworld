# Acid2

The Second Acid Test, as a Milestone 2 gate for the web engine (`docs/contracts/web-engine-plan.md`,
Verification).

## Provenance

`www.webstandards.org/files/acid2/test.html` is gone and `acid2.acidtests.org` now serves
a parked hosting page, so these files are the copy that web-platform-tests keeps under
`acid/acid2/` for browser vendors' convenience, taken at commit
`269bca0dd35c303639f3c9cf1d8bcb3d911bdb60` (the same pin as `tests/wpt/`):

| File | What |
|---|---|
| `../acid2.html` | `acid/acid2/test.html`, verbatim except that its two relative URLs now point into this directory (`acid2/reference.html`, `acid2/404.html?pipe=status(404)`) |
| `reference.html` | The reference rendering page: "Hello World!" over `reference.png` |
| `reference.png` | The reference image of the face |
| `px-reference.html` | WPT's pixel-for-pixel CSS-only reference (no image), what `acid/acid2/reftest.html` matches against |
| `404.html` | The document the WPT server serves with status 404 for the `<object>` fallback chain (its body is red on purpose: it must never render) |
| `LICENSE.md` | The WPT 3-Clause BSD licence that covers these files |

The reftest pair `tests/ref/acid2.html` / `acid2-ref.html` is the same test against
`px-reference.html`. The engine passes it pixel for pixel, with no tolerance.

## How the test is viewed

The test is written to be viewed at `#top`: the intro box sits above and the face is
pushed 100em down (`#top { margin: 100em 3em 0 }`, `html { overflow: hidden }`), and the
scalp, the eyes' backgrounds and the chin's are fixed to the viewport. So:

- The **reftest pair** is compared after a fragment navigation to `#top`
  (`NAVIGATE` in `tests/reftest.rs`, `support::run_at`): both documents are scrolled so
  `#top` is at the top of the viewport, clamped to the scrollable range, as WPT's
  `acid/acid2/reftest.html` does. `overflow: hidden` on the viewport stops the user
  scrolling, not the navigation (`layout/scroll.rs`).
- The **layout parity fixture** is dumped at scroll offset 0 on both sides (the dump
  carries every box's document position either way), so Chromium's screenshot of it
  shows only the intro.

## What the test needs beyond the cascade, layout and paint, and where it lives

1. **`data:` URIs as images.** The forehead, the eyes (`#eyes-a object object object`
   and `#eyes-b`), the chin and the `.image-height-test` cell load 1×1, 2×2 and 64×64
   PNGs from `data:image/png;base64,...` as backgrounds (two of them
   `background-attachment: fixed`), as an `<img>` and as an `<object>`.
   `paint/data_url.rs` parses the URL (percent-encoding, base64), `paint/png.rs` decodes
   the PNG (its own inflate; every colour type, `tRNS`, Adam7) and
   `paint::ImageMap::from_document` collects every `data:` image the document and its
   computed styles name; the same map gives layout the intrinsic sizes and paint the
   pixels. The harness builds it for every page it renders.
2. **The `<object>` fallback chain.** `#eyes-a` nests three `<object>` elements: the outer
   one loads `data:application/x-unknown,ERROR` (unknown type, must fall back), the middle
   one loads `404.html?pipe=status(404)` with `type="text/html"` (an HTTP 404, must fall
   back), and only the innermost, a `data:image/png` of the two eyes, must render. In
   `layout/boxes.rs` an `<object>` whose data is a decoded image is replaced content, and
   any other `<object>` renders its children as an ordinary element of its `display`
   (the engine nests no documents and has no plugins, so everything that is not an image
   falls back). Under `file://` Chromium ignores the `?pipe=status(404)` query and would
   load `404.html` with status 200, so `scripts/web-parity/dump.mjs` answers
   `pipe=status(N)` requests the way the WPT server does; the checked-in dump shows the
   fallback, as a browser on the web does.
3. **`<link rel="appendix stylesheet" href="data:text/css,...">`.** The persistent
   stylesheet that removes `.picture`'s red background is a `data:` URL sheet; the harness
   reads `data:` stylesheets (`support::linked_data_stylesheet`). A `rel` that names
   `alternate` is an alternate sheet and is not applied.
4. **`:hover`.** The nose changes colour on hover. No pointer, so the resting state is
   what is compared, which is what the reference shows.
5. **Painting order.** The eyes are an absolutely positioned box inside the
   `z-index: auto` `.picture`, and must cover a `position: fixed` paragraph that comes
   before them in the document. `paint/display_list.rs` hands the positioned descendants
   of a box that is painted atomically without being a stacking context to the enclosing
   context, puts viewport-contained boxes in the root element's context, and orders each
   layer by document position.
6. **Margins through an empty block, negative clearance.** `.empty div`'s `-6em` bottom
   margin collapses through `.empty` into the set that decides where the smile would sit
   without clearance (`layout/block.rs`, `empty_block_margins`); the clearance is then
   negative.

Every node of the layout parity fixture is within tolerance of Chromium's dump.
