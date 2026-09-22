#!/usr/bin/env node
// Convert a capture.mjs box tree into a computerworld site seed.
//
//   node scripts/dom-to-site/convert.mjs <capture.json> <site-id> <domain> [out.json]
//       [--path /] [--no-stubs] [--as static|press] [--node <network node>]
//       [--address 203.0.113.250] [--link pop-west]
//
// Emits a file in the shape of worlds/company-2026/sites/<site-id>.json: a static-site
// service whose "/" page is the converted capture, stub pages for every same-site link
// (so the internet_links crawl finds no dead link), and `place` so the site brings
// its own host. `--as press` instead emits a seed for the press service, synthesised from
// the headlines the capture contains: a real backend behind a familiar front page.
//
// A sidecar <out>.report.json records what was dropped, the detected site kind and the
// theme that was inferred, so a reviewer can see what the page lost.
import {readFile, writeFile} from 'node:fs/promises';
import {dirname, resolve} from 'node:path';
import {decodePNG, meanColour} from './png.mjs';
import {validateSite} from './validate.mjs';

const args = process.argv.slice(2);
const flag = (name, fallback) => { const i = args.indexOf(name); return i < 0 ? fallback : args[i + 1]; };
const positional = args.filter((a, i) => !a.startsWith('--') && !(args[i - 1]?.startsWith('--') && args[i - 1] !== '--no-stubs'));
const [captureFile, siteId, domain, outArg] = positional;
if (!captureFile || !siteId || !domain) {
  console.error('usage: convert.mjs <capture.json> <site-id> <domain> [out.json] [--path /] [--no-stubs] [--as static|press]');
  process.exit(2);
}
const out = outArg ?? captureFile.replace(/\.capture\.json$|\.json$/, '') + '.site.json';
const pagePath = flag('--path', '/');
const stubs = !args.includes('--no-stubs');
const as = flag('--as', 'static');

const capture = JSON.parse(await readFile(captureFile, 'utf8'));
let screenshot = null;
try { screenshot = decodePNG(await readFile(resolve(dirname(captureFile), capture.screenshot))); } catch { screenshot = null; }

// ---------- colour and number helpers ----------
function hex(css) {
  if (!css) return null;
  const m = /rgba?\(\s*(\d+)\s*,\s*(\d+)\s*,\s*(\d+)\s*(?:,\s*([\d.]+))?\)/.exec(css);
  if (!m) return null;
  const a = m[4] === undefined ? 1 : Number(m[4]);
  if (a === 0) return null;
  const h = n => Number(n).toString(16).padStart(2, '0');
  return `#${h(m[1])}${h(m[2])}${h(m[3])}${a < 1 ? h(Math.round(a * 255)) : ''}`;
}
const rgbHex = ([r, g, b]) => `#${[r, g, b].map(n => n.toString(16).padStart(2, '0')).join('')}`;
const px = v => Math.round(parseFloat(v) || 0);
const clamp = (v, lo, hi) => Math.max(lo, Math.min(hi, v));
const slug = s => String(s || '').toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 24);

