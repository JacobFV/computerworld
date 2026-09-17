# Bundled fonts

`DejaVuSansMono.ttf` supplies the original fixed-cell `Text` primitive.
`DejaVuSans.ttf` supplies proportional `UiText` for desktop/mobile interfaces.
Both are DejaVu fonts, derived from Bitstream Vera, distributed under the
permissive font license reproduced in `FONT-LICENSE.txt`. DejaVu additions are
public domain. Font bytes are embedded at compilation; no runtime host fonts or
network access are consulted. SHA-256 fingerprints are checked by renderer tests.

Upstream: https://dejavu-fonts.github.io/

The renderer uses fontdue antialiasing, integer raster origins, 1/64-pixel
quantized UI advances, and integer line heights. UI text supports Unicode glyphs
available in the bundled font but does not implement bidirectional shaping or
complex-script layout. `Text` retains its original fixed-cell pixel output.

## Desktop resources

`wallpapers/{macos,windows,ios,android}.png` are original AI-generated raster
artwork created for Computerworld using the built-in imagegen tool. They are
OS-inspired artwork, not Apple/Microsoft/Google supplied wallpapers or real
photographs. Prompts are recorded in `WALLPAPER-PROMPTS.md`. The original generated
Ubuntu alternative is retained as `wallpapers/ubuntu-original-generated.png`.
These original project assets and the original vector icon sources are offered
under the repository MIT license to the extent applicable.

`wallpapers/ubuntu.png` is the official Noble Numbat dimmed wallpaper from the
Ubuntu `ubuntu-wallpapers` package, resized from 3480×2160 to 1600×993 using
ImageMagick. Source: `/usr/share/backgrounds/Numbat_wallpaper_dimmed_3480x2160.png`,
upstream <https://launchpad.net/ubuntu-wallpapers>. It remains **CC-BY-SA-3.0** under
the package's default asset license, credited to the Ubuntu community
contributors. Full package copyright and license: `UBUNTU-WALLPAPER-COPYRIGHT.txt`.

`icons/ubuntu-*.png` are **CC-BY-SA-4.0** Yaru icons, credited to Sam Hewitt and
Yaru contributors, copied without artistic modifications from the Ubuntu
`yaru-theme-icon` package's `256x256/apps` (or `48x48` fallback) and `places`
directories. Upstream: <https://github.com/ubuntu/yaru>. Full attribution and
license: `YARU-COPYRIGHT.txt`. These asset licenses are separate from the Rust code
license, and must accompany redistributed bundles.

`icons/{macos,windows,ios,android}-*.svg` are original vector artwork; PNGs are
128×128 offline rasterizations, reproducible with `generate-icons.py` (CairoSVG).
These icon approximations are not vendor-distributed artwork. Platform-specific
silhouettes, backgrounds and colors are intentional. DejaVuSans-Bold.ttf uses
the same bundled DejaVu font license as the other fonts (`FONT-LICENSE.txt`).

Runtime identifiers: `wallpaper/{macos,windows,ubuntu,ios,android}` and
`icon/{platform}/{files,browser,terminal,docs,mail,calendar,chat,settings,camera,photos,phone,store,launcher,trash}`.
`editor` aliases `docs`, and `messages` aliases `chat`; unqualified `icon/{app}`
uses macOS artwork. Unknown names paint nothing and never access a host path or
URL. All PNGs decode lazily once per process/Wasm instance into immutable shared
`Arc<Frame>` resources. Scenes contain identifiers, never repeated image bytes.

`Primitive::Shadow` uses a rounded mask inset by `blur` pixels inside its bounds,
then three separable integer box-filter passes. Bounds therefore include the
padding. The cached shadow mask is renderer-local and color-independent; cache
retention is bounded. `UiTextBold` uses a separately bundled bold font and has
separate glyph/text cache keys. Neither feature consults host fonts, clocks,
networking or browser rendering.
