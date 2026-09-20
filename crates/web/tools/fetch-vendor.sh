#!/usr/bin/env bash
# Fetches the pinned third-party bundles the script fixtures run unmodified
# (crates/web/tests/vendor/), verifying each file's SHA-256. Re-run after bumping
# a version below and updating its hash.
set -euo pipefail
cd "$(dirname "$0")/../tests/vendor"
fetch() {
  local url="$1" out="$2" sha="$3"
  curl -sSL --max-time 120 -o "$out" "$url"
  echo "$sha  $out" | sha256sum -c -
}
fetch https://cdn.jsdelivr.net/npm/jquery@3.7.1/dist/jquery.min.js jquery-3.7.1.min.js fc9a93dd241f6b045cbff0481cf4e1901becd0e12fb45166a8f17f95823f0b1a
fetch https://cdn.jsdelivr.net/npm/jquery@3.7.1/LICENSE.txt jquery.LICENSE.txt d4db9ebe6f29f5168eac45ad713f055623ac5d0dcd5ba92da23d650ae012020d
fetch https://cdn.jsdelivr.net/npm/preact@10.19.3/dist/preact.umd.js preact-10.19.3.umd.js 91c14e75b9ac7c0317cd8ebbde735e12c0a3120d09d8a9f9704a0bb0f92b011b
fetch https://cdn.jsdelivr.net/npm/preact@10.19.3/hooks/dist/hooks.umd.js preact-hooks-10.19.3.umd.js 269644b2eb663fb74a92d5bb3b8eb5fac6828bb6a1f8dbe3d483c4952b31f678
fetch https://cdn.jsdelivr.net/npm/preact@10.19.3/LICENSE preact.LICENSE 1fe6958409c8c257a70c587a18b6f7f412b179b456630790d30b2ec9a8e4b7d4

# React 18.3 and 17.0, production UMD builds, run unmodified by the react18*/react17
# fixtures (ReactDOM's UMD needs React's own global, so both halves are pinned).
fetch https://cdn.jsdelivr.net/npm/react@18.3.1/umd/react.production.min.js react-18.3.1.production.min.js d949f1c3687aedadcedac85261865f29b17cd273997e7f6b2bfc53b2f9d4c4dd
fetch https://cdn.jsdelivr.net/npm/react-dom@18.3.1/umd/react-dom.production.min.js react-dom-18.3.1.production.min.js 35f4f974f4b2bcd44da73963347f8952e341f83909e4498227d4e26b98f66f0d
fetch https://cdn.jsdelivr.net/npm/react@17.0.2/umd/react.production.min.js react-17.0.2.production.min.js 229bbf4d0e7488209564152c6723497f1ac3934136ca1684233d2fa88fa4146f
fetch https://cdn.jsdelivr.net/npm/react-dom@17.0.2/umd/react-dom.production.min.js react-dom-17.0.2.production.min.js 9db33292007ab6c38527b39d5663e976a305564e19b2a5a8713ea2b2c00f505d
fetch https://cdn.jsdelivr.net/npm/react@18.3.1/LICENSE react.LICENSE 52412d7bc7ce4157ea628bbaacb8829e0a9cb3c58f57f99176126bc8cf2bfc85

# Vue 3.4, the global production build (it carries the runtime template compiler the
# vue3 fixture's in-DOM template needs).
fetch https://cdn.jsdelivr.net/npm/vue@3.4.38/dist/vue.global.prod.js vue-3.4.38.global.prod.js b50eeefe35d41636bb96c92b40f1df0b4fb7914e07b3c625b1ec15e9748767b9
fetch https://cdn.jsdelivr.net/npm/vue@3.4.38/LICENSE vue.LICENSE 1bb85cc9b13b81ef41c81c51866172fc345e0503c86726a6755b796590b70175

# styled-components 6 and emotion 11, the runtime CSS-in-JS pair.
fetch https://cdn.jsdelivr.net/npm/styled-components@6.1.13/dist/styled-components.min.js styled-components-6.1.13.min.js 037c48cdde368529fd43c48f23efde32a87be0b6618f56b99acc31089ef1a387
fetch https://cdn.jsdelivr.net/npm/styled-components@6.1.13/LICENSE styled-components.LICENSE 5b893305a717230e5b0024ff5ca3eec1dc05ba600c9b140730488d97ce56f1e6
fetch https://cdn.jsdelivr.net/npm/@emotion/css@11.13.4/dist/emotion-css.umd.min.js emotion-css-11.13.4.umd.min.js e8488d0f8f7ea39f377752f08e6bec16bb41eda2dd6a3110538bd2ce0badd551
fetch https://cdn.jsdelivr.net/npm/@emotion/react@11.13.3/dist/emotion-react.umd.min.js emotion-react-11.13.3.umd.min.js e4d29de63518a2e41810dfd058fc2b82213721cf1decde0525a662fd50c8bdfc
fetch https://cdn.jsdelivr.net/npm/@emotion/react@11.13.3/LICENSE emotion.LICENSE 6e8d23763cf7cb707ad8f6677aa502ebeca1dda447e6d6e57f6549c1625b9c4a

fetch https://cdn.jsdelivr.net/npm/tailwindcss@3.4.17/LICENSE tailwind.LICENSE 60e0b68c0f35c078eef3a5d29419d0b03ff84ec1df9c3f9d6e39a519a5ae7985

# Tailwind isn't a static bundle: it's generated from crates/web/tools/tailwind/
# (tailwind.config.js's content + safelist, input.css) against the pinned version below,
# so this rebuilds tailwind-3.4.17.css instead of downloading it.
#
# Tailwind 3.4.17's CLI treats `--minify=false` as `--minify` (any value makes the `arg`
# boolean flag true), so the flag is simply omitted here; that's already the default and
# is what produced the checked-in file (unminified, one selector per line).
npx --yes tailwindcss@3.4.17 \
  -c ../../tools/tailwind/tailwind.config.js \
  -i ../../tools/tailwind/input.css \
  -o tailwind-3.4.17.css
echo "e053322e0e2efe2be33e9a04d18e31cc1f8fc18b603919c178244ac1a81f2183  tailwind-3.4.17.css" | sha256sum -c -

fetch https://cdn.jsdelivr.net/npm/svelte@4.2.19/LICENSE.md svelte.LICENSE.md 97a941bae1c510c556c5daf017180e4939ea392357d9a7bae2657e3e20c69c42

# Svelte isn't a static bundle either: `crates/web/tests/script/svelte/Todos.svelte`
# and `Counter.svelte` are hand-written components (checked in as source, next to
# their shared store in stores.js) that get compiled ahead of time, since the
# fixture runs with no bundler and no ES module support at runtime. build-svelte.mjs
# downloads the pinned svelte@4.2.19 compiler and runtime straight from the CDN
# (file by file, since jsdelivr doesn't serve a single package archive), compiles
# both components with svelte/compiler, and bundles the result with the runtime
# (svelte/internal, svelte/store, svelte/transition) into one classic script via
# esbuild, so it produces svelte-app-4.2.19.js instead of downloading it.
node ../../tools/build-svelte.mjs
echo "78dd6313c5633be73fc7a8b52bc0a03416405660510203b232553f617bf9db0e  svelte-app-4.2.19.js" | sha256sum -c -

echo "vendor files verified"
