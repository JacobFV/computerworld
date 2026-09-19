#!/usr/bin/env python3
"""Assemble the npm package from the two wasm-bindgen outputs, pack it, and prove it installs.

`npm install computerworld` has to serve Node and the browser. wasm-bindgen emits one
glue module per target around the same `.wasm`, so the package holds the Wasm, the font
pack and the notices once and both glues beside them: `exports` sends Node to the
CommonJS one (which reads `computerworld_bg.wasm` next to itself) and everything else to
the ES module (which fetches it relative to its own URL).

    python scripts/package-npm.py [--output target/release-assets]

Writes `computerworld-<version>.tgz`, then installs that file into an empty project
outside the source tree and runs a machine through both Node entry styles.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import shutil
import subprocess
import tempfile
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, default=ROOT / 'target/release-assets')
args = parser.parse_args()
out = args.output.resolve()
out.mkdir(parents=True, exist_ok=True)

version = tomllib.loads((ROOT / 'Cargo.toml').read_text(encoding='utf-8'))['workspace']['package']['version']
commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
web, node = ROOT / 'pkg/web', ROOT / 'pkg/node'
digest = lambda path: hashlib.sha256(path.read_bytes()).hexdigest()
if digest(web / 'computerworld_bg.wasm') != digest(node / 'computerworld_bg.wasm'):
    raise SystemExit('pkg/web and pkg/node hold different Wasm; run scripts/build-wasm.sh so both come from one build')

package = ROOT / 'target/npm/computerworld'
if package.exists():
    shutil.rmtree(package)
package.mkdir(parents=True)
for name in ('computerworld.js', 'computerworld.d.ts', 'computerworld_bg.wasm', 'computerworld_bg.wasm.d.ts'):
    shutil.copy(web / name, package / name)
shutil.copy(node / 'computerworld.js', package / 'computerworld.cjs')
shutil.copy(node / 'computerworld.d.ts', package / 'computerworld.d.cts')
for folder in ('fonts', 'notices'):
    shutil.copytree(web / folder, package / folder)
shutil.copytree(ROOT / 'worlds/company-2026', package / 'worlds/company-2026')
shutil.copy(ROOT / 'LICENSE', package / 'LICENSE')
(package / 'release.json').write_text(json.dumps(dict(version=version, source_commit=commit), indent=2) + '\n', encoding='utf-8')
(package / 'package.json').write_text(json.dumps({
    'name': 'computerworld',
    'version': version,
    'description': 'An ultra computer world simulator written in Rust: five OS shells, real applications, '
                   'a synthetic internet and byte-identical replay, as WebAssembly for Node and the browser.',
    'keywords': ['simulator', 'computer-use', 'agents', 'reinforcement-learning', 'environment', 'wasm', 'deterministic'],
    'license': 'MIT',
    'homepage': 'https://github.com/JacobFV/computerworld',
    'repository': {'type': 'git', 'url': 'git+https://github.com/JacobFV/computerworld.git'},
    'bugs': 'https://github.com/JacobFV/computerworld/issues',
    'type': 'module',
    'main': './computerworld.cjs',
    'module': './computerworld.js',
    'browser': './computerworld.js',
    'types': './computerworld.d.ts',
    'exports': {
        '.': {
            'node': {'types': './computerworld.d.cts', 'default': './computerworld.cjs'},
            'types': './computerworld.d.ts',
            'default': './computerworld.js',
        },
        './computerworld_bg.wasm': './computerworld_bg.wasm',
        './worlds/*': './worlds/*',
        './fonts/*': './fonts/*',
        './package.json': './package.json',
    },
    'sideEffects': False,
    'engines': {'node': '>=18'},
}, indent=2) + '\n', encoding='utf-8')
(package / 'README.md').write_text(f'''# ComputerWorld

An ultra computer world simulator written in Rust. macOS, Windows 11, Ubuntu, iOS and
Android shells, real applications, a synthetic internet and byte-identical replay, in one
WebAssembly module: no server, no browser automation, no network.

```sh
npm install computerworld
```

```js
// Node: CommonJS or ES modules
const {{ World, engineVersion }} = require('computerworld');
const definition = require('computerworld/worlds/company-2026/world.json');

const world = new World(definition, 42n);
const env = world.environment({{ actor: 'alice', machines: ['alice-mac'],
  actions: ['terminal.v1', 'application.v1', 'pointer.v1', 'keyboard.v1'],
  observations: ['terminal.v1', 'semantic.v1'] }});
env.step([{{ family: 'terminal.v1', op: 'execute', machine: 'alice-mac', payload: {{ command: 'python3 -c "print(6 * 7)"' }} }}]);
const scene = env.scene(1440, 900);      // roles, names, geometry
const frame = env.render(1440, 900);     // or exact RGBA
const checkpoint = world.snapshot();     // fork it, replay it, diff it
```

```js
// Browser, through a bundler or an import map: the default export loads the Wasm
import init, {{ World }} from 'computerworld';
await init();
```

World handles are privileged; give acting agents only the environment handles you
configure. `fonts/` is the CJK and emoji font pack, installed on demand with
`installFont`. This is a 0.x release: pin the version, and keep your world, seed and
actions with your records. Documentation, the Python binding and the source are at
https://github.com/JacobFV/computerworld. Built from `{commit}`; see `notices/` for
third-party licenses.
''', encoding='utf-8')

packed = subprocess.check_output(['npm', 'pack', '--pack-destination', str(out), '--json'], cwd=package, text=True)
tarball = out / json.loads(packed)[0]['filename']

# The proof: an empty project somewhere else installs the tarball and runs a machine.
with tempfile.TemporaryDirectory() as project:
    subprocess.run(['npm', 'init', '-y'], cwd=project, check=True, stdout=subprocess.DEVNULL)
    subprocess.run(['npm', 'install', '--no-audit', '--no-fund', str(tarball)], cwd=project, check=True, stdout=subprocess.DEVNULL)
    drive = ("const world = new World(definition, 42n);"
             "const env = world.environment({actor:'alice',machines:['alice-mac'],actions:['terminal.v1'],observations:['terminal.v1']});"
             "const result = env.step([{family:'terminal.v1',op:'execute',machine:'alice-mac',payload:{command:'echo from-npm'}}]);"
             f"if (engineVersion() !== '{version}') throw new Error('engine is ' + engineVersion());"
             "if (!JSON.stringify(result).includes('from-npm')) throw new Error('the machine did not run the command');")
    (Path(project) / 'check.cjs').write_text(
        "const {World, engineVersion} = require('computerworld');"
        "const definition = require('computerworld/worlds/company-2026/world.json');" + drive, encoding='utf-8')
    (Path(project) / 'check.mjs').write_text(
        "import {World, engineVersion} from 'computerworld';"
        "import {createRequire} from 'node:module';"
        "const definition = createRequire(import.meta.url)('computerworld/worlds/company-2026/world.json');" + drive +
        # The browser entry cannot run here, but what `exports` hands a bundler can be checked.
        "import {readFileSync} from 'node:fs';"
        "const web = readFileSync(new URL('./node_modules/computerworld/computerworld.js', import.meta.url), 'utf8');"
        "if (!/as default|export default/.test(web) || web.includes('require(')) throw new Error('the browser entry is not the ES module');", encoding='utf-8')
    for script in ('check.cjs', 'check.mjs'):
        subprocess.run(['node', script], cwd=project, check=True)

print(json.dumps(dict(tarball=str(tarball), version=version, bytes=tarball.stat().st_size, sha256=digest(tarball)), indent=2))