// ---------- pruning ----------
const NOISE = /(^|[-_ ])(ad|ads|advert|advertisement|sponsor|sponsored|promo|cookie|consent|gdpr|tracking|tracker|pixel|gtm|analytics|banner-ad|skyscraper|leaderboard)([-_ ]|$)/i;
const report = {source: capture.source, dropped: [], inline_links_dropped: [], notes: []};
function noisy(n) {
  const key = `${n.id ?? ''} ${n.class ?? ''}`;
  return NOISE.test(key) || n.tag === 'iframe';
}
function textOf(n) {
  return [n.text ?? '', ...n.children.map(textOf)].join(' ').replace(/\s+/g, ' ').trim();
}
const INLINE = new Set(['a', 'span', 'strong', 'em', 'b', 'i', 'u', 'small', 'abbr', 'code', 'time', 'mark', 'sup', 'sub', 'br', 'label']);
function isInline(n) {
  return INLINE.has(n.tag) || /^inline/.test(n.style.display);
}
/// A node whose whole subtree is one run of text (inline descendants only).
function isTextLeaf(n) {
  if (n.image || n.control || n.button || n.form) return false;
  if (!n.children.length) return !!n.text;
  if (links(n).length > 1) return false;
  return n.children.every(c => isInline(c) && isTextLeaf(c) && !c.image && !c.control) && (n.text || n.children.some(textOf));
}
function links(n, acc = []) {
  if (n.href) acc.push(n);
  n.children.forEach(c => links(c, acc));
  return acc;
}
function prune(n, ancestorsHidden = false) {
  if (noisy(n)) { report.dropped.push({tag: n.tag, id: n.id, class: n.class, why: 'ad/tracker pattern'}); return null; }
  if (parseFloat(n.style.opacity) === 0) { report.dropped.push({tag: n.tag, id: n.id, class: n.class, why: 'opacity 0'}); return null; }
  n.children = n.children.map(c => prune(c, ancestorsHidden)).filter(Boolean);
  if (!n.text && !n.children.length && !n.image && !n.control && !n.button && !hex(n.style['background-color']) && n.bounds.height < 3) return null;
  return n;
}
const root = prune(capture.root);

// ---------- theme ----------
function tally(map, key, weight = 1) { if (key) map.set(key, (map.get(key) ?? 0) + weight); }
function top(map) { return [...map.entries()].sort((a, b) => b[1] - a[1])[0]?.[0] ?? null; }
const linkColours = new Map(), textColours = new Map(), surfaces = new Map();
const bodyBg = hex(capture.body.background) ?? '#ffffff';
const bodyInk = hex(capture.body.color) ?? '#1c1f26';
(function scan(n) {
  if (n.href) tally(linkColours, hex(n.style.color));
  if (isTextLeaf(n) && !n.href) tally(textColours, hex(n.style.color), textOf(n).length);
  const bg = hex(n.style['background-color']);
  if (bg && bg !== bodyBg && n.children.length > 1 && n.bounds.width < capture.viewport.width * 0.9) tally(surfaces, bg, n.bounds.width * n.bounds.height);
  n.children.forEach(scan);
})(root);
const ink = top(textColours) ?? bodyInk;
const muted = [...textColours.entries()].sort((a, b) => b[1] - a[1]).map(e => e[0]).find(c => c !== ink) ?? '#6f7885';
function contentWidth(n) {
  let best = null;
  (function walk(m) {
    const w = m.bounds.width;
    if (w >= capture.viewport.width * 0.5 && w <= capture.viewport.width - 32 && m.children.length) {
      const score = m.bounds.height * (m.tag === 'main' ? 4 : 1);
      if (!best || score > best.score) best = {score, width: Math.round(w)};
    }
    m.children.forEach(walk);
  })(n);
  return best?.width ?? null;
}
const theme = {accent: top(linkColours) ?? '#2270cd', background: bodyBg, surface: top(surfaces) ?? '#ffffff', ink, muted};
const cw = contentWidth(root);
if (cw) theme.content_width = cw;

