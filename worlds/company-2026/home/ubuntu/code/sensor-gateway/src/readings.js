'use strict';
/**
 * Readings from the Atlas sensor-node: validation, aggregation and alerts.
 *
 * A reading is `{ node, channel, value, unit, at }` where `at` is an ISO 8601
 * timestamp. The board posts a batch every ten seconds; the gateway keeps the
 * batches in a JSON file and reports per-node, per-channel aggregates.
 */

/** @typedef {{node: string, channel: string, value: number, unit: string, at: string}} Reading */

const CHANNELS = new Set(['vin', 'v33', 'iload', 'temp', 'rh']);

/**
 * Check one reading; returns an error message or null.
 * @param {unknown} r
 * @returns {string | null}
 */
function validate(r) {
  if (typeof r !== 'object' || r === null) return 'reading must be an object';
  const { node, channel, value, unit, at } = /** @type {Record<string, unknown>} */ (r);
  if (typeof node !== 'string' || !/^node-[0-9a-f]{4}$/.test(node)) return `bad node ${JSON.stringify(node)}`;
  if (typeof channel !== 'string' || !CHANNELS.has(channel)) return `unknown channel ${JSON.stringify(channel)}`;
  if (typeof value !== 'number' || !Number.isFinite(value)) return `bad value for ${channel}`;
  if (typeof unit !== 'string' || unit === '') return `missing unit for ${channel}`;
  if (typeof at !== 'string' || Number.isNaN(Date.parse(at))) return `bad timestamp ${JSON.stringify(at)}`;
  return null;
}

/**
 * Split a batch into accepted readings and rejections.
 * @param {unknown[]} batch
 * @returns {{accepted: Reading[], rejected: {index: number, reason: string}[]}}
 */
function ingest(batch) {
  const accepted = [];
  const rejected = [];
  batch.forEach((r, index) => {
    const reason = validate(r);
    if (reason) rejected.push({ index, reason });
    else accepted.push(/** @type {Reading} */ (r));
  });
  return { accepted, rejected };
}

/**
 * Per node and channel: count, min, max, mean and the latest value.
 * @param {Reading[]} readings
 * @returns {Record<string, Record<string, {count: number, min: number, max: number, mean: number, last: number, unit: string}>>}
 */
function aggregate(readings) {
  /** @type {Record<string, Record<string, any>>} */
  const out = {};
  const sorted = [...readings].sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
  for (const r of sorted) {
    const node = (out[r.node] ??= {});
    const c = node[r.channel];
    if (!c) {
      node[r.channel] = { count: 1, min: r.value, max: r.value, mean: r.value, last: r.value, unit: r.unit };
    } else {
      c.count += 1;
      c.min = Math.min(c.min, r.value);
      c.max = Math.max(c.max, r.value);
      c.mean += (r.value - c.mean) / c.count;
      c.last = r.value;
    }
  }
  for (const node of Object.values(out)) {
    for (const c of Object.values(node)) c.mean = Math.round(c.mean * 1000) / 1000;
  }
  return out;
}

/** Limits from the LDO spec and the design review; a reading outside raises an alert. */
const LIMITS = {
  v33: [3.234, 3.366],
  vin: [4.75, 5.25],
  iload: [0, 0.25],
  temp: [-10, 60],
  rh: [0, 100],
};

/**
 * @param {Reading[]} readings
 * @returns {{node: string, channel: string, value: number, limit: [number, number], at: string}[]}
 */
function alerts(readings) {
  const out = [];
  for (const r of readings) {
    const limit = LIMITS[r.channel];
    if (!limit) continue;
    if (r.value < limit[0] || r.value > limit[1]) {
      out.push({ node: r.node, channel: r.channel, value: r.value, limit, at: r.at });
    }
  }
  return out;
}

module.exports = { CHANNELS, LIMITS, validate, ingest, aggregate, alerts };
