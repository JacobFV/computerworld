# Acid2

The Second Acid Test, as a Milestone 2 gate for the web engine (`docs/web-engine-plan.md`,
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
`px-reference.html`.

## What the engine cannot do without script or network semantics

The layout parity fixture and the reftest pair are dumped and run at scroll offset 0.
The test is written to be viewed at `#top`: the intro box sits above and the face is
pushed 100em down (`#top { margin: 100em 3em 0 }`, `html { overflow: hidden }`), so the
Chromium screenshot shows only the intro, while the dump still carries every box's
position. The integration step is expected to navigate to `acid2.html#top` (scroll the
viewport to the `#top` anchor) before comparing pixels against `px-reference.html`; the
reftest pair fails until then.

Parts of the test that depend on things outside the cascade, layout and paint:

1. **`data:` URIs as images.** The forehead, the eyes (`#eyes-a object object object`
   and `#eyes-b`), the chin and the `.image-height-test` cell load 1×1, 2×2 and 64×64
   PNGs from `data:image/png;base64,...` as backgrounds (two of them `background-attachment:
   fixed`) and as an `<img>`. The engine has no image decoding or `data:` URL handling; until
   it does, those backgrounds stay their fallback colour (the eyes show red instead of
   yellow squares).
2. **The `<object>` fallback chain.** `#eyes-a` nests three `<object>` elements: the outer
   one loads `data:application/x-unknown,ERROR` (unknown type, must fall back), the middle
   one loads `404.html?pipe=status(404)` with `type="text/html"` (an HTTP 404, must fall
   back), and only the innermost, a `data:image/png` of the two eyes, must render. Deciding
   which `<object>` renders needs the fetch to fail with a status code; under `file://` in
   Chromium the `?pipe=status(404)` query is ignored and `404.html` loads with status 200,
   so Chromium's own dump of this fixture shows the red 404 document in the middle `<object>`
   rather than the eyes. The integration must serve `404.html` with status 404 (the WPT
   server's `pipe=status(404)` does that) for the fixture to be right in either engine.
3. **`<link rel="appendix stylesheet" href="data:text/css,...">`.** The preferred
   stylesheet that removes `.picture`'s red background is a `data:` URL alternate sheet.
   It needs `data:` URL fetching for `<link>`, which the runner does not do; the engine
   sees the red `.picture` background until it does.
4. **`:hover`.** The nose changes colour on hover. No pointer, so the resting state is
   what is compared, which is what the reference shows.
5. **`position: fixed` and `background-attachment: fixed`** are relative to the viewport,
   so they only line up with the reference once the page is scrolled to `#top`.

Everything else (HTML 4.01 parsing quirks including the `<p><table>` close and the
comment `<!-- ->ERROR<!- -->`, attribute selectors with escaped spaces, the CSS parser
error-recovery tests, floats and clearance with negative margins, absolute and fixed
positioning, min/max sizing, auto margins, `display: table*` on list items, inline
`object` sizing, paint order) is within the engine's remit and counts toward the
threshold in `thresholds.json`.
