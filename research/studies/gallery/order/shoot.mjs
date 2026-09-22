#!/usr/bin/env node
// Screenshot the homepage at the opening scene and its four neighbours, at three widths,
// so the new team slides and the order can be looked at.
//
//   PLAYWRIGHT_MODULE=... CHROME_BIN=... node research/studies/gallery/order/shoot.mjs [label]
//     [--url http://127.0.0.1:8123/] [--live] [--reduced] [--flat] [--turn] [--no-posters]
//     [--size desk] [--scene <id> ...]
//
// Writes research/studies/gallery/order/<label>-<width>-<scene>.png. Without --live the simulator
// is never fetched and the tiles keep their stills (fast); with it, every tile on the
// scene is waited for until it is running.
import {mkdir} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const label = args[0] && !args[0].startsWith('--') ? args[0] : 'shot';
const flag = (n, d) => { const i = args.indexOf(n); return i < 0 ? d : args[i + 1]; };
const url = flag('--url', 'http://127.0.0.1:8123/');
const live = args.includes('--live');
const reduced = args.includes('--reduced');
const flat = args.includes('--flat');
const turn = args.includes('--turn');
const noPosters = args.includes('--no-posters');
const scenes = args.reduce((list, a, i) => (a === '--scene' ? [...list, args[i + 1]] : list), []);
const wanted = scenes.length ? scenes : ['x-everywhere', 'art-studio', 'hardware-team', 'swe-team', 'office-team'];

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
await mkdir(here, {recursive: true});

const only = flag('--size', null);   // one of the names below, when all three are not wanted
const sizes = [
  {name: 'wide', width: 1600, height: 1000},
  {name: 'desk', width: 1280, height: 900},
  {name: 'phone', width: 390, height: 844},
].filter(size => !only || size.name === only);

for (const size of sizes) {
  for (const scene of wanted) {
    const context = await browser.newContext({
      viewport: {width: size.width, height: size.height},
      deviceScaleFactor: 1,
      ...(reduced ? {reducedMotion: 'reduce'} : {}),
    });
    const page = await context.newPage();
    // Without --live the simulator is not downloaded at all: what is being judged here is
    // the geometry and the stills, and booting seven machines blocks the tab for minutes.
    if (!live) await page.route('**/live.js', route => route.abort());
    // What a scene whose stills nobody has rendered yet looks like.
    if (noPosters) await page.route('**/media/scenes/*.jpg', route => route.abort());
    const errors = [];
    page.on('pageerror', e => errors.push(String(e)));
    page.on('console', m => m.type() === 'error' && errors.push(`console: ${m.text()}`));
    await page.goto(`${url}#${scene}`, {waitUntil: 'load'});
    await page.evaluate(() => document.querySelectorAll('img').forEach(i => (i.loading = 'eager')));
    if (live) {
      await page.waitForFunction(
        id => { const t = [...document.querySelectorAll(`#scene-${id} .tile`)]; return t.length && t.every(x => x.classList.contains('live')); },
        scene, {timeout: 180_000}).catch(e => errors.push(`live: ${e.message.split('\n')[0]}`));
      await page.evaluate(() => window.computerworldFonts).catch(() => {});
    } else {
      await page.waitForTimeout(1200);
      await page.addStyleTag({content: '.booting { display: none !important; }'});
    }
    if (flat) await page.addStyleTag({content: `
      .track { perspective: none !important; }
      .slide { transform: translate(calc(-50% + var(--x, 0px)), -50%) scale(.94) !important; }
      .slide.active { transform: translate(calc(-50% + var(--x, 0px)), -50%) !important; }`});
    await page.waitForTimeout(700);
    // Mid-turn, when the strip is half way to the next machine and the arc is doing its
    // work, rather than at rest.
    if (turn) {
      await page.click('#next');
      await page.waitForTimeout(220);
    }
    await page.screenshot({path: resolve(here, `${label}-${size.name}-${scene}.png`)});

    const state = await page.evaluate(id => {
      const doc = document.documentElement;
      const active = document.querySelector('.slide.active');
      const box = active.getBoundingClientRect();
      return {
        hash: location.hash,
        opensOn: active.id,
        overflow: doc.scrollWidth - doc.clientWidth,
        slide: `${Math.round(box.width)}x${Math.round(box.height)} left=${Math.round(box.left)} right=${Math.round(box.right)}`,
        track: getComputedStyle(document.getElementById('track')).height,
        tiles: [...active.querySelectorAll('.tile')].map(t => {
          const r = t.getBoundingClientRect();
          return `${t.dataset.machine} ${Math.round(r.width)}x${Math.round(r.height)}`;
        }),
        blank: [...active.querySelectorAll('.tile.blank')].length,
        near: [...document.querySelectorAll('.slide')].filter(s => !s.classList.contains('far')).length,
      };
    }, scene);
    console.log(`\n== ${label} ${size.name} ${size.width}x${size.height} #${scene}`);
    console.log(`   opens on ${state.opensOn}  hash=${state.hash}  page overflow=${state.overflow}px  track=${state.track}`);
    console.log(`   slide ${state.slide}  drawn slides=${state.near}  blank tiles=${state.blank}`);
    console.log(`   ${state.tiles.join('  ')}`);
    if (errors.length) console.log('   errors:', errors.slice(0, 4));
    await context.close();
  }
}
await browser.close();
