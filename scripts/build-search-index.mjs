#!/usr/bin/env node
// Rebuild the search engines' document index from every site's `search_entries`.
//
// A static index, not a crawl: Service::initialize has no network handle, so crawling would mean a
// lazy HTTP fan-out whose first result set is empty and whose contents depend on scheduler
// interleaving. The index is a pure function of the sites files instead, and the engines differ
// only by their `authority_overrides`, which is what makes two engines disagree on ranking.
//
// Run after build-world.mjs: it writes into services that the splice has already landed.
import {readSites, readWorld, write, worldUrl} from './build-world.mjs';
const ENGINES = ['google-search', 'bing-search', 'ddg-search'];
function index(sites) {
  const documents = [];
  for (const site of sites) {
    const from = (site.domains ?? [])[0] ?? site.id;
    for (const entry of site.search_entries ?? []) {
      if (!entry.url || !entry.title) throw new Error(`${site.id}: search entry needs url and title`);
      documents.push({site: from, vertical: 'all', authority: 0, keywords: [], ...entry});
    }
  }
  // (site, url) is a total order, so the generated array diffs cleanly whatever the read order.
  return documents.sort((a, b) => a.site.localeCompare(b.site) || a.url.localeCompare(b.url));
}
async function main() {
  const world = await readWorld();
  const sites = await readSites();
  const documents = index(sites);
  let written = 0;
  for (const id of ENGINES) {
    const at = world.services.findIndex((s) => s.id === id);
    if (at < 0) continue; // The package that owns this engine has not landed it yet.
    const overrides = (sites.find((s) => s.id === id) ?? {}).authority_overrides ?? {};
    world.services[at].initial_state = {
      ...world.services[at].initial_state,
      documents: documents.map((d) => (d.site in overrides ? {...d, authority: overrides[d.site]} : d)),
    };
    written += 1;
  }
  await write(worldUrl, world);
  console.log(`Indexed ${documents.length} document(s) into ${written} of ${ENGINES.length} engine(s).`);
}
await main();
