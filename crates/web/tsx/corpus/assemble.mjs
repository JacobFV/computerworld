// Assembles cw-tsx's held-out corpus: real open-source React + TypeScript code,
// written by other people, that the compiled subset was never tuned on.
//
//     node crates/web/tsx/corpus/assemble.mjs <clones-dir>
//
// `<clones-dir>` holds a clone of every source below (any depth; `git clone
// --filter=blob:none --no-checkout` is enough), named by the source's `id`. Each
// clone is checked at its pinned commit, the files chosen by the rules below are
// copied to `src/<id>/<path in the repository>` with the source's licence, and
// `manifest.json` lists every project with its files, its kind and its split.
//
// Nothing here looks at whether code compiles. Projects are chosen by rule, and
// where a source has more than the cap, by a seeded hash of the project's id; the
// split into DEV and TEST is another seeded hash of the id, decided here, before
// any compiler work, and never changed afterwards.

import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';

const HERE = path.dirname(new URL(import.meta.url).pathname);
const SEED = 'cw-tsx-corpus-v1';

const SOURCES = [
  {
    id: 'todomvc',
    repository: 'https://github.com/tastejs/todomvc',
    commit: 'ff43b02e59dfa604386bb382034b2cd07c2bcd8a',
    license: 'MIT',
    licenseFile: 'license.md',
    projects: { dirs: ['examples/typescript-react/js'] },
  },
  {
    id: 'vite',
    repository: 'https://github.com/vitejs/vite',
    commit: 'bc598a6a8a6b7d6e157e9f19c16911cff8d2360c',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirs: ['packages/create-vite/template-react-ts/src'] },
  },
  {
    id: 'zustand',
    repository: 'https://github.com/pmndrs/zustand',
    commit: 'b57db4f86ef179285da216eeb291266da82c361c',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirs: ['examples/starter/src'] },
  },
  {
    id: 'realworld',
    repository: 'https://github.com/angelguzmaning/ts-redux-react-realworld-example-app',
    commit: 'cb397562724ea7f53758d5511c220690b99fe2f5',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirs: ['src'] },
  },
  {
    id: 'tanstack-table',
    repository: 'https://github.com/TanStack/table',
    commit: '21d713fc4947d2a08cc2136bb055889a61412ded',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirPattern: /^examples\/react\/[^/]+\/src$/, cap: 24 },
  },
  {
    id: 'tanstack-query',
    repository: 'https://github.com/TanStack/query',
    commit: '2443290fb5d8d31e7016087fc78216a0703fbfa0',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirPattern: /^examples\/react\/[^/]+\/src$/ },
  },
  {
    id: 'redux-toolkit',
    repository: 'https://github.com/reduxjs/redux-toolkit',
    commit: 'c9dac937d77adc3bf04842a43955e81d0e7a46da',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirPattern: /^examples\/query\/react\/[^/]+\/src$/ },
  },
  {
    id: 'refine',
    repository: 'https://github.com/refinedev/refine',
    commit: '2352eb5b6539e2f39ad9aef652279ad1dcf2c467',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { dirPattern: /^examples\/[^/]+\/src$/, cap: 12 },
  },
  {
    id: 'react-admin',
    repository: 'https://github.com/marmelab/react-admin',
    commit: '99e8c52b7db1712c0afa4eeed533e4a713dfc1ec',
    license: 'MIT',
    licenseFile: 'LICENSE.md',
    projects: { dirs: ['examples/demo/src', 'examples/crm/src', 'examples/simple/src'] },
  },
  {
    id: 'shadcn',
    repository: 'https://github.com/shadcn-ui/ui',
    commit: '98a1fe67b439324ddc857f47fbdce056600a4329',
    license: 'MIT',
    licenseFile: 'LICENSE.md',
    aliases: { '@/': 'apps/v4/' },
    support: ['apps/v4/registry/new-york-v4/ui', 'apps/v4/registry/new-york-v4/lib', 'apps/v4/registry/new-york-v4/hooks', 'apps/v4/lib', 'apps/v4/hooks'],
    projects: [
      { filePattern: /^apps\/v4\/registry\/new-york-v4\/examples\/[^/]+\.tsx$/, cap: 40 },
      { dirPattern: /^apps\/v4\/registry\/new-york-v4\/blocks\/[^/]+$/, cap: 15 },
      { filePattern: /^apps\/v4\/registry\/new-york-v4\/charts\/[^/]+\.tsx$/, cap: 15 },
    ],
  },
  {
    id: 'chakra',
    repository: 'https://github.com/chakra-ui/chakra-ui',
    commit: '961161428b8c59157ad921dd23303b73c294d73f',
    license: 'MIT',
    licenseFile: 'LICENSE',
    aliases: { 'compositions/': 'apps/compositions/src/' },
    support: ['apps/compositions/src/ui', 'apps/compositions/src/lib'],
    projects: { filePattern: /^apps\/compositions\/src\/examples\/[^/]+\.tsx$/, cap: 40 },
  },
  {
    id: 'mui',
    repository: 'https://github.com/mui/material-ui',
    commit: '190a83cf3784c53b57d7a80cf96df27276eed079',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { filePattern: /^docs\/data\/material\/.+\.tsx$/, cap: 40 },
  },
  {
    id: 'mantine-ui',
    repository: 'https://github.com/mantinedev/ui.mantine.dev',
    commit: '7bce6d790cf29472862a6bc9cfc68eecc79aedd1',
    license: 'MIT',
    licenseFile: 'LICENCE',
    projects: { dirPattern: /^lib\/[^/]+$/, cap: 30 },
  },
  {
    id: 'headlessui',
    repository: 'https://github.com/tailwindlabs/headlessui',
    commit: 'eea57cf46fd6767ed1059012f7073b88eb159fba',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { filePattern: /^playgrounds\/react\/page-examples\/.+\.tsx$/, cap: 15 },
  },
  {
    id: 'react-hook-form',
    repository: 'https://github.com/react-hook-form/react-hook-form',
    commit: '72a4c98f770d618c05a3fc726a7643b2d2db665b',
    license: 'MIT',
    licenseFile: 'LICENSE',
    projects: { filePattern: /^examples\/V7\/.+\.tsx$/, cap: 15 },
  },
];