// ---------- element factory ----------
const used = new Set();
function id(base) {
  let s = slug(base) || 'el';
  let candidate = s, i = 2;
  while (used.has(candidate)) candidate = `${s}-${i++}`;
  used.add(candidate);
  return candidate;
}
function style(n, extra = {}) {
  const s = {};
  const size = clamp(px(n.style['font-size']), 6, 96);
  if (size && size !== 13) s.size = size;
  const weight = px(n.style['font-weight']);
  if (weight >= 600) s.weight = 'bold'; else if (weight >= 500) s.weight = 'medium';
  if (n.style['font-style'] === 'italic') s.italic = true;
  const colour = hex(n.style.color);
  if (colour && colour !== ink) s.color = colour;
  const align = n.style['text-align'];
  if (align === 'center' || align === 'right') s.align = align;
  if (n.style['white-space'] === 'nowrap' && n.style['text-overflow'] === 'ellipsis') s.one_line = true;
  return {...s, ...extra};
}
function boxStyle(n) {
  const s = {};
  const bg = hex(n.style['background-color']);
  if (bg) s.background = bg;
  const bw = px(n.style['border-top-width']), bl = px(n.style['border-left-width']);
  if (bw > 0 && bl > 0 && n.style['border-top-style'] !== 'none') s.border = hex(n.style['border-top-color']) ?? '#e1e5eb';
  const radius = px(n.style['border-top-left-radius']);
  if (radius) s.radius = clamp(radius, 0, 512);
  const v = Math.max(px(n.style['padding-top']), px(n.style['padding-bottom']));
  const h = Math.max(px(n.style['padding-left']), px(n.style['padding-right']));
  // The format has one padding for every side: a single-line box keeps its height
  // (vertical padding); a stack keeps its inset (the larger side).
  const pad = lines(n.children).length <= 1 ? v : h > 0 ? Math.max(v, h) : 0;
  if (pad) s.padding = clamp(pad, 0, 64);
  return s;
}
function sameSite(href) {
  try {
    const u = new URL(href), s = new URL(capture.source);
    return u.protocol === s.protocol && u.host === s.host;
  } catch { return false; }
}
const sitePaths = new Set();
function url(href) {
  try {
    const u = new URL(href);
    if (sameSite(href)) {
      let path = u.pathname + u.search;
      if (u.protocol === 'file:') {
        const dir = new URL('.', capture.source).pathname;
        if (path.startsWith(dir)) path = '/' + path.slice(dir.length);
        path = path.replace(/\.html?$/, '');
        if (path === '/' + new URL(capture.source).pathname.split('/').pop().replace(/\.html?$/, '')) path = '/';
      }
      if (path === '/index') path = '/';
      sitePaths.add(path);
      return path;
    }
    return `http://${u.host}${u.pathname}${u.search}`;
  } catch { return href; }
}
const transparent = '#00000000';
/// The renderer measures DejaVu Sans, a wide face. A fixed width measured in Chrome grows
/// by the ratio of that face to the page's own: serifs (Georgia, Times) are about 40%
/// narrower at the same size, most sans faces about 20%, monospace about the same.
function widthRatio(n) {
  const family = n.style['font-family'] || capture.body.fontFamily || '';
  if (/mono|courier|consolas|menlo/i.test(family)) return 1.05;
  if (/georgia|times|serif|garamond|cambria|palatino|book/i.test(family) && !/sans/i.test(family)) return 1.28;
  return 1.18;
}
const textWidth = (n, w) => Math.round(w * widthRatio(n)) + 6;
const bgOf = (n, inherited) => hex(n.style['background-color']) ?? inherited;

