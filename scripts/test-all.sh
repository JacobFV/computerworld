#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/check-boundaries.py
cargo fmt --all -- --check
cargo test --workspace
cargo test -p cw-host-adapters --features native-http
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p cw-wasm --target wasm32-unknown-unknown --release
# Binding execution checks run separately after their generated artifacts exist.
