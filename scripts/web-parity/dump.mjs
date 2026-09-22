#!/usr/bin/env node
// Dump what Chromium computes for a fixture page: every element's box and a fixed set of
// computed properties, every text node's line boxes, and a screenshot. The engine's own
// dump (`crates/web/engine/tests/parity.rs`) has the same shape, and `compare.mjs` reads both.
//
//   PLAYWRIGHT_MODULE=/usr/lib/chatgpt/resources/cua_node/lib/node_modules/playwright/index.mjs \
//   CHROME_BIN=/usr/bin/google-chrome \
//     node scripts/web-parity/dump.mjs crates/web/engine/tests/parity/google-1998.html \
//       [--width 1280] [--height 800] [--dpr 1] [--out <name>.chromium.json] [--full] \
//       [--props extra-a,extra-b] [--state states.json]
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
// Playwright before collecting, so :hover/:focus(-visible) variants are captured.
import {readFile, writeFile} from 'node:fs/promises';
import {basename, dirname, extname, join, resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';
import {PROPERTIES} from './common.mjs';

// The same third-party bundles the Rust harness serves at https://example.test/vendor/*
// (see crates/web/engine/tests/vendor/), so a fixture loaded from a file: URL that links
// `/vendor/whatever` (root-relative, exactly as the Rust test expects it) gets the real
// file instead of a filesystem 404.
const vendorDir = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..', 'crates', 'web', 'engine', 'tests', 'vendor');
const VENDOR_MIME = {'.css': 'text/css', '.js': 'text/javascript', '.json': 'application/json'};

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i < 0 ? fallback : args[i + 1];
};
const positional = args.filter((a, i) => !a.startsWith('--') && !(args[i - 1]?.startsWith('--') && args[i - 1] !== '--full'));
const [fixture] = positional;
if (!fixture) {
  console.error('usage: dump.mjs <fixture.html> [--width 1280] [--height 800] [--dpr 1] [--out file.json] [--full] [--props a,b] [--state states.json]');
  process.exit(2);
}
const width = Number(flag('--width', 1280));
const height = Number(flag('--height', 800));
const dpr = Number(flag('--dpr', 1));
const full = args.includes('--full');
const stem = basename(fixture).replace(/\.html?$/, '');
const out = resolve(flag('--out', join(dirname(resolve(fixture)), `${stem}.chromium.json`)));
const outStem = out.replace(/\.json$/, '');
// Extra properties are appended (deduped, shared ones win their original slot) so
// existing fixtures that pass no --props get exactly PROPERTIES, unchanged.
const extraProps = (flag('--props', '') || '').split(',').map(p => p.trim()).filter(Boolean);
const properties = extraProps.length ? [...PROPERTIES, ...extraProps.filter(p => !PROPERTIES.includes(p))] : PROPERTIES;
const statePath = flag('--state', null);
const states = statePath ? JSON.parse(await readFile(resolve(statePath), 'utf8')) : [];


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

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
try {
  const context = await browser.newContext({viewport: {width, height}, deviceScaleFactor: dpr, reducedMotion: 'reduce'});
  const page = await context.newPage();
  const url = pathToFileURL(resolve(fixture)).href;
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
  await page.evaluate(() => document.fonts.ready);
  for (const {selector, action} of states) {
    if (action === 'hover') await page.hover(selector);
    else if (action === 'focus') await page.focus(selector);
    else throw new Error(`--state: unknown action "${action}" for ${selector}`);
  }
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
