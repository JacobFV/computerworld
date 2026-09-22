#!/usr/bin/env node
// chromium.png | computerworld.png, with a 24 px white gutter and a caption strip.
import {readFile, writeFile} from 'node:fs/promises';
import {fileURLToPath} from 'node:url';
const root = fileURLToPath(new URL('../../..', import.meta.url));
const here = fileURLToPath(new URL('.', import.meta.url));
const {decodePNG, encodePNG} = await import(`${root}/scripts/dom-to-site/png.mjs`);
const [a, b] = await Promise.all(['chromium.png', 'computerworld.png'].map(async f => decodePNG(await readFile(here + f))));
const gutter = 24, cap = 0;
const W = a.width + gutter + b.width, H = Math.max(a.height, b.height) + cap;
const rgba = new Uint8Array(W * H * 4).fill(255);
const blit = (img, ox) => { for (let y = 0; y < img.height; y++) rgba.set(img.rgba.subarray(y * img.width * 4, (y + 1) * img.width * 4), ((y + cap) * W + ox) * 4); };
blit(a, 0); blit(b, a.width + gutter);
// a 1 px rule around each frame so white pages read as two panels
const rule = (x0, y0, w, h) => { for (let x = x0; x < x0 + w; x++) for (const y of [y0, y0 + h - 1]) rgba.set([200, 200, 200, 255], (y * W + x) * 4); for (let y = y0; y < y0 + h; y++) for (const x of [x0, x0 + w - 1]) rgba.set([200, 200, 200, 255], (y * W + x) * 4); };
rule(0, cap, a.width, a.height); rule(a.width + gutter, cap, b.width, b.height);
await writeFile(here + 'side-by-side.png', encodePNG({width: W, height: H, rgba}));
console.log(`side-by-side.png ${W}x${H}`);
