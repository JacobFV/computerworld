"""Persistent Python -> native canonical Rust; no Node or subprocess per action.

Run: python examples/python/computer_interaction.py --output target/python-demo
Optional: --world world.json --machine alice-mac --compare target/javascript-demo
Agent targeting uses only its permitted scene. Snapshot/replay belongs to the owner.
"""
import argparse
import hashlib
import json
import re
from pathlib import Path
from computerworld import World

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--world', type=Path, default=ROOT / 'worlds/company-2026/world.json')
parser.add_argument('--machine', default='alice-mac')
parser.add_argument('--url', default='http://intranet.internal/')
parser.add_argument('--output', type=Path, default=ROOT / 'target/python-demo')
parser.add_argument('--compare', type=Path, help='JavaScript output directory to verify cross-language parity')
args = parser.parse_args()
definition = json.loads(args.world.read_text())
computer = next(c for c in definition['computers'] if c['id'] == args.machine)
definition.setdefault('metadata', {}).setdefault('desktop_themes', {})[args.machine] = 'virtual-macos-golden-gate'
world = World(definition, 42)
env = world.environment(dict(actor=computer['user'], machines=[args.machine],
    actions=['application.v1', 'keyboard.v1', 'pointer.v1', 'terminal.v1', 'browser.v1'],
    observations=['terminal.v1', 'semantic.v1']))
initial = world.snapshot()
actions = []
width, height = 960, 640


def step(family, op, payload):
    action = dict(family=family, op=op, machine=args.machine, payload=payload)
    result = env.step([action])
    assert result['outcomes'][0]['success'], result
    actions.append(action)
    return result


def target(pattern):
    # Structured scenes are acting-agent observations, not privileged world inspection.
    node = next(n for n in reversed(env.scene(width, height)['nodes'])
                if re.fullmatch(pattern, n.get('interaction') or ''))
    t = node.get('transform', dict(a=1024, b=0, c=0, d=1024, tx=0, ty=0))
    b = node['bounds']
    x, y = b['x'] + b['width']//2, b['y'] + b['height']//2
    return dict(x=int((t['a']*x+t['c']*y)/1024+t['tx']),
                y=int((t['b']*x+t['d']*y)/1024+t['ty']))


def pointer(op, point):
    return step('pointer.v1', op, dict(point, width=width, height=height, button=0))


def click(pattern):
    pointer('click', target(pattern))


def drag(pattern, dx, dy):
    start = target(pattern)
    end = dict(x=start['x']+dx, y=start['y']+dy)
    pointer('down', start)
    pointer('move', end)
    pointer('up', end)


click(r'shell:launch:terminal')
step('keyboard.v1', 'type', {'text': 'echo programmatic-computer-interaction'})
step('keyboard.v1', 'key', {'key': 'Enter'})
assert 'programmatic-computer-interaction' in json.dumps(env.observe())
drag(r'window:\d+:drag', 24, 20)
drag(r'window:\d+:resize:se', 18, 16)
click(r'shell:launch:browser')
step('browser.v1', 'navigate', {'url': args.url})
observation = env.observe()
scene = env.scene(width, height)  # Structured-only consumers stop here.
assert args.url in json.dumps(scene)
frame = env.render(width, height)  # Rasterization is explicit and optional.
rgba = frame['rgba']
pixel_hash = hashlib.sha256(rgba).hexdigest()
state_hash = world.state_hash()
# Owner/controller capability: do not expose World to an untrusted acting agent.
fork = world.fork(world.snapshot())
assert fork.state_hash() == state_hash
assert fork.session(env.id).observe() == observation
world.restore(initial)
for action in actions:
    assert env.step([action])['outcomes'][0]['success']
assert world.state_hash() == state_hash, 'Replay must reproduce semantic state'
assert env.observe() == observation
summary = dict(stateHash=state_hash, pixelHash=pixel_hash, steps=len(actions), width=width, height=height)
args.output.mkdir(parents=True, exist_ok=True)
for name, value in dict(observation=observation, scene=scene, actions=actions,
                        trajectory=world.trajectory(), summary=summary).items():
    (args.output / f'{name}.json').write_text(json.dumps(value, indent=2) + '\n')
(args.output / 'snapshot.json').write_text(world.export_snapshot())
(args.output / 'frame.rgba').write_bytes(rgba)
rgb = bytearray(width * height * 3)
for i in range(width * height):
    rgb[i*3:i*3+3] = rgba[i*4:i*4+3]
(args.output / 'frame.ppm').write_bytes(f'P6\n{width} {height}\n255\n'.encode() + rgb)
if args.compare:
    def load_javascript(name):
        # The demo JSON exporter tags JS BigInts rather than rounding node IDs.
        return json.loads((args.compare / name).read_text(),
                          object_hook=lambda v: int(v['$bigint']) if set(v) == {'$bigint'} else v)
    assert summary == load_javascript('summary.json')
    assert actions == load_javascript('actions.json')
    assert scene == load_javascript('scene.json')
    assert observation == load_javascript('observation.json')
    imported = World(definition, 0)
    imported.import_snapshot((args.compare / 'snapshot.json').read_text())
    assert imported.state_hash() == state_hash
    assert imported.session(env.id).observe() == observation
print(json.dumps(dict(output=str(args.output), **summary, replay=True, fork=True,
                      crossLanguageParity=bool(args.compare)), indent=2))
