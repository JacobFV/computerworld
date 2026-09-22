#!/usr/bin/env node
// Render a static-site JSON through the in-world browser with the window maximised and
// the desktop sized so the page pane is exactly <width>x<height>; writes <out>.png (the
// page pane), <out>.desktop.png (the whole screen) and <out>.scene.json for compare.mjs.
//   node render-page.mjs <site.json> <out.png> [--width 1280] [--height 800]
import {readFile, writeFile} from 'node:fs/promises';
import {createRequire} from 'node:module';
import {fileURLToPath} from 'node:url';
const root = fileURLToPath(new URL('../../..', import.meta.url));
const {encodePNG} = await import(`${root}/scripts/dom-to-site/png.mjs`);
const args = process.argv.slice(2);
const flag = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
const [siteFile, out] = args.filter((a, i) => !a.startsWith('--') && !args[i - 1]?.startsWith('--'));
const width = Number(flag('--width', 1280)), height = Number(flag('--height', 800));
const require = createRequire(import.meta.url);
const wasm = require(`${root}/pkg/node/computerworld.js`);
const site = JSON.parse(await readFile(siteFile, 'utf8'));
const service = {...site};
for (const key of ['search_entries', 'authority_overrides', 'place']) delete service[key];
const address = site.place?.address ?? '203.0.113.250';
const definition = {
  schema_version: 1, id: `google-ceiling-${site.id}`,
  profiles: [{id: 'ubuntu', name: 'Ubuntu 24.04 workstation', family: 'linux', home: '/home/{user}', case_sensitive: true, shell: 'posix'}],
  computers: [{id: 'workstation', profile: 'ubuntu', address: '10.0.0.10', user: 'ada', initial_files: {}, installed_apps: ['browser'], packages: []}],
  network: {
    nodes: [{id: 'workstation', address: '10.0.0.10', zone: 'local'}, {id: site.node, address, zone: 'internet'}],
    links: [{from: 'workstation', to: site.node, bidirectional: true, latency_us: 1500, loss_per_million: 0}],
    dns: site.domains.map(name => ({name, address, ttl_us: 60000000, resolver: site.node})),
    routes: [], gateway: {allow_local: true, allow_internet: true, allow_host: false},
  },
  services: [service], metadata: {desktop_themes: {workstation: 'ubuntu'}},
};
let world;
try { world = new wasm.World(definition, 1); } catch (e) { console.error(`world rejected the site: ${e.message ?? e}`); process.exit(1); }
const env = world.environment({actor: 'ada', machines: ['workstation'], actions: ['browser.v1', 'pointer.v1', 'application.v1'], observations: ['semantic.v1']});
const nav = env.step([{family: 'browser.v1', op: 'navigate', machine: 'workstation', payload: {url: `http://${site.domains[0]}/`}}]);
if (!nav.outcomes[0].success) { console.error(`navigate failed: ${JSON.stringify(nav.outcomes[0])}`); process.exit(1); }
const max = env.step([{family: 'application.v1', op: 'maximize', machine: 'workstation', payload: {}}]);
if (!max.outcomes[0].success) console.error(`maximize: ${JSON.stringify(max.outcomes[0])}`);
const paneOf = (w, h) => (env.scene(w, h).scrolls ?? []).find(s => /pane:page$/.test(String(s.target)))?.bounds;
// Probe once, then grow the screen by the chrome the shell and the browser add.
let pane = paneOf(width, height);
if (!pane) { console.error('no page pane in the scene'); process.exit(1); }
let W = width + (width - Math.round(pane.width)), H = height + (height - Math.round(pane.height));
pane = paneOf(W, H);
const frame = env.render(W, H);
const img = {width: frame.width, height: frame.height, rgba: frame.rgba};
const crop = {x: Math.round(pane.x), y: Math.round(pane.y), width: Math.round(pane.width), height: Math.round(pane.height)};
const rgba = new Uint8Array(crop.width * crop.height * 4);
for (let y = 0; y < crop.height; y++) rgba.set(img.rgba.subarray(((crop.y + y) * img.width + crop.x) * 4, ((crop.y + y) * img.width + crop.x + crop.width) * 4), y * crop.width * 4);
await writeFile(out, encodePNG({width: crop.width, height: crop.height, rgba}));
await writeFile(out.replace(/\.png$/, '') + '.desktop.png', encodePNG(img));
// compare.mjs reads <out>.page.png next to <out>.scene.json; the pane is the page, so
// they are the same pixels, with node bounds shifted to page coordinates.
await writeFile(out.replace(/\.png$/, '') + '.page.png', encodePNG({width: crop.width, height: crop.height, rgba}));
// Page nodes are already in page-pane coordinates; shell and window-chrome nodes (all of
// which carry a shell: or window:N: interaction that is not window:N:content:) are in
// screen coordinates and are not the page, so they are dropped.
const pageNode = n => (typeof n.interaction === 'string') ? /^window:\d+:content:(?!pane:|shell:)/.test(n.interaction) : !/^(button|link)$/.test(n.semantic.role);
const nodes = (env.scene(W, H).nodes ?? []).filter(n => n.semantic && pageNode(n) && !/^browser — /.test(n.semantic.label ?? '')).map(n => ({id: String(n.id), role: n.semantic.role, label: n.semantic.label, value: n.semantic.value, bounds: n.bounds, interaction: typeof n.interaction === 'string' ? n.interaction.replace(/^window:\d+:content:/, '') : null}));
await writeFile(out.replace(/\.png$/, '') + '.scene.json', JSON.stringify({width: crop.width, height: crop.height, scroll: 0, crop, screen: {W, H}, url: `http://${site.domains[0]}/`, nodes}, null, 1) + '\n');
const observed = env.observe();
console.log(`${out}: page pane ${crop.width}x${crop.height} at ${crop.x},${crop.y} on a ${W}x${H} screen, ${nodes.length} semantic nodes`);
if (observed?.channels?.['semantic.v1']?.browser?.error) console.log(`browser error: ${JSON.stringify(observed.channels['semantic.v1'].browser.error)}`);
