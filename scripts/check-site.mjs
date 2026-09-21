/** Does the project site still run its machines?
 *
 * Nothing else in CI opens the page. The Rust suites prove the engine, the bindings smoke
 * test proves the two language bindings, and the docs check proves the guides build — and
 * between them they say nothing about whether a visitor gets a running computer, because
 * the whole of that lives in site/live.js, site/worker.js and site/engine.js and in a
 * postMessage protocol between them. This is the check that would have caught it.
 *
 * It asserts behaviour, not pixels: that machines come up in both of the hosts the page
 * can use, that a pointer crossing one machine redraws that machine and none of the others
 * on the slide, that one machine can take the whole screen and still be driven, that a
 * scene the slideshow has left behind keeps its last frame, and that a page whose worker
 * will not load falls back to the tab rather than showing a spinner for ever. Each of
 * those is a bug this page has actually had.
 *
 *   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
 *     node scripts/check-site.mjs [check name ...]
 *
 * The machines need the Wasm bundle in site/pkg/ (see site/README.md).
 */
import {spawn} from 'node:child_process';
import {createServer} from 'node:net';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));

/** Playwright is not a dependency of this project — it is a browser driver, not something
 * the site or the engine needs — so it is found wherever the caller has one: named
 * outright, installed as `playwright-core` beside a Chrome that already exists, or as the
 * full package with a browser of its own. */
async function driver() {
  const named = process.env.PLAYWRIGHT_MODULE;
  for (const where of [named, 'playwright-core', 'playwright'].filter(Boolean)) {
    try {
      const module = await import(where);
      return (module.default ?? module).chromium;
    } catch (error) {
      if (where === named) throw error;      // an explicit path that does not load is a mistake, not a fallback
    }
  }
  throw new Error('no Playwright: set PLAYWRIGHT_MODULE, or install playwright-core');
}

/** A port nobody is on. A fixed one would mean two of these runs sharing a server, and the
 * first to finish taking it away from the other. */
const freePort = () => new Promise((resolve, reject) => {
  const probe = createServer();
  probe.on('error', reject);
  probe.listen(0, '127.0.0.1', () => {
    const {port} = probe.address();
    probe.close(() => resolve(port));
  });
});

// A two-machine scene: a Mac, which paints its own pointer into the frame, beside an
// iPhone, which does not. That is what makes it the fixture for the repaint rule — moving
// the mouse over the Mac MUST change the Mac and MUST NOT change the phone — and it costs
// two worlds to boot rather than the seven a team scene would.
const SCENE = 'x-everywhere';
const DESKTOP = 'mac-x-thread';
const PHONE = 'iphone-x-thread';

const BOOT = 180_000;    // a cold CI runner building two worlds, with room to spare

/** Wait for every machine on the slide in the middle to be running. */
const live = (page, timeout = BOOT) => page.waitForFunction(
  () => {
    const tiles = [...document.querySelectorAll('.slide.active .tile')];
    return tiles.length > 0 && tiles.every(tile => tile.classList.contains('live'));
  },
  null, {timeout});

/** What a machine is showing. Not read off the canvas: a canvas whose control has gone to
 * a worker hands the page back the frame it was FIRST given, however many have been drawn
 * since, so the machine is asked instead. */
const shown = (page, machine) => page.evaluate(async id => {
  const frame = await window.computerworldFrame(id);
  return frame ? [...frame.data] : null;
}, machine);

const differing = (before, after) => {
  if (!before || !after || before.length !== after.length) return null;
  let n = 0;
  for (let i = 0; i < before.length; i += 4) if (before[i] !== after[i]) n++;
  return n;
};

/** Move the host pointer across a machine, one position per call, and give back what every
 * named machine is showing once it has settled. */
async function after(page, machine, fx, fy, watching) {
  await page.evaluate(({id, fx, fy}) => {
    const canvas = document.querySelector(`[data-machine="${id}"] canvas`);
    const box = canvas.getBoundingClientRect();
    canvas.dispatchEvent(new PointerEvent('pointermove', {
      clientX: box.left + box.width * fx, clientY: box.top + box.height * fy,
      pointerType: 'mouse', bubbles: true, isPrimary: true, pointerId: 1,
    }));
  }, {id: machine, fx, fy});
  await page.waitForTimeout(600);
  const seen = {};
  for (const id of watching) seen[id] = await shown(page, id);
  return seen;
}

