#!/usr/bin/env bash
set -euo pipefail
SCE_SOURCE="${SCE_SOURCE:-/tmp/computerworld-sources/synthetic-computer-environment}"
export SCE_SOURCE
config=$(mktemp)
trap 'rm -f "$config"' EXIT
python3 - "$SCE_SOURCE" "$config" <<'PY'
import json,pathlib,sys
root=pathlib.Path(sys.argv[1]); paths={}
for d in [*root.glob('packages/*'),*root.glob('ecosystems/*')]:
 p=d/'package.json'
 if p.exists(): paths[json.loads(p.read_text())['name']]=[str(d/'src/index.ts')]
pathlib.Path(sys.argv[2]).write_text(json.dumps({'compilerOptions':{'baseUrl':str(root),'paths':paths}}))
PY
loader=$(find "$SCE_SOURCE/node_modules" -path '*/tsx/dist/loader.mjs' -print -quit)
if [ -z "$loader" ]; then echo 'Install SCE dependencies (tsx required)' >&2; exit 1; fi
TSX_TSCONFIG_PATH="$config" node --import "$loader" "$(dirname "$0")/predecessor.mjs"
