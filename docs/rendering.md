# Rendering and observations

The rendering pipeline is semantic application state → native page → layout →
scene → optional RGBA. Structured-only observations stop before rasterization.
No DOM, JavaScript executor or Chromium process is required for synthetic apps.

`cw-scene::Scene` stores dimensions, background, revision, nodes and an optional
`typeface`. A node has a stable ID, integer bounds, primitive, optional semantic
role/label/value, interaction identifier, clipping, optional `rounded_clip`
(`{rect, radius}`, honoured by hit testing), transform, z-order and opacity.
A client that round-trips scene JSON must carry `typeface` and `rounded_clip`
through, or it will silently lose font selection and window-corner clipping.

The twelve primitives are `Box`, `RoundedBox`, `UiText`, `UiTextBold`, `Text`,
`AssetImage`, `Shadow`, `Image`, `Path`, `Symbol`, `Backdrop` and `Region`
(invisible interaction area). `Symbol` is a tintable vector glyph; `Backdrop` is a
frosted-glass material. Affine coefficients use 1/1024 units; clipping is expressed
in scene coordinates.

The scene supplies deterministic flow layout. Text measurement is per-primitive:
`UiText`/`UiTextBold` are measured proportionally from the generated advance tables
in `cw-scene::metrics`, with word wrapping and ellipsizing; the legacy `Text`
primitive keeps fixed-cell measurement and its golden pixels.
Hit testing uses transformed coordinates, clipping, disabled state and z-order;
later nodes win equal-z ties. It operates without generating pixels.

`ScenePatch` applies upserts, removals and background changes against an expected
revision and produces damaged rectangles. Invalid revision/duplicate-ID updates
are rejected atomically. The rasterizer can use damage to update the existing
frame; compare full and incremental output in tests when adding primitives.
Eleven font files are embedded, not one: DejaVu Sans, DejaVu Sans Bold and
DejaVu Sans Mono, plus regular and bold Inter, Open Sans, Roboto and Ubuntu for the
per-platform shells. They ship under three licenses (DejaVu, SIL OFL 1.1 and the
Ubuntu Font Licence 1.0); all notices are in
[`crates/render/assets`](../crates/render/assets), which is the authoritative list.

This is a deliberately smaller layout and text system than a web browser. It does
not claim complete CSS, advanced script shaping, arbitrary DOM execution or
browser compositor compatibility. Native pages should be designed for this
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
fixed cells for `Text`, with fallback for unavailable glyphs; bidirectional layout
and a shaping engine are outside the current contract.

## OS desktop scenes

The optional [native OS presentation](desktop-gui.md) composes application scenes
with deterministic desktop/mobile chrome in Rust. `UiText` is proportional and picks
its face from `Scene::typeface` — Inter, Open Sans, Ubuntu or Roboto per platform,
DejaVu Sans as the fallback. `RoundedBox` supplies antialiased rounded surfaces with
matching hit tests. Existing `Text` and its golden pixels remain unchanged. Application nodes
can now have translated scene coordinates inside window content clips.
