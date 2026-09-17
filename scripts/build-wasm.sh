#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p cw-wasm --release --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/release/cw_wasm.wasm --target web --out-dir pkg/web --out-name computerworld
wasm-bindgen target/wasm32-unknown-unknown/release/cw_wasm.wasm --target nodejs --out-dir pkg/node --out-name computerworld
for package in pkg/web pkg/node; do
  mkdir -p "$package/notices"
  cp LICENSE "$package/notices/PROJECT-LICENSE.txt"
  cp crates/render/assets/FONT-LICENSE.txt crates/render/assets/YARU-COPYRIGHT.txt crates/render/assets/UBUNTU-WALLPAPER-COPYRIGHT.txt "$package/notices/"
  cp crates/render/assets/README.md "$package/notices/ASSET-ATTRIBUTION.md"
done
