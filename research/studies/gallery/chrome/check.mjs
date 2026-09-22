#!/usr/bin/env node
/** What the slideshow has to keep doing, checked in Chromium — the things the last round
 * established, plus the three the overlay, the caption and the fullscreen control add.
 *
 *   PLAYWRIGHT_MODULE=… CHROME_BIN=… node research/studies/gallery/chrome/check.mjs [--url http://127.0.0.1:8123/]
 */
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
const note = (name, got) => console.log(`  --   ${name}: ${got}`);

// Every scene that has ever had a machine come up in it, counted from before app.js runs,
// so "twenty keypresses, two worlds" is a count of worlds actually built.
const watch = page => page.addInitScript(() => {
  window.__started = new Set();
  new MutationObserver(records => records.forEach(record => {
    const el = record.target;
    if (el.classList?.contains('live') && el.classList.contains('tile'))
      window.__started.add(el.closest('.slide')?.id ?? '?');
  })).observe(document, {subtree: true, attributes: true, attributeFilter: ['class']});
});

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
    transform3d: getComputedStyle(active).transform.startsWith('matrix3d'),
    // The slide in the middle is square on, so its own transform is flat by construction;
    // the arc is whether the one beside it is set back and turned.
    neighbour3d: (() => {
      const all = [...document.querySelectorAll('.slide')];
      const near = all[(all.indexOf(active) + 1) % all.length];
      return getComputedStyle(near).transform.startsWith('matrix3d');
    })(),
    inertHere: [...active.querySelectorAll('.tile')].some(t => t.inert),
    rot: getComputedStyle(document.querySelectorAll('.slide')[0]).getPropertyValue('--rot').trim(),
    widest: Math.max(...[...document.querySelectorAll('.slide')].map(s => s.offsetWidth)),
    caption: (() => {
      const b = document.getElementById('caption').getBoundingClientRect();
      const t = document.getElementById('caption-title').getBoundingClientRect();
      const s = document.getElementById('caption-summary').getBoundingClientRect();
      const type = el => {
        const c = getComputedStyle(el);
        return {size: parseFloat(c.fontSize), weight: c.fontWeight, colour: c.color, mono: c.fontFamily.includes('mono') || c.fontFamily.includes('Menlo')};
      };
      return {
        // Baselines, not tops: the two are set at different sizes, so "on one line"
        // is their boxes overlapping vertically, not their tops agreeing.
        height: Math.round(b.height), lines: t.top < s.bottom && s.top < t.bottom ? 1 : 2,
        title: type(document.getElementById('caption-title')),
        summary: type(document.getElementById('caption-summary')),
      };
    })(),
  };
});

// ------------------------------------------------------- the strip, at the three widths
for (const size of [{name: 'wide', width: 1600, height: 1000}, {name: 'desk', width: 1280, height: 900}, {name: 'phone', width: 390, height: 844}]) {
  const context = await browser.newContext({viewport: {width: size.width, height: size.height}, deviceScaleFactor: 1});
  const page = await context.newPage();
  await page.route('**/live.js', route => route.abort());   // geometry only; no 35 MB
  console.log(`\n== ${size.name} ${size.width}x${size.height}`);

  await open(page);
  let now = await state(page);
  ok('opens on hardware-team with no hash', now.scene, 'hardware-team');
  ok('and nothing on that slide is inert before it is turned', now.inertHere, false);
  ok('no horizontal overflow', now.overflow, 0);
  ok('track height is the active slide', now.trackVsSlide, 0);
  ok('widest slide inside the window', now.widest <= size.width, true);

  // The caption: one object, two kinds of thing, and a reserved height that does not step.
  const cap = now.caption;
  ok('caption title is the larger', cap.title.size > cap.summary.size * 1.4, true);
  ok('caption title is not monospace', cap.title.mono, false);
  ok('caption summary is monospace', cap.summary.mono, true);
  ok('caption title is the brighter', cap.title.colour !== cap.summary.colour, true);
  note('title / summary px', `${cap.title.size} / ${cap.summary.size} (weight ${cap.title.weight} / ${cap.summary.weight})`);
  ok(size.name === 'phone' ? 'caption wraps here' : 'caption is one line here', cap.lines, size.name === 'phone' ? 2 : 1);
  const heights = [];
  for (let n = 0; n < 8; n++) {
    heights.push((await state(page)).caption.height);
    await page.keyboard.press('ArrowRight');
    await page.waitForTimeout(400);
  }
  ok('caption height never steps', new Set(heights).size, 1);
  note('reserved caption height', `${heights[0]}px`);

  await open(page);
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

  // Turning is a transform and nothing else. One turn per frame, which is what a held
  // arrow key is; the single worst sample on a loaded machine is the machine, so what is
  // asserted is where the distribution sits.
  const turns = await page.evaluate(async () => {
    for (let n = 0; n < 10; n++) { await new Promise(r => requestAnimationFrame(r)); document.getElementById('next').click(); }
    const times = [];
    for (let n = 0; n < 40; n++) {
      await new Promise(r => requestAnimationFrame(r));
      const t = performance.now();
      document.getElementById('next').click();
      times.push(performance.now() - t);
    }
    return times.sort((a, b) => a - b);
  });
  const at = q => turns[Math.floor(q * (turns.length - 1))];
  ok('turning stays in single-digit milliseconds', at(0.5) < 10 && at(0.9) < 10, true);
  note('turn median / p90 / worst ms', `${at(0.5).toFixed(2)} / ${at(0.9).toFixed(2)} / ${turns.at(-1).toFixed(2)}`);

  await open(page, '#office-team');
  ok('#office-team opens there', (await state(page)).scene, 'office-team');
  await open(page, '#texting');
  ok('#texting still works', (await state(page)).scene, 'texting');
  await open(page, '#no-such-scene');
  ok('an unknown #id falls back to the opening scene', (await state(page)).scene, 'hardware-team');
  await page.evaluate(() => { location.hash = '#gimp'; });
  await page.waitForTimeout(700);
  ok('a fragment typed into an open page turns the strip', (await state(page)).scene, 'gimp');

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
  note('embed slack', `${-embed.y}px`);
  await open(page, 'embed.html#phones-music');
  ok('embed.html#phones-music opens there', (await state(page)).scene, 'phones-music');
  await context.close();
}

