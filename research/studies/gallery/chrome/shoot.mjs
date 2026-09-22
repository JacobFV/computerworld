#!/usr/bin/env node
/** Pictures of the three changes, taken in Chromium with the cache off and the line
 * throttled so the download overlay is actually on screen long enough to be looked at.
 *
 *   PLAYWRIGHT_MODULE=… CHROME_BIN=… node research/studies/gallery/chrome/shoot.mjs [what…]
 *
 * what: overlay | nonumber | savedata | caption | fullscreen | embed   (default: all)
 */
import {mkdir} from 'node:fs/promises';
import {resolve, dirname} from 'node:path';
import {fileURLToPath} from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
await mkdir(here, {recursive: true});
const want = process.argv.slice(2);
const doing = name => want.length === 0 || want.includes(name);
const site = 'http://127.0.0.1:8123/';
const gzipped = 'http://127.0.0.1:8124/';

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN, args: ['--allow-running-insecure-content']});

/** A page with no cache and a line you can watch bytes arrive over. */
async function fresh(size, {bytesPerSecond = 0, saveData = false, url = site, at = '', stills = false} = {}) {
  const context = await browser.newContext({
    viewport: {width: size.width, height: size.height},
    deviceScaleFactor: 1,
    ...(saveData ? {extraHTTPHeaders: {'save-data': 'on'}} : {}),
  });
  const page = await context.newPage();
  // Some of these pictures are of type, not of machines; the 35 MB stays on the shelf.
  if (stills) await page.route('**/live.js', route => route.abort());
  if (saveData) await page.addInitScript(() => {
    Object.defineProperty(navigator, 'connection', {value: {saveData: true}, configurable: true});
  });
  const cdp = await context.newCDPSession(page);
  await cdp.send('Network.enable');
  await cdp.send('Network.setCacheDisabled', {cacheDisabled: true});
  if (bytesPerSecond) await cdp.send('Network.emulateNetworkConditions', {
    offline: false, latency: 20, downloadThroughput: bytesPerSecond, uploadThroughput: bytesPerSecond,
  });
  page.on('pageerror', e => console.log('  ! pageerror', String(e).slice(0, 200)));
  await page.goto(url + at, {waitUntil: 'commit'});
  return {context, page};
}

/** A picture of the whole viewport, or of the box one element occupies in it. Taken with
 * a clip rather than off the element: the track's height is on a transition, so an element
 * screenshot waits for a box that is still settling. */
async function shot(page, name, target) {
  const path = resolve(here, `${name}.png`);
  // A world being built holds the main thread for seconds at a time, which is the whole
  // point of the picture being taken; give the screenshot room to wait for it.
  const patience = {timeout: 120000};
  if (!target) return page.screenshot({path, ...patience});
  const box = await page.locator(target).first().boundingBox(patience);
  const clip = {
    x: Math.max(0, Math.floor(box.x)), y: Math.max(0, Math.floor(box.y)),
    width: Math.ceil(box.width), height: Math.ceil(box.height),
  };
  const view = page.viewportSize();
  clip.width = Math.min(clip.width, view.width - clip.x);
  clip.height = Math.min(clip.height, view.height - clip.y);
  return page.screenshot({path, clip, ...patience});
}

// ---------------------------------------------------------------- the download overlay
if (doing('overlay')) {
  console.log('== overlay, 1280x900, 3 MB/s, cache off');
  const {context, page} = await fresh({width: 1280, height: 900}, {bytesPerSecond: 3e6});
  const seen = [];
  for (let n = 0; n < 200; n++) {
    const now = await page.evaluate(() => ({
      phase: document.documentElement.dataset.boot ?? '',
      said: document.querySelector('.slide.active .pct')?.textContent ?? '',
    }));
    const percent = Number((/(\d+)%/.exec(now.said) ?? [])[1]);
    if (Number.isFinite(percent) && !seen.length && percent >= 1) seen.push(percent);
    else if (Number.isFinite(percent) && percent >= seen.at(-1) + 25) seen.push(percent);
    else { await page.waitForTimeout(250); continue; }
    await shot(page, `overlay-${String(seen.at(-1)).padStart(2, '0')}pct`, '#stage');
    console.log(`   ${now.phase}  "${now.said}"`);
    if (seen.length >= 4) break;
    await page.waitForTimeout(250);
  }
  // And the moment after: the bytes are in, a world is being built, no number is claimed.
  for (let n = 0; n < 120; n++) {
    const phase = await page.evaluate(() => document.documentElement.dataset.boot ?? '');
    if (phase === 'work') { await shot(page, 'overlay-building', '#stage'); console.log('   work'); break; }
    if (!phase) break;
    await page.waitForTimeout(200);
  }
  await page.waitForFunction(() => document.querySelectorAll('.slide.active .tile.live').length > 0, {timeout: 120000});
  await page.waitForTimeout(500);
  await shot(page, 'overlay-gone', '#stage');
  await context.close();
}

