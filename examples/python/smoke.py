"""Execute Python -> canonical Rust directly, optionally verify WASM parity."""
import json
from pathlib import Path
import sys
from computerworld import World
root = Path(__file__).resolve().parents[2]
definition = json.loads((root / 'worlds/company-2026/world.json').read_text(encoding='utf-8'))
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
    wasm = json.loads(Path(sys.argv[1]).read_text(encoding='utf-8'))
    assert wasm['hash'] == after_hash, (wasm['hash'],after_hash)
    imported.import_snapshot(wasm['snapshot'])
    assert imported.state_hash() == after_hash
    imported.import_snapshot(wasm['topologyCheckpoint'])
    assert imported.state_hash() == wasm['topologyHash']
    assert any(c['id'] == 'test-phone' for c in imported.definition()['computers'])
dynamic = World(definition,42)
computer = dict(next(c for c in definition['computers'] if c['id']=='carol-ubuntu'),id='test-phone',node='test-phone',address='10.0.0.77')
dynamic.add_computer(computer,dict(id='test-phone',address='10.0.0.77',zone='local'),[{'from':'test-phone','to':'alice-mac','bidirectional':True,'latency_us':0,'loss_per_million':0}])
assert any(c['id']=='test-phone' for c in dynamic.definition()['computers'])
phone = dynamic.environment(dict(config,actor=computer['user'],machines=['test-phone']))
assert phone.step([dict(family='terminal.v1',op='execute',machine='test-phone',payload={'command':'echo mobile'})])['outcomes'][0]['success']
topology_hash = dynamic.state_hash()
topology_snapshot = dynamic.snapshot()
dynamic.remove_computer('test-phone')
assert not any(c['id']=='test-phone' for c in dynamic.definition()['computers'])
dynamic.restore(topology_snapshot)
assert dynamic.state_hash() == topology_hash
if len(sys.argv)>1:
    assert topology_hash == wasm['topologyHash']
dynamic.reset(42)
assert not any(c['id']=='test-phone' for c in dynamic.definition()['computers'])
world.reset(42)
assert world.state_hash() == initial_hash
assert env.observe() == world.session(env.id).observe()
print(json.dumps(dict(runtime='python-rust',hash=after_hash,rgba_bytes=len(frame['rgba']))))
