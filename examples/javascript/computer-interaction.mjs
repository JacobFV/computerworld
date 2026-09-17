/** Persistent JavaScript -> Wasm -> canonical Rust computer interaction.
 * Run: node examples/javascript/computer-interaction.mjs --output target/javascript-demo
 * Optional: --world world.json --machine alice-mac --binding /path/computerworld.js
 * The agent receives only env; owner-only snapshot/replay is demonstrated separately.
 */
import assert from 'node:assert/strict';
import {readFileSync, writeFileSync, mkdirSync} from 'node:fs';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';
import {createRequire} from 'node:module';
import {createHash} from 'node:crypto';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..');
const options = {};
for (let i = 2; i < process.argv.length; i += 2) {
  assert(['--world', '--machine', '--output', '--binding', '--url'].includes(process.argv[i]), `Unknown option ${process.argv[i]}`);
  assert(process.argv[i + 1], `Missing value for ${process.argv[i]}`);
  options[process.argv[i].slice(2)] = process.argv[i + 1];
}
const {World} = createRequire(import.meta.url)(resolve(options.binding ?? `${root}/pkg/node/computerworld.js`));
const definition = JSON.parse(readFileSync(options.world ?? `${root}/worlds/company-2026/world.json`, 'utf8'));
const machine = options.machine ?? 'alice-mac';
const computer = definition.computers.find(c => c.id === machine);
assert(computer, `No computer ${machine}`);
// This example chooses a desktop presentation through the public world schema.
definition.metadata ??= {};
definition.metadata.desktop_themes = {...definition.metadata.desktop_themes, [machine]: 'virtual-macos-golden-gate'};
const world = new World(definition, 42);
const env = world.environment({actor: computer.user, machines: [machine],
  actions: ['application.v1', 'keyboard.v1', 'pointer.v1', 'terminal.v1', 'browser.v1'],
  observations: ['terminal.v1', 'semantic.v1']});
const initial = world.snapshot();
const actions = [];
const width = 960, height = 640;
function step(family, op, payload) {
  const action = {family, op, machine, payload};
  const result = env.step([action]);
  assert(result.outcomes[0].success, JSON.stringify(result));
  actions.push(action);
  return result;
}
function target(pattern) {
  // Targets come from the acting agent's permitted structured scene, never inspection.
  const node = env.scene(width, height).nodes.findLast(n => pattern.test(n.interaction ?? ''));
  assert(node, `Missing visible target ${pattern}`);
  const t = node.transform ?? {a: 1024, b: 0, c: 0, d: 1024, tx: 0, ty: 0};
  const x = node.bounds.x + Math.floor(node.bounds.width / 2);
  const y = node.bounds.y + Math.floor(node.bounds.height / 2);
  return {x: Math.trunc((t.a*x+t.c*y)/1024+t.tx), y: Math.trunc((t.b*x+t.d*y)/1024+t.ty)};
}
function pointer(op, point) { return step('pointer.v1', op, {...point, width, height, button: 0}); }
function click(pattern) { pointer('click', target(pattern)); }
function drag(pattern, dx, dy) {
  const from = target(pattern), to = {x: from.x + dx, y: from.y + dy};
  pointer('down', from); pointer('move', to); pointer('up', to);
}
click(/^shell:launch:terminal$/);
step('keyboard.v1', 'type', {text: 'echo programmatic-computer-interaction'});
step('keyboard.v1', 'key', {key: 'Enter'});
assert(JSON.stringify(env.observe()).includes('programmatic-computer-interaction'));
drag(/^window:\d+:drag$/, 24, 20);
drag(/^window:\d+:resize:se$/, 18, 16);
click(/^shell:launch:browser$/);
step('browser.v1', 'navigate', {url: options.url ?? 'http://intranet.internal/'});
const observation = env.observe();
const scene = env.scene(width, height); // No rasterization needed for structured agents.
assert(JSON.stringify(scene, (_key, v) => typeof v === 'bigint' ? v.toString() : v).includes(options.url ?? 'http://intranet.internal/'));
const frame = env.render(width, height); // Vision agents opt into pixels.
const rgba = frame.rgba;
const pixelHash = createHash('sha256').update(rgba).digest('hex');
frame.free();
const stateHash = world.stateHash();
// Owner/controller operations. Do not give this handle to an untrusted acting agent.
const final = world.snapshot();
const fork = world.fork(final);
assert.equal(fork.stateHash(), stateHash);
const forkEnv = fork.session(env.id);
assert.deepEqual(forkEnv.observe(), observation);
forkEnv.free(); fork.free(); final.free();
world.restore(initial);
for (const action of actions) assert(env.step([action]).outcomes[0].success);
assert.equal(world.stateHash(), stateHash, 'Replay must reproduce semantic state');
assert.deepEqual(env.observe(), observation);
initial.free();
const out = resolve(options.output ?? `${root}/target/javascript-demo`);
mkdirSync(out, {recursive: true});
// Preserve full-width scene IDs when exporting JavaScript BigInt values.
const json = value => JSON.stringify(value, (_key, v) => typeof v === 'bigint' ? {$bigint: v.toString()} : v, 2) + '\n';
for (const [name, value] of Object.entries({observation, scene, actions, trajectory: world.trajectory(), summary: {stateHash, pixelHash, steps: actions.length, width, height}})) {
  writeFileSync(`${out}/${name}.json`, json(value));
}
writeFileSync(`${out}/snapshot.json`, world.exportSnapshot());
writeFileSync(`${out}/frame.rgba`, rgba);
const rgb = Buffer.alloc(width * height * 3);
for (let i = 0; i < width * height; i++) rgb.set(rgba.subarray(i*4, i*4+3), i*3);
writeFileSync(`${out}/frame.ppm`, Buffer.concat([Buffer.from(`P6\n${width} ${height}\n255\n`), rgb]));
console.log(json({output: out, stateHash, pixelHash, steps: actions.length, replay: true, fork: true}));
env.free(); world.free();
