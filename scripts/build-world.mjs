#!/usr/bin/env node
// Splice per-site seed data into the reference world.
//
// Every site owns exactly one file, worlds/company-2026/sites/<service-id>.json, so the content
// packages never open world.json and never collide with each other. A site file is a complete
// ServiceDefinition plus two build-only keys, which are stripped before the splice:
//
//   {"id": "theverge", "kind": "press", "node": "theverge",
//    "domains": ["theverge.com", "www.theverge.com"], "port": 80,
//    "initial_state": {...},
//    "search_entries": [{"url", "title", "snippet", "vertical", "authority", "keywords"}],
//    "authority_overrides": {"<site>": 7}}   // search engines only
//
// Upsert by id: a service already declared with that id is replaced in place, every other service
// is left untouched, and unknown ids append. Re-running is a no-op, so CI can rebuild and diff.
import {readdir, readFile, writeFile} from 'node:fs/promises';
import {pathToFileURL} from 'node:url';
const root = new URL('../', import.meta.url);
export const worldUrl = new URL('worlds/company-2026/world.json', root);
export const sitesUrl = new URL('worlds/company-2026/sites/', root);
/// Keys that exist for the build only and must never reach a ServiceDefinition.
const BUILD_ONLY = ['search_entries', 'authority_overrides'];
/// The world file is hand-read constantly; keep its exact on-disk shape so diffs stay reviewable.
export async function write(url, value) {
  await writeFile(url, `${JSON.stringify(value, null, 2)}\n`);
}
export async function readSites() {
  let names;
  try {
    names = (await readdir(sitesUrl)).filter((n) => n.endsWith('.json')).sort();
  } catch {
    return [];
  }
  return Promise.all(names.map(async (name) => {
    const site = JSON.parse(await readFile(new URL(name, sitesUrl), 'utf8'));
    const id = name.slice(0, -'.json'.length);
    // One file per service id is the whole basis of the parallel split; drift breaks ownership.
    if (site.id !== id) throw new Error(`${name}: id "${site.id}" must match the file name`);
    return site;
  }));
}
export async function readWorld() {
  return JSON.parse(await readFile(worldUrl, 'utf8'));
}
async function main() {
  const world = await readWorld();
  const nodes = new Set(world.network.nodes.map((n) => n.id));
  const sites = await readSites();
  for (const site of sites) {
    for (const key of ['id', 'kind', 'node']) {
      if (!site[key]) throw new Error(`${site.id ?? '<unnamed>'}: "${key}" is required`);
    }
    // WP-0 owns the network; a site may only bind to a node that already exists there.
    if (!nodes.has(site.node)) throw new Error(`${site.id}: unknown network node "${site.node}"`);
    const service = {...site};
    for (const key of BUILD_ONLY) delete service[key];
    const at = world.services.findIndex((s) => s.id === site.id);
    if (at < 0) world.services.push(service);
    else world.services[at] = service;
  }
  await write(worldUrl, world);
  console.log(`Spliced ${sites.length} site file(s); world declares ${world.services.length} services.`);
}
// Importable for the index build, runnable on its own.
if (import.meta.url === pathToFileURL(process.argv[1]).href) await main();