/// Map one DOM box to zero or more page elements. `inherited` is the effective background.
function convert(n, inherited, context) {
  const name = n.id || (n.class || '').split(' ')[0] || n.tag;
  if (n.image) {
    const tint = screenshot ? meanColour(screenshot, n.bounds.x, n.bounds.y, n.bounds.width, n.bounds.height) : null;
    const s = {height: clamp(Math.round(n.bounds.height), 1, 8192), radius: clamp(px(n.style['border-top-left-radius']), 0, 512)};
    if (n.bounds.width < (context.width ?? Infinity) - 8) s.width = Math.round(n.bounds.width);
    if (tint) s.background = rgbHex(tint);
    s.color = ink;
    return [{kind: 'thumbnail', id: id(`img-${slug(n.image.alt) || name}`), label: n.image.alt || '', style: s}];
  }
  if (n.control) {
    return [{kind: 'input', id: id(n.control.name || n.id || 'field'), label: n.control.label ?? n.control.placeholder ?? n.control.name ?? 'Field', value: n.control.value ?? '', placeholder: n.control.placeholder ?? ''}];
  }
  if (n.button) {
    const action = context.form ?? {method: 'GET', url: pagePath, fields: {}};
    return [{kind: 'button', id: id(`${slug(n.button.text)}-submit`), text: n.button.text || 'Submit', action}];
  }
  if (n.tag === 'label' && context.consumedLabels?.has(textOf(n))) return [];
  if (n.tag === 'hr' || (n.bounds.height <= 2 && n.bounds.width > 40 && hex(n.style['background-color']))) {
    return [{kind: 'divider', id: id('rule'), style: {color: hex(n.style['background-color']) ?? hex(n.style['border-top-color']) ?? '#e1e5eb'}}];
  }
  if (n.form) {
    // The static-site service answers GET only, and drops the query, so a submission
    // lands on a stub confirmation page at the form's own path. A real backend (press,
    // shop, ...) is what --as <kind> is for.
    const form = {method: 'GET', url: url(n.form.action), fields: {}};
    if (n.form.method !== 'GET') report.notes.push(`form ${n.form.method} ${n.form.action} mapped to GET ${form.url} (static-site has no POST routes)`);
    const labels = new Set();
    (function collect(m) { if (m.control?.label) labels.add(m.control.label); m.children.forEach(collect); })(n);
    const children = stack(n, inherited, {...context, form, consumedLabels: labels});
    return [{kind: 'form', id: id(`${slug(name)}-form`), action: form, children}];
  }
  const els = isTextLeaf(n) ? textElement(n, inherited, context) : container(n, inherited, context);
  const fullBorder = px(n.style['border-top-width']) > 0 && px(n.style['border-left-width']) > 0;
  if (els.length && !fullBorder && px(n.style['border-bottom-width']) > 0 && n.style['border-bottom-style'] !== 'none' && !els.some(e => e.kind === 'divider')) {
    els.push({kind: 'divider', id: id('rule'), style: {color: hex(n.style['border-bottom-color']) ?? '#e1e5eb'}});
  }
  return els;
}

function textElement(n, inherited, context) {
  const raw = textOf(n);
  if (!raw) return [];
  const text = context.ordinal && n.tag === 'li' ? `${context.ordinal}. ${raw}` : raw;
  const ls = links(n);
  const bg = hex(n.style['background-color']);
  const short = text.length <= 24 && n.bounds.height <= 30 && n.bounds.width < 200;
  // A small filled, rounded run of text is a badge.
  if (bg && short && px(n.style['border-top-left-radius']) >= 4 && !ls.length) {
    return [{kind: 'badge', id: id(`badge-${text}`), text, style: {...style(n), background: bg, padding: clamp(px(n.style['padding-left']), 0, 64), radius: clamp(px(n.style['border-top-left-radius']), 0, 512)}}];
  }
  const s = style(n);
  if (bg) Object.assign(s, boxStyle(n));
  // The whole run is one link: a clickable card so the words keep their typography.
  const only = ls.length === 1 && textOf(ls[0]) === raw ? ls[0] : (n.href ? n : null);
  if (only?.href) {
    const inner = {kind: 'styled', id: id(`${slug(text)}-text`), text, style: {...s, color: hex(only.style.color) ?? theme.accent}};
    delete inner.style.background; delete inner.style.border;
    const card = {kind: 'card', id: id(`${slug(text)}-link`), children: [inner], style: {background: bg ?? transparent, padding: bg ? clamp(px(n.style['padding-left']), 0, 64) : 0, radius: bg ? clamp(px(n.style['border-top-left-radius']), 0, 512) : 0}, action: {method: 'GET', url: url(only.href), fields: {}}};
    if (s.border) card.style.border = s.border;
    if (n.bounds.width < (context.width ?? Infinity) * 0.6 && !context.grid) card.style.width = textWidth(n, n.bounds.width);
    return [card];
  }
  if (ls.length) report.inline_links_dropped.push({text: text.slice(0, 60), links: ls.map(l => ({text: textOf(l), href: l.href}))});
  const ts = text.length > 80 && s.size && s.size < 12 ? {...s, size: 12} : s;
  return [{kind: 'styled', id: id(`${slug(text)}`), text, style: ts}];
}

