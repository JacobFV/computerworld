#!/usr/bin/env node
// Dump what Chromium computes for a fixture page: every element's box and a fixed set of
// computed properties, every text node's line boxes, and a screenshot. The engine's own
// dump (`crates/web/engine/tests/parity.rs`) has the same shape, and `compare.mjs` reads both.
//
//   PLAYWRIGHT_MODULE=/usr/lib/chatgpt/resources/cua_node/lib/node_modules/playwright/index.mjs \
//   CHROME_BIN=/usr/bin/google-chrome \
//     node scripts/web-parity/dump.mjs crates/web/engine/tests/parity/google-1998.html \
//       [--width 1280] [--height 800] [--dpr 1] [--out <name>.chromium.json] [--full] \
//       [--props extra-a,extra-b] [--state states.json [--state-name <name>]]
//
// Writes <name>.chromium.json, <name>.chromium.png (the viewport, or the whole page with
// --full) and <name>.fonts.json (each distinct `font-family` list the page asked for and
// the platform face Chromium actually shaped it with, so a parity report can say which
// stand-in the engine chose against which real face). Node order is document order;
// nothing in the output depends on timing.
//
// --props is a comma-separated list of extra computed properties (including custom
// properties, e.g. `--tw-shadow`) appended to the shared PROPERTIES for this dump only;
// they show up in the output's `properties` array alongside the shared ones, so a reader
// can see exactly what was asked for. --state points at a JSON file of
// `[{"selector": "...", "action": "hover" | "focus"}, ...]`, each applied through
// Playwright before collecting, so :hover/:focus(-visible) variants are captured. The
// steps may also be `{"action": "click", "selector"}`, `{"action": "type", "text"}` (to
// the focused element) and `{"action": "press", "key"}`, which is how a page a framework
// renders is driven into a state (tests/framework-parity/*.steps.json); there the file
// is an object of named step lists and --state-name picks one. The page is let settle
// (two animation frames and a task, and any finite transition or animation finished)
// after loading and after every step, so a framework that commits asynchronously has
// committed, and a transition has reached its end state, before anything is read.
//
// A fixture named `<name>.site.json` is a site rather than a page: an app served as the
// world serves it, for apps a file: URL cannot load (ES modules, chunks loaded from
// root-relative paths, an API on another host). The file names the URL to open, the
// directory each static host serves (a node-app package's `public/`, with its manifest's
// `spa_fallback` for HTML navigations), and a recording of the API answers
// (`<name>.api.json`, written from the world's own services by
// crates/computerworld/tests/oss_parity_record.rs). Every request is answered from those
// two; anything else fails as it does offline in the world, and API requests that are
// not in the recording are listed and fail the dump, so the recording can be extended.
// With PARITY_HTML=<file> the page's serialised DOM at the end of the steps is written
// there too, for choosing the selectors a steps file clicks.
//
// --baseline-fonts runs Chromium with a fontconfig that knows only the Liberation and
// DejaVu families, the stock Linux desktop the engine's `FontEnvironment::LinuxBaseline`
// models. A page whose font stack names a face this machine happens to have (JSON
// Server's system stack reaches Ubuntu here) is then shaped with what the engine
// resolves it to, not with a font the engine cannot know about.
import {mkdtemp, readFile, writeFile} from 'node:fs/promises';
import {tmpdir} from 'node:os';
import {basename, dirname, extname, join, resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {PROPERTIES} from './common.mjs';

// The same third-party bundles the Rust harness serves at https://example.test/vendor/*
// (see crates/web/engine/tests/vendor/), so a fixture loaded from a file: URL that links
// `/vendor/whatever` (root-relative, exactly as the Rust test expects it) gets the real
// file instead of a filesystem 404.
const vendorDir = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', 'crates', 'web', 'engine', 'tests', 'vendor');
const VENDOR_MIME = {'.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json', '.ttf': 'font/ttf', '.woff2': 'font/woff2'};

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i < 0 ? fallback : args[i + 1];
};
const BOOLEAN_FLAGS = ['--full', '--baseline-fonts'];
const positional = args.filter((a, i) => !a.startsWith('--') && !(args[i - 1]?.startsWith('--') && !BOOLEAN_FLAGS.includes(args[i - 1])));
const [fixture] = positional;
if (!fixture) {
  console.error('usage: dump.mjs <fixture.html> [--width 1280] [--height 800] [--dpr 1] [--out file.json] [--full] [--props a,b] [--state states.json]');
  process.exit(2);
}
const width = Number(flag('--width', 1280));
const height = Number(flag('--height', 800));
const dpr = Number(flag('--dpr', 1));
const full = args.includes('--full');
const stem = basename(fixture).replace(/\.html?$/, '').replace(/\.site\.json$/, '');
const out = resolve(flag('--out', join(dirname(resolve(fixture)), `${stem}.chromium.json`)));
const outStem = out.replace(/\.json$/, '');
// Extra properties are appended (deduped, shared ones win their original slot) so
// existing fixtures that pass no --props get exactly PROPERTIES, unchanged.
const extraProps = (flag('--props', '') || '').split(',').map(p => p.trim()).filter(Boolean);
const properties = extraProps.length ? [...PROPERTIES, ...extraProps.filter(p => !PROPERTIES.includes(p))] : PROPERTIES;
const statePath = flag('--state', null);
const stateName = flag('--state-name', null);
const stateFile = statePath ? JSON.parse(await readFile(resolve(statePath), 'utf8')) : [];
const states = stateName ? stateFile[stateName] : stateFile;
if (!Array.isArray(states)) {
  console.error(`--state: ${statePath} has no step list${stateName ? ` named "${stateName}"` : ''}`);
  process.exit(2);
}


