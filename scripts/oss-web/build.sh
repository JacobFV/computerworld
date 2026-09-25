#!/usr/bin/env bash
# Builds the open-source web apps the world runs as sites (docs/oss-webapps.md) from
# their upstream repositories at pinned commits, with each app's own toolchain, into
# crates/services/oss-web/packages/<id>/{public,server}. The manifests beside them are
# authored and checked in; this script writes only the built files.
#
#     bash scripts/oss-web/build.sh [app ...]      # all apps when none is named
#
# Needs git, node and npm (npx fetches yarn/pnpm at the versions the upstreams pin), and
# the network. Sources and their node_modules are cached under $OSS_WEB_CACHE
# (default ~/.cache/computerworld/oss-web), so a rebuild is quick. Every install uses
# the upstream's own lockfile, and every commit is pinned, so the output is the same
# on every machine up to what the bundlers themselves embed (they embed no dates).
set -euo pipefail
cd "$(dirname "$0")/../.."
ROOT=$PWD
OUT=$ROOT/crates/services/oss-web/packages
SHIMS=$ROOT/crates/services/oss-web/shims
CACHE=${OSS_WEB_CACHE:-$HOME/.cache/computerworld/oss-web}
mkdir -p "$CACHE"
export CYPRESS_INSTALL_BINARY=0 PUPPETEER_SKIP_DOWNLOAD=1 HUSKY=0 CI=true
export npm_config_audit=false npm_config_fund=false npm_config_update_notifier=false

# fetch <name> <repository> <commit>: a checkout of exactly that commit.
fetch() {
  local dir=$CACHE/$1
  if [ ! -d "$dir/.git" ]; then
    git init -q "$dir"
    git -C "$dir" remote add origin "$2"
  fi
  if [ "$(git -C "$dir" rev-parse -q --verify HEAD 2>/dev/null)" != "$3" ]; then
    git -C "$dir" fetch -q --depth 1 origin "$3"
    git -C "$dir" checkout -q --force "$3"
  fi
  echo "$dir"
}

# publish <id> <kind> <source dir>: replace packages/<id>/<kind> with the files of a
# build, leaving out source maps (they are for the upstream's own debugging, and
# are most of the bytes).
publish() {
  local dest=$OUT/$1/$2
  rm -rf "$dest"
  mkdir -p "$dest"
  (cd "$3" && find . -type f ! -name '*.map' -print0 | sort -z | xargs -0 -I{} cp --parents {} "$dest")
}

TODOMVC=ff43b02e59dfa604386bb382034b2cd07c2bcd8a
REALWORLD_API=30b68e1e881462b2f4164ea09ab4c4f5699c7b0b
CONDUIT_REACT=53b0b4c0b8c371053a8d082ff9a42bfae68f3755
CONDUIT_VUE=741c215ef0f674f90fcb03c5493a1b3a3a7f1b03
REACT_ADMIN=99e8c52b7db1712c0afa4eeed533e4a713dfc1ec
JSON_SERVER=78ea71375666d49145734689c097654c54f90686 # v0.17.4

