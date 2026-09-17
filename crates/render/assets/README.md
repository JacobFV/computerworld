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
