# Rendering and observations

The rendering pipeline is semantic application state → native page → layout →
scene → optional RGBA. Structured-only observations stop before rasterization.
No DOM, JavaScript executor or Chromium process is required for synthetic apps.

`cw-scene::Scene` stores dimensions, background, revision, nodes and an optional
`typeface`. A node has a stable ID, integer bounds, primitive, optional semantic
role/label/value, interaction identifier, clipping, optional `rounded_clip`
(`{rect, radius}`, honoured by hit testing), transform, z-order and opacity.
A client that round-trips scene JSON must carry `typeface` and `rounded_clip`
through, or it will silently lose font selection and window-corner clipping. A
`UiText`/`UiTextBold` node may carry its own `typeface` (omitted when absent), which
overrides the scene's for that node: browser pages whose theme names a `font` set every
text node in the family the list resolves to (`cw_scene::fonts::resolve_family`).

The twelve primitives are `Box`, `RoundedBox`, `UiText`, `UiTextBold`, `Text`,
`AssetImage`, `Shadow`, `Image`, `Path`, `Symbol`, `Backdrop` and `Region`
(invisible interaction area). `Symbol` is a tintable vector glyph; `Backdrop` is a
frosted-glass material. Affine coefficients use 1/1024 units; clipping is expressed
in scene coordinates.

The scene supplies deterministic flow layout. Text measurement is per-primitive:
`UiText`/`UiTextBold` are measured proportionally from the generated advance tables
in `cw-scene::metrics`, with word wrapping and ellipsizing; the `Text` primitive
(terminals, editors) keeps fixed-cell measurement and its golden pixels for plain
rows. `UiText`/`UiTextBold` take two optional fields, omitted from scene JSON when
unset so existing scenes serialize unchanged: `italic` (bool) and `lang` (`"zh-Hans"`,
`"zh-Hant"`, `"ja"`, `"ko"`), which picks regional Han forms. `cw_scene::Style`
(bold, italic, lang) is what the metrics functions take; a plain `bool` still means
bold.
Hit testing uses transformed coordinates, clipping, disabled state and z-order;
later nodes win equal-z ties. It operates without generating pixels.

`ScenePatch` applies upserts, removals and background changes against an expected
revision and produces damaged rectangles. Invalid revision/duplicate-ID updates
are rejected atomically. The rasterizer can use damage to update the existing
frame; compare full and incremental output in tests when adding primitives.
Thirty-five font files are embedded, not one: DejaVu Sans, Sans Bold, Sans Oblique,
Sans Bold Oblique and Sans Mono; regular, bold, italic and bold italic Inter, Open
Sans, Roboto and Ubuntu for the per-platform shells; and regular and bold Noto Sans
Hebrew, Arabic, Thai, Devanagari, Bengali, Georgian and Armenian. The CJK faces (Noto
Sans SC and KR in two weights, and Traditional Chinese, Japanese and Korean locale
forms), Noto Emoji, Noto Color Emoji and eight further scripts form a *font pack* that
native builds embed and the Wasm build fetches on demand. They ship under three
licenses (DejaVu, SIL OFL 1.1 and the Ubuntu Font Licence 1.0); all notices are in
[`crates/render/assets`](../crates/render/assets), which is the authoritative list and
documents the fallback chain, italics, colour emoji, locale forms and the pack.

`UiText` is laid out by `cw_scene::text`: a deterministic fallback chain, Unicode
bidirectional reordering (right-to-left paragraphs and mixed runs), OpenType shaping
with `rustybuzz` (Arabic joining and lam-alef, Indic reordering and conjuncts, Thai
and Hebrew mark attachment, emoji ZWJ/flag/keycap/skin-tone ligatures) and line
breaking between CJK characters with kinsoku. Scene metrics and the renderer share
that one layout, so measured widths, wraps and ellipses are exactly what is drawn.

Emoji draw in colour from Noto Color Emoji's COLRv1 tables, painted by the
renderer's own deterministic COLR rasterizer (gradients, clip boxes, transforms and
composite modes), identically natively and in Wasm; the monochrome glyphs remain the
fallback while the colour file is absent.

This is a deliberately smaller layout and text system than a web browser. It does
not claim complete CSS, vertical or justified text, synthetic italics,
arbitrary DOM execution or browser compositor compatibility. Native pages should be designed for this
contract. A real-browser adapter would be an optional compatibility backend and
must not become the state model for ordinary synthetic services.

Benchmarks must distinguish layout, scene patching, rasterization, readback and
image encoding. See [performance](performance.md) for reproducible commands and
comparison limits; lower scene-update time alone does not establish faster full
pixel production.

Raster details: source-over compositing uses straight RGBA. `Image` uses nearest
neighbour sampling; `AssetImage` larger than 256x256 (wallpapers) uses a centred,
aspect-preserving crop with bilinear filtering. Polygon paths support fills and
strokes, antialiased. `fontdue` is pinned and uses the bundled fonts.
Glyph/text-mask caches are bounded. The checked raster API rejects frames above
16,777,216 pixels. Text wraps and ellipsizes on measured advances for `UiText`, on
fixed cells for `Text`, with fallback for unavailable glyphs. `Text` lays out as a
terminal does (`cw_scene::text::terminal`): grapheme clusters in cells, combining
marks in their base's cell, East Asian wide characters and emoji across two cells,
right-to-left runs reordered within each row (VTE's implicit bidi mode) and script
runs shaped, so Arabic joins cell by cell.

## OS desktop scenes

The optional [native OS presentation](desktop-gui.md) composes application scenes
with deterministic desktop/mobile chrome in Rust. `UiText` is proportional and picks
its face from `Scene::typeface` — Inter, Open Sans, Ubuntu or Roboto per platform,
DejaVu Sans as the fallback — or from its own `typeface`, which may also be one of the
thirteen web families (Arimo, Tinos, Cousine, Gelasio, Carlito, Caladea, Lato, Source
Sans 3, Source Serif 4, Poppins, Montserrat, Playfair Display, JetBrains Mono; see
`crates/render/assets/README.md`, "Web faces"). `RoundedBox` supplies antialiased rounded surfaces with
matching hit tests. Existing `Text` and its golden pixels remain unchanged. Application nodes
can now have translated scene coordinates inside window content clips. The mouse pointer
is part of a desktop frame: a `Path` glyph at the last pointer position, shaped for what is
under it (see [Pointer](desktop-gui.md#pointer)); phones draw none.
