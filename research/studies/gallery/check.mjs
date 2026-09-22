#!/usr/bin/env node
// The things the arc must not have broken: the keyboard, the ticks, the arrows, the
// focus ring, the page's own scrolling, and the console.
const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
const url = process.argv[2] ?? 'http://localhost:8123/';

for (const reduced of [false, true]) {
  const context = await browser.newContext({viewport: {width: 1280, height: 800}, ...(reduced ? {reducedMotion: 'reduce'} : {})});
  const page = await context.newPage();
  const errors = [];
  page.on('pageerror', e => errors.push(String(e)));
  page.on('console', m => m.type() === 'error' && errors.push(m.text()));
  await page.goto(url, {waitUntil: 'load'});
  await page.waitForTimeout(1200);

  const selected = () => page.evaluate(() => [...document.querySelectorAll('.ticks button')].findIndex(b => b.getAttribute('aria-selected') === 'true'));
  const hash = () => page.evaluate(() => location.hash);
  const report = {};
  report.start = await selected();

  await page.keyboard.press('ArrowRight');
  await page.waitForTimeout(700);
  report.afterArrowRight = await selected();
  await page.keyboard.press('ArrowLeft');
  await page.keyboard.press('ArrowLeft');
  await page.waitForTimeout(700);
  report.afterTwoArrowLefts = await selected();      // wraps to the end of the ring
  report.hash = await hash();

  await page.click('#next');
  await page.waitForTimeout(700);
  report.afterNextButton = await selected();
  await page.evaluate(() => document.querySelectorAll('.ticks button')[4].click());
  await page.waitForTimeout(700);
  report.afterTickClick = await selected();

  // Tab to the pager and check the focus ring is drawn on the tick that has focus.
  await page.keyboard.press('Tab'); await page.keyboard.press('Tab');
  await page.keyboard.press('Tab'); await page.keyboard.press('Tab');
  report.focus = await page.evaluate(() => {
    const el = document.activeElement;
    return {tag: el.tagName, role: el.getAttribute('role'), outline: getComputedStyle(el).outlineColor, matches: el.matches(':focus-visible')};
  });

  // Clicking a neighbour still brings it to the middle: the turned tiles are hit where
  // they are drawn, so aim at the sliver of the left one.
  const before = await selected();
  await page.mouse.click(60, 430);
  await page.waitForTimeout(700);
  report.clickedLeftNeighbour = {from: before, to: await selected()};

  report.pageScrolls = await page.evaluate(async () => {
    window.scrollTo(0, 400); await new Promise(r => requestAnimationFrame(r));
    const y = window.scrollY; window.scrollTo(0, 0);
    return {scrolledTo: y, horizontalOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth};
  });
  report.trackHeightSet = await page.evaluate(() => !!document.getElementById('track').style.height);
  report.errors = errors;
  console.log(`\n== ${reduced ? 'reduced motion' : 'normal'}`);
  console.log(JSON.stringify(report, null, 2));
  await context.close();
}
await browser.close();
