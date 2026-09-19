#!/usr/bin/env node
// Capture a real page as a box tree the converter can map into a computerworld page.
//
//   PLAYWRIGHT_MODULE=/path/to/playwright/index.mjs CHROME_BIN=/usr/bin/google-chrome \
//     node scripts/dom-to-site/capture.mjs <url|file> <out.json> [--width 1280] [--height 800]
//
// Writes <out.json> (the box tree, computed styles, text, links, form controls, image
// bounds + alt, the accessibility snapshot) and <out>.png (a full-page screenshot). Images
// are recorded as geometry only: nothing is downloaded, so no copyrighted pixels leave the
// page. A URL is checked against the site's robots.txt before it is opened; pass
// --ignore-robots only for a page you own.
import {mkdir, writeFile} from 'node:fs/promises';
import {dirname, resolve} from 'node:path';
import {pathToFileURL} from 'node:url';

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const i = args.indexOf(name);
  return i < 0 ? fallback : args[i + 1];
};
const positional = args.filter((a, i) => !a.startsWith('--') && !args[i - 1]?.startsWith('--') || (args[i - 1] === '--ignore-robots'));
let [target, out] = positional;
if (!target || !out) {
  console.error('usage: capture.mjs <url|file> <out.json> [--width 1280] [--height 800] [--ignore-robots]');
  process.exit(2);
}
const width = Number(flag('--width', 1280));
const height = Number(flag('--height', 800));
const ignoreRobots = args.includes('--ignore-robots');
if (!/^[a-z]+:/i.test(target)) target = pathToFileURL(resolve(target)).href;

/// Only the properties the target format can express, plus the few that steer mapping.
const PROPERTIES = [
  'display', 'position', 'visibility', 'opacity', 'overflow-x',
  'flex-direction', 'flex-wrap', 'flex-grow', 'justify-content', 'align-items',
  'grid-template-columns', 'column-gap', 'row-gap', 'gap',
  'color', 'background-color', 'background-image',
  'border-top-width', 'border-top-color', 'border-top-style',
  'border-bottom-width', 'border-bottom-color', 'border-bottom-style',
  'border-left-width', 'border-right-width', 'border-top-left-radius',
  'font-size', 'font-weight', 'font-style', 'font-family', 'line-height', 'text-align',
  'text-transform', 'letter-spacing', 'white-space', 'text-overflow',
  'padding-top', 'padding-right', 'padding-bottom', 'padding-left',
  'margin-top', 'margin-bottom', 'cursor',
];

