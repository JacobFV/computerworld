"""Cross-binding parity: import a Node/Wasm desktop checkpoint and re-render it in Python.

Both bindings run the same Rust engine, so the same checkpoint must yield the same
semantic state hash and the same RGBA bytes on five OS themes.

Run: python examples/python/desktop_pixels.py [checkpoint.json]

The checkpoint is produced by the Wasm side, not by this script:

    bash scripts/build-wasm.sh
    node scripts/checks/smoke-desktop-pixels.cjs target/binding-checks/desktop.json

Checkpoint, wheel and Wasm bundle must all be built from the same revision.
`scripts/smoke-bindings.sh` does that in order; see docs/determinism.md.
"""
import hashlib
import json
import re
import sys
from pathlib import Path

from computerworld import World

checkpoint = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2] / 'target/binding-checks/desktop.json'
if not checkpoint.exists():
    raise SystemExit(f'No checkpoint at {checkpoint}. Generate one with:\n'
                     '  bash scripts/build-wasm.sh && node scripts/checks/smoke-desktop-pixels.cjs')
data = json.loads(checkpoint.read_text(encoding='utf-8'))
w = World(data['definition'], 0)
try:
    w.import_snapshot(data['snapshot'])
except ValueError as e:
    # Engine, checkpoint and wheel are one compatibility unit; a mismatch here is
    # almost always a stale artifact rather than a real divergence.
    raise SystemExit(f'{checkpoint} is not compatible with the installed binding ({e}).\n'
                     'Rebuild both from the same revision: bash scripts/smoke-bindings.sh') from None
assert w.state_hash() == data['stateHash']
e = w.session(data['session'])
frame = e.render(960, 640)
pixel_hash = hashlib.sha256(frame['rgba']).hexdigest()
assert pixel_hash == data['pixelHash'], (pixel_hash, data['pixelHash'])
assert any(re.fullmatch(r'window:\d+:maximize', n.get('interaction') or '') for n in e.scene(960, 640)['nodes'])
for variant in data['variants']:
    runtime = World(variant['definition'], 0)
    runtime.import_snapshot(variant['snapshot'])
    assert runtime.state_hash() == variant['stateHash'], variant['theme']
    image = runtime.session(variant['session']).render(variant['width'], variant['height'])
    actual = hashlib.sha256(image['rgba']).hexdigest()
    assert actual == variant['pixelHash'], (variant['theme'], actual, variant['pixelHash'])
print(json.dumps({'desktopPixelHash': pixel_hash, 'stateHash': w.state_hash(),
                  'pythonImportsWasmDesktop': True,
                  'platformsVerified': [v['theme'] for v in data['variants']]}))
