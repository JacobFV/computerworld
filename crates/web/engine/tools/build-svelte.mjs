#!/usr/bin/env node
// Compiles the two hand-written Svelte 4 fixtures ahead of time into a single
// classic script the script-layer tests can load with `<script src>` (no ES
// module support, no runtime bundler).
//
// Svelte components normally get their `svelte/internal`, `svelte/store` and
// `svelte/transition` runtime resolved by whatever bundler compiles them.
// There's no bundler in the test fixture, so this script bundles that runtime
// in too: it downloads the pinned `svelte@4.2.19` package straight from the
// jsdelivr CDN (compiler + runtime sources, file by file, via jsdelivr's file
// listing API), compiles `Todos.svelte` and `Counter.svelte` with
// `svelte/compiler` (`generate: 'dom', css: 'injected', dev: false`), and
// bundles the compiled output together with the downloaded runtime sources
// using esbuild (pinned) into one IIFE that assigns `window.SvelteApp`.
//
// Run from anywhere: `node crates/web/engine/tools/build-svelte.mjs`. Requires
// network access to cdn.jsdelivr.net and the `npx` binary (to fetch the
// pinned esbuild). Writes crates/web/engine/tests/vendor/svelte-app-4.2.19.js;
// `fetch-vendor.sh` calls this and then checks the result's SHA-256.

import { createRequire } from 'node:module';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SVELTE_VERSION = '4.2.19';
const ESBUILD_VERSION = '0.24.0';
const CDN_BASE = `https://cdn.jsdelivr.net/npm/svelte@${SVELTE_VERSION}`;
const FLAT_LIST_URL = `https://data.jsdelivr.com/v1/package/npm/svelte@${SVELTE_VERSION}/flat`;

const HERE = path.dirname(fileURLToPath(import.meta.url));
const CRATE = path.dirname(HERE);
const SVELTE_SRC_DIR = path.join(CRATE, 'tests/script/svelte');
const OUT_FILE = path.join(CRATE, 'tests/vendor', `svelte-app-${SVELTE_VERSION}.js`);

async function fetchText(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`GET ${url}: ${res.status} ${res.statusText}`);
  return await res.text();
}

// Downloads every runtime/compiler source file of the pinned svelte package
// from the CDN into `destDir/src/...` and `destDir/compiler.cjs`, mirroring
// the package's own layout so the compiled components' bare `svelte`,
// `svelte/internal`, `svelte/store` and `svelte/transition` imports can be
// pointed straight at the downloaded files.
async function downloadSvelte(destDir) {
  const listing = JSON.parse(await fetchText(FLAT_LIST_URL));
  const wanted = listing.files.filter((f) => f.name === '/compiler.cjs' || f.name === '/LICENSE.md' || (f.name.startsWith('/src/') && f.name.endsWith('.js')));
  if (wanted.length === 0) throw new Error('jsdelivr file listing for svelte returned nothing usable');
  for (const f of wanted) {
    const body = await fetchText(`${CDN_BASE}${f.name}`);
    const dest = path.join(destDir, f.name);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.writeFileSync(dest, body);
  }
  return { fileCount: wanted.length, license: wanted.find((f) => f.name === '/LICENSE.md') !== undefined };
}

function compileComponent(compile, srcPath, componentName) {
  const source = fs.readFileSync(srcPath, 'utf8');
  const result = compile(source, {
    generate: 'dom',
    css: 'injected',
    dev: false,
    name: componentName,
    filename: path.basename(srcPath),
  });
  if (result.warnings.length) {
    for (const w of result.warnings) console.error(`svelte warning (${componentName}): ${w.message}`);
  }
  return result.js.code;
}

