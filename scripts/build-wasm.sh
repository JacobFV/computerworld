#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

# Size flags for the Wasm build only. They are passed with `--config` rather
# than written into `[profile.release]` because that profile is shared with the
# native builds the benchmarks in docs/performance.md measure, and optimizing
# those for size instead of speed would silently invalidate them.
#
#   opt-level="z"   size over speed for the world, services, kernel and glue.
#   lto="fat"       whole-program, not just per-crate; the module is one cdylib.
#   strip="symbols" drops the Wasm `name` section, 1.4 MB of debug symbols that a
#                   shipped bundle never reads. wasm-bindgen works from the export
#                   table and its own custom sections, not from names.
#
# The pixel path stays at opt-level 3: the rasterizer and the decoders it calls.
# Measured on a 960x640 desktop render in Node (warm p50; gzip -9 of pkg/web):
#
#                                        gzip -9     render p50
#   opt-level 3 everywhere             6,004,786       ~50 ms
#   opt-level z everywhere             4,906,012       ~88 ms
#   z, raster path at 3 (this build)   4,931,475       ~70 ms
#   z, raster + scene building at 3    5,369,981       ~50 ms
#
# The per-package overrides survive fat LTO (the numbers above are with it).
# panic is deliberately not set: wasm32-unknown-unknown already aborts, so there
# is nothing to gain and `[profile.release]` would leak it into native builds.
fast=()
for package in cw-render fontdue jpeg-decoder png miniz_oxide fdeflate; do
  fast+=(--config "profile.release.package.$package.opt-level=3")
done
cargo build -p cw-wasm --release --target wasm32-unknown-unknown \
  --config 'profile.release.opt-level="z"' \
  --config 'profile.release.lto="fat"' \
  --config 'profile.release.strip="symbols"' \
  "${fast[@]}"

wasm-bindgen target/wasm32-unknown-unknown/release/cw_wasm.wasm --target web --out-dir pkg/web --out-name computerworld
wasm-bindgen target/wasm32-unknown-unknown/release/cw_wasm.wasm --target nodejs --out-dir pkg/node --out-name computerworld

# No `wasm-opt` pass, deliberately. Measured on this module with binaryen 119,
# every optimization level shrinks the raw file but makes the gzipped download —
# the number that matters — larger, because its rewrites destroy the repetition
# DEFLATE exploits:
#
#                  raw bytes    gzip -9
#   no wasm-opt   10,039,467  4,905,851
#   wasm-opt -Oz   9,063,190  5,058,318  (+152 KB gzip)
#   wasm-opt -Os   9,187,046  5,057,108
#   wasm-opt -O2   9,301,169  5,066,508
#
# Revisit if the bundle is ever served raw, or if a binaryen release changes this.

for package in pkg/web pkg/node; do
  mkdir -p "$package/notices"
  cp LICENSE "$package/notices/PROJECT-LICENSE.txt"
  cp crates/render/assets/FONT-LICENSE.txt crates/render/assets/YARU-COPYRIGHT.txt crates/render/assets/UBUNTU-WALLPAPER-COPYRIGHT.txt crates/render/assets/fonts/*.txt "$package/notices/"
  cp crates/render/assets/README.md "$package/notices/ASSET-ATTRIBUTION.md"
  # The CJK/emoji font pack is not in the module (native builds embed it). Pages
  # fetch `fonts/<file>` on demand and hand the bytes to `installFont`; see
  # crates/render/assets/README.md. Its OFL notices are among fonts/*.txt above.
  mkdir -p "$package/fonts"
  cp crates/render/assets/fonts/pack/*.ttf "$package/fonts/"
done

for package in pkg/web pkg/node; do
  raw=$(wc -c <"$package/computerworld_bg.wasm")
  gz=$(gzip -9 -c "$package/computerworld_bg.wasm" | wc -c)
  printf '%s %12d raw %12d gzip -9\n' "$package" "$raw" "$gz"
done
for font in pkg/web/fonts/*.ttf; do
  raw=$(wc -c <"$font")
  gz=$(gzip -9 -c "$font" | wc -c)
  printf '%s %12d raw %12d gzip -9 (font pack, fetched on demand)\n' "$font" "$raw" "$gz"
done