/** Files a project keeps: its TypeScript and JSON, never tests, stories or builds. */
function keep(file) {
  if (!/\.(tsx|ts|json)$/.test(file)) return false;
  if (/(^|\/)(node_modules|dist|build|__tests__|__mocks__|__snapshots__|e2e|cypress|test|tests)\//.test(file)) return false;
  if (/\.(test|spec|story|stories|cy)\.(tsx?|json)$/.test(file)) return false;
  if (/(^|\/)(tsconfig[^/]*|package-lock)\.json$/.test(file)) return false;
  return true;
}

function hash(salt, id) {
  return createHash('sha256').update(`${SEED}:${salt}:${id}`).digest();
}

function git(dir, ...args) {
  return execFileSync('git', ['-C', dir, ...args], { maxBuffer: 1 << 30 }).toString();
}

function main() {
  const clones = process.argv[2];
  if (!clones) {
    console.error('usage: node assemble.mjs <clones-dir>');
    process.exit(2);
  }
  const out = path.join(HERE, 'src');
  fs.rmSync(out, { recursive: true, force: true });
  const manifest = { seed: SEED, split_rule: 'DEV when the first byte of sha256("cw-tsx-corpus-v1:split:<project id>") is even, TEST when odd', sources: [], projects: [] };
  for (const s of SOURCES) {
    const dir = path.join(clones, s.id);
    const head = git(dir, 'rev-parse', 'HEAD').trim();
    if (head !== s.commit) throw new Error(`${s.id}: clone is at ${head}, want ${s.commit}`);
    const all = git(dir, 'ls-tree', '-r', '--name-only', s.commit).split('\n').filter(Boolean);
    const rules = Array.isArray(s.projects) ? s.projects : [s.projects];
    const chosen = [];
    for (const r of rules) {
      let cands = [];
      if (r.dirs) cands = r.dirs.map((d) => ({ root: d, files: all.filter((f) => f.startsWith(d + '/')) }));
      if (r.dirPattern) {
        const dirs = new Set();
        for (const f of all) {
          const parts = f.split('/');
          for (let i = 1; i < parts.length; i++) {
            const d = parts.slice(0, i).join('/');
            if (r.dirPattern.test(d)) dirs.add(d);
          }
        }
        cands = [...dirs].sort().map((d) => ({ root: d, files: all.filter((f) => f.startsWith(d + '/')) }));
      }
      if (r.filePattern) cands = all.filter((f) => r.filePattern.test(f) && keep(f)).sort().map((f) => ({ root: f, files: [f] }));
      cands = cands
        .map((c) => ({ ...c, files: c.files.filter(keep) }))
        .filter((c) => c.files.some((f) => /\.tsx$/.test(f)));
      if (r.cap && cands.length > r.cap) {
        cands = cands
          .map((c) => ({ c, h: hash('sample', `${s.id}/${c.root}`) }))
          .sort((a, b) => Buffer.compare(a.h, b.h))
          .slice(0, r.cap)
          .map((x) => x.c)
          .sort((a, b) => (a.root < b.root ? -1 : 1));
      }
      chosen.push(...cands);
    }
    const support = all.filter((f) => (s.support || []).some((d) => f.startsWith(d + '/')) && keep(f));
    const files = new Set([...chosen.flatMap((c) => c.files), ...support, s.licenseFile]);
    const list = [...files].sort();
    // Check the files out in the clone (a blobless clone fetches them in batches),
    // then copy them.
    for (let i = 0; i < list.length; i += 500) {
      execFileSync('git', ['-C', dir, 'checkout', '-q', s.commit, '--', ...list.slice(i, i + 500)], { stdio: 'ignore' });
    }
    for (const f of list) {
      const body = fs.readFileSync(path.join(dir, f));
      if (f.endsWith('.json') && body.length > 200_000 && f !== s.licenseFile) continue;
      const dest = path.join(out, s.id, f);
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.writeFileSync(dest, body);
    }
    manifest.sources.push({
      id: s.id,
      repository: s.repository,
      commit: s.commit,
      license: s.license,
      license_file: `src/${s.id}/${s.licenseFile}`,
      aliases: s.aliases || {},
      support: s.support || [],
    });
    for (const c of chosen) {
      const id = `${s.id}/${c.root}`;
      const split = hash('split', id)[0] % 2 === 0 ? 'dev' : 'test';
      const kept = c.files.filter((f) => fs.existsSync(path.join(out, s.id, f)));
      manifest.projects.push({ id, source: s.id, root: c.root, split, files: kept });
    }
  }
  fs.writeFileSync(path.join(HERE, 'manifest.json'), JSON.stringify(manifest, null, 1) + '\n');
  const n = (sp) => manifest.projects.filter((p) => p.split === sp).length;
  console.log(`${manifest.projects.length} projects: ${n('dev')} dev, ${n('test')} test`);
}

main();
