#!/usr/bin/env node
// Score a converted site against its capture: how much of the page's accessible
// structure survived, and how close the in-world render looks to the screenshot.
//
//   node scripts/dom-to-site/compare.mjs <capture.json> <render.scene.json> [more .scene.json ...] [--out report.json]
//
// Each render.scene.json (from render.mjs) names its scroll offset and sits next to its
// .page.png. The structure score is the recall of the capture's accessibility names
// (links, headings, buttons, text boxes, images) among the render's semantic labels. The
// visual score compares the render with the capture's screenshot at the same scroll
// offset: mean colour per cell of a coarse grid, and a grey-level SSIM over a 64-column
// downsample. Both are coarse on purpose: the renderer uses its own typeface and its
// own column, so pixel identity is not the goal; layout, colour and legibility are.
import {readFile, writeFile} from 'node:fs/promises';
import {dirname, resolve} from 'node:path';
import {decodePNG} from './png.mjs';

const args = process.argv.slice(2);
const outIndex = args.indexOf('--out');
const outFile = outIndex >= 0 ? args[outIndex + 1] : null;
const files = args.filter((a, i) => !a.startsWith('--') && args[i - 1] !== '--out');
const [captureFile, ...sceneFiles] = files;
if (!captureFile || !sceneFiles.length) { console.error('usage: compare.mjs <capture.json> <render.scene.json> [...] [--out report.json]'); process.exit(2); }

const capture = JSON.parse(await readFile(captureFile, 'utf8'));
const shot = decodePNG(await readFile(resolve(dirname(captureFile), capture.screenshot)));

// ---------- structure ----------
const norm = s => String(s ?? '').toLowerCase().replace(/[·•|]/g, ' ').replace(/[^\p{L}\p{N} ]/gu, '').replace(/\s+/g, ' ').trim();
const wanted = [];
if (typeof capture.accessibility === 'string') {
  for (const line of capture.accessibility.split('\n')) {
    const m = /^\s*-\s+(link|heading|button|textbox|img|combobox|checkbox)\s+"([^"]*)"/.exec(line);
    if (m) wanted.push({role: m[1], name: m[2]});
  }
} else if (capture.accessibility) {
  (function walk(n) { if (['link', 'heading', 'button', 'textbox', 'img'].includes(n.role) && n.name) wanted.push({role: n.role, name: n.name}); (n.children ?? []).forEach(walk); })(capture.accessibility);
}
const scenes = await Promise.all(sceneFiles.map(async f => ({file: f, ...JSON.parse(await readFile(f, 'utf8'))})));
const have = new Map();
for (const s of scenes) for (const n of s.nodes) {
  const key = norm(n.label);
  if (!key) continue;
  if (!have.has(key)) have.set(key, new Set());
  have.get(key).add(n.role);
}
const ROLE_OK = {link: ['link'], heading: ['heading', 'text'], button: ['button', 'link'], textbox: ['textbox'], img: ['img'], combobox: ['textbox'], checkbox: ['button', 'checkbox']};
const structure = {wanted: wanted.length, found: 0, role_kept: 0, missing: []};
for (const w of wanted) {
  const key = norm(w.name);
  const roles = [...have.entries()].find(([k]) => k === key || (key.length > 12 && (k.startsWith(key) || key.startsWith(k))))?.[1];
  if (!roles) { structure.missing.push(w); continue; }
  structure.found++;
  if (ROLE_OK[w.role]?.some(r => roles.has(r))) structure.role_kept++;
}
structure.recall = wanted.length ? Math.round((structure.found / wanted.length) * 1000) / 1000 : null;
structure.role_agreement = structure.found ? Math.round((structure.role_kept / structure.found) * 1000) / 1000 : null;
// Interactive targets: links and buttons the actor can click, versus the capture's.
const captureLinks = wanted.filter(w => w.role === 'link' || w.role === 'button').length;
const renderLinks = new Set(scenes.flatMap(s => s.nodes.filter(n => n.interaction && (n.role === 'link' || n.role === 'button')).map(n => n.interaction))).size;
structure.interactive = {capture: captureLinks, render: renderLinks};