/// Children grouped into lines by vertical overlap: the geometry decides the layout,
/// not the CSS that produced it.
function lines(children) {
  const sorted = [...children].sort((a, b) => a.bounds.y - b.bounds.y || a.bounds.x - b.bounds.x);
  const out = [];
  for (const c of sorted) {
    const last = out[out.length - 1];
    if (last && c.bounds.y < last.bottom - 4 && c.bounds.y >= last.top - 4) {
      last.items.push(c);
      last.bottom = Math.max(last.bottom, c.bounds.y + c.bounds.height);
    } else out.push({top: c.bounds.y, bottom: c.bounds.y + c.bounds.height, items: [c]});
  }
  for (const l of out) l.items.sort((a, b) => a.bounds.x - b.bounds.x);
  return out;
}
function gapBetween(items, axis) {
  const gaps = [];
  for (let i = 1; i < items.length; i++) {
    const a = items[i - 1].bounds, b = items[i].bounds;
    gaps.push(axis === 'x' ? b.x - (a.x + a.width) : b.y - (a.y + a.height));
  }
  gaps.sort((a, b) => a - b);
  return gaps.length ? clamp(Math.round(gaps[gaps.length >> 1]), 0, 128) : 0;
}
/// A vertical stack of the node's children, with spacers where the page left air.
function stack(n, inherited, context) {
  const bg = bgOf(n, inherited);
  const out = [];
  let prevBottom = null;
  let ordinal = 0;
  for (const line of lines(n.children)) {
    if (n.tag === 'ol' && line.items.length === 1 && line.items[0].tag === 'li') context = {...context, ordinal: ++ordinal};
    if (prevBottom !== null) {
      const air = Math.round(line.top - prevBottom) - 12;
      if (air >= 8) out.push({kind: 'spacer', id: id('air'), height: clamp(air, 1, 8192)});
    }
    if (line.items.length === 1) out.push(...convert(line.items[0], bg, context));
    else out.push(rowOf(line.items, bg, context, n));
    prevBottom = line.bottom;
  }
  return out;
}
function rowOf(items, bg, context, parent) {
  const width = items.reduce((a, c) => a + c.bounds.width, 0);
  // The gap is the typical small gap; a much larger one (margin-left: auto,
  // justify-content: space-between) becomes a flexible spacer between the two children.
  const gaps = items.slice(1).map((c, i) => Math.max(0, c.bounds.x - (items[i].bounds.x + items[i].bounds.width)));
  const sorted = [...gaps].sort((a, b) => a - b);
  // The typical gap is the median of the small ones; a lone gap wider than 64 px is a push.
  const typical = sorted.length === 1 ? (sorted[0] > 64 ? 16 : sorted[0]) : sorted[Math.floor((sorted.length - 1) / 2)];
  const gap = clamp(Math.round(typical), 0, 128);
  const push = g => g > Math.max(gap * 3 + 16, 64);
  const children = items.flatMap((c, index) => {
    const els = convert(c, bg, {...context, width: c.bounds.width, row: true});
    const share = c.bounds.width / width;
    const textual = e => e.kind === 'styled' || e.kind === 'badge' || (e.kind === 'card' && e.children?.length === 1 && e.children[0].kind === 'styled' && !e.style?.border);
    for (const e of els) {
      if (e.kind === 'spacer' || e.kind === 'divider' || !e.style) continue;
      if (e.style.width !== undefined) continue;
      const text = textual(e) ? (e.text ?? e.children[0].text) : null;
      // Short text keeps its measured width (grown for the renderer's face); everything
      // else shares the leftover in proportion to its measured width.
      if (text !== null && text.length <= 40 && share < 0.6) e.style.width = Math.min(textWidth(c, c.bounds.width), Math.round(c.bounds.width * 1.6));
      else if (share < 0.2 && c.bounds.width < 200) e.style.width = Math.round(c.bounds.width);
      else e.style.flex = clamp(Math.round(share * 12) || 1, 1, 64);
    }
    const wrapped = els.length > 1 || (els[0] && ['group', 'form'].includes(els[0].kind))
      ? [{kind: 'card', id: id(`${slug(c.id || c.class || c.tag)}-cell`), children: els, style: {background: transparent, padding: 0, radius: 0, flex: clamp(Math.round(share * 12) || 1, 1, 64)}}]
      : els;
    if (index > 0 && push(gaps[index - 1])) wrapped.unshift({kind: 'spacer', id: id('push'), height: 0});
    return wrapped;
  });
  const ai = parent?.style['align-items'];
  const flexy = parent && /^(flex|grid|inline-flex|inline-grid)$/.test(parent.style.display);
  const align = ai === 'center' ? 'center' : ai === 'flex-end' || ai === 'end' ? 'end' : flexy && (ai === 'stretch' || ai === 'normal') ? 'stretch' : 'start';
  const s = {};
  if (parent && lines(parent.children).length === 1) Object.assign(s, boxStyle(parent));
  return {kind: 'row', id: id(`${slug(parent?.id || parent?.class || parent?.tag || 'row')}-row`), children, gap, align, style: s};
}
function container(n, inherited, context) {
  const name = n.id || (n.class || '').split(' ')[0] || n.tag;
  const bg = hex(n.style['background-color']);
  const box = boxStyle(n);
  const framed = (bg && bg !== inherited) || box.border;
  const ls = lines(n.children);
  const bottomRule = false; // convert() appends the rule for every box kind
  // A grid: several lines with the same number of columns at the same x positions.
  const counts = ls.map(l => l.items.length);
  const trackList = n.style.display === 'grid' ? (n.style['grid-template-columns'] || '').split(/\s+/).filter(t => /px|fr|%|auto|minmax/.test(t)) : [];
  const trackPx = trackList.map(t => parseFloat(t)).filter(v => v > 0);
  const equal = trackPx.length < 2 || Math.max(...trackPx) - Math.min(...trackPx) < Math.max(...trackPx) * 0.1;
  const tracks = equal ? trackList.length : 0;
  // Unequal columns (2fr 1fr) cannot be a Grid, whose cells are equal: every line
  // becomes a Row whose children carry their measured shares as flex.
  if (!equal && ls.length > 1 && ls.every(l => l.items.length > 1)) {
    const rows = ls.map(l => rowOf(l.items, bgOf(n, inherited), {...context, width: n.bounds.width}, null));
    rows.forEach(r => { r.gap = gapBetween(ls[0].items, 'x'); });
    return [{kind: 'group', id: id(`${slug(name)}-rows`), children: rows}];
  }
  const isGrid = (tracks > 1 && counts.every(c => c <= tracks) && counts[0] === tracks && ls.length >= 1) || ls.length > 1 && counts[0] > 1 && counts.every((c, i) => c === counts[0] || (i === ls.length - 1 && c < counts[0]))
    && ls.every(l => l.items.every((it, i) => Math.abs(it.bounds.x - ls[0].items[i].bounds.x) < 6));
  let element;
  if (isGrid) {
    const columns = clamp(tracks > 1 ? tracks : counts[0], 1, 12);
    const cellBg = bgOf(n, inherited);
    const children = ls.flatMap(l => l.items.flatMap(it => {
      const els = convert(it, cellBg, {...context, width: it.bounds.width, grid: true});
      return els.length === 1 && els[0].kind !== 'group' ? els : [{kind: 'card', id: id(`${slug(name)}-cell`), children: els, style: {background: transparent, padding: 0, radius: 0}}];
    }));
    element = {kind: 'grid', id: id(`${slug(name)}-grid`), columns, children, gap: Math.max(gapBetween(ls[0].items, 'x'), gapBetween(ls.map(l => l.items[0]), 'y')), style: framed ? box : {}};
  } else if (ls.length === 1 && ls[0].items.length > 1) {
    element = rowOf(ls[0].items, bgOf(n, inherited), {...context, width: n.bounds.width}, n);
    if (framed) element.style = {...element.style, ...box};
  } else {
    const children = stack(n, inherited, {...context, width: n.bounds.width - px(n.style['padding-left']) - px(n.style['padding-right'])});
    if (!children.length) return [];
    if (framed || (box.padding && box.padding >= 8)) {
      element = {kind: 'card', id: id(`${slug(name)}-card`), children, style: {background: bg ?? transparent, padding: box.padding ?? 0, radius: box.radius ?? 0, ...(box.border ? {border: box.border} : {})}};
    } else if (children.length === 1) {
      return bottomRule ? [...children, {kind: 'divider', id: id('rule'), style: {color: hex(n.style['border-bottom-color']) ?? '#e1e5eb'}}] : children;
    } else {
      element = {kind: 'group', id: id(`${slug(name)}-group`), children};
    }
  }
  if (context.row || context.grid) {
    if (element.style && n.bounds.height > 0 && (framed || context.grid)) { /* height follows content; the renderer equalises grid rows */ }
  }
  return bottomRule ? [element, {kind: 'divider', id: id('rule'), style: {color: hex(n.style['border-bottom-color']) ?? '#e1e5eb'}}] : [element];
}

