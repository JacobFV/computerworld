"""Execute Python -> canonical Rust directly, optionally verify WASM parity."""
import json
from pathlib import Path
import sys
from computerworld import World
root = Path(__file__).resolve().parents[2]
definition = json.loads((root / 'worlds/company-2026/world.json').read_text())
config = dict(actor='alice', machines=['alice-mac'], actions=['terminal.v1','browser.v1'], observations=['terminal.v1','semantic.v1'])
actions = [dict(family='terminal.v1',op='execute',machine='alice-mac',payload={'command':'echo binding-parity'})]
world = World(definition,42)
env = world.environment(config)
before = world.snapshot()
initial_hash = world.state_hash()
result = env.step(actions)
assert result['outcomes'][0]['success'], result
assert 'binding-parity' in json.dumps(result)
after_hash = world.state_hash()
portable = world.export_snapshot()
world.restore(before)
assert env.step(actions) == result
assert world.state_hash() == after_hash
assert world.fork(world.snapshot()).state_hash() == after_hash
imported = World(definition,0)
imported.import_snapshot(portable)
assert imported.state_hash() == after_hash
assert not hasattr(env,'inspect') and not hasattr(env,'snapshot')
assert not env.step([dict(family='private.v1',op='inspect',machine='alice-mac')])['outcomes'][0]['success']
frame = env.render(320,240)
assert isinstance(frame['rgba'],bytes) and len(frame['rgba']) == 320*240*4
assert world.trajectory()
try:
    env.step('invalid')
except ValueError:
    pass
else:
    raise AssertionError('invalid actions accepted')
if len(sys.argv)>1:
    wasm = json.loads(Path(sys.argv[1]).read_text())
    assert wasm['hash'] == after_hash, (wasm['hash'],after_hash)
    imported.import_snapshot(wasm['snapshot'])
    assert imported.state_hash() == after_hash
world.reset(42)
assert world.state_hash() == initial_hash
assert env.observe() == world.session(env.id).observe()
print(json.dumps(dict(runtime='python-rust',hash=after_hash,rgba_bytes=len(frame['rgba']))))
