#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/check-boundaries.py
# Formatting is gated by the `format` job in CI, not here. It used to be the line
# below this one, and because `set -e` stops the script, a formatting failure
# meant none of the checks that follow ever ran. Run `cargo fmt --all -- --check`
# alongside this script, not inside it.
cargo test --workspace
cargo test -p cw-host-adapters --features native-http
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p cw-wasm --target wasm32-unknown-unknown --release
# Generated world content must match its sources: a hand-edited world.json would be
# silently overwritten by the next build, so catch the drift here instead. Compared
# against a fresh regeneration, not against git, so an uncommitted tree still checks out.
generated=$(mktemp -d)
trap 'rm -rf "$generated"' EXIT
generated_files=(worlds/company-2026/world.json
                 worlds/company-2026/index/google-search.json
                 worlds/company-2026/index/bing-search.json
                 worlds/company-2026/index/ddg-search.json
                 worlds/agent-desktop/world.json
                 site/world-definition.js)
for file in "${generated_files[@]}"; do
  mkdir -p "$generated/$(dirname "$file")"
  cp "$file" "$generated/$file"
done
scripts/build-content.sh >/dev/null
for file in "${generated_files[@]}"; do
  cmp -s "$file" "$generated/$file" \
    || { echo "generated content is stale: run scripts/build-content.sh ($file)" >&2; exit 1; }
done
# Binding execution checks run separately after their generated artifacts exist.