// ------------------------------------------------- the same, where nothing can be counted
if (doing('nonumber')) {
  console.log('== overlay with a gzipping host: no honest denominator, so no number');
  const {context, page} = await fresh({width: 1280, height: 900}, {bytesPerSecond: 3e6, url: gzipped});
  for (let n = 0; n < 200; n++) {
    const now = await page.evaluate(() => ({
      phase: document.documentElement.dataset.boot ?? '',
      said: document.querySelector('.slide.active .pct')?.textContent ?? '',
    }));
    if (now.phase === 'load') {
      await page.waitForTimeout(2500);
      const after = await page.evaluate(() => document.querySelector('.slide.active .pct')?.textContent ?? '');
      await shot(page, 'overlay-no-number', '#stage');
      console.log(`   phase=load  label=${JSON.stringify(after)}`);
      break;
    }
    await page.waitForTimeout(200);
  }
  await context.close();
}

// --------------------------------------------------------------------------- Save-Data
if (doing('savedata')) {
  console.log('== Save-Data: the same overlay, carrying the one control that starts it');
  const {context, page} = await fresh({width: 1280, height: 900}, {bytesPerSecond: 3e6, saveData: true});
  await page.waitForFunction(() => document.documentElement.dataset.boot === 'ask', {timeout: 60000});
  await page.waitForTimeout(400);
  await shot(page, 'savedata-ask', '#stage');
  console.log('   ask:', await page.evaluate(() => document.querySelector('.slide.active .get')?.getAttribute('aria-label')));
  await page.click('.slide.active .tile .get');
  await page.waitForFunction(() => document.documentElement.dataset.boot !== 'ask', {timeout: 30000});
  await page.waitForTimeout(1500);
  await shot(page, 'savedata-going', '#stage');
  console.log('   after the tap:', await page.evaluate(() => [document.documentElement.dataset.boot, document.querySelector('.slide.active .pct')?.textContent]));
  await context.close();
}

// ----------------------------------------------------------------------------- caption
if (doing('caption')) {
  for (const size of [{name: '1600', width: 1600, height: 1000}, {name: '1280', width: 1280, height: 900}, {name: '390', width: 390, height: 844}]) {
    console.log(`== caption ${size.name}`);
    const {context, page} = await fresh(size, {stills: true});
    await page.waitForTimeout(1500);
    await shot(page, `caption-${size.name}`, '.pager');
    await shot(page, `caption-${size.name}-page`);
    // The longest summary in the cast, so the worst case is the one in the picture.
    await page.evaluate(() => { location.hash = '#art-studio'; });
    await page.waitForTimeout(800);
    await shot(page, `caption-${size.name}-longest`, '.pager');
    await context.close();
  }
}

// -------------------------------------------------------------------------- fullscreen
if (doing('fullscreen')) {
  console.log('== the fullscreen control, and a machine using it');
  const {context, page} = await fresh({width: 1280, height: 900});
  await page.waitForFunction(() => document.querySelectorAll('.slide.active .tile.live').length > 0, {timeout: 180000});
  await page.waitForTimeout(800);
  await shot(page, 'fullscreen-control', '#stage');
  await shot(page, 'fullscreen-control-tile', '.slide.active .tile.live');
  await page.click('.slide.active .tile.live .grow');
  await page.waitForTimeout(900);
  console.log('   in:', await page.evaluate(() => ({
    api: !!document.fullscreenElement,
    lifted: !!document.querySelector('.fs-host'),
    canvas: (() => { const c = document.querySelector('.tile.full canvas')?.getBoundingClientRect(); return c && {w: Math.round(c.width), h: Math.round(c.height)}; })(),
    focus: document.activeElement?.tagName,
  })));
  await shot(page, 'fullscreen-machine');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(900);
  await shot(page, 'fullscreen-after-escape', '#stage');
  await context.close();
}

// ------------------------------------------------------------------------------- embed
if (doing('embed')) {
  console.log('== embed.html in a frame, allowed and sandboxed');
  for (const [name, attrs] of [
    ['allowed', 'allow="fullscreen"'],
    ['sandboxed', 'sandbox="allow-scripts allow-same-origin"'],
    // A frame told it may not have the screen. The API says no, and the machine fills
    // the frame it was given instead — which is what the fallback is for.
    ['refused', 'allow="fullscreen \'none\'"'],
  ]) {
    const {context, page} = await fresh({width: 1100, height: 760}, {url: `${site}framed.html?how=${encodeURIComponent(attrs)}`});
    const frame = page.frameLocator('iframe');
    await frame.locator('.slide.active .tile.live').first().waitFor({timeout: 180000});
    await page.waitForTimeout(700);
    await shot(page, `embed-${name}-control`);
    await frame.locator('.slide.active .tile.live .grow').first().click();
    await page.waitForTimeout(1000);
    const how = await page.evaluate(() => !!document.fullscreenElement);
    const inside = await frame.locator('.tile.full').first().evaluate(tile => ({
      lifted: !!tile.closest('.fs-host'),
      box: (b => `${Math.round(b.width)}x${Math.round(b.height)}`)(tile.querySelector('canvas').getBoundingClientRect()),
      frame: `${innerWidth}x${innerHeight}`,
    })).catch(() => null);
    console.log(`   ${name}: the host page's fullscreenElement = ${how}; inside the frame ${JSON.stringify(inside)}`);
    await shot(page, `embed-${name}-fullscreen`);
    await page.keyboard.press('Escape');
    await page.waitForTimeout(900);
    await shot(page, `embed-${name}-after-escape`);
    await context.close();
  }
}

await browser.close();
console.log('\nwrote research/studies/gallery/chrome/');