const checks = {
  /** The ordinary way in: machines in workers, drawing through an OffscreenCanvas. */
  async workers(open) {
    const page = await open(`/#${SCENE}`);
    await live(page);
    const near = await after(page, DESKTOP, 0.25, 0.25, [DESKTOP, PHONE]);
    const far = await after(page, DESKTOP, 0.75, 0.75, [DESKTOP, PHONE]);
    const moved = differing(near[DESKTOP], far[DESKTOP]);
    const spread = differing(near[PHONE], far[PHONE]);
    if (!(moved > 0)) throw new Error(`the pointer changed nothing on ${DESKTOP}`);
    if (spread !== 0) throw new Error(`a hover over ${DESKTOP} redrew ${spread} pixels of ${PHONE}`);
    return `${moved} pixels on the machine under the pointer, none on the one beside it`;
  },

  /** The same engine on the page's own thread, which is what a browser without workers or
   * OffscreenCanvas falls back to. It must reach the same machines. */
  async tab(open) {
    const page = await open(`/#${SCENE}`, {blind: true});
    await live(page);
    const near = await after(page, DESKTOP, 0.25, 0.25, [DESKTOP, PHONE]);
    const far = await after(page, DESKTOP, 0.75, 0.75, [DESKTOP, PHONE]);
    const moved = differing(near[DESKTOP], far[DESKTOP]);
    const spread = differing(near[PHONE], far[PHONE]);
    if (!(moved > 0)) throw new Error(`the pointer changed nothing on ${DESKTOP} in the tab`);
    if (spread !== 0) throw new Error(`a hover redrew ${spread} pixels of ${PHONE} in the tab`);
    return `${moved} pixels, the same machines as in the workers`;
  },

  /** The slideshow on its own, in a frame, which is the same app.js and the same cast. */
  async embed(open) {
    const page = await open(`/embed.html#github`);
    await live(page);
    return 'the machine came up in the frame';
  },

  /** One machine, the whole screen — and still a machine, not a picture of one. */
  async fullscreen(open) {
    const page = await open('/#github');
    await live(page);
    const machine = await page.evaluate(() => document.querySelector('.slide.active .tile').dataset.machine);
    await page.evaluate(() => document.querySelector('.slide.active .tile .grow').click());
    await page.waitForTimeout(800);
    const taken = await page.evaluate(() =>
      !!document.fullscreenElement || !!document.querySelector('.fs-host'));
    if (!taken) throw new Error('the fullscreen control did nothing');
    const near = await after(page, machine, 0.3, 0.3, [machine]);
    const far = await after(page, machine, 0.7, 0.7, [machine]);
    if (!(differing(near[machine], far[machine]) > 0)) throw new Error('the machine stopped answering full screen');
    await page.evaluate(() => document.dispatchEvent(new KeyboardEvent('keydown', {key: 'Escape', bubbles: true})));
    await page.waitForTimeout(800);
    if (await page.evaluate(() => !!document.querySelector('.tile.full'))) throw new Error('Escape did not give the screen back');
    const back = await after(page, machine, 0.3, 0.3, [machine]);
    if (!(differing(back[machine], far[machine]) > 0)) throw new Error('the machine stopped answering after fullscreen');
    return 'taken, driven, and given back';
  },

  /** A scene the visitor has walked away from is stopped, and its screens stay as pictures
   * of where they were left rather than going blank. */
  async retire(open) {
    const page = await open('/#github');
    await live(page);
    const machine = await page.evaluate(() => document.querySelector('.slide.active .tile').dataset.machine);
    const running = await shown(page, machine);
    for (let step = 0; step < 9; step++) {
      await page.evaluate(() => document.dispatchEvent(new KeyboardEvent('keydown', {key: 'ArrowRight', bubbles: true})));
      await page.waitForTimeout(2500);
    }
    const state = await page.evaluate(id => {
      const tile = document.querySelector(`[data-machine="${id}"]`);
      return {stopped: !tile.classList.contains('live'), hasCanvas: !!tile.querySelector('canvas')};
    }, machine);
    if (!state.stopped) throw new Error('walking the strip retired nothing');
    if (!state.hasCanvas) throw new Error('a retired machine lost its screen');
    if (!running) throw new Error('the machine gave back no frame while it was running');
    return 'stopped, and still showing the frame it was left at';
  },

  /** The CJK and emoji faces are not in the bundle; they are fetched afterwards and
   * installed into whichever engine instance needs them, and which instances those are
   * depends on where the visitor has been. That leaves a way for the page to wait on a
   * worker that was never told to fetch anything, so this is the check that it settles —
   * and the nightly stills run is the check that the glyphs are really there, since a
   * still drawn without them drifts. */
  async fonts(open) {
    const page = await open(`/#${SCENE}`);
    await live(page);
    const settled = await page.evaluate(() => Promise.race([
      window.computerworldFonts.then(() => true),
      new Promise(resolve => setTimeout(() => resolve(false), 90_000)),
    ]));
    if (!settled) throw new Error('the font pack never reported itself in');
    return 'the pack reported itself in';
  },

  /** A page cached from before a deploy, meeting a worker from after it. The two are
   * separate files over a private protocol and GitHub Pages serves them with ten minutes
   * of cache, so they can genuinely arrive out of step; the page must notice it is being
   * answered in a language it does not speak and run the machines itself instead. */
  async protocol(open) {
    const page = await open('/#github', {protocol: 999});
    await live(page, 90_000);
    const noticed = page.said.some(line => line.includes('protocol') && line.includes('999'));
    if (!noticed) throw new Error('the machine ran, but the page never said it had met a worker it does not understand');
    return 'noticed the mismatch and ran the machine itself';
  },

  /** A deploy that leaves the worker behind altogether. Same requirement: the machines run
   * in the tab rather than the page sitting under a spinner. */
  async workerless(open) {
    const page = await open('/#github', {breakWorker: true});
    // Sooner than the others: this one is the tab host, which has no worlds to build in
    // parallel and nothing to wait for but one module instantiating, and a check that has
    // regressed should say so rather than sit out the full boot allowance.
    await live(page, 90_000);
    const spinning = await page.evaluate(() => {
      const tile = document.querySelector('.slide.active .tile');
      return !!document.documentElement.dataset.boot && !tile.classList.contains('live');
    });
    if (spinning) throw new Error('the overlay is still turning over a machine that will not come');
    const noticed = page.said.some(line => line.includes('protocol'));
    if (!noticed) throw new Error('the machine ran, but the page never said its workers had gone missing');
    return 'fell back to the tab and ran the machine';
  },
};

