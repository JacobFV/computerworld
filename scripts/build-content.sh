#!/usr/bin/env bash
# Regenerate everything derived from worlds/company-2026/sites/*.json.
# Order matters: the splice must land the services before the index can write into them.
set -euo pipefail
cd "$(dirname "$0")/.."
node scripts/build-world.mjs
node scripts/build-search-index.mjs
node examples/browser/build.mjs
