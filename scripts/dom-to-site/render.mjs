#!/usr/bin/env node
// Load a converted site into the in-world browser (the Wasm build in pkg/node) and save
// what the actor sees, plus the semantic scene, so the result can be compared with the
// capture. This is also the authoritative validation: the World constructor runs every
// service's `initialize`, which parses each page through the Rust serde types.
//
//   node scripts/dom-to-site/render.mjs <site.json> <out.png> [--width 1280] [--height 800] [--scroll 0]
//
// Writes <out.png> (the browser's page area at 1:1) and <out>.scene.json (the page's
// semantic nodes: role, label, bounds). Needs pkg/node (scripts/build-wasm.sh).
import {readFile, writeFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
import {encodePNG} from './png.mjs';

const args = process.argv.slice(2);
const flag = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
const [siteFile, out] = args.filter((a, i) => !a.startsWith('--') && !args[i - 1]?.startsWith('--'));
if (!siteFile || !out) { console.error('usage: render.mjs <site.json> <out.png> [--width 1280] [--height 800] [--scroll 0]'); process.exit(2); }
const width = Number(flag('--width', 1280)), height = Number(flag('--height', 800)), scroll = Number(flag('--scroll', 0));
const root = fileURLToPath(new URL('../..', import.meta.url));
const require = createRequire(import.meta.url);
let wasm;
try { wasm = require(`${root}/pkg/node/computerworld.js`); } catch (e) { console.error(`pkg/node is not built (${e.message.split('\n')[0]}); run scripts/build-wasm.sh`); process.exit(4); }

const site = JSON.parse(await readFile(siteFile, 'utf8'));
const service = {...site};
for (const key of ['search_entries', 'authority_overrides', 'network_node']) delete service[key];
const address = site.network_node?.address ?? '203.0.113.250';
/// One workstation and the site, on documentation-range addresses, nothing else.
const definition = {
  schema_version: 1,
  id: `dom-to-site-${site.id}`,
  profiles: [{id: 'ubuntu', name: 'Ubuntu 24.04 workstation', family: 'linux', home: '/home/{user}', case_sensitive: true, shell: 'posix'}],
  computers: [{id: 'workstation', profile: 'ubuntu', address: '10.0.0.10', user: 'ada', initial_files: {}, installed_apps: ['browser'], packages: []}],
  network: {
    nodes: [{id: 'workstation', address: '10.0.0.10', zone: 'local'}, {id: site.node, address, zone: 'internet'}],
    links: [{from: 'workstation', to: site.node, bidirectional: true, latency_us: 1500, loss_per_million: 0}],
    dns: site.domains.map(name => ({name, address, ttl_us: 60000000, resolver: site.node})),
    routes: [],
    gateway: {allow_local: true, allow_internet: true, allow_host: false},
  },
  services: [service],
  metadata: {desktop_themes: {workstation: 'ubuntu'}},
};
let world;
try { world = new wasm.World(definition, 1); } catch (e) { console.error(`world rejected the site: ${e.message ?? e}`); process.exit(1); }
const env = world.environment({actor: 'ada', machines: ['workstation'], actions: ['browser.v1', 'pointer.v1'], observations: ['semantic.v1']});
const nav = env.step([{family: 'browser.v1', op: 'navigate', machine: 'workstation', payload: {url: `http://${site.domains[0]}/`}}]);
if (!nav.outcomes[0].success) { console.error(`navigate failed: ${JSON.stringify(nav.outcomes[0])}`); process.exit(1); }
if (scroll) env.step([{family: 'browser.v1', op: 'scroll', machine: 'workstation', payload: {y: scroll}}]);

// The browser window is one window on a desktop; find the page pane so the saved image is
// the page alone, at the same scale as the capture.
const scene = env.scene(width, height);
const nodes = scene.nodes ?? [];
const pageNodes = nodes.filter(n => n.semantic && ['heading', 'text', 'link', 'button', 'textbox', 'img'].includes(n.semantic.role));
const errors = nav.outcomes[0];
const frame = env.render(width, height);
const frameImg = {width: frame.width, height: frame.height, rgba: frame.rgba};
// Page area: the union of the page's semantic nodes, widened to the browser's content pane
// when the scene names one; otherwise the whole frame.
let crop = null;
const pane = nodes.find(n => typeof n.id === 'string' ? /pane:(browser|page)/.test(n.id) : false) ?? nodes.find(n => n.interaction && /^pane:/.test(String(n.interaction)));
if (pane) crop = pane.bounds;
else if (pageNodes.length) {
  const xs = pageNodes.map(n => n.bounds.x), ys = pageNodes.map(n => n.bounds.y);
  const xe = pageNodes.map(n => n.bounds.x + n.bounds.width), ye = pageNodes.map(n => n.bounds.y + n.bounds.height);
  crop = {x: Math.max(0, Math.min(...xs) - 16), y: Math.max(0, Math.min(...ys) - 16), width: Math.min(width, Math.max(...xe) + 16) - Math.max(0, Math.min(...xs) - 16), height: Math.min(height, Math.max(...ye) + 16) - Math.max(0, Math.min(...ys) - 16)};
}
function cropImage(img, r) {
  const rgba = new Uint8Array(r.width * r.height * 4);
  for (let y = 0; y < r.height; y++) rgba.set(img.rgba.subarray(((r.y + y) * img.width + r.x) * 4, ((r.y + y) * img.width + r.x + r.width) * 4), y * r.width * 4);
  return {width: r.width, height: r.height, rgba};
}
await writeFile(out, encodePNG(frameImg));
if (crop) await writeFile(out.replace(/\.png$/, '') + '.page.png', encodePNG(cropImage(frameImg, {x: Math.round(crop.x), y: Math.round(crop.y), width: Math.round(crop.width), height: Math.round(crop.height)})));
const semantic = nodes.filter(n => n.semantic).map(n => ({id: n.id, role: n.semantic.role, label: n.semantic.label, value: n.semantic.value, bounds: n.bounds, interaction: n.interaction ?? null}));
await writeFile(out.replace(/\.png$/, '') + '.scene.json', JSON.stringify({width, height, scroll, crop, url: `http://${site.domains[0]}/`, nodes: semantic}, null, 1) + '\n');
const observed = env.observe();
console.log(`${out}: ${frame.width}x${frame.height} frame, ${semantic.length} semantic nodes, page area ${crop ? `${Math.round(crop.width)}x${Math.round(crop.height)} at ${Math.round(crop.x)},${Math.round(crop.y)}` : 'unknown'}, navigate ${errors.success ? 'ok' : 'failed'}`);
if (observed?.channels?.['semantic.v1']?.browser?.error) console.log(`browser error: ${JSON.stringify(observed.channels['semantic.v1'].browser.error)}`);