const wanted = process.argv.slice(2);
const chosen = Object.keys(checks).filter(name => !wanted.length || wanted.includes(name));
if (!chosen.length) {
  console.error(`no such check. known: ${Object.keys(checks).join(', ')}`);
  process.exit(2);
}

const port = await freePort();
const chromium = await driver();
const server = spawn(process.execPath, [`${root}scripts/serve-site.mjs`, String(port)], {stdio: 'ignore'});
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

// A Chrome that is already on the machine if the caller named one, and otherwise whatever
// browser the installed Playwright brought with it.
const browser = await chromium.launch(
  process.env.CHROME_BIN ? {executablePath: process.env.CHROME_BIN} : {});
let failed = 0;

try {
  for (const name of chosen) {
    // A page each, so one check's machines are never another's, and every one of them
    // starts from a browser that has downloaded nothing.
    const complaints = [];
    const open = async (path, {blind = false, breakWorker = false, protocol = 0} = {}) => {
      const page = await browser.newPage({viewport: {width: 1440, height: 900}});
      // Everything the page says, for the checks that are about it noticing something and
      // saying so, and separately the subset that counts as a complaint.
      page.said = [];
      page.on('pageerror', error => complaints.push(`uncaught: ${error.message.split('\n')[0]}`));
      page.on('console', message => {
        page.said.push(message.text());
        if (message.type() === 'error') complaints.push(`console: ${message.text().split('\n')[0]}`);
      });
      // A browser with neither workers nor OffscreenCanvas, which is what sends the page
      // down its other path.
      if (blind) await page.addInitScript(() => {
        delete window.OffscreenCanvas;
        delete HTMLCanvasElement.prototype.transferControlToOffscreen;
      });
      if (breakWorker) await page.route('**/worker.js', route => route.fulfill({status: 404, body: 'gone'}));
      // The same worker, speaking a number this page was not written against. Rewriting
      // its own literal is the honest way to stage it: nothing else about the file, or
      // about how the page loads it, is different from a real mismatched deploy.
      if (protocol) await page.route('**/worker.js', async route => {
        const real = await route.fetch();
        const body = (await real.text()).replace(/const PROTOCOL = \d+;/, `const PROTOCOL = ${protocol};`);
        if (!body.includes(`const PROTOCOL = ${protocol};`))
          throw new Error('worker.js no longer declares a PROTOCOL literal this check can change');
        await route.fulfill({status: 200, headers: {'content-type': 'text/javascript'}, body});
      });
      await page.goto(`http://127.0.0.1:${port}${path}`);
      return page;
    };
    const started = Date.now();
    try {
      const said = await checks[name](open);
      // The one request the page makes to anything outside itself is the star count, and
      // it is allowed to fail; nothing else may complain.
      const real = complaints.filter(c => !c.includes('api.github.com') && !c.includes('ERR_ABORTED'));
      if (real.length) throw new Error(real[0]);
      console.log(`ok    ${name.padEnd(12)} ${said} (${((Date.now() - started) / 1000).toFixed(0)}s)`);
    } catch (error) {
      failed++;
      console.error(`FAIL  ${name.padEnd(12)} ${error.message.split('\n')[0]} (${((Date.now() - started) / 1000).toFixed(0)}s)`);
    }
  }
} finally {
  await browser.close();
  server.kill();
}

if (failed) console.error(`\n${failed} of ${chosen.length} checks failed.`);
process.exit(failed ? 1 : 0);