async function main() {
  const workDir = fs.mkdtempSync(path.join(os.tmpdir(), 'build-svelte-'));
  const svelteDir = path.join(workDir, 'svelte');
  console.log(`fetching svelte@${SVELTE_VERSION} runtime + compiler from ${CDN_BASE} ...`);
  const { fileCount } = await downloadSvelte(svelteDir);
  console.log(`  downloaded ${fileCount} files`);

  const compilerPath = path.join(svelteDir, 'compiler.cjs');
  const require = createRequire(import.meta.url);
  const { compile } = require(compilerPath);

  // Compile Counter.svelte first (Todos.svelte imports it), writing the
  // compiled intermediate files next to the .svelte sources so their own
  // relative imports (`./stores.js`, `./Counter.svelte`) keep resolving.
  const counterSrc = path.join(SVELTE_SRC_DIR, 'Counter.svelte');
  const todosSrc = path.join(SVELTE_SRC_DIR, 'Todos.svelte');
  const tmpCounter = path.join(SVELTE_SRC_DIR, '.tmp.Counter.js');
  const tmpTodos = path.join(SVELTE_SRC_DIR, '.tmp.Todos.js');

  try {
    console.log('compiling Counter.svelte ...');
    fs.writeFileSync(tmpCounter, compileComponent(compile, counterSrc, 'Counter'));

    console.log('compiling Todos.svelte ...');
    let todosCode = compileComponent(compile, todosSrc, 'Todos');
    // The compiler leaves `import Counter from "./Counter.svelte"` untouched
    // (compiling nested component imports is normally a bundler plugin's
    // job); point it at the sibling component's own compiled output instead.
    todosCode = todosCode.replace(/(['"])\.\/Counter\.svelte\1/, '$1./.tmp.Counter.js$1');
    fs.writeFileSync(tmpTodos, todosCode);

    // esbuild's `--global-name` assigns the entry module's *exports* to the
    // named global (that's what actually produces `window.SvelteApp`) -
    // export the API rather than assigning `window.SvelteApp` by hand, which
    // would just get clobbered by that same mechanism (with no exports of
    // its own, the wrapper's empty export object would overwrite it back to
    // `undefined` immediately after). Named exports (not a default export)
    // so `window.SvelteApp` is `{ Todos, Counter, stores }` directly, rather
    // than esbuild's CJS-interop `{ default: {...} }` wrapper.
    const entry = path.join(workDir, 'entry.mjs');
    fs.writeFileSync(
      entry,
      [
        `import Todos from ${JSON.stringify(tmpTodos)};`,
        `import Counter from ${JSON.stringify(tmpCounter)};`,
        `import { count, doubled } from ${JSON.stringify(path.join(SVELTE_SRC_DIR, 'stores.js'))};`,
        '',
        'const stores = { count, doubled };',
        'export { Todos, Counter, stores };',
        '',
      ].join('\n'),
    );

    const runtime = (p) => path.join(svelteDir, 'src/runtime', p);
    // Preserve the original provenance banner: fixture bytes are checksum-pinned.
    const banner = `/* svelte@${SVELTE_VERSION} (compiled components + bundled svelte/internal, svelte/store, svelte/transition runtime) - built by crates/web/tools/build-svelte.mjs - MIT, see svelte.LICENSE.md */`;

    console.log(`bundling with esbuild@${ESBUILD_VERSION} ...`);
    fs.mkdirSync(path.dirname(OUT_FILE), { recursive: true });
    execFileSync(
      'npx',
      [
        '-y',
        `esbuild@${ESBUILD_VERSION}`,
        entry,
        '--bundle',
        '--format=iife',
        '--global-name=SvelteApp',
        `--alias:svelte=${runtime('index.js')}`,
        `--alias:svelte/internal=${runtime('internal/index.js')}`,
        `--alias:svelte/internal/disclose-version=${runtime('internal/disclose-version/index.js')}`,
        `--alias:svelte/store=${runtime('store/index.js')}`,
        `--alias:svelte/transition=${runtime('transition/index.js')}`,
        '--minify',
        `--banner:js=${banner}`,
        `--outfile=${OUT_FILE}`,
        '--log-level=warning',
      ],
      { stdio: 'inherit' },
    );
  } finally {
    for (const f of [tmpCounter, tmpTodos]) {
      if (fs.existsSync(f)) fs.unlinkSync(f);
    }
  }

  const bytes = fs.readFileSync(OUT_FILE);
  const sha256 = createHash('sha256').update(bytes).digest('hex');
  console.log(`wrote ${path.relative(CRATE, OUT_FILE)}: ${bytes.length} bytes, sha256 ${sha256}`);

  fs.rmSync(workDir, { recursive: true, force: true });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
