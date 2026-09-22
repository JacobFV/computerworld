#!/usr/bin/env node
// Builds the documentation site: `docs/*.md` become pages under `site/docs/`, with the
// guides in reading order in a sidebar, and the API references beside them. No
// dependencies: the Markdown the guides use (headings, paragraphs, fenced code, inline
// code, emphasis, links, tables, nested lists, block quotes, rules) is converted here.
//
//   node scripts/build-docs.mjs            # writes site/docs/ and docs/README.md
//   node scripts/build-docs.mjs --check    # converts everything, writes nothing, and
//                                          # fails if docs/README.md is stale
//
// `docs/README.md` is generated from `sections` and `unpublished` below, so the index a
// reader sees in the repository cannot drift from the one the site is built with.
//
// The Rust reference is rustdoc's output, copied to site/docs/api/rust/ by the pages
// workflow (`cargo doc --no-deps -p computerworld --lib`). The JavaScript reference is
// rendered from `pkg/web/computerworld.d.ts` when the Wasm bundle has been built.

import { readFileSync, writeFileSync, mkdirSync, existsSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const out = join(root, 'site', 'docs');
const repo = 'https://github.com/JacobFV/computerworld';
const check = process.argv.includes('--check');

// The two icon buttons in the top bar, and the snippet that settles the theme before the
// first paint — the same control and the same storage key as the rest of the site.
const hamburger = '<svg class="bars" viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M2 4h12M2 8h12M2 12h12"/></svg>';
const cross = '<svg class="cross" viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"><path d="M3.5 3.5l9 9M12.5 3.5l-9 9"/></svg>';
const sun = '<svg class="sun" viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"><circle cx="8" cy="8" r="3.1"/><path d="M8 1.1v1.7M8 13.2v1.7M1.1 8h1.7M13.2 8h1.7M3.15 3.15l1.2 1.2M11.65 11.65l1.2 1.2M12.85 3.15l-1.2 1.2M4.35 11.65l-1.2 1.2"/></svg>';
const moon = '<svg class="moon" viewBox="0 0 16 16" width="15" height="15" aria-hidden="true" fill="currentColor"><path d="M13.6 10.4A6 6 0 0 1 5.6 2.4a6 6 0 1 0 8 8Z"/></svg>';
const themeSnippet = `<script>/* Before the first paint: the reader's stored choice, or the device's own setting. */
(function(){try{var t=localStorage.getItem('cw-theme');if(t!=='light'&&t!=='dark')t=matchMedia('(prefers-color-scheme: light)').matches?'light':'dark';document.documentElement.dataset.theme=t}catch(e){}})();<\/script>`;

// The sidebar, in reading order. A guide's title is its first heading.
const sections = [
  { title: 'Start', pages: [
    { file: 'walkthrough.md', slug: 'index', blurb: 'What ComputerWorld is, a first episode in each language, and where everything else lives.' },
    { file: 'python.md', blurb: 'Install the wheel, build from source, check versions.' },
    { file: 'wasm.md', blurb: 'The npm package in Node and the browser, and the font pack.' },
    { file: 'native.md', blurb: 'The Rust crate: features, the owner and actor objects, the `cw` binary.' },
  ]},
  { title: 'Driving a machine', pages: [
    { file: 'agent-api.md', blurb: 'Grants, sessions, `step`, observations, evaluation.' },
    { file: 'action-families.md', blurb: 'Every family, op and payload, and the privileged/actor split.' },
    { file: 'programmatic-computer-use.md', blurb: 'Pointer, keyboard and application control from Python and JavaScript, with demos.' },
    { file: 'desktop-gui.md', blurb: 'The five OS shells, windows, launchers and interaction targets.' },
    { file: 'shell.md', blurb: 'The POSIX and PowerShell subsets a terminal runs.' },
    { file: 'debugging.md', blurb: 'Debugging a program inside the world from Visual Studio Code.' },
    { file: 'video-editing.md', blurb: 'The video editor and its deterministic media pipeline.' },
  ]},
  { title: 'Building worlds', pages: [
    { file: 'world-schema.md', blurb: 'The world definition: computers, OS profiles, network, services.' },
    { file: 'custom-world.md', blurb: 'Writing a world of your own.' },
    { file: 'blueprint.md', blurb: 'Building a world from YAML, directories and declared inputs.' },
    { file: 'application-sdk.md', blurb: 'What an application is to the kernel.' },
    { file: 'custom-application.md', blurb: 'Adding a native application.' },
    { file: 'service-sdk.md', blurb: 'What a service is to the network.' },
    { file: 'custom-service.md', blurb: 'Adding a synthetic-internet service.' },
    { file: 'html-migration.md', blurb: 'Moving a service from the `Page` format to HTML, with the search service as the worked example.' },
  ]},
  { title: 'How it works', pages: [
    { file: 'architecture.md', blurb: 'Crates and the boundaries between them.' },
    { file: 'computers.md', blurb: 'Filesystems, processes, users and packages.' },
    { file: 'networking.md', blurb: 'DNS, routes, transports, HTTP and the browser.' },
    { file: 'rendering.md', blurb: 'Scenes, text, rasterization and the frame contract.' },
    { file: 'determinism.md', blurb: 'What is promised to be byte-identical, and frames as labelled data.' },
    { file: 'checking.md', blurb: 'Bounded policy checks, search configuration, and replayable counterexamples.' },
    { file: 'security.md', blurb: 'Isolation, host access and what a restricted handle can reach.' },
    { file: 'performance.md', blurb: 'Measurements and what they were measured on.' },
  ]},
  { title: 'Project', pages: [
    { file: 'provenance.md', blurb: 'What was learned from the predecessor repositories.' },
    { file: 'migration.md', blurb: 'Moving between 0.x releases, and what the predecessor projects left behind.' },
    { file: 'releasing.md', blurb: 'How a release is built, verified and published.' },
  ]},
  { title: 'API reference', pages: [
    { file: 'api-python.md', blurb: 'The `computerworld` Python module: World, Environment, Snapshot.' },
    { slug: 'api-javascript', title: 'JavaScript API', blurb: 'The npm package’s exported classes and functions.', javascript: true },
    { href: 'api/rust/computerworld/index.html', title: 'Rust API (rustdoc)', blurb: 'The `computerworld` crate, generated by rustdoc.' },
  ]},
];

// ---------- Markdown ----------

const escape = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');
// A heading is Markdown, and a title is drawn as plain text: strip the code ticks and
// emphasis markers, and any tags an already-converted heading brought with it.
const plain = (s) => s.replace(/<[^>]+>/g, '').replace(/[`*_]/g, '');
const slugify = (s) => s.toLowerCase().replace(/<[^>]+>/g, '').replace(/&[a-z]+;/g, '').replace(/[^a-z0-9 _-]/g, '').trim().replace(/\s+/g, '-');

function rewriteHref(href, pages) {
  if (/^(https?:|mailto:|#)/.test(href)) return href;
  const [path, hash] = href.split('#');
  const target = path.replace(/^\.\//, '');
  const page = pages.get(target);
  if (page) return `${page.slug}.html${hash ? '#' + hash : ''}`;
  // A `docs/*.md` that is not in the sidebar has no page here, so it goes to the
  // repository like any other file rather than to a slug nothing wrote.
  // Anything else lives in the repository.
  const inRepo = target.startsWith('../') ? target.slice(3) : `docs/${target}`;
  const kind = inRepo.endsWith('/') || !/\.[a-z0-9]+$/i.test(inRepo) ? 'tree' : 'blob';
  return `${repo}/${kind}/main/${inRepo.replace(/\/$/, '')}${hash ? '#' + hash : ''}`;
}

function inline(text, pages) {
  // Protect code spans first; nothing inside them is Markdown.
  const codes = [];
  let s = text.replace(/(`+)([^`]|[^`][\s\S]*?[^`])\1(?!`)/g, (_, _t, code) => {
    codes.push(`<code>${escape(code)}</code>`);
    return `\u0000${codes.length - 1}\u0000`;
  });
  s = escape(s);
  s = s.replace(/!\[([^\]]*)\]\(([^)\s]+)\)/g, (_, alt, src) => `<img src="${rewriteHref(src, pages)}" alt="${alt}">`);
  s = s.replace(/\[([^\]]+)\]\(([^)\s]+)\)/g, (_, label, href) => `<a href="${rewriteHref(href, pages)}">${label}</a>`);
  s = s.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');
  s = s.replace(/(^|[\s(])\*([^*\s][^*]*?)\*(?=[\s.,;:)]|$)/g, '$1<em>$2</em>');
  s = s.replace(/(^|[\s(])_([^_\s][^_]*?)_(?=[\s.,;:)]|$)/g, '$1<em>$2</em>');
  s = s.replace(/\u0000(\d+)\u0000/g, (_, i) => codes[i]);
  return s;
}

function markdown(src, pages) {
  const lines = src.replace(/\r\n/g, '\n').split('\n');
  const headings = [];
  let i = 0;

  function block(indent = 0) {
    // Renders blocks until a line that is not indented at least `indent` (for lists).
    const parts = [];
    while (i < lines.length) {
      const line = lines[i];
      if (indent && line.trim() !== '' && line.search(/\S/) < indent) break;
      const t = line.slice(indent);
      if (t.trim() === '') { i++; continue; }
      let m;
      if ((m = /^(```+)\s*([\w,-]*)\s*$/.exec(t))) {
        const fence = m[1]; const lang = m[2].split(',')[0];
        const code = [];
        i++;
        while (i < lines.length && !lines[i].slice(indent).startsWith(fence)) { code.push(lines[i].slice(indent)); i++; }
        i++;
        const cls = lang ? ` class="language-${lang}"` : '';
        const note = lang === 'mermaid' ? '<p class="note">A Mermaid diagram, shown as its source.</p>' : '';
        parts.push(`${note}<pre><code${cls}>${escape(code.join('\n'))}</code></pre>`);
        continue;
      }
      if ((m = /^(#{1,6})\s+(.+?)\s*#*$/.exec(t))) {
        const level = m[1].length; const text = inline(m[2], pages); let id = slugify(m[2]);
        let n = 1; const base = id; while (headings.some((h) => h.id === id)) id = `${base}-${++n}`;
        headings.push({ level, text, id });
        parts.push(`<h${level} id="${id}">${text}<a class="anchor" href="#${id}" aria-label="Link to this section">#</a></h${level}>`);
        i++; continue;
      }
      if (/^(-{3,}|\*{3,}|_{3,})$/.test(t.trim())) { parts.push('<hr>'); i++; continue; }
      if (t.startsWith('>')) {
        const quote = [];
        while (i < lines.length && lines[i].slice(indent).startsWith('>')) { quote.push(lines[i].slice(indent).replace(/^>\s?/, '')); i++; }
        parts.push(`<blockquote>${markdown(quote.join('\n'), pages).html}</blockquote>`);
        continue;
      }
      if (/^\|/.test(t) && i + 1 < lines.length && /^\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/.test(lines[i + 1].slice(indent))) {
        const cells = (row) => row.trim().replace(/^\|/, '').replace(/\|$/, '').split(/(?<!\\)\|/).map((c) => c.trim().replace(/\\\|/g, '|'));
        const head = cells(t); i += 2;
        const rows = [];
        while (i < lines.length && /^\|/.test(lines[i].slice(indent))) { rows.push(cells(lines[i].slice(indent))); i++; }
        parts.push(`<div class="table"><table><thead><tr>${head.map((c) => `<th>${inline(c, pages)}</th>`).join('')}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((c) => `<td>${inline(c, pages)}</td>`).join('')}</tr>`).join('')}</tbody></table></div>`);
        continue;
      }
      if ((m = /^([-*+]|\d+[.)])\s+/.exec(t))) {
        const ordered = /\d/.test(m[1]); const tag = ordered ? 'ol' : 'ul';
        const marker = ordered ? /^\d+[.)]\s+/ : /^[-*+]\s+/;
        const items = [];
        const start = ordered ? parseInt(m[1], 10) : 1;
        while (i < lines.length) {
          const l = lines[i].slice(indent);
          const im = marker.exec(l);
          if (!im) break;
          const inner = indent + im[0].length;
          // First line of the item, then any continuation indented to the item's text.
          lines[i] = ' '.repeat(inner) + l.slice(im[0].length);
          const startAt = i;
          i++;
          while (i < lines.length && (lines[i].trim() === '' || lines[i].search(/\S/) >= inner)) {
            // A blank line followed by something less indented ends the item.
            if (lines[i].trim() === '' && (i + 1 >= lines.length || (lines[i + 1].trim() !== '' && lines[i + 1].search(/\S/) < inner))) break;
            i++;
          }
          const end = i;
          const saved = i; i = startAt;
          const body = block(inner);
          i = Math.max(i, end); if (i < saved) i = saved;
          items.push(`<li>${body.replace(/^<p>([\s\S]*?)<\/p>/, '$1')}</li>`);
        }
        parts.push(`<${tag}${ordered && start !== 1 ? ` start="${start}"` : ''}>${items.join('')}</${tag}>`);
        continue;
      }
      if (/^<[a-zA-Z!]/.test(t)) {
        const raw = [];
        while (i < lines.length && lines[i].trim() !== '') { raw.push(lines[i].slice(indent)); i++; }
        parts.push(raw.join('\n').replace(/href="([^"]+)"/g, (_, h) => `href="${rewriteHref(h, pages)}"`));
        continue;
      }
      // Paragraph: until a blank line or the start of another block.
      const para = [];
      while (i < lines.length) {
        const l = lines[i].slice(indent);
        if (l.trim() === '' || (indent && lines[i].search(/\S/) < indent)) break;
        if (para.length && (/^(```|#{1,6}\s|>|\|)/.test(l) || /^([-*+]|\d+[.)])\s+/.test(l) || /^(-{3,}|\*{3,})$/.test(l.trim()))) break;
        para.push(l.trim()); i++;
      }
      parts.push(`<p>${inline(para.join('\n'), pages)}</p>`);
    }
    return parts.join('\n');
  }

  const body = block();
  return { html: body, headings };
}

// ---------- Pages ----------

const pages = new Map();
for (const section of sections) {
  for (const page of section.pages) {
    if (page.href) continue;
    page.slug ??= page.file.replace(/\.md$/, '');
    if (page.file) {
      page.source = readFileSync(join(root, 'docs', page.file), 'utf8');
      page.title = plain(/^#\s+(.+)$/m.exec(page.source)?.[1] ?? page.slug);
      pages.set(page.file, page);
    }
  }
}

function javascriptReference() {
  const dts = join(root, 'pkg', 'web', 'computerworld.d.ts');
  const intro = [
    '# JavaScript API',
    '',
    'The npm package `computerworld` is one module for Node and the browser: Node gets the',
    'CommonJS glue through `exports`, everything else the ES module. These are its exported',
    'types, as `computerworld.d.ts` declares them. Objects that cross the boundary as `any`',
    'are plain JSON: the world definition, an environment config, actions, results,',
    'observations and scenes are the same documents the [agent API](agent-api.md) and',
    '[action families](action-families.md) describe.',
    '',
    'See the [Wasm guide](wasm.md) for loading the module and the font pack.',
    '',
  ];
  if (!existsSync(dts)) {
    intro.push('> The Wasm bundle was not built when this page was made, so the declarations are not shown. Run `bash scripts/build-wasm.sh` first.');
    return intro.join('\n');
  }
  // The generated glue for wasm-bindgen's own init is noise to a reader.
  const text = readFileSync(dts, 'utf8')
    .replace(/\/\* tslint:disable \*\/\n\/\* eslint-disable \*\/\n/, '')
    .replace(/export interface InitOutput \{[\s\S]*?\n\}\n/, 'export interface InitOutput { readonly memory: WebAssembly.Memory; /* wasm-bindgen internals */ }\n');
  intro.push('```ts', text.trim(), '```', '');
  return intro.join('\n');
}

const layout = ({ title, body, headings, page, section }) => {
  const nav = sections.map((s) => `<div class="group"><div class="group-title">${escape(s.title)}</div>${s.pages.map((p) => {
    const href = p.href ?? `${p.slug}.html`;
    const current = p === page ? ' aria-current="page"' : '';
    return `<a href="${href}"${current}>${escape(p.title)}</a>`;
  }).join('')}</div>`).join('');
  const toc = headings.filter((h) => h.level === 2).map((h) => `<a href="#${h.id}">${h.text}</a>`).join('');
  const flat = sections.flatMap((s) => s.pages).filter((p) => !p.href);
  const at = flat.indexOf(page);
  const prev = at > 0 ? flat[at - 1] : null; const next = at >= 0 && at < flat.length - 1 ? flat[at + 1] : null;
  const pager = (prev || next) ? `<nav class="pager"><span>${prev ? `<a href="${prev.slug}.html">&larr; ${escape(prev.title)}</a>` : ''}</span><span>${next ? `<a href="${next.slug}.html">${escape(next.title)} &rarr;</a>` : ''}</span></nav>` : '';
  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>${escape(title)} — ComputerWorld docs</title>
<meta name="description" content="${escape(page.blurb ?? title)}">
<meta name="theme-color" content="#0c0b0a" media="(prefers-color-scheme: dark)">
<meta name="theme-color" content="#faf8f4" media="(prefers-color-scheme: light)">
<link rel="icon" href="data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='7' fill='%230c0b0a'/%3E%3Cpath d='M9 20l4-8 3 5 2-3 5 6z' fill='%23ffb454'/%3E%3C/svg%3E">
<link rel="stylesheet" href="./docs.css">
${themeSnippet}
</head>
<body>
<a class="skip" href="#content">Skip to content</a>
<header class="top">
  <a class="brand" href="../">ComputerWorld</a>
  <nav class="links">
    <a href="./" aria-current="${page.slug === 'index' ? 'page' : 'false'}">Docs</a>
    <a href="${repo}">GitHub</a>
    <button class="icon-button menu" id="menu" type="button" aria-expanded="false" aria-controls="side" aria-label="Show the documentation menu">${hamburger}${cross}</button>
    <button class="icon-button theme" type="button" hidden aria-label="Switch theme">${sun}${moon}</button>
  </nav>
</header>
<div class="shell">
  <aside class="side" id="side">${nav}</aside>
  <main id="content">
    <p class="crumb">${escape(section.title)}</p>
    <article>${body}</article>
    ${pager}
    <footer><span>MIT licensed.</span>${page.file ? ` <a href="${repo}/blob/main/docs/${page.file}">Edit this page</a>` : ''}</footer>
  </main>
  ${toc ? `<nav class="toc" aria-label="On this page"><div class="group-title">On this page</div>${toc}</nav>` : ''}
</div>
<script src="./theme.js" defer></script>
<script>
// The menu covers the screen below the bar on a phone. What made it feel wrong was scroll
// chaining: a flick that reached the end of the menu carried on into the article behind
// it, so closing the menu left the reader somewhere they had never scrolled to. The panel
// keeps its own scrolling to itself (overscroll-behavior: contain) and opens at its top;
// the page is never locked, so the bar stays stuck to the top and the reader comes back to
// exactly the line they left.
const menu = document.getElementById('menu'), side = document.getElementById('side');
const wide = matchMedia('(min-width: 761px)');
function setMenu(open) {
  side.classList.toggle('open', open);
  menu.setAttribute('aria-expanded', String(open));
  menu.setAttribute('aria-label', open ? 'Hide the documentation menu' : 'Show the documentation menu');
  if (open) side.scrollTop = 0;
}
menu.addEventListener('click', () => setMenu(!side.classList.contains('open')));
// Widened past the breakpoint: the sidebar is a column again and there is nothing to lock.
wide.addEventListener('change', (e) => { if (e.matches && side.classList.contains('open')) setMenu(false); });
</script>
</body>
</html>
`;
};

// ---------- The repository's own index ----------

// Guides that are not on the site, because they are not guides. Each says its own status
// in its first lines; this says which kind it is from the outside, where it matters most:
// a reader in `docs/` cannot otherwise tell a live contract from a dated record.
const unpublished = [
  { file: 'web-engine-plan.md', kind: 'contract', blurb: 'The web engine\u2019s milestones, constraints and gates. Its milestones shipped in 0.2.0, but `crates/web/src/lib.rs` and `crates/web/DESIGN.md` still point here for the contract the engine is held to.' },
  { file: 'architecture-proposal.md', kind: 'record', blurb: 'The phase-2 design, approved 2026-09-17. Superseded by the guides above; kept for why the shape was chosen.' },
  { file: 'implementation-plan.md', kind: 'record', blurb: 'The acceptance plan that phase-2 approval authorised, 2026-09-17.' },
  { file: 'final-report.md', kind: 'record', blurb: 'What was measured and what was still missing when the implementation was reported complete.' },
  { file: 'IMPLEMENTATION-CONTRACTS.md', kind: 'record', blurb: 'How the agents that built phase 3 divided file ownership between them.' },
];

// Every guide in `docs/` is in one list or the other. A new file in neither would be
// invisible from the index and from the site at once, which is how a guide goes stale
// without anyone noticing it exists.
function checkIndexIsComplete() {
  const listed = new Set([
    ...sections.flatMap((section) => section.pages.map((page) => page.file)),
    ...unpublished.map((page) => page.file),
    'README.md',
  ]);
  const missing = readdirSync(join(root, 'docs'))
    .filter((name) => name.endsWith('.md') && !listed.has(name))
    .sort();
  if (missing.length) {
    console.error(`docs/ has guides in neither \`sections\` nor \`unpublished\` in ${'scripts/build-docs.mjs'}:`);
    for (const name of missing) console.error(`  docs/${name}`);
    process.exit(1);
  }
}

