#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build -p cw-wasm --release --target wasm32-unknown-unknown
wasm-bindgen target/wasm32-unknown-unknown/release/cw_wasm.wasm --target web --out-dir pkg/web --out-name computerworld
wasm-bindgen target/wasm32-unknown-unknown/release/cw_wasm.wasm --target nodejs --out-dir pkg/node --out-name computerworld
