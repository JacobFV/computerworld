/** Render the still each machine on the project site shows before it is running.
 *
 * The stills are not drawn separately: this opens the site in Chrome, lets every scene in
 * site/cast.js boot exactly as a visitor's tab boots it, and saves each machine's canvas
 * to site/media/scenes/<machine>.jpg. Each scene is opened in a page of its own, so a
 * still shows what a visitor sees who lands on that scene first.
 *
 * A scene may hold a whole team — seven machines in one world, booted one after another —
 * so a scene is given time in proportion to how many machines it has rather than a flat
 * minute. Every machine in the scene is saved, whatever its shape. The run ends by naming
 * any machine in the cast that still has no still on disk: the site shows those as empty
 * screens rather than broken images, so a new scene can land before its pictures do.
 *
 *   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
 *     node scripts/render-site-stills.mjs [scene-id ...]
 *
 * The machines need the Wasm bundle in site/pkg/ (see site/README.md).
 */
import {spawn} from 'node:child_process';
import {mkdir, stat, writeFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const {cast} = await import(new URL('../site/cast.js', import.meta.url));
const wanted = process.argv.slice(2);
const scenes = wanted.length ? cast.filter(scene => wanted.includes(scene.id)) : cast;

/** A port nobody is on. A fixed one would mean two of these runs sharing a server, and
 * the first to finish taking it away from the other. */
const freePort = () => new Promise((resolve, reject) => {
  const probe = createServer();
  probe.on('error', reject);
  probe.listen(0, '127.0.0.1', () => {
    const {port} = probe.address();
    probe.close(() => resolve(port));
  });
});
const port = await freePort();

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const server = spawn(process.execPath, [`${root}scripts/serve-site.mjs`, String(port)], {stdio: 'ignore'});
// Every scene is a fresh page against this server, so make sure it is really up, and say
// so plainly rather than failing scene by scene if it is not.
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
        scene.id, {timeout: 40_000 + 30_000 * scene.machines.length});
      // A still is a frame the machine really draws, fonts included: wait for the pack.
      await page.evaluate(() => window.computerworldFonts);
      for (const machine of scene.machines) {
        // Desktops are saved at three quarters of their size: the still is a placeholder
        // behind a dimming filter, and there are dozens of them.
        const jpeg = await page.evaluate(async ({id, scale}) => {
          // The machine's own frame, asked of whoever is running it — not read off the
          // canvas. The machines run in workers and draw through an OffscreenCanvas, and
          // what a page gets back from one of those placeholders is the frame it was first
          // given, not the frame it is showing: every still would be the moment before the
          // fonts landed. A scene edited while this is running, or a machine the shell
          // never brought up, answers null; say which one and leave the others alone.
          const frame = await window.computerworldFrame(id);
          if (!frame) return null;
          const shown = document.createElement('canvas');
          shown.width = frame.width;
          shown.height = frame.height;
          shown.getContext('2d').putImageData(frame, 0, 0);
          const still = document.createElement('canvas');
          still.width = Math.round(frame.width * scale);
          still.height = Math.round(frame.height * scale);
          const context = still.getContext('2d');
          context.imageSmoothingQuality = 'high';
          context.drawImage(shown, 0, 0, still.width, still.height);
          return still.toDataURL('image/jpeg', 0.8).split(',')[1];
        }, {id: machine.id, scale: machine.size[0] > machine.size[1] ? 0.75 : 1});
        if (!jpeg) {
          failed++;
          console.error(`${scene.id}: ${machine.id} gave back no frame`);
          continue;
        }
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

// What the site will draw as an empty screen until someone renders it.
const missing = [];
for (const scene of cast) {
  for (const machine of scene.machines) {
    const still = `${root}site/media/scenes/${machine.id}.jpg`;
    if (!await stat(still).then(() => true, () => false)) missing.push(`${scene.id}: ${machine.id}`);
  }
}
if (missing.length) console.log(`\nno still yet (the site shows an empty screen):\n  ${missing.join('\n  ')}`);
process.exit(failed ? 1 : 0);