// ------------------------------------------------------- no horizontal overflow, anywhere
{
  const context = await browser.newContext({viewport: {width: 1280, height: 900}, deviceScaleFactor: 1});
  const page = await context.newPage();
  await page.route('**/live.js', route => route.abort());
  console.log('\n== every width from 320 to 1920');
  await open(page);
  let over = [];
  for (let w = 320; w <= 1920; w += 40) {
    await page.setViewportSize({width: w, height: 900});
    await page.waitForTimeout(90);
    const x = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
    if (x > 0) over.push(`${w}:${x}`);
  }
  ok('no width overflows', over.join(',') || 'none', 'none');
  await context.close();
}

// ---------------------------------------------- less motion, and a browser with no depth
for (const kind of ['reduced', 'flat']) {
  const context = await browser.newContext({viewport: {width: 1280, height: 900}, deviceScaleFactor: 1, ...(kind === 'reduced' ? {reducedMotion: 'reduce'} : {})});
  const page = await context.newPage();
  await page.route('**/live.js', route => route.abort());
  console.log(`\n== ${kind === 'reduced' ? 'less motion' : 'no perspective'} 1280x900`);
  await open(page);
  if (kind === 'flat') await page.addStyleTag({content: `
    .track { perspective: none !important; }
    .slide { transform: translate(calc(-50% + var(--x, 0px)), -50%) scale(.94) !important; }
    .slide.active { transform: translate(calc(-50% + var(--x, 0px)), -50%) !important; }`});
  // Past the .55s the strip takes to settle: a transform mid-transition is interpolated
  // as a 3D matrix whatever it started and ended as.
  await page.waitForTimeout(1000);
  const now = await state(page);
  ok('still opens on hardware-team', now.scene, 'hardware-team');
  ok('no 3D transform anywhere on the strip', now.transform3d || now.neighbour3d, false);
  ok('no horizontal overflow', now.overflow, 0);
  ok('track height is the active slide', now.trackVsSlide, 0);
  await context.close();
}

// ----------------------------------------------------- the arc is there when it can be
{
  const context = await browser.newContext({viewport: {width: 1280, height: 900}, deviceScaleFactor: 1});
  const page = await context.newPage();
  await page.route('**/live.js', route => route.abort());
  console.log('\n== the arc 1280x900');
  await open(page);
  const now = await state(page);
  ok('the slide beside the middle is set back in 3D', now.neighbour3d, true);
  ok('a neighbour is turned', /-?\d/.test(now.rot) && parseFloat(now.rot) !== 0, true);
  note('turn one place out', now.rot);
  await context.close();
}

