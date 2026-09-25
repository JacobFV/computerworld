#!/usr/bin/env node
// Collects, for every state of every framework-parity fixture named, Chromium's render
// (the ground truth dump.mjs took) beside computerworld's render of the same state
// (the engine PNG `cargo test -p cw-web --features pipeline --test framework_parity`
// writes into crates/web/engine/target-parity/), plus a difference picture and a
// side-by-side, into one directory with an index.html to page through them.
//
//   node scripts/web-parity/gallery.mjs <out-dir> <fixture> [<fixture> ...]
//
// Per state it writes <NN>-<fixture>/<state>.{chromium,computerworld,diff,side-by-side}.png.
// The difference is red where any channel differs by more than 8 of 255 (antialiasing
// between the two rasterisers stays white), scaled so small differences still show.
import {readFile, writeFile, mkdir, copyFile} from 'node:fs/promises';
import {existsSync} from 'node:fs';
import {fileURLToPath} from 'node:url';
import {join} from 'node:path';
import {encodePNG, decodePNG} from '../dom-to-site/png.mjs';

const root = fileURLToPath(new URL('../..', import.meta.url));
const fixtures = join(root, 'crates/web/engine/tests/framework-parity');
const engineDir = join(root, 'crates/web/engine/target-parity');
const [out, ...names] = process.argv.slice(2);
if (!out || names.length === 0) {
  console.error('usage: gallery.mjs <out-dir> <fixture> [<fixture> ...]');
  process.exit(2);
}
const NOISE = 8;

function difference(a, b) {
  const width = Math.max(a.width, b.width), height = Math.max(a.height, b.height);
  const rgba = new Uint8Array(width * height * 4);
  let differing = 0;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const inA = x < a.width && y < a.height, inB = x < b.width && y < b.height;
      const sa = (y * a.width + x) * 4, sb = (y * b.width + x) * 4;
      let d = 0;
      for (let c = 0; c < 3; c++) d = Math.max(d, Math.abs((inA ? a.rgba[sa + c] : 255) - (inB ? b.rgba[sb + c] : 255)));
      const o = (y * width + x) * 4;
      if (d > NOISE) {
        differing++;
        const v = 255 - Math.min(255, d * 3);
        rgba.set([255, v, v, 255], o);
      } else {
        // A faint copy of Chromium's picture, so the red lands somewhere recognisable.
        const g = inA ? Math.round(235 + (a.rgba[sa] + a.rgba[sa + 1] + a.rgba[sa + 2]) / 3 / 255 * 20) : 255;
        rgba.set([g, g, g, 255], o);
      }
    }
  }
  return {picture: {width, height, rgba}, share: differing / (width * height)};
}

function sideBySide(pictures, gap = 12) {
  const width = pictures.reduce((w, p) => w + p.width, 0) + gap * (pictures.length - 1);
  const height = Math.max(...pictures.map(p => p.height));
  const rgba = new Uint8Array(width * height * 4).fill(255);
  let x0 = 0;
  for (const p of pictures) {
    for (let y = 0; y < p.height; y++) {
      rgba.set(p.rgba.subarray(y * p.width * 4, (y + 1) * p.width * 4), (y * width + x0) * 4);
    }
    x0 += p.width + gap;
  }
  return {width, height, rgba};
}

