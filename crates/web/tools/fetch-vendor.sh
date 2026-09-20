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
echo "vendor files verified"
