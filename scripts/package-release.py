#!/usr/bin/env python3
"""Package the already-built Wasm bindings for release."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tarfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--output', type=Path, default=ROOT / 'target/release-assets')
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)
version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
manifest = dict(version=version, source_commit=commit, engine='canonical Rust runtime', prerelease='-' in version)
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
    demo.write_text(demo.read_text(encoding='utf-8').replace('`${root}/pkg/node/computerworld.js`', '`${root}/computerworld.js`'))
    metadata = dict(name=f'@jacobfv/computerworld-{target}', version=version,
                    description='Deterministic synthetic computer worlds: canonical Rust via WebAssembly',
                    license='MIT', repository='https://github.com/JacobFV/computerworld',
                    main='computerworld.js', types='computerworld.d.ts',
                    files=['*.js', '*.wasm', '*.ts', '*.md', 'LICENSE', 'notices', 'fonts', 'worlds', 'examples', 'release.json'])
    metadata['type'] = 'module' if target == 'web' else 'commonjs'
    (package / 'package.json').write_text(json.dumps(metadata, indent=2) + '\n', encoding='utf-8')
    (package / 'release.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
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

CJK (Han, kana, Hangul; bold, and Traditional Chinese, Japanese and Korean forms),
emoji (colour and monochrome) and the Gujarati, Ethiopic, Myanmar and Sinhala glyphs
are a separate font pack in fonts/, not in the module: pass each file's bytes to
`installFont` (at startup, or when `fontPackStatus().missing` lists it). Layout does
not depend on it; until a file is installed its glyphs draw as boxes (emoji draw
monochrome until the colour file is installed).

See PROGRAMMATIC-USE.md, generated TypeScript declarations, and notices/ for third-party licenses.
World handles are privileged; give acting agents only configured environment handles.
''', encoding='utf-8')
    with tarfile.open(out / f'{name}.tar.gz', 'w:gz') as archive:
        archive.add(package, arcname=name)

(out / 'release-manifest.json').write_text(json.dumps(manifest, indent=2) + '\n', encoding='utf-8')
print(json.dumps(dict(output=str(out), **manifest), indent=2))