async function robotsAllow(url) {
  const u = new URL(url);
  if (!/^https?:$/.test(u.protocol)) return true;
  try {
    const text = await (await fetch(`${u.origin}/robots.txt`)).text();
    let applies = false;
    const disallow = [];
    for (const raw of text.split('\n')) {
      const line = raw.replace(/#.*/, '').trim();
      const m = /^([a-z-]+)\s*:\s*(.*)$/i.exec(line);
      if (!m) continue;
      const [, key, value] = m;
      if (key.toLowerCase() === 'user-agent') applies = value.trim() === '*';
      else if (applies && key.toLowerCase() === 'disallow' && value.trim()) disallow.push(value.trim());
    }
    return !disallow.some(prefix => u.pathname.startsWith(prefix));
  } catch {
    return true;
  }
}

if (!ignoreRobots && !(await robotsAllow(target))) {
  console.error(`${target}: disallowed by robots.txt (pass --ignore-robots only for a page you own)`);
  process.exit(3);
}

const {chromium} = await import(process.env.PLAYWRIGHT_MODULE ?? 'playwright').then(m => m.default ?? m);
const browser = await chromium.launch({executablePath: process.env.CHROME_BIN});
try {
  const page = await browser.newPage({viewport: {width, height}});
  await page.goto(target, {waitUntil: 'networkidle'});
  const tree = await page.evaluate(properties => {
    const SKIP = new Set(['SCRIPT', 'STYLE', 'NOSCRIPT', 'TEMPLATE', 'LINK', 'META', 'HEAD', 'IFRAME', 'OBJECT', 'EMBED', 'CANVAS']);
    const round = v => Math.round(v * 10) / 10;
    const labels = new Map();
    for (const l of document.querySelectorAll('label[for]')) labels.set(l.htmlFor, l.textContent.trim());
    function walk(el, depth) {
      if (SKIP.has(el.tagName)) return null;
      const cs = getComputedStyle(el);
      if (cs.display === 'none' || cs.visibility === 'hidden' || el.getAttribute('aria-hidden') === 'true') return null;
      const r = el.getBoundingClientRect();
      const bounds = {x: round(r.left + scrollX), y: round(r.top + scrollY), width: round(r.width), height: round(r.height)};
      if (bounds.width === 0 && bounds.height === 0 && !el.children.length) return null;
      const style = {};
      for (const p of properties) style[p] = cs.getPropertyValue(p);
      const node = {
        tag: el.tagName.toLowerCase(),
        id: el.id || undefined,
        class: el.className && typeof el.className === 'string' ? el.className : undefined,
        role: el.getAttribute('role') || undefined,
        label: el.getAttribute('aria-label') || undefined,
        bounds, style, depth,
        text: [...el.childNodes].filter(n => n.nodeType === 3).map(n => n.textContent).join('').replace(/\s+/g, ' ').trim() || undefined,
        children: [],
      };
      if (el.tagName === 'A' && el.getAttribute('href')) node.href = el.href;
      if (el.tagName === 'IMG' || el.tagName === 'PICTURE' || el.tagName === 'SVG') {
        node.image = {alt: el.getAttribute('alt') ?? el.getAttribute('aria-label') ?? '', naturalWidth: el.naturalWidth ?? 0, naturalHeight: el.naturalHeight ?? 0, host: (() => { try { return new URL(el.currentSrc || el.src || '', location.href).host; } catch { return ''; } })()};
      }
      if (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') {
        node.control = {type: el.type || el.tagName.toLowerCase(), name: el.name || undefined, value: el.type === 'password' ? '' : (el.value ?? ''), placeholder: el.placeholder || undefined, label: labels.get(el.id) || el.getAttribute('aria-label') || undefined};
      }
      if (el.tagName === 'BUTTON' || (el.tagName === 'INPUT' && /^(submit|button)$/.test(el.type))) {
        node.button = {type: el.type || 'submit', text: (el.value || el.textContent || '').trim()};
      }
      if (el.tagName === 'FORM') node.form = {action: el.action || location.href, method: (el.method || 'get').toUpperCase()};
      for (const child of el.children) {
        const c = walk(child, depth + 1);
        if (c) node.children.push(c);
      }
      return node;
    }
    const body = walk(document.body, 0);
    const bodyStyle = getComputedStyle(document.body);
    return {
      title: document.title,
      lang: document.documentElement.lang || undefined,
      url: location.href,
      viewport: {width: innerWidth, height: innerHeight},
      document: {width: document.documentElement.scrollWidth, height: document.documentElement.scrollHeight},
      body: {color: bodyStyle.color, background: bodyStyle.backgroundColor, fontFamily: bodyStyle.fontFamily, fontSize: bodyStyle.fontSize},
      root: body,
    };
  }, PROPERTIES);
  let accessibility = null;
  try {
    accessibility = await page.locator('body').ariaSnapshot();
  } catch {
    try { accessibility = await page.accessibility.snapshot(); } catch { accessibility = null; }
  }
  await mkdir(dirname(resolve(out)), {recursive: true});
  const png = out.replace(/\.json$/, '') + '.png';
  await page.screenshot({path: png, fullPage: true});
  const capture = {version: 1, captured_at: new Date().toISOString(), source: target, screenshot: png.split('/').pop(), ...tree, accessibility};
  await writeFile(out, JSON.stringify(capture, null, 1) + '\n');
  let count = 0;
  (function n(e) { count++; e.children.forEach(n); })(tree.root);
  console.log(`${out}: ${count} boxes, document ${tree.document.width}x${tree.document.height}, screenshot ${png}`);
} finally {
  await browser.close();
}