// ----------------------------------------- the stills, and how many worlds get built
{
  const context = await browser.newContext({viewport: {width: 1280, height: 900}, deviceScaleFactor: 1});
  const page = await context.newPage();
  await watch(page);
  console.log('\n== stills, and twenty held keypresses');
  await open(page);
  await page.waitForTimeout(6000);
  const stills = await page.evaluate(() => {
    const tiles = [...document.querySelectorAll('.tile')];
    const shown = tiles.map(t => t.querySelector('img')).filter(Boolean);
    return {
      tiles: tiles.length, stills: shown.length,
      live: tiles.filter(t => t.classList.contains('live')).length,
      asked: shown.filter(i => i.src).length,
      decoded: shown.filter(i => i.complete && i.naturalWidth > 0).length,
      nearReady: [...document.querySelectorAll('.slide.active .tile img')].every(i => i.ready === true),
    };
  });
  ok('every tile is a machine or a still', stills.stills + stills.live, stills.tiles);
  ok('every still has been asked for', stills.asked, stills.stills);
  ok('and has arrived', stills.decoded, stills.stills);
  ok('the ones under the visitor are decoded ahead', stills.nearReady, true);
  note('stills', `${stills.decoded} of ${stills.stills} in, ${stills.live} tiles already running`);

  // Twenty presses at a key-repeat rate, then a stop. What is counted is what the burst
  // itself cost: the scene it opened on, and the neighbour that came up beside it while
  // nothing was happening, were already built before a key was touched.
  const before = await page.evaluate(() => [...window.__started]);
  for (let n = 0; n < 20; n++) { await page.keyboard.press('ArrowRight'); await page.waitForTimeout(32); }
  const during = await page.evaluate(() => [...window.__started]);
  ok('nothing is built while the key is held', during.length, before.length);
  await page.waitForFunction(() => document.querySelectorAll('.slide.active .tile.live').length > 0, {timeout: 240000});
  await page.waitForTimeout(1200);
  const after = await page.evaluate(() => ({started: [...window.__started], scene: document.querySelector('.slide.active').id}));
  ok('twenty keypresses build two worlds, not twenty-two', after.started.length - before.length <= 2, true);
  note('worlds built', `${before.length} before the burst, ${after.started.length} after: ${after.started.join(', ')}  (stopped on ${after.scene})`);
  await context.close();
}

