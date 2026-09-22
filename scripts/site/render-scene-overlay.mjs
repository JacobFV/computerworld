/** The pair of pictures that argue the scene graph: one frame, and the same frame with
 * everything `scene()` already knows drawn on top of it.
 *
 * Nothing here is hand-drawn. The frame is `env.render(w, h)` — the bytes the machine
 * really paints — and every box is a node out of `env.scene(w, h)` at the same viewport,
 * put through the node's own transform and clip. The two images come out of one canvas
 * at one size, so a crossfade or a slider between them lines up to the pixel.
 *
 *   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
 *     node scripts/site/render-scene-overlay.mjs [options]
 *
 * Pixel output is not stable across engine releases: regenerate both images together
 * when the renderer or the scene changes, and never one without the other.
 *
 * The machines need the Wasm bundle in site/pkg/ (see site/README.md).
 */
import {spawn} from 'node:child_process';
import {mkdir, writeFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));

const usage = `Render a scene twice: the frame, and the frame under its own scene graph.

  node scripts/site/render-scene-overlay.mjs [options]

  --scene <id>       scene in site/scenes/ to boot        (default: github)
  --machine <id>     which of its machines to picture     (default: the first)
  --width <px>       render viewport                      (default: the machine's own)
  --height <px>
  --scale <n>        deliver at this multiple of the render size   (default: 1)
                     The scene is always asked for at the render size, so the boxes are
                     the coordinates it returned; anything but 1 resamples them.
  --quality <0..1>   JPEG quality for both images         (default: 0.82)
  --labels <n>       how many widget names to call out    (default: 16)
  --frame <path>     where the plain frame goes     (default: site/media/t2-frame.jpg)
  --overlay <path>   where the overlaid frame goes  (default: site/media/t2-overlay.jpg)
  --dump <path>      also write the scene JSON there, for inspection
  --help

The environment needs PLAYWRIGHT_MODULE (there is no playwright package in the project)
and CHROME_BIN. The bundle in site/pkg/ is what is booted, so it is what is pictured.
`;

const options = {
  scene: 'github', machine: null, width: null, height: null, scale: 1,
  quality: 0.82, labels: 16,
  frame: `${root}site/media/t2-frame.jpg`,
  overlay: `${root}site/media/t2-overlay.jpg`,
  dump: null,
};
const numbers = new Set(['width', 'height', 'scale', 'quality', 'labels']);
for (let i = 2; i < process.argv.length; i++) {
  const argument = process.argv[i];
  if (argument === '--help' || argument === '-h') { process.stdout.write(usage); process.exit(0); }
  const name = argument.startsWith('--') ? argument.slice(2) : null;
  if (!name || !(name in options)) { process.stderr.write(`unknown option ${argument}\n\n${usage}`); process.exit(2); }
  const value = process.argv[++i];
  if (value === undefined) { process.stderr.write(`${argument} wants a value\n`); process.exit(2); }
  options[name] = numbers.has(name) ? Number(value) : value;
}

const {cast} = await import(new URL('../../site/cast.js', import.meta.url));
const scene = cast.find(s => s.id === options.scene);
if (!scene) {
  process.stderr.write(`no scene ${options.scene}. The cast is:\n  ${cast.map(s => s.id).join('\n  ')}\n`);
  process.exit(2);
}
const machine = options.machine ? scene.machines.find(m => m.id === options.machine) : scene.machines[0];
if (!machine) {
  process.stderr.write(`${scene.id} has no machine ${options.machine}: ${scene.machines.map(m => m.id).join(', ')}\n`);
  process.exit(2);
}
const width = options.width ?? machine.size[0];
const height = options.height ?? machine.size[1];