// Runs inside the page. Everything it returns is plain JSON.
function collect(properties) {
  const q = v => Math.round(v * 64) / 64;
  const rect = r => ({x: q(r.x), y: q(r.y), width: q(r.width), height: q(r.height)});
  const nodes = [];
  const families = new Map();
  const pathOf = new Map();
  const walk = (el, path) => {
    pathOf.set(el, path);
    const cs = getComputedStyle(el);
    const computed = {};
    for (const p of properties) computed[p] = cs.getPropertyValue(p);
    // The first element that sets its own text in this family answers the platform
    // font query (an element with no direct text has no glyphs to report).
    const ownText = Array.from(el.childNodes).some(c => c.nodeType === 3 && /\S/.test(c.data));
    const fam = computed['font-family'];
    if (!families.has(fam) || (ownText && !families.get(fam).ownText)) families.set(fam, {path, ownText});
    nodes.push({
      kind: 'element', path, tag: el.tagName.toLowerCase(), id: el.id || '',
      rect: rect(el.getBoundingClientRect()), computed,
    });
    const counts = {};
    let textIndex = 0;
    for (const child of el.childNodes) {
      if (child.nodeType === 1) {
        if (child.tagName === 'HEAD') continue;
        const tag = child.tagName.toLowerCase();
        // `:nth-child` counts element siblings of any tag; html and body carry no index.
        const n = (counts.all = (counts.all ?? 0) + 1);
        const seg = tag === 'body' && el.tagName === 'HTML' ? 'body' : `${tag}:nth-child(${n})`;
        walk(child, `${path}>${seg}`);
      } else if (child.nodeType === 3) {
        if (!/\S/.test(child.data)) continue;
        const range = document.createRange();
        range.selectNodeContents(child);
        const rects = Array.from(range.getClientRects()).map(rect);
        nodes.push({kind: 'text', path: `${path}>#text:nth(${textIndex})`, parent: path, text: child.data, rects});
        textIndex++;
      }
    }
  };
  walk(document.documentElement, 'html');
  const de = document.documentElement;
  return {
    document: {width: q(de.scrollWidth), height: q(de.scrollHeight)},
    nodes,
    families: Array.from(families, ([family, {path}]) => ({family, path})),
  };
}

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
const siteMode = fixture.endsWith('.site.json');
const site = siteMode ? JSON.parse(await readFile(resolve(fixture), 'utf8')) : null;
const SITE_MIME = {...VENDOR_MIME, '.html': 'text/html', '.mjs': 'text/javascript', '.svg': 'image/svg+xml', '.png': 'image/png', '.ico': 'image/x-icon', '.woff': 'font/woff', '.map': 'application/json', '.txt': 'text/plain', '.md': 'text/markdown'};
// Recorded API answers, keyed by method, URL and body.
const apiKey = (method, url, body) => `${method} ${url} ${body ?? ''}`;
const recorded = new Map();
if (siteMode && site.api) {
  const rec = JSON.parse(await readFile(join(dirname(resolve(fixture)), site.api), 'utf8'));
  for (const r of rec.requests) if (r.response) recorded.set(apiKey(r.method, r.url, r.body), r.response);
}
const missing = [];

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const launchEnv = {...process.env};
if (args.includes('--baseline-fonts')) {
  const dir = await mkdtemp(join(tmpdir(), 'parity-fonts-'));
  const conf = join(dir, 'fonts.conf');
  await writeFile(conf, `<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>/usr/share/fonts/truetype/liberation</dir>
  <dir>/usr/share/fonts/truetype/dejavu</dir>
  <cachedir>${join(dir, 'cache')}</cachedir>
  <!-- The system's rendering settings (hinting, antialiasing) and generic aliases, so
       only the set of faces differs from an ordinary dump. -->
  <include ignore_missing="yes">/etc/fonts/conf.d</include>
</fontconfig>
`);
  launchEnv.FONTCONFIG_FILE = conf;
}
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN, env: launchEnv});
try {
  // A site runs as in the world, whose clock is UTC (a page's dates format there);
  // page fixtures keep the machine's zone, as their dumps always have.
  const context = await browser.newContext({viewport: {width, height}, deviceScaleFactor: dpr, reducedMotion: 'reduce', ...(siteMode ? {timezoneId: 'UTC'} : {})});
  const page = await context.newPage();
  const url = siteMode ? site.url : pathToFileURL(resolve(fixture)).href;
  const inflight = new Set();
  page.on('request', r => inflight.add(r));
  page.on('requestfinished', r => inflight.delete(r));
  page.on('requestfailed', r => inflight.delete(r));
  if (siteMode) {
    await page.route(() => true, async route => {
      const req = route.request();
      const u = new URL(req.url());
      const dir = site.static?.[u.host];
      if (dir && (req.method() === 'GET' || req.method() === 'HEAD')) {
        const root = join(repoRoot, dir, 'public');
        const manifest = JSON.parse(await readFile(join(repoRoot, dir, 'manifest.json'), 'utf8'));
        let rel = decodeURIComponent(u.pathname);
        if (rel.endsWith('/')) rel += 'index.html';
        let body = await readFile(join(root, rel)).catch(() => null);
        if (!body && manifest.spa_fallback && (req.headers().accept ?? '').includes('text/html')) {
          rel = manifest.spa_fallback;
          body = await readFile(join(root, rel)).catch(() => null);
        }
        if (body) return route.fulfill({status: 200, contentType: SITE_MIME[extname(rel)] ?? 'application/octet-stream', body});
        return route.fulfill({status: 404, body: 'not found'});
      }
      const key = apiKey(req.method(), req.url(), req.postData());
      const answer = recorded.get(key);
      if (answer) return route.fulfill({status: answer.status, headers: answer.headers, body: answer.text ?? Buffer.from(answer.bytes)});
      if (req.method() === 'OPTIONS') return route.fulfill({status: 204, headers: {'access-control-allow-origin': '*', 'access-control-allow-headers': '*', 'access-control-allow-methods': '*'}});
      if (site.api_hosts?.includes(u.host)) missing.push({method: req.method(), url: req.url(), body: req.postData() ?? undefined});
      return route.abort('internetdisconnected');
    });
  }
  // web-platform-tests' server answers `?pipe=status(N)` with that HTTP status (Acid2's
  // `<object data="acid2/404.html?pipe=status(404)">` must fail to load and fall back). A
  // file: URL has no status, so Chromium would load the document; answer for the server.
  await page.route(u => /[?&]pipe=status\((\d{3})\)/.test(u.search), route => {
    const status = Number(/pipe=status\((\d{3})\)/.exec(new URL(route.request().url()).search)[1]);
    return status >= 400 ? route.abort('failed') : route.continue();
  });
  await page.route(u => u.pathname.startsWith('/vendor/'), async route => {
    const rel = decodeURIComponent(new URL(route.request().url()).pathname.replace(/^\/vendor\//, ''));
    try {
      const body = await readFile(join(vendorDir, rel));
      await route.fulfill({status: 200, contentType: VENDOR_MIME[extname(rel)] ?? 'application/octet-stream', body});
    } catch {
      await route.fulfill({status: 404, body: 'not found'});
    }
  });
  await page.goto(url, {waitUntil: 'load'});
  // A site's app fetches its data after load: let the network go quiet first.
  const quiet = async () => {
    if (!siteMode) return;
    // A site's own timers (react-admin's fake REST provider answers after 300 ms)
    // get `settle_ms` of wall time first.
    if (site.settle_ms) await new Promise(r => setTimeout(r, site.settle_ms));
    // Until no request has been in flight for 300 ms (at most ten seconds).
    for (let i = 0, still = 0; i < 200 && still < 6; i++) {
      await new Promise(r => setTimeout(r, 50));
      still = inflight.size ? 0 : still + 1;
    }
  };
  await quiet();
  await page.evaluate(() => document.fonts.ready);
  // Two frames and a task, then every finite CSS transition or animation run to its end
  // (a class change under `transition-colors` would otherwise be dumped mid-way), then
  // the frames again. A page with no animations settles exactly as before.
  const frames = () => page.evaluate(() => new Promise(done =>
    requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(done, 0)))));
  const settle = async () => {
    await frames();
    const running = await page.evaluate(() => {
      const finite = document.getAnimations().filter(a => a.effect?.getComputedTiming().endTime !== Infinity);
      finite.forEach(a => a.finish());
      return finite.length;
    });
    if (running) await frames();
  };
  await settle();
  for (const {selector, action, text, key} of states) {
    if (action === 'hover') await page.hover(selector);
    else if (action === 'focus') await page.focus(selector);
    else if (action === 'click') await page.click(selector);
    else if (action === 'type') await page.keyboard.type(text);
    else if (action === 'press') await page.keyboard.press(key);
    else throw new Error(`--state: unknown action "${action}" for ${selector}`);
    await quiet();
    await settle();
  }
  if (missing.length) {
    console.error(`${basename(fixture)}: API requests not in ${site.api}:\n${JSON.stringify(missing, null, 1)}`);
    process.exitCode = 1;
  }
  if (process.env.PARITY_HTML) await writeFile(process.env.PARITY_HTML, await page.content());
  if (process.env.PARITY_EVAL) console.log('PARITY_EVAL', await page.evaluate(process.env.PARITY_EVAL));
  const dump = await page.evaluate(collect, properties);

  // Which platform faces Chromium really used, per distinct family list, through CDP.
  const fonts = [];
  try {
    const cdp = await context.newCDPSession(page);
    await cdp.send('DOM.enable');
    await cdp.send('CSS.enable');
    const {root} = await cdp.send('DOM.getDocument', {depth: -1});
    for (const {family, path} of dump.families) {
      let used = [];
      try {
        const {nodeId} = await cdp.send('DOM.querySelector', {nodeId: root.nodeId, selector: path});
        const r = await cdp.send('CSS.getPlatformFontsForNode', {nodeId});
        used = r.fonts.map(f => ({family: f.familyName, glyphs: f.glyphCount}));
      } catch {
        used = [];
      }
      fonts.push({family, first: path, chromium: used});
    }
  } catch (e) {
    for (const {family, path} of dump.families) fonts.push({family, first: path, chromium: [], error: String(e.message ?? e)});
  }
  delete dump.families;

  const result = {
    fixture: basename(fixture),
    engine: 'chromium',
    version: browser.version(),
    viewport: {width, height, dpr},
    properties,
    ...dump,
  };
  await writeFile(out, JSON.stringify(result, null, 1) + '\n');
  await page.screenshot({path: `${outStem}.png`, fullPage: full});
  await writeFile(`${outStem.replace(/\.chromium$/, '')}.fonts.json`, JSON.stringify(fonts, null, 1) + '\n');
  const elements = dump.nodes.filter(n => n.kind === 'element').length;
  const texts = dump.nodes.length - elements;
  console.log(`${basename(fixture)}: ${elements} elements, ${texts} text nodes, ${fonts.length} font families -> ${out}`);
} finally {
  await browser.close();
}
