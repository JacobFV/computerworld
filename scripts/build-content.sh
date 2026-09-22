#!/usr/bin/env bash
# Regenerate everything derived from worlds/company-2026/{world.yml,sites,home,files}.
#
# The world itself is resolved by cw-world from worlds/company-2026/world.yml: the
# includes splice in the site files, the `copy:` blocks seed the desktops from
# home/, and each site's `place:` block derives its node, its link and its DNS
# records. The search index runs first because the world pulls each engine's
# documents in with `from_file`, and the live site world is derived from the
# finished world.json afterwards.
set -euo pipefail
cd "$(dirname "$0")/.."
node scripts/build-search-index.mjs
cargo run --quiet -p cw-blueprint --bin cw-world -- build worlds/company-2026/world.yml
cargo run --quiet -p cw-blueprint --bin cw-world -- \
  build examples/worlds/agent-desktop/world.yml -o examples/worlds/agent-desktop.json
node scripts/build-live-world.mjs
