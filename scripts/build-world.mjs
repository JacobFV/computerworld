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
///
/// `network_node` lets a new site bring its own host: {"address": "203.0.113.27",
/// "zone": "internet", "link": {"from": "pop-west", "latency_us": 1500}}. The node takes the
/// site's `node` id and is upserted with its link, so a site needs no hand edit of world.json.
const BUILD_ONLY = ['search_entries', 'authority_overrides', 'network_node'];
// The splice also emits a DNS A record for every site domain that world.json does not already
// name by hand, so a new site resolves without depending on the runtime's auto-add.
/// What each reference computer is, which decides what its shell shows (a laptop and a
/// phone report a battery; a desktop computer and a server have none). Declared here so
/// the world and every build from it (the live site world adds its phones) say the same.
export const DEVICE_PRESENTATIONS = {
  'alice-mac': 'desktop',
  'bob-windows': 'desktop',
  'carol-ubuntu': 'laptop',
  'app-server': 'server',
  'git-server': 'server',
};
/// The world file is hand-read constantly; keep its exact on-disk shape so diffs stay reviewable.
///
/// The hand-written parts (computers, network, the `.internal` services) stay pretty-printed.
/// A service spliced from `sites/` is reviewed in its own seed file, so it is written on one
/// line: the Wasm package ships this file, and 88 sites plus three copies of the search index
/// pretty-printed would push it well past 5 MB.
export async function write(url, value, compactServices = new Set()) {
  const token = (id) => `@@service:${id}@@`;
  const shape = {
    ...value,
    services: value.services.map((s) => (compactServices.has(s.id) ? token(s.id) : s)),
  };
  const text = JSON.stringify(shape, null, 2).replace(/"@@service:([^"@]+)@@"/g, (_, id) =>
    JSON.stringify(value.services.find((s) => s.id === id)));
  await writeFile(url, `${text}\n`);
}
/// Ids of the services that came from `sites/`: the ones `write` compacts.
export function siteIds(sites) {
  return new Set(sites.map((s) => s.id));
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
    if (site.network_node) {
      const {address, zone = 'internet', link} = site.network_node;
      if (!address || !link?.from) throw new Error(`${site.id}: network_node needs an address and link.from`);
      const node = {id: site.node, address, zone};
      const at = world.network.nodes.findIndex((n) => n.id === site.node);
      if (at < 0) world.network.nodes.push(node);
      else world.network.nodes[at] = node;
      const edge = {from: link.from, to: site.node, bidirectional: true, latency_us: link.latency_us ?? 1500, loss_per_million: 0};
      const existing = world.network.links.findIndex((l) => l.from === link.from && l.to === site.node);
      if (existing < 0) world.network.links.push(edge);
      else world.network.links[existing] = edge;
      nodes.add(site.node);
    }
    // Otherwise the network owns the node; a site may only bind to one that already exists.
    if (!nodes.has(site.node)) throw new Error(`${site.id}: unknown network node "${site.node}"`);
    const service = {...site};
    for (const key of BUILD_ONLY) delete service[key];
    const at = world.services.findIndex((s) => s.id === site.id);
    if (at < 0) world.services.push(service);
    else world.services[at] = service;
  }
  // Every site domain gets an A record at its node, so no name depends on the runtime's
  // auto-add. Hand-written records (the .northstar.example CNAMEs) win; the array is kept
  // sorted by name so the generated file diffs cleanly whatever the read order.
  const addressOf = new Map(world.network.nodes.map((n) => [n.id, n]));
  const records = new Map(world.network.dns.map((r) => [r.name.toLowerCase(), r]));
  for (const site of sites) {
    const node = addressOf.get(site.node);
    const local = node.zone === 'local';
    for (const name of site.domains ?? []) {
      const record = records.get(name.toLowerCase());
      if (record) {
        // A hand-written A record must agree with the node the site actually sits on.
        if (/^\d+\.\d+\.\d+\.\d+$/.test(record.address)) record.address = node.address;
        continue;
      }
      const fresh = {
        name,
        address: node.address,
        ttl_us: local ? 60_000_000 : 300_000_000,
        resolver: local ? 'app-server' : 'dns-public',
      };
      records.set(name.toLowerCase(), fresh);
      world.network.dns.push(fresh);
    }
  }
  world.network.dns.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
  for (const id of Object.keys(DEVICE_PRESENTATIONS)) {
    if (!world.computers.some((c) => c.id === id)) throw new Error(`device_presentations: no computer "${id}"`);
  }
  world.metadata = {...world.metadata, device_presentations: DEVICE_PRESENTATIONS};
  await write(worldUrl, world, siteIds(sites));
  console.log(`Spliced ${sites.length} site file(s); world declares ${world.services.length} services.`);
}
// Importable for the index build, runnable on its own.
if (import.meta.url === pathToFileURL(process.argv[1]).href) await main();