/** A port nobody is on, so two of these runs never share a server. */
const freePort = () => new Promise((resolve, reject) => {
  const probe = createServer();
  probe.on('error', reject);
  probe.listen(0, '127.0.0.1', () => {
    const {port} = probe.address();
    probe.close(() => resolve(port));
  });
});
const port = await freePort();

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const server = spawn(process.execPath, [`${root}scripts/site/serve-site.mjs`, String(port)], {stdio: 'ignore'});
const up = async () => {
  for (let tries = 0; tries < 50; tries++) {
    try { if ((await fetch(`http://127.0.0.1:${port}/`)).ok) return true; } catch {}
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  return false;
};
if (!await up()) {
  server.kill();
  console.error(`the site server did not come up on ${port}`);
  process.exit(1);
}

/** What runs in the tab. It boots the machine the way site/engine.js boots one — the same
 * module, the same world definition, the same opening actions — then asks it for a frame
 * and for the scene behind that frame, and draws the second on a copy of the first. It
 * does this on the page's own thread rather than in a worker, as the site does: there is
 * nothing here to keep answerable while it works. */
async function picture({sceneId, machineId, width, height, scale, quality, labels}) {
  const {default: init, World, installFont, fontPackStatus} = await import('/pkg/computerworld.js');
  await init();
  const {default: definition} = await import('/generated/world-definition.js');
  const {default: scene} = await import(`/scenes/${sceneId}.js`);

  // The bundle ships without the CJK and emoji faces. A picture of labelled text should
  // hold no boxes where a glyph belongs, so unlike a running tab this waits for all of
  // them before the frame is taken.
  await Promise.all(fontPackStatus().files.filter(f => !f.installed).map(async f => {
    const response = await fetch(`/pkg/${f.path}`);
    if (response.ok) installFont(new Uint8Array(await response.arrayBuffer()));
  }));

  // site/engine.js, `populate`: the reference company plus this scene's machines, each a
  // copy of a reference computer on an address of its own.
  const world = structuredClone(definition);
  let host = 10;
  for (const {id, like} of scene.machines) {
    const computer = structuredClone(world.computers.find(c => c.id === like));
    Object.assign(computer, {id, address: `10.0.7.${host++}`});
    world.computers.push(computer);
    world.network.nodes.push({id, address: computer.address, zone: 'local'});
    world.network.links.push({from: 'app-server', to: id, bidirectional: true, latency_us: 10, loss_per_million: 0});
    world.metadata.device_presentations[id] = world.metadata.device_presentations[like];
  }

  const actions = ['terminal.v1', 'browser.v1', 'keyboard.v1', 'pointer.v1', 'application.v1', 'filesystem.v1', 'http.v1'];
  const observations = ['terminal.v1', 'semantic.v1', 'browser.v1'];
  const running = new World(world, 2026);
  const envs = scene.machines.map(({id, size}) => {
    const {user} = running.definition().computers.find(c => c.id === id);
    return {id, size, env: running.environment({actor: user, machines: [id], actions, observations, action_budget: 1_000_000})};
  });

  // site/engine.js, `hands`: the verbs a scene's opening is written against.
  const hands = (env, machine, size) => {
    const step = (family, op, payload) => env.step([{family, op, machine, payload}]);
    const [w, h] = size;
    const find = target => env.scene(w, h).nodes
      .filter(n => n.interaction === target || n.interaction?.endsWith(`:content:${target}`))
      .sort((a, b) => (a.z ?? 0) - (b.z ?? 0))
      .pop();
    const clickAt = (x, y) => step('pointer.v1', 'click', {x, y, width: w, height: h});
    return {
      step, clickAt,
      launch: (kind, argument) => step('application.v1', 'launch', argument ? {kind, argument} : {kind}),
      sh: command => step('terminal.v1', 'execute', {command}),
      visit: url => step('browser.v1', 'navigate', {url}),
      type: text => step('keyboard.v1', 'type', {text}),
      key: key => step('keyboard.v1', 'key', {key}),
      click(target) {
        const hit = find(target);
        if (!hit) return false;
        const t = hit.transform ?? {a: 1024, b: 0, c: 0, d: 1024, tx: 0, ty: 0};
        const cx = hit.bounds.x + hit.bounds.width / 2;
        const cy = hit.bounds.y + hit.bounds.height / 2;
        clickAt(Math.round((t.a * cx + t.c * cy) / 1024 + t.tx), Math.round((t.b * cx + t.d * cy) / 1024 + t.ty));
        return true;
      },
    };
  };
  scene.open(...envs.map(m => hands(m.env, m.id, m.size)));
  if (scene.sync) {
    const subject = envs.find(m => m.id === machineId);
    scene.sync(envs.filter(m => m !== subject).map(m => hands(m.env, m.id, m.size)));
  }

  // One machine, one viewport, both asked in the same breath: the pixels and the geometry
  // behind them describe the same instant.
  const {env} = envs.find(m => m.id === machineId);
  const frame = env.render(width, height);
  const graph = env.scene(width, height);

  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  const context = canvas.getContext('2d');
  context.putImageData(new ImageData(new Uint8ClampedArray(frame.rgba), width, height), 0, 0);
  frame.free();
  // Both pictures come off this one canvas, so the overlay cannot drift from the frame by
  // a pixel: the second is the first with more paint on it.
  const deliver = () => {
    if (scale === 1) return canvas.toDataURL('image/jpeg', quality).split(',')[1];
    const out = document.createElement('canvas');
    out.width = Math.round(width * scale);
    out.height = Math.round(height * scale);
    const to = out.getContext('2d');
    to.imageSmoothingQuality = 'high';
    to.drawImage(canvas, 0, 0, out.width, out.height);
    return out.toDataURL('image/jpeg', quality).split(',')[1];
  };
  const plain = deliver();

  /** Where a node lands on the screen: its bounds through its own transform (1/1024
   * units), cut by its own clip, cut by the viewport. Nothing here is measured off the
   * picture — this is the same arithmetic the renderer does. */
  const place = node => {
    const t = node.transform ?? {a: 1024, b: 0, c: 0, d: 1024, tx: 0, ty: 0};
    const b = node.bounds;
    const xs = [], ys = [];
    for (const [px, py] of [[b.x, b.y], [b.x + b.width, b.y], [b.x, b.y + b.height], [b.x + b.width, b.y + b.height]]) {
      xs.push((t.a * px + t.c * py) / 1024 + t.tx);
      ys.push((t.b * px + t.d * py) / 1024 + t.ty);
    }
    let left = Math.min(...xs), top = Math.min(...ys), right = Math.max(...xs), bottom = Math.max(...ys);
    for (const cut of [node.clip, {x: 0, y: 0, width, height}]) {
      if (!cut) continue;
      left = Math.max(left, cut.x); top = Math.max(top, cut.y);
      right = Math.min(right, cut.x + cut.width); bottom = Math.min(bottom, cut.y + cut.height);
    }
    return right - left < 2 || bottom - top < 2 ? null : {x: left, y: top, w: right - left, h: bottom - top};
  };
  const close = (a, b) => Math.abs(a.x - b.x) < 3 && Math.abs(a.y - b.y) < 3 &&
    Math.abs(a.w - b.w) < 3 && Math.abs(a.h - b.h) < 3;

  /** Painted, but not seen: the desktop and its icons go on before the windows and are
   * covered by them. The scene says so — those nodes belong to no window and sit below
   * the window layer — and a box drawn over one would point at nothing. */
  const buried = (node, box) => node.window == null && (node.z ?? 0) < 100 &&
    graph.windows.some(w => {
      const r = w.minimized ? null : (w.exposed ?? w.bounds);
      return r && box.x >= r.x && box.y >= r.y && box.x + box.w <= r.x + r.width && box.y + box.h <= r.y + r.height;
    });

  // Every run of painted text, with the string it paints. Shells draw a label and its
  // outline as several nodes a pixel apart; one box is drawn where they coincide.
  const runs = [];
  for (const node of graph.nodes) {
    if (!['ui_text', 'ui_text_bold', 'text'].includes(node.primitive.kind)) continue;
    const text = (node.primitive.text ?? '').trim();
    if (!text || node.opacity === 0) continue;
    const box = place(node);
    if (!box || buried(node, box)) continue;
    if (runs.some(r => r.text === text && close(r, box))) continue;
    runs.push({...box, text});
  }

  // Every affordance that is actually enabled: what an agent may do from this one state,
  // each with the rectangle to do it in. The window's own focus and drag targets cover
  // most of the screen and stand for the window rather than for anything on it.
  // `listitem` is left out: every list item that can be acted on wraps the link that
  // does the acting, and two boxes round one tab say nothing the inner box does not.
  const acts = ['button', 'link', 'textbox', 'searchbox', 'checkbox', 'radio', 'menuitem',
    'menuitemcheckbox', 'tab', 'switch', 'slider', 'combobox', 'option'];
  const widgets = [];
  const seen = new Set();
  for (const node of [...graph.nodes].sort((a, b) => acts.indexOf(a.semantic?.role) - acts.indexOf(b.semantic?.role))) {
    const {interaction, semantic} = node;
    if (!interaction || !semantic || semantic.disabled || !acts.includes(semantic.role)) continue;
    if (seen.has(interaction)) continue;
    const box = place(node);
    if (!box || buried(node, box)) continue;
    if (box.w * box.h > width * height * 0.3) continue;
    if (widgets.some(w => close(w, box))) continue;
    seen.add(interaction);
    widgets.push({...box, role: semantic.role, label: semantic.label, interaction});
  }

  // The overlay proper. Two treatments, never mixed up: a hairline in cyan for text the
  // scene already spells out, a thick amber frame for something that can be acted on.
  const TEXT = '79, 214, 224', ACT = '255, 180, 84';
  context.fillStyle = 'rgba(10, 9, 8, 0.2)';
  context.fillRect(0, 0, width, height);
  for (const run of runs) {
    context.fillStyle = `rgba(${TEXT}, 0.13)`;
    context.fillRect(run.x, run.y, run.w, run.h);
    context.strokeStyle = `rgba(${TEXT}, 0.95)`;
    context.lineWidth = 1;
    context.strokeRect(Math.round(run.x) + 0.5, Math.round(run.y) + 0.5, Math.round(run.w) - 1, Math.round(run.h) - 1);
  }
  for (const widget of widgets) {
    context.fillStyle = `rgba(${ACT}, 0.1)`;
    context.fillRect(widget.x, widget.y, widget.w, widget.h);
    context.strokeStyle = `rgba(${ACT}, 1)`;
    context.lineWidth = 2;
    context.strokeRect(Math.round(widget.x) + 1, Math.round(widget.y) + 1, Math.round(widget.w) - 2, Math.round(widget.h) - 2);
    context.fillStyle = `rgba(${ACT}, 1)`;
    context.fillRect(Math.round(widget.x), Math.round(widget.y), 6, 6);
  }

  // A few of the names read straight out of the graph, drawn where the thing they name
  // is, in the way a detector's class label sits on its box.
  const taken = [];
  const room = box => !taken.some(t => t.x < box.x + box.w && box.x < t.x + t.w && t.y < box.y + box.h && box.y < t.y + t.h);
  const chip = (box, text, colour, above) => {
    context.font = '11px ui-monospace, "DejaVu Sans Mono", monospace';
    const w = Math.ceil(context.measureText(text).width) + 10, h = 16;
    // Above the box or under it, never beside it: a name to one side reads as a name for
    // whatever it is lying on.
    const places = above
      ? [[box.x, box.y - h - 1], [box.x, box.y + box.h + 1]]
      : [[box.x, box.y + box.h + 1], [box.x, box.y - h - 1]];
    for (const [px, py] of places) {
      // A name that had to be shoved back into the picture no longer sits on its box.
      if (px < 2 || px + w > width - 2 || py < 2 || py + h > height - 2) continue;
      const at = {x: px, y: py, w, h};
      if (!room(at)) continue;
      context.fillStyle = 'rgba(18, 16, 13, 0.93)';
      context.fillRect(at.x, at.y, w, h);
      context.strokeStyle = `rgba(${colour}, 0.9)`;
      context.lineWidth = 1;
      context.strokeRect(at.x + 0.5, at.y + 0.5, w - 1, h - 1);
      context.fillStyle = `rgba(${colour}, 1)`;
      context.textBaseline = 'middle';
      context.fillText(text, at.x + 5, at.y + h / 2 + 0.5);
      taken.push(at);
      return true;
    }
    return false;
  };
  const cut = (text, n) => text.length > n ? `${text.slice(0, n - 1)}…` : text;
  // The legend is placed first so nothing is written under it.
  const legend = {x: 12, y: height - 84, w: 336, h: 72};
  taken.push(legend);
  // Biggest first, so the names that land are the ones with room for them.
  let named = 0;
  for (const widget of [...widgets].sort((a, b) => b.w * b.h - a.w * a.h)) {
    if (named >= labels) break;
    // A name is drawn where it can be read against the thing it names: the window's own
    // drag bar and the tab strip are affordances too, but a name on them floats over half
    // the screen rather than over a control.
    if (!widget.label || widget.w < 24 || widget.w > width * 0.4) continue;
    if (chip(widget, `${widget.role} · ${cut(widget.label, 34)}`, ACT, true)) named++;
  }
  let quoted = 0;
  for (const run of [...runs].sort((a, b) => b.w * b.h - a.w * a.h)) {
    if (quoted >= Math.round(labels / 3)) break;
    if (run.text.length < 6) continue;
    if (chip(run, `"${cut(run.text, 38)}"`, TEXT, false)) quoted++;
  }

  context.fillStyle = 'rgba(18, 16, 13, 0.94)';
  context.fillRect(legend.x, legend.y, legend.w, legend.h);
  context.strokeStyle = `rgba(${ACT}, 0.55)`;
  context.lineWidth = 1;
  context.strokeRect(legend.x + 0.5, legend.y + 0.5, legend.w - 1, legend.h - 1);
  context.textBaseline = 'middle';
  context.fillStyle = '#efe9df';
  context.font = 'bold 12px ui-monospace, "DejaVu Sans Mono", monospace';
  context.fillText(`scene(${width}, ${height}) → ${graph.nodes.length} nodes, every one bounded`, legend.x + 12, legend.y + 17);
  const key = (row, colour, swatch, line) => {
    const y = legend.y + 38 + row * 20;
    context.fillStyle = `rgba(${colour}, 0.13)`;
    context.fillRect(legend.x + 12, y - 5, 14, 11);
    context.strokeStyle = `rgba(${colour}, 1)`;
    context.lineWidth = swatch;
    context.strokeRect(legend.x + 12 + swatch / 2, y - 5 + swatch / 2, 14 - swatch, 11 - swatch);
    context.fillStyle = '#d8d2c8';
    context.font = '11px ui-monospace, "DejaVu Sans Mono", monospace';
    context.fillText(line, legend.x + 34, y + 0.5);
  };
  key(0, TEXT, 1, `${runs.length} text runs, string included`);
  key(1, ACT, 2, `${widgets.length} enabled actions, not ${(width * height).toLocaleString('en-US')} pixels`);

  const overlay = deliver();
  return {
    plain, overlay,
    stats: {nodes: graph.nodes.length, runs: runs.length, widgets: widgets.length, named, quoted},
    graph: JSON.stringify(graph, (k, v) => typeof v === 'bigint' ? String(v) : v),
  };
}

const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
const page = await browser.newPage({viewport: {width: 1440, height: 900}});
// A blank document on the site's own origin: the machine's modules are imported from the
// server by the page itself, so nothing needs to be written into site/ to take a picture.
await page.route('**/scene-overlay.html', route =>
  route.fulfill({contentType: 'text/html; charset=utf-8', body: '<!doctype html><meta charset="utf-8"><title>overlay</title>'}));
page.on('console', message => message.type() !== 'log' && console.warn(`  ${message.text()}`));
page.on('pageerror', error => console.warn(`  ${error.message.split('\n')[0]}`));

let result;
try {
  await page.goto(`http://127.0.0.1:${port}/scene-overlay.html`);
  result = await page.evaluate(picture, {
    sceneId: scene.id, machineId: machine.id, width, height,
    scale: options.scale, quality: options.quality, labels: options.labels,
  });
} finally {
  await browser.close();
  server.kill();
}

await mkdir(dirname(options.frame), {recursive: true});
await mkdir(dirname(options.overlay), {recursive: true});
const written = [];
for (const [path, base64] of [[options.frame, result.plain], [options.overlay, result.overlay]]) {
  const bytes = Buffer.from(base64, 'base64');
  await writeFile(path, bytes);
  written.push([path, bytes.length]);
}
if (options.dump) await writeFile(options.dump, result.graph);

const {stats} = result;
const out = [Math.round(width * options.scale), Math.round(height * options.scale)];
console.log(`${scene.id}/${machine.id}: scene(${width}, ${height}) → ${stats.nodes} nodes, ` +
  `${stats.runs} text runs, ${stats.widgets} enabled affordances ` +
  `(${stats.named} named, ${stats.quoted} quoted)`);
for (const [path, bytes] of written) {
  console.log(`  ${path.startsWith(root) ? path.slice(root.length) : path}  ${out[0]}x${out[1]}  ${(bytes / 1024).toFixed(0)} KB`);
}
if (written.some(([, bytes]) => bytes > 400 * 1024)) {
  console.warn('  one of these is over 400 KB: lower --quality or --scale');
}