// ---------- site kind detection ----------
function detectKind(n) {
  const text = textOf(n).toLowerCase();
  const count = (re) => (text.match(re) || []).length;
  const tags = new Map();
  (function walk(m) { tally(tags, m.tag); m.children.forEach(walk); })(n);
  const signals = {
    press: (tags.get('article') ?? 0) * 2 + count(/\b(by [a-z]+ [a-z]+|minutes ago|hours ago|opinion|breaking)\b/g),
    shop: count(/[$€£]\s?\d+(\.\d\d)?/g) * 2 + count(/\b(add to (cart|basket)|checkout|in stock|free shipping)\b/g) * 3,
    forum: count(/\b(reply|replies|upvote|points?|comments?|posted by)\b/g),
    docs: (tags.get('pre') ?? 0) * 3 + (tags.get('code') ?? 0) + count(/\b(api reference|getting started|installation)\b/g) * 2,
    mail: count(/\b(inbox|compose|unread|sign in|password)\b/g) * 2 + (tags.get('input') ?? 0),
    social: count(/\b(follow|followers|like|repost|share)\b/g),
  };
  const best = Object.entries(signals).sort((a, b) => b[1] - a[1])[0];
  const total = Object.values(signals).reduce((a, b) => a + b, 0) || 1;
  return {kind: best[1] > 0 ? best[0] : 'static', confidence: Math.round((best[1] / total) * 100) / 100, signals};
}