await mkdir(out, {recursive: true});
const rows = [];
let n = 0;
for (const name of names) {
  n++;
  const steps = JSON.parse(await readFile(join(fixtures, `${name}.steps.json`), 'utf8'));
  const dir = `${String(n).padStart(2, '0')}-${name}`;
  await mkdir(join(out, dir), {recursive: true});
  for (const state of Object.keys(steps)) {
    const chromium = join(fixtures, `${name}.${state}.chromium.png`);
    const engine = join(engineDir, `${name}.${state}.engine.png`);
    if (!existsSync(chromium) || !existsSync(engine)) {
      console.error(`${name}.${state}: missing ${existsSync(chromium) ? engine : chromium}`);
      process.exit(1);
    }
    const a = decodePNG(await readFile(chromium));
    const b = decodePNG(await readFile(engine));
    const {picture, share} = difference(a, b);
    await copyFile(chromium, join(out, dir, `${state}.chromium.png`));
    // The engine's PNG is stored uncompressed (4 MB); write it compressed.
    await writeFile(join(out, dir, `${state}.computerworld.png`), encodePNG(b));
    await writeFile(join(out, dir, `${state}.diff.png`), encodePNG(picture));
    await writeFile(join(out, dir, `${state}.side-by-side.png`), encodePNG(sideBySide([a, b])));
    let parity = '';
    const report = join(engineDir, `${name}.${state}.report.md`);
    if (existsSync(report)) {
      const m = /- passed: (\d+) \(([\d.]+)%\)/.exec(await readFile(report, 'utf8'));
      const total = /- nodes: (\d+)/.exec(await readFile(report, 'utf8'));
      if (m && total) parity = `${m[1]}/${total[1]} nodes (${m[2]}%)`;
    }
    rows.push({dir, name, state, share, parity});
    console.log(`${dir}/${state}: ${(share * 100).toFixed(2)}% of pixels differ; layout ${parity}`);
  }
}

const esc = s => s.replace(/&/g, '&amp;').replace(/</g, '&lt;');
const html = `<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>React apps: Chromium vs computerworld</title>
<style>
  body { margin: 0; padding: 16px; font: 14px/1.4 system-ui, sans-serif; background: #f4f4f6; color: #1d1c24; }
  h1 { font-size: 20px; } h2 { margin: 32px 0 8px; font-size: 16px; }
  .state { margin: 0 0 24px; padding: 12px; background: #fff; border: 1px solid #ddd; border-radius: 8px; }
  .meta { margin-bottom: 8px; color: #555; }
  .pics { display: grid; grid-template-columns: repeat(3, 1fr); gap: 8px; }
  .pics figure { margin: 0; } .pics img { width: 100%; border: 1px solid #ccc; }
  figcaption { font-size: 12px; color: #666; }
  table { border-collapse: collapse; background: #fff; } td, th { padding: 4px 10px; border: 1px solid #ddd; text-align: left; }
</style></head><body>
<h1>React apps: Chromium (ground truth) vs computerworld</h1>
<p>Each state was reached from a fresh load by the same clicks and typing in both. Left: Chromium via Playwright. Middle: computerworld's engine (React on the in-world JS VM, cw-web layout and paint). Right: pixels differing by more than ${NOISE}/255 in red. "Layout" is the share of DOM nodes whose boxes and computed styles match Chromium's within 1 px (2 px for text-sized boxes).</p>
<table><tr><th>App</th><th>State</th><th>Pixels differing</th><th>Layout</th></tr>
${rows.map(r => `<tr><td>${esc(r.name)}</td><td><a href="#${r.dir}-${r.state}">${esc(r.state)}</a></td><td>${(r.share * 100).toFixed(2)}%</td><td>${esc(r.parity)}</td></tr>`).join('\n')}
</table>
${names.map((name, i) => {
  const dir = `${String(i + 1).padStart(2, '0')}-${name}`;
  return `<h2>${esc(dir)}</h2>\n` + rows.filter(r => r.dir === dir).map(r => `<div class="state" id="${r.dir}-${r.state}">
<div class="meta"><b>${esc(r.state)}</b> · ${(r.share * 100).toFixed(2)}% of pixels differ · layout ${esc(r.parity)}</div>
<div class="pics">
<figure><a href="${dir}/${r.state}.chromium.png"><img src="${dir}/${r.state}.chromium.png"></a><figcaption>Chromium</figcaption></figure>
<figure><a href="${dir}/${r.state}.computerworld.png"><img src="${dir}/${r.state}.computerworld.png"></a><figcaption>computerworld</figcaption></figure>
<figure><a href="${dir}/${r.state}.diff.png"><img src="${dir}/${r.state}.diff.png"></a><figcaption>difference</figcaption></figure>
</div></div>`).join('\n');
}).join('\n')}
</body></html>
`;
await writeFile(join(out, 'index.html'), html);
console.log(`${rows.length} states from ${names.length} apps -> ${join(out, 'index.html')}`);