// ---------- visual ----------
function grey(img, x, y) { const o = (y * img.width + x) * 4; return 0.299 * img.rgba[o] + 0.587 * img.rgba[o + 1] + 0.114 * img.rgba[o + 2]; }
function downsample(img, region, cols) {
  const cell = region.width / cols, rows = Math.max(1, Math.round(region.height / cell));
  const out = {cols, rows, mean: [], grey: []};
  for (let r = 0; r < rows; r++) for (let c = 0; c < cols; c++) {
    const x0 = Math.floor(region.x + c * cell), y0 = Math.floor(region.y + r * cell);
    const x1 = Math.min(img.width, Math.floor(region.x + (c + 1) * cell)), y1 = Math.min(img.height, Math.floor(region.y + (r + 1) * cell));
    let R = 0, G = 0, B = 0, n = 0;
    for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) { const o = (y * img.width + x) * 4; R += img.rgba[o]; G += img.rgba[o + 1]; B += img.rgba[o + 2]; n++; }
    if (!n) { out.mean.push([255, 255, 255]); out.grey.push(255); continue; }
    out.mean.push([R / n, G / n, B / n]);
    out.grey.push(0.299 * R / n + 0.587 * G / n + 0.114 * B / n);
  }
  return out;
}
/// Global SSIM over a downsampled grey grid (constants for 8-bit range).
function ssim(a, b) {
  const n = Math.min(a.length, b.length);
  let ma = 0, mb = 0;
  for (let i = 0; i < n; i++) { ma += a[i]; mb += b[i]; }
  ma /= n; mb /= n;
  let va = 0, vb = 0, cov = 0;
  for (let i = 0; i < n; i++) { va += (a[i] - ma) ** 2; vb += (b[i] - mb) ** 2; cov += (a[i] - ma) * (b[i] - mb); }
  va /= n - 1; vb /= n - 1; cov /= n - 1;
  const c1 = (0.01 * 255) ** 2, c2 = (0.03 * 255) ** 2;
  return ((2 * ma * mb + c1) * (2 * cov + c2)) / ((ma ** 2 + mb ** 2 + c1) * (va + vb + c2));
}
// Landmarks: every captured text run with its page y, so a render viewport can be aligned
// on the first label it shows rather than on a raw scroll offset (the converted page is
// never exactly the capture's height).
const landmarks = new Map();
(function walk(n) {
  const own = norm(n.text);
  if (own && !landmarks.has(own)) landmarks.set(own, n.bounds.y);
  n.children.forEach(walk);
})(capture.root);
function alignedTop(s) {
  // The median offset over the first distinctive labels in view: one ambiguous short
  // label (a section name that is also a badge) must not decide the alignment.
  const first = [...s.nodes].filter(n => n.bounds.y >= 0 && norm(n.label).length >= 10).sort((a, b) => a.bounds.y - b.bounds.y);
  const offsets = [];
  for (const n of first.slice(0, 16)) {
    const key = norm(n.label);
    const y = landmarks.get(key) ?? [...landmarks.entries()].find(([k]) => k.startsWith(key) || key.startsWith(k))?.[1];
    if (y !== undefined) offsets.push(y - n.bounds.y);
  }
  if (!offsets.length) return s.scroll;
  offsets.sort((a, b) => a - b);
  return Math.max(0, Math.round(offsets[offsets.length >> 1]));
}
const visual = [];
for (const s of scenes) {
  const pagePng = s.file.replace(/\.scene\.json$/, '') + '.page.png';
  let render;
  try { render = decodePNG(await readFile(pagePng)); } catch { continue; }
  // The render's page area against the same viewport of the screenshot. The renderer
  // centres a content column; compare the column against the capture's own content
  // column (its widest block narrower than the viewport) so margins are not penalised.
  const cw = Math.min(render.width, shot.width);
  const top = alignedTop(s);
  const shotRegion = {x: Math.round((shot.width - cw) / 2), y: top, width: cw, height: Math.min(render.height, shot.height - top)};
  const renderRegion = {x: Math.round((render.width - cw) / 2), y: 0, width: cw, height: shotRegion.height};
  const a = downsample(shot, shotRegion, 64), b = downsample(render, renderRegion, 64);
  let dist = 0;
  const n = Math.min(a.mean.length, b.mean.length);
  for (let i = 0; i < n; i++) dist += Math.hypot(...a.mean[i].map((v, k) => v - b.mean[i][k])) / 441.67;
  visual.push({scroll: s.scroll, aligned_to: top, colour_similarity: Math.round((1 - dist / n) * 1000) / 1000, ssim: Math.round(ssim(a.grey, b.grey) * 1000) / 1000, cells: n});
}
const colour = visual.length ? visual.reduce((t, v) => t + v.colour_similarity, 0) / visual.length : null;
const structural = visual.length ? visual.reduce((t, v) => t + v.ssim, 0) / visual.length : null;
// One number, weighted towards what an actor needs: names and targets first, looks second.
const fidelity = Math.round(((structure.recall ?? 0) * 0.4 + (structure.role_agreement ?? 0) * 0.1 + (colour ?? 0) * 0.25 + (Math.max(0, structural ?? 0)) * 0.25) * 1000) / 1000;
const report = {capture: captureFile, structure, visual, fidelity};
if (outFile) await writeFile(outFile, JSON.stringify(report, null, 2) + '\n');
console.log(`structure: ${structure.found}/${structure.wanted} accessible names found (recall ${structure.recall}), role agreement ${structure.role_agreement}, interactive targets ${structure.interactive.render} vs ${structure.interactive.capture}`);
for (const v of visual) console.log(`visual @${v.scroll} (capture y ${v.aligned_to}): colour similarity ${v.colour_similarity}, grey SSIM ${v.ssim} (${v.cells} cells)`);
if (structure.missing.length) console.log(`missing: ${structure.missing.slice(0, 8).map(m => `${m.role} "${m.name}"`).join('; ')}${structure.missing.length > 8 ? ' ...' : ''}`);
console.log(`fidelity ${fidelity}`);
