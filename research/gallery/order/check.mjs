#!/usr/bin/env node
// What the slideshow has to keep doing, checked in Chromium: where it opens, the #id deep
// links, the arrow keys, the ticks, the track's height, the embed, and that nothing runs
// off the side of the page at any width.
//
//   PLAYWRIGHT_MODULE=... CHROME_BIN=... node research/gallery/order/check.mjs [--url http://127.0.0.1:8123/]
import {fileURLToPath} from 'node:url';

const args = process.argv.slice(2);
const flag = (n, d) => { const i = args.indexOf(n); return i < 0 ? d : args[i + 1]; };
const url = flag('--url', 'http://127.0.0.1:8123/');
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});

let bad = 0;
const ok = (name, got, want) => {
  const good = String(got) === String(want);
  if (!good) bad++;
  console.log(`  ${good ? 'ok  ' : 'FAIL'} ${name}: ${got}${good ? '' : ` (wanted ${want})`}`);
};

// A fragment on its own is a same-document navigation, which would leave the page as it
// was; every one of these is meant to be someone arriving fresh, so the page is dropped
// first. (That the strip also follows a fragment typed into an open page is checked below.)
const open = async (page, at = '') => {
  await page.goto('about:blank');
  await page.goto(`${url}${at}`, {waitUntil: 'load'});
  await page.waitForTimeout(500);
};
const state = page => page.evaluate(() => {
  const active = document.querySelector('.slide.active');
  const doc = document.documentElement;
  const track = document.getElementById('track');
  return {
    scene: active?.id.replace('scene-', ''),
    hash: location.hash,
    overflow: doc.scrollWidth - doc.clientWidth,
    trackVsSlide: Math.round(parseFloat(getComputedStyle(track).height) - active.getBoundingClientRect().height),
    tab: document.querySelector('[role="tab"][aria-selected="true"]')?.getAttribute('aria-label'),
    transform3d: getComputedStyle(active).transform.startsWith('matrix3d'),
    widest: Math.max(...[...document.querySelectorAll('.slide')].map(s => s.offsetWidth)),
  };
});

for (const size of [{name: 'wide', width: 1600, height: 1000}, {name: 'desk', width: 1280, height: 900}, {name: 'phone', width: 390, height: 844}]) {
  const context = await browser.newContext({viewport: {width: size.width, height: size.height}, deviceScaleFactor: 1});
  const page = await context.newPage();
  await page.route('**/live.js', route => route.abort());
  console.log(`\n== ${size.name} ${size.width}x${size.height}`);

  await open(page);
  let now = await state(page);
  ok('opens on hardware-team with no hash', now.scene, 'hardware-team');
  ok('and writes no hash', now.hash === '' || now.hash === '#', true);
  ok('no horizontal overflow', now.overflow, 0);
  ok('track height is the active slide', now.trackVsSlide, 0);
  ok('widest slide inside the window', now.widest <= size.width, true);

  await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(700);
  now = await state(page);
  ok('arrow right goes to swe-team', now.scene, 'swe-team');
  ok('and names it in the address bar', now.hash, '#swe-team');
  ok('track height follows', now.trackVsSlide, 0);
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(700);
  now = await state(page);
  ok('two lefts reach art-studio', now.scene, 'art-studio');
  ok('no horizontal overflow after turning', now.overflow, 0);

  await open(page, '#office-team');
  ok('#office-team opens there', (await state(page)).scene, 'office-team');
  await open(page, '#texting');
  ok('#texting still works', (await state(page)).scene, 'texting');
  await open(page, '#no-such-scene');
  ok('an unknown #id falls back to the opening scene', (await state(page)).scene, 'hardware-team');
  await page.evaluate(() => { location.hash = '#gimp'; });
  await page.waitForTimeout(700);
  ok('a fragment typed into an open page turns the strip', (await state(page)).scene, 'gimp');

  await page.evaluate(() => document.querySelectorAll('[role="tab"]')[43].click());
  await page.waitForTimeout(700);
  now = await state(page);
  ok('the last tick is the last scene', now.scene, 'x');
  ok('and the tick is selected', now.tab, 'Posting on X in Safari on macOS');
  ok('no horizontal overflow at the end of the ring', now.overflow, 0);

  // The embed is the same slideshow in a frame, and must not scroll in either direction.
  await open(page, 'embed.html');
  const embed = await page.evaluate(() => ({
    scene: document.querySelector('.slide.active')?.id.replace('scene-', ''),
    x: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    y: document.documentElement.scrollHeight - document.documentElement.clientHeight,
  }));
  ok('embed opens on hardware-team', embed.scene, 'hardware-team');
  ok('embed does not scroll sideways', embed.x, 0);
  ok('embed does not scroll down', embed.y <= 0, true);
  await open(page, 'embed.html#phones-music');
  ok('embed.html#phones-music opens there', (await state(page)).scene, 'phones-music');
  await context.close();
}

// Less motion: the arc is dropped and the plain transform comes back, at every width.
for (const size of [{name: 'desk', width: 1280, height: 900}]) {
  const context = await browser.newContext({viewport: size, deviceScaleFactor: 1, reducedMotion: 'reduce'});
  const page = await context.newPage();
  await page.route('**/live.js', route => route.abort());
  console.log(`\n== reduced motion ${size.width}x${size.height}`);
  await open(page);
  const now = await state(page);
  ok('still opens on hardware-team', now.scene, 'hardware-team');
  ok('no 3D transform on the active slide', now.transform3d, false);
  ok('no horizontal overflow', now.overflow, 0);
  ok('track height is the active slide', now.trackVsSlide, 0);
  await context.close();
}

await browser.close();
console.log(bad ? `\n${bad} failed` : '\nall good');
process.exit(bad ? 1 : 0);
