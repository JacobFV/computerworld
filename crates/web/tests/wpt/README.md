# Web Platform Tests reftest corpus

A sparse copy of [web-platform-tests](https://github.com/web-platform-tests/wpt) at commit `269bca0dd35c303639f3c9cf1d8bcb3d911bdb60`, produced by `crates/web/tools/fetch-wpt.py` (pinned tarball SHA-256 `0cc977e9e5e0c249bb11deb9ea86b7d01e7efa8a8d3118039e5b7c2421bf43fa`). The tests are copyright the W3C and the WPT contributors and are used under the 3-Clause BSD License in `LICENSE.md`.

The runner is `crates/web/tests/wpt.rs`: for every pair in `manifest.json` it parses the test and its reference, resolves `<link rel=stylesheet>` and `@import` from this tree (`/css/support/...` and relative paths), maps `font-family: Ahem` to the bundled JetBrains Mono on both sides, runs the pipeline at 800x600 (the WPT default), rasterises both and compares the pixels; a `rel=match` pair passes when the rasters are identical and a `rel=mismatch` pair when they differ. The report lands in `crates/web/target-parity/wpt-report.md`, grouped by directory; the test fails only when a directory's pass count drops below `expectations.json`, which starts at 0 and which the integration step raises as the engine improves.

## Selection

Reftests only (`<link rel=match>` or `rel=mismatch` whose target exists). A test is excluded, with its reference, when either document has a `<script>`, is or contains SVG, uses vertical writing modes, needs `reftest-wait`, targets print media, nests another document (`iframe`, `object`, `embed`, `video`, `canvas`), or declares `@font-face` (other than `fonts/ahem.css`). Whole leaf directories are taken in the plan's order while the total stays under 1600 pairs and each area under its own budget (the `AREAS` table in `fetch-wpt.py`, so that one large directory cannot crowd out the areas after it); directories named `tentative`, `animation(s)`, `invalidation`, `hidpi`, `multicol`, `overlay`, `urls`, `grid-lanes`, `scroll-markers`, `line-clamp`, `run-in` and the font-dependent text directories are out of scope. `css/CSS2` contributes its block, inline, float and positioning directories only (`linebox`, `floats`, `floats-clear`, `box`, `box-display`, `abspos`); `css/css-writing-modes` is skipped on purpose.

**1586 pairs in 22 directories, 2481 files.**

## Included directories

| Directory | Pairs | Candidates | script | svg | writing-mode | reftest-wait | print | nested-document | font-face | missing-reference | missing-stylesheet |
|---|---|---|---|---|---|---|---|---|---|---|---|
| `css/CSS2/linebox` | 188 | 191 | 0 | 0 | 0 | 0 | 0 | 3 | 0 | 0 | 0 |
| `css/CSS2/floats` | 103 | 111 | 8 | 0 | 0 | 4 | 0 | 0 | 0 | 0 | 0 |
| `css/CSS2/box` | 9 | 9 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `css/CSS2/abspos` | 25 | 29 | 4 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 |
| `css/css-flexbox` | 706 | 900 | 55 | 15 | 106 | 5 | 2 | 28 | 0 | 0 | 2 |
| `css/css-flexbox/alignment` | 6 | 8 | 0 | 0 | 2 | 0 | 0 | 0 | 0 | 0 | 0 |
| `css/css-flexbox/flex-lines` | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `css/css-flexbox/order` | 4 | 4 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| `css/css-grid` | 34 | 58 | 14 | 1 | 8 | 1 | 0 | 1 | 0 | 2 | 0 |
| `css/css-grid/alignment/self-baseline` | 6 | 42 | 0 | 0 | 35 | 0 | 0 | 1 | 0 | 0 | 0 |
| `css/css-grid/grid-definition` | 18 | 26 | 5 | 0 | 4 | 0 | 0 | 0 | 0 | 0 | 0 |
| `css/css-grid/grid-model` | 29 | 49 | 2 | 0 | 18 | 0 | 0 | 1 | 0 | 0 | 0 |
| `css/css-grid/implicit-grids` | 2 | 3 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 |
| `css/css-grid/layout-algorithm` | 20 | 30 | 2 | 0 | 2 | 1 | 0 | 7 | 0 | 0 | 0 |
| `css/css-tables` | 87 | 123 | 25 | 0 | 6 | 6 | 2 | 4 | 0 | 0 | 0 |
| `css/css-tables/height-distribution` | 11 | 12 | 0 | 0 | 1 | 0 | 0 | 0 | 0 | 0 | 0 |
| `css/css-tables/paint` | 1 | 6 | 4 | 0 | 1 | 3 | 0 | 0 | 0 | 0 | 0 |
| `css/css-position` | 59 | 115 | 45 | 3 | 6 | 17 | 0 | 4 | 0 | 0 | 0 |
| `css/selectors` | 89 | 136 | 44 | 3 | 0 | 31 | 0 | 1 | 0 | 0 | 0 |
| `css/css-cascade` | 38 | 50 | 9 | 1 | 0 | 0 | 1 | 0 | 0 | 0 | 3 |
| `css/css-values` | 105 | 150 | 12 | 5 | 18 | 1 | 1 | 4 | 21 | 0 | 1 |
| `css/css-display` | 42 | 80 | 36 | 1 | 0 | 7 | 0 | 3 | 0 | 0 | 0 |

## Skipped for budget (next in line)

These directories passed the same filter but did not fit under the cap; raising `CAP` in `fetch-wpt.py` takes them in this order.

| Directory | Pairs | Candidates |
|---|---|---|
| `css/CSS2/floats-clear` | 203 | 214 |
| `css/CSS2/box-display` | 85 | 120 |
| `css/css-flexbox/abspos` | 18 | 32 |
| `css/css-flexbox/balance` | 31 | 39 |
| `css/css-flexbox/intrinsic-size` | 22 | 24 |
| `css/css-grid/abspos` | 99 | 150 |
| `css/css-grid/alignment` | 93 | 125 |
| `css/css-grid/grid-items` | 111 | 169 |
| `css/css-grid/placement` | 15 | 16 |
| `css/css-grid/subgrid` | 58 | 99 |
| `css/css-position/static-position` | 18 | 36 |
| `css/css-position/sticky` | 20 | 76 |
| `css/selectors/selectors-4` | 27 | 29 |
| `css/css-variables` | 176 | 182 |
| `css/css-backgrounds` | 295 | 358 |
| `css/css-backgrounds/background-clip` | 46 | 50 |
| `css/css-backgrounds/background-origin` | 12 | 12 |
| `css/css-backgrounds/background-position` | 2 | 3 |
| `css/css-backgrounds/background-repeat` | 7 | 7 |
| `css/css-backgrounds/background-size` | 11 | 11 |
| `css/css-backgrounds/box-shadow` | 7 | 7 |
| `css/css-text` | 1 | 2 |
| `css/css-text/bidi` | 4 | 4 |
| `css/css-text/boundary-shaping` | 2 | 10 |
| `css/css-text/hanging-punctuation` | 20 | 23 |
| `css/css-text/hyphens` | 39 | 47 |
| `css/css-text/letter-spacing` | 28 | 34 |
| `css/css-text/line-break` | 50 | 84 |
| `css/css-text/line-breaking` | 126 | 126 |
| `css/css-text/overflow-wrap` | 47 | 49 |
| `css/css-text/tab-size` | 14 | 16 |
| `css/css-text/text-align` | 86 | 89 |
| `css/css-text/text-indent` | 21 | 25 |
| `css/css-text/text-justify` | 13 | 15 |
| `css/css-text/text-transform` | 36 | 106 |
| `css/css-text/white-space` | 412 | 422 |
| `css/css-text/word-break` | 55 | 88 |
| `css/css-text/word-break/auto-phrase` | 13 | 16 |
| `css/css-text/word-space-transform` | 29 | 29 |
| `css/css-text/word-spacing` | 3 | 9 |
| `css/css-text/writing-system` | 5 | 5 |
| `css/css-box/margin-trim` | 58 | 69 |
| `css/css-overflow` | 146 | 248 |

## Exclusion totals

| Reason | Tests (included directories) | Tests (skipped directories) |
|---|---|---|
| script | 266 | 386 |
| svg | 29 | 33 |
| writing-mode | 207 | 196 |
| reftest-wait | 78 | 150 |
| print | 6 | 0 |
| nested-document | 57 | 29 |
| font-face | 21 | 140 |
| missing-reference | 2 | 2 |
| missing-stylesheet | 6 | 1 |
