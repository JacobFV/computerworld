/** Render the still each machine on the project site shows before it is running.
 *
 * The stills are not drawn separately: this opens the site in Chrome, lets every scene in
 * site/cast.js boot exactly as a visitor's tab boots it, and saves each machine's canvas
 * to site/media/scenes/<machine>.jpg. Each scene is opened in a page of its own, so a
 * still shows what a visitor sees who lands on that scene first.
 *
 *   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
 *     node scripts/render-site-stills.mjs [scene-id ...]
 *
 * The machines need the Wasm bundle in site/demo/ (see site/README.md).
 */
import {spawn} from 'node:child_process';
import {mkdir, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const port = 8765;
const {cast} = await import(new URL('../site/cast.js', import.meta.url));
const wanted = process.argv.slice(2);
const scenes = wanted.length ? cast.filter(scene => wanted.includes(scene.id)) : cast;

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const server = spawn(process.execPath, [`${root}scripts/serve-site.mjs`, String(port)], {stdio: 'ignore'});
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
await mkdir(`${root}site/media/scenes`, {recursive: true});

let failed = 0;
try {
  for (const scene of scenes) {
    const page = await browser.newPage({viewport: {width: 1440, height: 900}});
    page.on('console', message => message.type() === 'warning' && console.warn(`  ${scene.id}: ${message.text()}`));
    await page.goto(`http://127.0.0.1:${port}/#${scene.id}`);
    try {
      await page.waitForFunction(
        id => { const tiles = [...document.querySelectorAll(`#scene-${id} .tile`)]; return tiles.length && tiles.every(t => t.classList.contains('live')); },
        scene.id, {timeout: 60_000});
      for (const machine of scene.machines) {
        // Desktops are saved at three quarters of their size: the still is a placeholder
        // behind a dimming filter, and there are dozens of them.
        const jpeg = await page.evaluate(({id, scale}) => {
          const canvas = document.querySelector(`[data-machine="${id}"] canvas`);
          const still = document.createElement('canvas');
          still.width = Math.round(canvas.width * scale);
          still.height = Math.round(canvas.height * scale);
          const context = still.getContext('2d');
          context.imageSmoothingQuality = 'high';
          context.drawImage(canvas, 0, 0, still.width, still.height);
          return still.toDataURL('image/jpeg', 0.8).split(',')[1];
        }, {id: machine.id, scale: machine.size[0] > machine.size[1] ? 0.75 : 1});
        await writeFile(`${root}site/media/scenes/${machine.id}.jpg`, Buffer.from(jpeg, 'base64'));
        console.log(`${scene.id}: ${machine.id}.jpg ${Math.round(jpeg.length * 0.75 / 1024)} KB`);
      }
    } catch (error) {
      failed++;
      console.error(`${scene.id}: ${error.message.split('\n')[0]}`);
    }
    await page.close();
  }
} finally {
  await browser.close();
  server.kill();
}
process.exit(failed ? 1 : 0);
