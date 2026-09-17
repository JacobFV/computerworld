import json, hashlib, sys
from pathlib import Path
from computerworld import World
checkpoint = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parents[2] / 'target/binding-checks/desktop.json'
with checkpoint.open() as f: data=json.load(f)
w=World(data['definition'],0)
w.import_snapshot(data['snapshot'])
assert w.state_hash()==data['stateHash']
e=w.session(data['session'])
frame=e.render(960,640)
pixel_hash=hashlib.sha256(frame['rgba']).hexdigest()
assert pixel_hash==data['pixelHash'],(pixel_hash,data['pixelHash'])
assert any(n.get('interaction')=='shell:maximize' for n in e.scene(960,640)['nodes'])
print(json.dumps({'desktopPixelHash':pixel_hash,'stateHash':w.state_hash(),'pythonImportsWasmDesktop':True}))
