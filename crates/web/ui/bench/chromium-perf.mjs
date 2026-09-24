#!/usr/bin/env node
// Chromium's side of crates/web/ui/tests/perf.rs: the same fixture page (a TSX app's
// React fallback), the same phases (boot, a click that re-renders `#check-2`, a
// second click on it (warm), a keystroke into the controlled `#new-task`), timed with the DevTools Performance
// domain's ScriptDuration (script only, as the engine-side numbers break out) and
// TaskDuration (everything the renderer did), best and median of N fresh pages.
//
//   PLAYWRIGHT_MODULE=/usr/lib/chatgpt/resources/cua_node/lib/node_modules/playwright/index.mjs \
//   CHROME_BIN=/usr/bin/google-chrome \
//     node crates/web/ui/bench/chromium-perf.mjs crates/web/engine/tests/framework-parity/tsx-tasks.html [--runs 15]
import {readFile} from 'node:fs/promises';
import {dirname, extname, join, resolve} from 'node:path';
import {fileURLToPath, pathToFileURL} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const vendorDir = resolve(here, '..', '..', 'engine', 'tests', 'vendor');
const args = process.argv.slice(2);
const fixture = args.find((a) => !a.startsWith('--'));
const runs = Number(args[args.indexOf('--runs') + 1] || 15) || 15;
if (!fixture) {
  console.error('usage: chromium-perf.mjs <fixture.html> [--runs N]');
  process.exit(2);
}
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE || 'playwright');
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN || undefined});

const settle = (page) => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(() => setTimeout(r, 0)))));
// Waits from Node, so no script of ours runs in the page while it is measured.
const idle = () => new Promise((r) => setTimeout(r, 100));

async function metrics(cdp) {
  const {metrics} = await cdp.send('Performance.getMetrics');
  const m = Object.fromEntries(metrics.map((x) => [x.name, x.value]));
  return {script: m.ScriptDuration * 1000, task: m.TaskDuration * 1000};
}

const centre = (page, sel) => page.evaluate((s) => {
  const r = document.querySelector(s).getBoundingClientRect();
  return [r.left + r.width / 2, r.top + r.height / 2];
}, sel);

async function click(cdp, [x, y]) {
  await cdp.send('Input.dispatchMouseEvent', {type: 'mouseMoved', x, y});
  await cdp.send('Input.dispatchMouseEvent', {type: 'mousePressed', x, y, button: 'left', clickCount: 1});
  await cdp.send('Input.dispatchMouseEvent', {type: 'mouseReleased', x, y, button: 'left', clickCount: 1});
}

async function key(cdp, ch) {
  await cdp.send('Input.dispatchKeyEvent', {type: 'keyDown', key: ch, text: ch, unmodifiedText: ch});
  await cdp.send('Input.dispatchKeyEvent', {type: 'keyUp', key: ch});
}

const phases = {boot: [], click: [], 'click again': [], key: []};
for (let i = 0; i < runs; i++) {
  const context = await browser.newContext({viewport: {width: 1280, height: 800}});
  const page = await context.newPage();
  await page.route((u) => u.pathname.startsWith('/vendor/'), async (route) => {
    const rel = decodeURIComponent(new URL(route.request().url()).pathname.replace(/^\/vendor\//, ''));
    const body = await readFile(join(vendorDir, rel));
    await route.fulfill({status: 200, contentType: extname(rel) === '.css' ? 'text/css' : 'text/javascript', body});
  });
  const cdp = await context.newCDPSession(page);
  await cdp.send('Performance.enable', {timeDomain: 'threadTicks'});
  await page.goto(pathToFileURL(resolve(fixture)).href);
  await idle();
  phases.boot.push(await metrics(cdp));
  const check = await centre(page, '#check-2');
  const input = await centre(page, '#new-task');
  await settle(page);
  await idle();
  let before = await metrics(cdp);
  await click(cdp, check);
  await idle();
  let after = await metrics(cdp);
  phases.click.push({script: after.script - before.script, task: after.task - before.task});
  before = await metrics(cdp);
  await click(cdp, check);
  await idle();
  after = await metrics(cdp);
  phases['click again'].push({script: after.script - before.script, task: after.task - before.task});
  await click(cdp, input);
  await idle();
  before = await metrics(cdp);
  await key(cdp, 'x');
  await idle();
  after = await metrics(cdp);
  phases.key.push({script: after.script - before.script, task: after.task - before.task});
  const typed = await page.evaluate(() => document.querySelector('#new-task').value);
  if (typed !== 'x') throw new Error(`the keystroke did not reach the input (value ${JSON.stringify(typed)})`);
  await context.close();
}
await browser.close();

const summary = (xs, k) => {
  const v = xs.map((x) => x[k]).sort((a, b) => a - b);
  return `${v[Math.floor(v.length / 2)].toFixed(3)} ms (best ${v[0].toFixed(3)})`;
};
for (const [name, xs] of Object.entries(phases)) {
  console.log(`chromium ${name}: script ${summary(xs, 'script')}, task ${summary(xs, 'task')} [median of ${runs}]`);
}