build_todomvc() {
  local src
  src=$(fetch todomvc https://github.com/tastejs/todomvc "$TODOMVC")
  local stage=$CACHE/todomvc-stage
  rm -rf "$stage"
  for app in react vue; do
    (cd "$src/examples/$app" && npm ci --ignore-scripts >/dev/null && npm run build >/dev/null)
    mkdir -p "$stage/examples/$app"
    cp -r "$src/examples/$app/dist" "$stage/examples/$app/dist"
  done
  cp "$src/license.md" "$stage/license.md"
  publish todomvc public "$stage"
  # The landing page is ours: the upstream's site index pulls in its whole bower tree.
  cp "$ROOT/crates/services/oss-web/shims/todomvc-index.html" "$OUT/todomvc/public/index.html"
}

build_realworld_api() {
  local src
  src=$(fetch realworld-api https://github.com/gothinkster/node-express-realworld-example-app "$REALWORLD_API")
  (cd "$src" && npm ci --ignore-scripts >/dev/null)
  # The one change to the app's own code: bcrypt's cost factor, 10 -> 4. At 10 a
  # single hash is 1024 key expansions, which on the in-world VM exceeds a request's
  # step budget; 4 keeps real bcrypt hashes (verifiable by any bcrypt library).
  # Staged inside the checkout, so the bundler resolves the app's own node_modules.
  local stage=$src/.cw-src
  rm -rf "$stage"
  cp -r "$src/src" "$stage"
  sed -i 's/bcrypt\.hash(password, 10)/bcrypt.hash(password, 4)/' "$stage/app/routes/auth/auth.service.ts"
  grep -q 'bcrypt.hash(password, 4)' "$stage/app/routes/auth/auth.service.ts"
  # Prisma's engine is a native binary talking to Postgres; the app's own client calls
  # go to a Prisma Client over one JSON file instead, which reads this schema.
  cp "$SHIMS/prisma-store.js" "$stage/prisma/cw-prisma-store.js"
  (cd "$src" && npx esbuild "$stage/main.ts" --bundle --platform=node --format=cjs \
    --target=node20 --log-level=warning --legal-comments=eof \
    --alias:@prisma/client="$stage/prisma/cw-prisma-store.js" --loader:.prisma=text \
    --outfile="$CACHE/realworld-api-out/server.js")
  publish realworld-api server "$CACHE/realworld-api-out"
  mkdir -p "$CACHE/realworld-api-public/images"
  cp "$src/src/assets/images/"* "$CACHE/realworld-api-public/images/"
  publish realworld-api public "$CACHE/realworld-api-public"
}

build_conduit_react() {
  local src
  src=$(fetch conduit-react https://github.com/khaledosman/react-redux-realworld-example-app "$CONDUIT_REACT")
  (cd "$src" && npx -y yarn@1.22.22 install --frozen-lockfile --ignore-scripts >/dev/null \
    && REACT_APP_BACKEND_URL=http://api.realworld.show/api GENERATE_SOURCEMAP=false CI=false \
       npx react-scripts build >/dev/null)
  local stage=$CACHE/conduit-react-stage
  rm -rf "$stage"
  cp -r "$src/build" "$stage"
  # The page links the RealWorld theme from demo.productionready.io, which is not on
  # this internet; the same stylesheet is served from the site itself.
  sed -i 's#//demo.productionready.io/main.css#/main.css#' "$stage/index.html"
  cp "$(fetch conduit-vue https://github.com/mutoe/vue3-realworld-example-app "$CONDUIT_VUE")/public/main.css" "$stage/main.css"
  cp "$src/LICENSE.md" "$stage/LICENSE.md"
  publish conduit-react public "$stage"
}

build_conduit_vue() {
  local src
  src=$(fetch conduit-vue https://github.com/mutoe/vue3-realworld-example-app "$CONDUIT_VUE")
  (cd "$src" && npx -y pnpm@10.33.0 install --frozen-lockfile --ignore-scripts >/dev/null \
    && VITE_API_HOST=http://api.realworld.show npx -y pnpm@10.33.0 exec vite build >/dev/null)
  cp "$src/LICENSE" "$src/dist/LICENSE"
  publish conduit-vue public "$src/dist"
}

build_react_admin() {
  local src
  src=$(fetch react-admin https://github.com/marmelab/react-admin "$REACT_ADMIN")
  (cd "$src" && node .yarn/releases/yarn-4.0.2.cjs install --immutable --mode=skip-build >/dev/null \
    && cd examples/simple && npx vite build >/dev/null)
  cp "$src/LICENSE.md" "$src/examples/simple/dist/LICENSE.md"
  publish react-admin public "$src/examples/simple/dist"
}

build_json_server() {
  local src
  src=$(fetch json-server https://github.com/typicode/json-server "$JSON_SERVER")
  (cd "$src" && npm ci --ignore-scripts >/dev/null && npx babel src -d lib >/dev/null)
  (cd "$src" && npx -y esbuild@0.19.12 lib/cli/bin.js --bundle --platform=node --format=cjs \
    --target=node20 --log-level=warning --legal-comments=eof \
    --outfile="$CACHE/json-server-out/server.js")
  # errorhandler reads its page template from beside its own source, which after
  # bundling is beside server.js.
  mkdir -p "$CACHE/json-server-out/public"
  cp "$src/node_modules/errorhandler/public/"* "$CACHE/json-server-out/public/"
  publish json-server server "$CACHE/json-server-out"
  mkdir -p "$CACHE/json-server-public"
  cp "$src/public/"* "$src/LICENSE" "$CACHE/json-server-public/"
  publish json-server public "$CACHE/json-server-public"
}

APPS=("$@")
[ ${#APPS[@]} -eq 0 ] && APPS=(todomvc realworld-api conduit-react conduit-vue react-admin json-server)
for app in "${APPS[@]}"; do
  echo "building $app"
  "build_${app//-/_}"
done
du -sh "$OUT"/*/
