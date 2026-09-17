# Rendering and observations

The rendering pipeline is semantic application state → native page → layout →
scene → optional RGBA. Structured-only observations stop before rasterization.
No DOM, JavaScript executor or Chromium process is required for synthetic apps.

`cw-scene::Scene` stores dimensions, background, revision and nodes. A node has a
stable ID, integer bounds, primitive, optional semantic role/label/value,
interaction identifier, clipping, transform, z-order and opacity. Primitives are
boxes, text, RGBA images, paths and invisible interaction regions. Affine
coefficients use 1/1024 units; clipping is expressed in scene coordinates.

The scene supplies deterministic flow layout and fixed-cell text measurement.
Hit testing uses transformed coordinates, clipping, disabled state and z-order;
later nodes win equal-z ties. It operates without generating pixels.

`ScenePatch` applies upserts, removals and background changes against an expected
revision and produces damaged rectangles. Invalid revision/duplicate-ID updates
are rejected atomically. The rasterizer can use damage to update the existing
frame; compare full and incremental output in tests when adding primitives.
The embedded font is DejaVu Sans Mono; its license ships in
[`crates/render/assets`](../crates/render/assets).

This is a deliberately smaller layout and text system than a web browser. It does
not claim complete CSS, advanced script shaping, arbitrary DOM execution or
browser compositor compatibility. Native pages should be designed for this
contract. A real-browser adapter would be an optional compatibility backend and
must not become the state model for ordinary synthetic services.

Benchmarks must distinguish layout, scene patching, rasterization, readback and
image encoding. See [performance](performance.md) for reproducible commands and
comparison limits; lower scene-update time alone does not establish faster full
pixel production.

Raster details: source-over compositing uses straight RGBA; images use nearest
neighbor sampling; polygon paths support fills/strokes. `fontdue` is pinned and
uses the bundled font. Glyph/text-mask caches are bounded. The checked raster API
rejects frames above 16,777,216 pixels. Text has fixed-cell wrapping and fallback
for unavailable glyphs; bidirectional layout and a shaping engine are outside the
current contract.

## OS desktop scenes

The optional [native OS presentation](desktop-gui.md) composes application scenes
with deterministic desktop/mobile chrome in Rust. `UiText` uses bundled proportional
DejaVu Sans; `RoundedBox` supplies antialiased rounded surfaces with matching hit
tests. Existing `Text` and its golden pixels remain unchanged. Application nodes
can now have translated scene coordinates inside window content clips.