// ---------- backend synthesis: press ----------
function pressSeed() {
  const sections = [];
  const nav = [];
  (function walk(m) { if (m.tag === 'nav') links(m).forEach(l => nav.push(l)); m.children.forEach(walk); })(root);
  for (const l of nav) {
    const p = url(l.href);
    if (p !== '/' && /^\/[a-z-]+$/.test(p)) sections.push({id: p.slice(1), title: textOf(l)});
  }
  const articles = {};
  let tick = 100;
  (function walk(m) {
    const heading = /^h[1-6]$/.test(m.tag) ? m : null;
    if (heading) {
      const link = links(m)[0];
      const title = textOf(m);
      if (title && link && sameSite(link.href)) {
        const slugId = url(link.href).split('/').filter(Boolean).pop() || slug(title);
        if (!articles[slugId]) {
          const path = url(link.href);
          const section = sections.find(s => path.startsWith(`/${s.id}/`))?.id ?? sections[0]?.id ?? 'news';
          articles[slugId] = {id: slugId, section, year: '2026', title, dek: '', byline: 'Staff', date: 'Sep 19, 2026', tick: tick--, read_minutes: 4, body: []};
        }
      }
    }
    m.children.forEach(walk);
  })(root);
  // Deks and bylines: the text that follows a headline inside the same block.
  (function walk(m) {
    const kids = m.children;
    for (let i = 0; i < kids.length; i++) {
      const link = /^h[1-6]$/.test(kids[i].tag) ? links(kids[i])[0] : null;
      if (!link) continue;
      const slugId = url(link.href).split('/').filter(Boolean).pop();
      const a = articles[slugId];
      if (!a) continue;
      for (let j = i + 1; j < kids.length && j < i + 3; j++) {
        const t = textOf(kids[j]);
        if (!t) continue;
        if (/^by |·|ago$/i.test(t) || t.length < 40) a.byline = t.replace(/^by /i, '').split('·')[0].trim() || a.byline;
        else if (!a.dek) a.dek = t;
      }
      if (!a.body.length && a.dek) a.body = [a.dek, 'This article body was synthesised: the converter keeps a headline and its standfirst and never copies a publication\'s prose.'];
    }
    kids.forEach(walk);
  })(root);
  if (!sections.length) sections.push({id: 'news', title: 'News'});
  return {layout: 'magazine', brand: capture.title, tagline: 'Converted from a captured front page.', theme, sections, articles, subscribers: [], saved: {}, follows: {}};
}

