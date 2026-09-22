#!/usr/bin/env python3
"""Re-encode the bundled wallpapers from their PNG masters to baseline JPEG.

Wallpapers are the only photographic assets in the bundle. PNG stores them at
roughly 8 bits per pixel and, because PNG is already DEFLATE, the outer `gzip`
of the Wasm bundle cannot compress them at all: they were 9,293,680 bytes raw
and 9,240,819 gzipped, two thirds of the entire download. Baseline JPEG at
quality 92 stores the same pixels at 35-54 dB PSNR for 1,765,706 bytes.

Encoder settings that matter, and why:

  * `subsampling=0` (4:4:4). Chroma subsampling would save a further ~20%, but
    it puts `jpeg-decoder`'s chroma upsampler on the decode path, and that
    upsampler uses `f32` arithmetic (`upsampler.rs`, `row_near.fract()`).
    Rendering here must be bit-identical between native and Wasm, so the
    wallpapers are encoded without subsampling and the upsampler never runs.
  * `progressive=False`. Baseline sequential only; the decoder's progressive
    path is more code in the bundle for no size win at these dimensions.
  * `optimize=True` computes per-image Huffman tables, worth ~3%.

Source dimensions are preserved: this changes the container, not the geometry.

Usage: build-wallpapers.py [--check]
Requires Pillow (built against libjpeg-turbo). Written with Pillow 10.2.0.
"""
import hashlib
import sys
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
WALLPAPERS = HERE / "wallpapers"
QUALITY = 92
# PNG master -> emitted JPEG. `ubuntu-original-generated.png` is kept as an
# unused alternative master and is not embedded, so it is not encoded here.
FACES = ["macos", "windows", "ubuntu", "ios", "android"]


def encode(name: str) -> bytes:
    master = WALLPAPERS / f"{name}.png"
    with Image.open(master) as im:
        # Every master is fully opaque; JPEG has no alpha channel and the
        # renderer fills alpha with 255 for any non-alpha source.
        if im.mode == "RGBA" and im.getchannel("A").getextrema() != (255, 255):
            raise SystemExit(f"{master.name} has real transparency; JPEG cannot carry it")
        rgb = im.convert("RGB")
        out = WALLPAPERS / f"{name}.jpg"
        rgb.save(out, "JPEG", quality=QUALITY, optimize=True,
                 progressive=False, subsampling=0)
    return out.read_bytes()


def main(argv):
    check = "--check" in argv
    total_png = total_jpg = 0
    for name in FACES:
        png = (WALLPAPERS / f"{name}.png").stat().st_size
        before = (WALLPAPERS / f"{name}.jpg").read_bytes() if check else None
        data = encode(name)
        if check and before != data:
            raise SystemExit(f"{name}.jpg is not reproducible on this Pillow build")
        total_png += png
        total_jpg += len(data)
        print(f"{name:8s} {png:>9,} -> {len(data):>9,}  "
              f"sha256={hashlib.sha256(data).hexdigest()[:16]}")
    print(f"{'total':8s} {total_png:>9,} -> {total_jpg:>9,}")


if __name__ == "__main__":
    main(sys.argv[1:])
