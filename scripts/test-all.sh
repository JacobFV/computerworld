#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/check-boundaries.py
# Formatting is gated by the `format` job in CI, not here: the tree carries a
# reformat that touches 246 files, and while it is outstanding a combined gate
# would hide whether the tests, the lint and the Wasm build are sound. Run
# `cargo fmt --all -- --check` to see what that job sees.
cargo test --workspace
cargo test -p cw-host-adapters --features native-http
cargo clippy --workspace --all-targets -- -D warnings
cargo build -p cw-wasm --target wasm32-unknown-unknown --release
# Generated world content must match its sources: a hand-edited world.json would be
# silently overwritten by the next build, so catch the drift here instead. Compared
# against a fresh regeneration, not against git, so an uncommitted tree still checks out.
generated=$(mktemp -d)
trap 'rm -rf "$generated"' EXIT
cp worlds/company-2026/world.json "$generated/world.json"
cp site/world-definition.js "$generated/world-definition.js"
scripts/build-content.sh >/dev/null
for pair in "worlds/company-2026/world.json:world.json" \
            "site/world-definition.js:world-definition.js"; do
  cmp -s "${pair%%:*}" "$generated/${pair##*:}" \
    || { echo "generated content is stale: run scripts/build-content.sh" >&2; exit 1; }
done
# Binding execution checks run separately after their generated artifacts exist.
