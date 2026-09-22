#!/usr/bin/env node
// Screenshot the homepage gallery at a few positions, so the arc can be looked at.
//
//   PLAYWRIGHT_MODULE=... CHROME_BIN=... node research/studies/gallery/shoot.mjs <label> [--url http://localhost:8123/]
//
// Writes research/studies/gallery/<label>-{desk-rest,desk-moving,desk-next,phone-rest,phone-moving}.png
import {mkdir} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const label = args[0] ?? 'shot';
const flag = (n, d) => { const i = args.indexOf(n); return i < 0 ? d : args[i + 1]; };
const url = flag('--url', 'http://localhost:8123/');
const reduced = args.includes('--reduced');

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
await mkdir(here, {recursive: true});

const sizes = [
  {name: 'desk', width: 1280, height: 800},
  {name: 'wide', width: 1600, height: 720},
  {name: 'phone', width: 390, height: 844},
];

for (const size of sizes) {
  const context = await browser.newContext({
    viewport: {width: size.width, height: size.height},
    deviceScaleFactor: 1,
    ...(reduced ? {reducedMotion: 'reduce'} : {}),
  });
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(String(e)));
  await page.goto(url, {waitUntil: 'load'});
  // The stills are what we are judging; give the lazy images a moment, not the wasm.
  await page.waitForTimeout(1200);
  await page.evaluate(() => document.querySelectorAll('img').forEach(i => i.loading = 'eager'));
  // The wasm is not built in this checkout, so every tile would carry the "could not
  // start" note over the still. Take it away: what is being looked at here is geometry.
  await page.addStyleTag({content: '.booting { display: none !important; }'});
  // What a browser that does not understand `perspective` sees: the @supports block never
  // applies, so the track has no depth and each slide keeps the plain transform under it.
  if (process.env.FLAT) await page.addStyleTag({content: `
    .track { perspective: none !important; }
    .slide { transform: translate(calc(-50% + var(--x, 0px)), -50%) scale(.94) !important; }
    .slide.active { transform: translate(calc(-50% + var(--x, 0px)), -50%) !important; }`});
  await page.waitForTimeout(600);

  const shot = n => page.screenshot({path: resolve(here, `${label}-${size.name}-${n}.png`)});
  await shot('rest');

  // Mid-turn: the strip is half way to the next machine.
  await page.click('#next');
  await page.waitForTimeout(220);
  await shot('moving');

  await page.waitForTimeout(900);
  await shot('next');

  const state = await page.evaluate(() => {
    const track = document.getElementById('track');
    return {
      perspective: getComputedStyle(track).perspective,
      slides: [...document.querySelectorAll('.slide')].slice(0, 6).map(s => ({
        x: s.style.getPropertyValue('--x'),
        rot: getComputedStyle(s).getPropertyValue('--rot'),
        transform: getComputedStyle(s).transform,
        active: s.classList.contains('active'),
        far: s.classList.contains('far'),
      })),
    };
  });
  console.log(`\n== ${label} ${size.name} ${size.width}x${size.height}${reduced ? ' (reduced motion)' : ''}`);
  console.log('track perspective:', state.perspective);
  for (const s of state.slides) console.log(`  x=${s.x.padStart(8)} rot=${(s.rot || '(none)').padStart(9)} active=${s.active} far=${s.far} ${s.transform}`);
  if (errors.length) console.log('  page errors:', errors.slice(0, 3));
  await context.close();
}
await browser.close();