function indexMarkdown() {
  const lines = [
    '# Documentation',
    '',
    'Generated by `scripts/build-docs.mjs` from the same reading order the',
    '[documentation site](https://jacobfv.github.io/computerworld/docs/) is built with. Edit',
    'the `sections` and `unpublished` lists in that script, not this file; `--check` runs in',
    'CI and fails if the two have drifted apart.',
    '',
  ];
  for (const section of sections) {
    lines.push(`## ${section.title}`, '');
    for (const page of section.pages) {
      const target = page.file ?? (page.href ? `https://jacobfv.github.io/computerworld/docs/${page.href}` : `https://jacobfv.github.io/computerworld/docs/${page.slug}.html`);
      lines.push(`- [${page.title}](${target}) — ${page.blurb}`);
    }
    lines.push('');
  }
  lines.push('## Not on the site', '');
  lines.push('Plans and reports rather than guides. A **contract** is still binding on the code;');
  lines.push('a **record** describes a decision already taken and is not current guidance.', '');
  for (const page of unpublished) {
    const title = plain(/^#\s+(.+)$/m.exec(readFileSync(join(root, 'docs', page.file), 'utf8'))?.[1] ?? page.file);
    lines.push(`- [${title}](${page.file}) — **${page.kind}.** ${page.blurb}`);
  }
  lines.push('');
  return lines.join('\n');
}

const css = readFileSync(join(root, 'site', 'docs.css'), 'utf8');
const themeJs = readFileSync(join(root, 'site', 'theme.js'), 'utf8');

if (!check) {
  rmSync(out, { recursive: true, force: true });
  mkdirSync(out, { recursive: true });
  writeFileSync(join(out, 'docs.css'), css);
  writeFileSync(join(out, 'theme.js'), themeJs);
}

let count = 0;
for (const section of sections) {
  for (const page of section.pages) {
    if (page.href) continue;
    const source = page.javascript ? javascriptReference() : page.source;
    const { html, headings } = markdown(source, pages);
    page.title ??= plain(headings[0]?.text ?? page.slug);
    const doc = layout({ title: page.title, body: html, headings, page, section });
    if (!check) writeFileSync(join(out, `${page.slug}.html`), doc);
    count++;
  }
}
checkIndexIsComplete();
const indexPath = join(root, 'docs', 'README.md');
const index = indexMarkdown();
if (check) {
  const onDisk = existsSync(indexPath) ? readFileSync(indexPath, 'utf8') : '';
  if (onDisk !== index) {
    console.error('docs/README.md is stale: run `node scripts/build-docs.mjs`');
    process.exit(1);
  }
} else {
  writeFileSync(indexPath, index);
}

console.log(`${check ? 'checked' : 'wrote'} ${count} pages${check ? '' : ` in ${out}`} and the docs index`);