// ---------- assemble ----------
const detected = detectKind(root);
const node = flag('--node', siteId);
const site = {
  id: siteId,
  kind: as === 'press' ? 'press' : 'static-site',
  node,
  domains: [domain, ...(domain.startsWith('www.') ? [] : [`www.${domain}`])],
  port: 80,
  place: {address: flag('--address', '203.0.113.250'), zone: 'internet', link: {from: flag('--link', 'pop-west'), latency_us: 1500}},
};
if (as === 'press') {
  site.initial_state = pressSeed();
  report.notes.push(`press seed synthesised: ${Object.keys(site.initial_state.articles).length} articles, ${site.initial_state.sections.length} sections`);
} else {
  const elements = stack(root, bodyBg, {width: capture.viewport.width});
  const page = {version: 1, title: capture.title || domain, elements, theme};
  if (capture.lang) page.lang = capture.lang;
  const pages = {[pagePath]: page};
  if (stubs) {
    for (const p of [...sitePaths].sort()) {
      if (pages[p]) continue;
      const name = p.split('/').filter(Boolean).pop()?.replace(/-/g, ' ') || 'page';
      pages[p] = {version: 1, title: capture.title || domain, theme, elements: [
        {kind: 'styled', id: 'stub-title', text: name.replace(/^\w/, c => c.toUpperCase()), style: {size: 26, weight: 'bold'}},
        {kind: 'styled', id: 'stub-note', text: 'This page was not captured; it stands in so every link on the front page resolves.', style: {color: theme.muted}},
        {kind: 'link', id: 'stub-home', text: 'Back to the front page', url: pagePath},
      ]};
    }
    report.notes.push(`${Object.keys(pages).length - 1} stub pages for same-site links`);
  }
  site.initial_state = {pages, records: {}, assets: {}};
}
report.detected_kind = detected;
report.theme = theme;
report.element_count = as === 'press' ? null : JSON.stringify(site.initial_state.pages[pagePath]).match(/"kind":/g).length;
const problems = validateSite(site);
report.validation = problems.length ? problems : 'ok';
await writeFile(out, JSON.stringify(site, null, 2) + '\n');
await writeFile(out.replace(/\.json$/, '') + '.report.json', JSON.stringify(report, null, 2) + '\n');
console.log(`${out}: ${site.kind} seed, ${report.element_count ?? '-'} elements, detected kind ${detected.kind} (${detected.confidence}), dropped ${report.dropped.length} boxes, ${report.inline_links_dropped.length} inline links lost, validation ${problems.length ? problems.length + ' problem(s)' : 'ok'}`);
if (problems.length) { console.error(problems.join('\n')); process.exit(1); }
