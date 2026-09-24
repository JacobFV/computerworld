#!/usr/bin/env bash
# Builds the app-* framework-parity fixtures from their TSX sources, as an app's own
# toolchain would: esbuild bundles each app's main.tsx into one classic script (React
# and ReactDOM stay external, taken from the production UMD builds the page loads from
# /vendor/), and the Tailwind CLI generates a sheet holding the utilities that app's
# sources use. Both land in tests/vendor/ as app-<name>.js and app-<name>.css, which
# the fixture pages (tests/framework-parity/app-<name>.html) link. Needs npx and the
# network the first time; the versions are pinned so the output is reproducible.
#
#     bash crates/web/engine/tests/framework-parity/app-src/build.sh [app ...]
set -euo pipefail
cd "$(dirname "$0")"
vendor=../../vendor
apps=("$@")
if [ ${#apps[@]} -eq 0 ]; then
  apps=(analytics kanban settings chat datatable shop)
fi
for app in "${apps[@]}"; do
  npx --yes esbuild@0.24.0 "$app/main.tsx" \
    --bundle --format=iife --target=es2020 --charset=utf8 --legal-comments=none \
    --jsx=automatic \
    --alias:react=./shims/react --alias:react-dom=./shims/react-dom \
    --log-level=warning \
    --outfile="$vendor/app-$app.js"
  npx --yes tailwindcss@3.4.17 -c tailwind.config.js \
    --content "./$app/**/*.{ts,tsx},./shared/**/*.tsx" \
    -i input.css -o "$vendor/app-$app.css" 2>/dev/null
  echo "app-$app: $(wc -c < "$vendor/app-$app.js") bytes of script, $(wc -c < "$vendor/app-$app.css") bytes of css"
done
