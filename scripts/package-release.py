#!/usr/bin/env python3
"""Package already-built Wasm bindings and the standalone browser demo."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, default=ROOT / 'target/release-assets')
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
version = tomllib.loads((ROOT / 'Cargo.toml').read_text())['workspace']['package']['version']
commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
manifest = dict(version=version, source_commit=commit, engine='canonical Rust runtime', prerelease=True)
staging = ROOT / 'target/release-staging'
if staging.exists():
    shutil.rmtree(staging)
staging.mkdir(parents=True)
for target in ('web', 'node'):
    name = f'computerworld-{version}-wasm-{target}'
    package = staging / name
    shutil.copytree(ROOT / 'pkg' / target, package)
    shutil.copytree(ROOT / 'worlds', package / 'worlds')
    shutil.copy(ROOT / 'LICENSE', package / 'LICENSE')
    shutil.copy(ROOT / 'docs/programmatic-computer-use.md', package / 'PROGRAMMATIC-USE.md')
    shutil.copytree(ROOT / 'examples/javascript', package / 'examples/javascript')
    # The distributed demo's relative paths deliberately match its source layout.
    demo = package / 'examples/javascript/computer-interaction.mjs'
    demo.write_text(demo.read_text().replace('`${root}/pkg/node/computerworld.js`', '`${root}/computerworld.js`'))
    metadata = dict(name=f'@jacobfv/computerworld-{target}', version=version,
                    description='Deterministic synthetic computer worlds: canonical Rust via WebAssembly',
                    license='MIT', repository='https://github.com/JacobFV/computerworld',
                    main='computerworld.js', types='computerworld.d.ts',
                    files=['*.js', '*.wasm', '*.ts', '*.md', 'LICENSE', 'notices', 'worlds', 'examples', 'release.json'])
    metadata['type'] = 'module' if target == 'web' else 'commonjs'
    (package / 'package.json').write_text(json.dumps(metadata, indent=2) + '\n')
    (package / 'release.json').write_text(json.dumps(manifest, indent=2) + '\n')
    usage = ("import init, { World } from './computerworld.js';\nawait init();\nconst definition = await fetch('./worlds/company-2026/world.json').then(r => r.json());" if target == 'web' else
             "const { World } = require('./computerworld.js');\nconst definition = require('./worlds/company-2026/world.json');")
    (package / 'README.md').write_text(f'''# Computerworld {version} — {target} Wasm

Source commit: `{commit}`. Alpha API and checkpoint compatibility may change.
This is the same canonical Rust runtime used by the native and Python interfaces.

```javascript
{usage}
const world = new World(definition, 42);
const env = world.environment({{actor: 'alice', machines: ['alice-mac'],
  actions: ['terminal.v1'], observations: ['terminal.v1']}});
console.log(env.step([{{family: 'terminal.v1', op: 'execute', machine: 'alice-mac',
  payload: {{command: 'echo hello'}}}}]));
env.free(); world.free();
```

{'Serve this directory over HTTP (for example `python -m http.server 8000`); initialization loads the sibling Wasm file. No simulator backend or outbound networking is needed.' if target == 'web' else 'Run the complete keyboard/mouse, rendering and replay example: `node examples/javascript/computer-interaction.mjs --output ./demo-output`.'}

See PROGRAMMATIC-USE.md, generated TypeScript declarations, and notices/ for third-party licenses.
World handles are privileged; give acting agents only configured environment handles.
''')
    with tarfile.open(out / f'{name}.tar.gz', 'w:gz') as archive:
        archive.add(package, arcname=name)

name = f'computerworld-{version}-browser-demo'
demo = staging / name
(demo / 'examples').mkdir(parents=True)
shutil.copytree(ROOT / 'examples/browser', demo / 'examples/browser', ignore=shutil.ignore_patterns('.openai'))
shutil.copytree(ROOT / 'pkg/web', demo / 'pkg/web')
shutil.copy(ROOT / 'LICENSE', demo / 'LICENSE')
(demo / 'release.json').write_text(json.dumps(manifest, indent=2) + '\n')
(demo / 'index.html').write_text('<!doctype html><meta charset="utf-8"><meta http-equiv="refresh" content="0;url=examples/browser/"><a href="examples/browser/">Open Computerworld</a>\n')
(demo / 'README.md').write_text(f'# Computerworld {version} browser demo\n\nRun `python -m http.server 8000` in this directory and open http://localhost:8000/.\nThe HTTP server serves static files only. Simulation runs entirely in your browser via Rust/Wasm.\nNo Node installation, simulation backend, API key, or real outbound network access is required.\nSource commit: `{commit}`. See pkg/web/notices/ for third-party licenses.\n')
with zipfile.ZipFile(out / f'{name}.zip', 'w', zipfile.ZIP_DEFLATED) as archive:
    for file in sorted(demo.rglob('*')):
        if file.is_file():
            archive.write(file, file.relative_to(staging))
(out / 'release-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
print(json.dumps(dict(output=str(out), **manifest), indent=2))
