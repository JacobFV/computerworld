import json, hashlib, sys, re
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
assert any(re.fullmatch(r'window:\d+:maximize',n.get('interaction') or '') for n in e.scene(960,640)['nodes'])
for variant in data['variants']:
    runtime=World(variant['definition'],0)
    runtime.import_snapshot(variant['snapshot'])
    assert runtime.state_hash()==variant['stateHash'],variant['theme']
    image=runtime.session(variant['session']).render(variant['width'],variant['height'])
    actual=hashlib.sha256(image['rgba']).hexdigest()
    assert actual==variant['pixelHash'],(variant['theme'],actual,variant['pixelHash'])
print(json.dumps({'desktopPixelHash':pixel_hash,'stateHash':w.state_hash(),'pythonImportsWasmDesktop':True,'platformsVerified':[v['theme'] for v in data['variants']]}))
