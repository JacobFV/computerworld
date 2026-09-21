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
 *     node scripts/render-site-stills.mjs [--check] [scene-id ...]
 *
 * `--check` renders everything the same way but writes nothing: each fresh frame is
 * compared against the .jpg already on disk, and the run fails naming the machines whose
 * picture no longer shows what they draw. The stills drift silently otherwise — a service
 * gains a row and the saved picture keeps the old one — so .github/workflows/stills.yml
 * runs this nightly over the whole cast.
 *
 * The machines need the Wasm bundle in site/pkg/ (see site/README.md).
 */
import {spawn} from 'node:child_process';
import {mkdir, stat, writeFile} from 'node:fs/promises';
import {createServer} from 'node:net';
import {fileURLToPath} from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const {cast} = await import(new URL('../site/cast.js', import.meta.url));
const argv = process.argv.slice(2);
const check = argv.includes('--check');
const wanted = argv.filter(argument => argument !== '--check');
const scenes = wanted.length ? cast.filter(scene => wanted.includes(scene.id)) : cast;

/** How much of a still may differ from the frame the machine draws before it counts as
 * stale, as a share of pixels off by more than 8 in any channel.
 *
 * Both sides are compared after the same JPEG pass, which is what leaves room for a
 * threshold at all. Against the raw frame a saved still is never close — mac-github's
 * own picture, written by the run a minute before, already had 9.5% of its pixels over
 * that line from quality-0.8 chroma loss on a screenful of text, against 32.6% for the
 * genuinely stale one — so the loss would be most of the signal. Put the fresh frame
 * through the same encoder first and identical content comes back identical: 0.00%
 * there, against 22.7% for the still mac-github carried until the day it was re-rendered
 * and 47.0% for another machine's picture standing in for it. 5% is headroom for a
 * Chrome whose encoder is not this one's, and still four times under the smallest real
 * drift measured. */
const drifted = 0.05;

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

// Playwright is not a dependency of this project: locally it is whatever browser
// automation is already installed, and in CI a --no-save playwright-core driving the
// runner's own Chrome. Take the module the caller names, and fall back to either package.
const playwright = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright-core')
  .catch(() => import('playwright'));
const {chromium} = playwright.default ?? playwright;
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
if (!check) await mkdir(`${root}site/media/scenes`, {recursive: true});

let failed = 0;
const stale = [];
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
        const shot = await page.evaluate(async ({id, scale, saved}) => {
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
          if (!saved) return {jpeg: still.toDataURL('image/jpeg', 0.8).split(',')[1]};
          // Checking, so compare decoded pixels rather than file bytes: the file on disk
          // is a JPEG some other Chrome encoded, and two encoders agree on the picture
          // long before they agree on the bytes. The site's own server hands back the
          // saved file, so the browser does the decoding.
          const picture = new Image();
          const arrived = await new Promise(settle => {
            picture.onload = () => settle(true);
            picture.onerror = () => settle(false);
            picture.src = saved;
          });
          if (!arrived) return {absent: true};
          if (picture.naturalWidth !== still.width || picture.naturalHeight !== still.height)
            return {was: [picture.naturalWidth, picture.naturalHeight], now: [still.width, still.height]};
          // The fresh frame goes through the encoder the still went through, so that what
          // is left between them is the picture rather than the compression.
          const encoded = new Image();
          await new Promise(settle => {
            encoded.onload = settle;
            encoded.src = still.toDataURL('image/jpeg', 0.8);
          });
          const read = picture => {
            const canvas = document.createElement('canvas');
            canvas.width = still.width;
            canvas.height = still.height;
            canvas.getContext('2d').drawImage(picture, 0, 0);
            return canvas.getContext('2d').getImageData(0, 0, still.width, still.height).data;
          };
          const was = read(picture);
          const now = read(encoded);
          let off = 0;
          for (let at = 0; at < now.length; at += 4)
            if (Math.abs(now[at] - was[at]) > 8 || Math.abs(now[at + 1] - was[at + 1]) > 8
              || Math.abs(now[at + 2] - was[at + 2]) > 8) off++;
          return {off: off / (now.length / 4)};
        }, {
          id: machine.id,
          scale: machine.size[0] > machine.size[1] ? 0.75 : 1,
          saved: check ? `/media/scenes/${machine.id}.jpg` : null,
        });
        if (!shot) {
          failed++;
          console.error(`${scene.id}: ${machine.id} gave back no frame`);
          continue;
        }
        if (!check) {
          await writeFile(`${root}site/media/scenes/${machine.id}.jpg`, Buffer.from(shot.jpeg, 'base64'));
          console.log(`${scene.id}: ${machine.id}.jpg ${Math.round(shot.jpeg.length * 0.75 / 1024)} KB`);
          continue;
        }
        // A machine with no still at all is not drift; the closing report names it.
        if (shot.absent) { console.log(`${scene.id}: ${machine.id}.jpg is not there`); continue; }
        if (shot.was) {
          stale.push({scene, machine, how: `${shot.was.join('×')} on disk, ${shot.now.join('×')} drawn`});
          console.error(`${scene.id}: ${machine.id}.jpg is the wrong size`);
          continue;
        }
        const how = `${(shot.off * 100).toFixed(1)}% of pixels differ`;
        if (shot.off > drifted) {
          stale.push({scene, machine, how});
          console.error(`${scene.id}: ${machine.id}.jpg drifted — ${how}`);
        } else console.log(`${scene.id}: ${machine.id}.jpg matches — ${how}`);
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

if (stale.length) {
  console.error(`\n${stale.length} still${stale.length > 1 ? 's no longer show' : ' no longer shows'} what the machine draws:`);
  for (const {scene, machine, how} of stale) console.error(`  ${machine.id} (${scene.id}): ${how}`);
  // Naming the scenes rather than the machines, because that is what the script takes.
  const again = [...new Set(stale.map(({scene}) => scene.id))].join(' ');
  console.error(`\nre-render them, and commit site/media/scenes/:\n  node scripts/render-site-stills.mjs ${again}`);
}
process.exit(failed || stale.length ? 1 : 0);
