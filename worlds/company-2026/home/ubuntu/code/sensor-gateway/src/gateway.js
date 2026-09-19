'use strict';
/**
 * sensor-gateway CLI.
 *
 *   node src/gateway.js ingest <batch.json>   validate a batch, append the good readings
 *   node src/gateway.js report                per-node aggregates and alerts
 *   node src/gateway.js health                what a load balancer would poll
 *
 * The store is data/readings.json (or $GATEWAY_STORE).
 */
const path = require('path');
const { ingest, aggregate, alerts } = require('./readings');
const store = require('./store');

const STORE = process.env.GATEWAY_STORE || path.join(__dirname, '..', 'data', 'readings.json');

function cmdIngest(file) {
  if (!file) throw new Error('ingest needs a batch file');
  const batch = JSON.parse(require('fs').readFileSync(file, 'utf8'));
  if (!Array.isArray(batch)) throw new Error('a batch is a JSON array');
  const { accepted, rejected } = ingest(batch);
  const all = store.load(STORE).concat(accepted);
  store.save(STORE, all);
  console.log(`accepted ${accepted.length}, rejected ${rejected.length}, stored ${all.length}`);
  for (const r of rejected) console.log(`  #${r.index}: ${r.reason}`);
  return rejected.length > 0 ? 1 : 0;
}

function cmdReport() {
  const readings = store.load(STORE);
  const agg = aggregate(readings);
  for (const [node, channels] of Object.entries(agg)) {
    console.log(node);
    for (const [channel, c] of Object.entries(channels)) {
      console.log(`  ${channel.padEnd(6)} n=${String(c.count).padStart(3)}  min ${c.min}  max ${c.max}  mean ${c.mean}  last ${c.last} ${c.unit}`);
    }
  }
  const a = alerts(readings);
  console.log(a.length === 0 ? 'no alerts' : `${a.length} alert(s):`);
  for (const x of a) console.log(`  ${x.at} ${x.node} ${x.channel}=${x.value} outside [${x.limit.join(', ')}]`);
  return 0;
}

function cmdHealth() {
  const readings = store.load(STORE);
  const latest = readings.map((r) => Date.parse(r.at)).reduce((m, t) => Math.max(m, t), 0);
  console.log(JSON.stringify({ status: 'ok', readings: readings.length, latest: latest ? new Date(latest).toISOString() : null }));
  return 0;
}

function main(argv) {
  const [command, ...rest] = argv;
  try {
    switch (command) {
      case 'ingest': return cmdIngest(rest[0]);
      case 'report': return cmdReport();
      case 'health': return cmdHealth();
      default:
        console.error('usage: gateway <ingest <batch.json> | report | health>');
        return 2;
    }
  } catch (e) {
    console.error(`gateway: ${e.message}`);
    return 2;
  }
}

if (require.main === module) process.exitCode = main(process.argv.slice(2));
module.exports = { main };