// ---------------------------------------------------------- the fullscreen control
{
  const context = await browser.newContext({viewport: {width: 1280, height: 900}, deviceScaleFactor: 1});
  const page = await context.newPage();
  console.log('\n== one machine, the whole screen');
  await open(page);
  await page.waitForFunction(() => document.querySelectorAll('.slide.active .tile.live').length > 0, {timeout: 240000});
  await page.waitForTimeout(400);

  const before = await page.evaluate(() => ({
    track: document.getElementById('track').style.height,
    transform: getComputedStyle(document.querySelector('.slide.active')).transform,
    tile: document.querySelector('.slide.active .tile.live').style.height,
  }));

  const named = await page.evaluate(() => {
    const tile = document.querySelector('.slide.active .tile.live');
    return {name: tile.querySelector('.grow').getAttribute('aria-label'), label: tile.dataset.label};
  });
  ok('the control has a name', named.name, `Fullscreen — ${named.label}`);
  note('the control is called', `"${named.name}"`);
  ok('a still carries no control in the tab order', await page.evaluate(() => {
    const tile = [...document.querySelectorAll('.tile')].find(t => !t.classList.contains('live'));
    return getComputedStyle(tile.querySelector('.grow')).display;
  }), 'none');
  ok('a live machine shows one', await page.evaluate(() => getComputedStyle(document.querySelector('.slide.active .tile.live .grow')).display), 'grid');

  // Keyboard: tab into the strip and take the control from there.
  const reached = await page.evaluate(() => {
    const button = document.querySelector('.slide.active .tile.live .grow');
    button.focus();
    return {took: document.activeElement === button, tabbable: button.tabIndex >= 0};
  });
  ok('the control takes focus', reached.took, true);
  ok('and is in the tab order', reached.tabbable, true);
  await page.keyboard.press('Enter');
  await page.waitForTimeout(900);
  const inside = await page.evaluate(() => {
    const tile = document.querySelector('.tile.full');
    const canvas = tile?.querySelector('canvas');
    const box = canvas?.getBoundingClientRect();
    return {
      api: !!tile && document.fullscreenElement === tile, lifted: !!document.querySelector('.fs-host'),
      w: box && Math.round(box.width), h: box && Math.round(box.height),
      vw: innerWidth, vh: innerHeight,
      ar: canvas && (canvas.width / canvas.height).toFixed(3),
      shown: box && (box.width / box.height).toFixed(3),
      focus: document.activeElement?.tagName,
      name: tile?.querySelector('.grow').getAttribute('aria-label'),
    };
  });
  ok('the Fullscreen API took it', inside.api, true);
  ok('the canvas keeps its own shape', inside.shown, inside.ar);
  ok('and fills the viewport in one direction', inside.w === inside.vw || inside.h === inside.vh, true);
  ok('the machine has the keyboard', inside.focus, 'CANVAS');
  ok('the control now says how to leave', inside.name, 'Leave fullscreen');
  note('canvas in fullscreen', `${inside.w}x${inside.h} of ${inside.vw}x${inside.vh}`);

  // The machine still takes input: click into it and see the world answer.
  const moved = await page.evaluate(async () => {
    const canvas = document.querySelector('.tile.full canvas');
    const before = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
    canvas.dispatchEvent(new PointerEvent('pointerdown', {bubbles: true, clientX: innerWidth / 2, clientY: innerHeight / 2, pointerId: 1, button: 0}));
    canvas.dispatchEvent(new PointerEvent('pointerup', {bubbles: true, clientX: innerWidth / 2, clientY: innerHeight / 2, pointerId: 1, button: 0}));
    await new Promise(r => setTimeout(r, 700));
    const after = canvas.getContext('2d').getImageData(0, 0, canvas.width, canvas.height).data;
    let different = 0;
    for (let i = 0; i < before.length; i += 4001) if (before[i] !== after[i]) different++;
    return different;
  });
  ok('the machine answers a click while it has the screen', moved > 0, true);

  // Arrowing while one machine has the screen leaves the strip where it is.
  const held = await page.evaluate(() => document.querySelector('.slide.active').id);
  await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(600);
  ok('the strip does not turn under a fullscreen machine', await page.evaluate(() => document.querySelector('.slide.active').id), held);

  await page.keyboard.press('Escape');
  await page.waitForTimeout(1200);
  const back = await page.evaluate(() => ({
    full: !!document.querySelector('.tile.full'), api: !!document.fullscreenElement, host: !!document.querySelector('.fs-host'),
    track: document.getElementById('track').style.height,
    transform: getComputedStyle(document.querySelector('.slide.active')).transform,
    tile: document.querySelector('.slide.active .tile.live').style.height,
    focus: document.activeElement?.className,
    overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
  }));
  ok('Escape gives the screen back', back.full || back.api || back.host, false);
  ok('the track is the height it was', back.track, before.track);
  ok('the arc is the transform it was', back.transform, before.transform);
  ok('the tile is the size it was', back.tile, before.tile);
  ok('focus comes back to the control', back.focus, 'grow');
  ok('and nothing runs off the side', back.overflow, 0);

  // The way in when the API is not on offer: the same picture, built by hand.
  await page.evaluate(() => {
    Element.prototype.requestFullscreen = () => Promise.reject(new Error('not allowed here'));
  });
  await page.click('.slide.active .tile.live .grow');
  await page.waitForTimeout(700);
  const lifted = await page.evaluate(() => {
    const tile = document.querySelector('.tile.full');
    const box = tile?.querySelector('canvas').getBoundingClientRect();
    return {
      host: !!document.querySelector('.fs-host'), api: !!document.fullscreenElement,
      w: box && Math.round(box.width), h: box && Math.round(box.height), vw: innerWidth, vh: innerHeight,
      gap: !!document.querySelector('.slide.active .tile[style*="visibility"]'),
      focus: document.activeElement?.tagName,
    };
  });
  ok('the scripted way in takes over', lifted.host, true);
  ok('without the API', lifted.api, false);
  ok('the canvas still fills one direction', lifted.w === lifted.vw || lifted.h === lifted.vh, true);
  ok('a hidden copy holds its place in the slide', lifted.gap, true);
  ok('the machine has the keyboard', lifted.focus, 'CANVAS');
  await page.keyboard.press('Escape');
  await page.waitForTimeout(900);
  const back2 = await page.evaluate(() => ({
    host: !!document.querySelector('.fs-host'),
    track: document.getElementById('track').style.height,
    transform: getComputedStyle(document.querySelector('.slide.active')).transform,
    live: document.querySelectorAll('.slide.active .tile.live').length,
    overflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
  }));
  ok('Escape puts the lifted machine back', back2.host, false);
  ok('the track is the height it was', back2.track, before.track);
  ok('the arc is the transform it was', back2.transform, before.transform);
  ok('the machine is still live', back2.live > 0, true);
  ok('and nothing runs off the side', back2.overflow, 0);
  await context.close();
}

await browser.close();
console.log(bad ? `\n${bad} failed` : '\nall good');
process.exit(bad ? 1 : 0);
