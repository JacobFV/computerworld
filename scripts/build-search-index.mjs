#!/usr/bin/env node
// Rebuild the search engines' document index from every site's `search_entries`.
//
// A static index, not a crawl: Service::initialize has no network handle, so crawling would mean a
// lazy HTTP fan-out whose first result set is empty and whose contents depend on scheduler
// interleaving. The index is a pure function of the sites files instead, and the engines differ
// only by their `authority_overrides`, which is what makes two engines disagree on ranking.
//
// Each engine's index is written to worlds/company-2026/index/<engine>.json and pulled into the
// world by that engine's `{"from_file": ...}` in sites/<engine>.json. It is data the blueprint
// reads, not a stage that reaches into a built world and edits it, so it may run before or after
// `cw-world build` and never has to agree with it about formatting.
import {readdir, readFile, writeFile} from 'node:fs/promises';
const root = new URL('../', import.meta.url);
const sitesUrl = new URL('worlds/company-2026/sites/', root);
const indexUrl = new URL('worlds/company-2026/index/', root);
const ENGINES = ['google-search', 'bing-search', 'ddg-search'];

async function readSites() {
  const names = (await readdir(sitesUrl)).filter((n) => n.endsWith('.json')).sort();
  return Promise.all(names.map(async (name) => {
    const site = JSON.parse(await readFile(new URL(name, sitesUrl), 'utf8'));
    const id = name.slice(0, -'.json'.length);
    if (site.id !== id) throw new Error(`${name}: id "${site.id}" must match the file name`);
    return site;
  }));
}

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
  const sites = await readSites();
  const documents = index(sites);
  for (const id of ENGINES) {
    const seed = sites.find((s) => s.id === id);
    if (!seed) throw new Error(`no site file for the search engine "${id}"`);
    const overrides = seed.authority_overrides ?? {};
    // An engine refuses to initialise on a vertical it does not declare, so a typo in an entry is
    // dropped with a warning rather than taking the whole world down at boot.
    const verticals = new Set(seed.initial_state.verticals ?? ['all']);
    const known = documents.filter((d) => {
      if (verticals.has(d.vertical)) return true;
      console.warn(`${id}: skipping ${d.url}: unknown vertical "${d.vertical}"`);
      return false;
    });
    const ranked = known.map((d) => (d.site in overrides ? {...d, authority: overrides[d.site]} : d));
    await writeFile(new URL(`${id}.json`, indexUrl), `${JSON.stringify(ranked, null, 2)}\n`);
  }
  console.log(`Indexed ${documents.length} document(s) into ${ENGINES.length} engine(s).`);
}
await main();
