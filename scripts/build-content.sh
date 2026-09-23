#!/usr/bin/env bash
# Regenerate every world's world.json from its blueprint, and what is derived from them.
#
# The world itself is resolved by cw-world from worlds/company-2026/world.yml: the
# includes splice in each computers/<id>/computer.json, seeded from the root/
# beside it, and each services/<id>/service.json and overlay.json, and each
# service's `place:` block derives its node, its link and its DNS records. The search engines index the sites themselves when the world boots,
# and the live site world is derived from the finished world.json afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."
cargo run --quiet -p cw-blueprint --bin cw-world -- build worlds/internet/world.yml
cargo run --quiet -p cw-blueprint --bin cw-world -- build worlds/company-2026/world.yml
cargo run --quiet -p cw-blueprint --bin cw-world -- build worlds/agent-desktop/world.yml
node scripts/content/build-live-world.mjs
