'use strict';
const assert = require('assert');
const { validate, ingest, aggregate, alerts } = require('../src/readings');

const ok = (over = {}) => ({ node: 'node-01af', channel: 'v33', value: 3.3, unit: 'V', at: '2026-09-15T14:02:10Z', ...over });
let passed = 0;
function test(name, fn) {
  fn();
  passed += 1;
  console.log(`ok - ${name}`);
}

test('a good reading validates', () => {
  assert.strictEqual(validate(ok()), null);
});

test('bad readings say why', () => {
  assert.match(validate(ok({ node: 'x' })), /bad node/);
  assert.match(validate(ok({ channel: 'volts' })), /unknown channel/);
  assert.match(validate(ok({ value: 'high' })), /bad value/);
  assert.match(validate(ok({ at: 'noon' })), /bad timestamp/);
  assert.match(validate(null), /object/);
});

test('ingest splits a batch', () => {
  const { accepted, rejected } = ingest([ok(), ok({ unit: '' }), ok({ channel: 'temp', value: 31.5, unit: 'degC' })]);
  assert.strictEqual(accepted.length, 2);
  assert.deepStrictEqual(rejected, [{ index: 1, reason: 'missing unit for v33' }]);
});

test('aggregate keeps min, max, running mean and the latest', () => {
  const agg = aggregate([
    ok({ value: 3.30, at: '2026-09-15T14:02:30Z' }),
    ok({ value: 3.28, at: '2026-09-15T14:02:10Z' }),
    ok({ value: 3.32, at: '2026-09-15T14:02:20Z' }),
    ok({ node: 'node-02b3', channel: 'temp', value: 29.6, unit: 'degC' }),
  ]);
  assert.deepStrictEqual(agg['node-01af'].v33, { count: 3, min: 3.28, max: 3.32, mean: 3.3, last: 3.3, unit: 'V' });
  assert.strictEqual(agg['node-02b3'].temp.count, 1);
});

test('alerts flag readings outside the limits', () => {
  const a = alerts([ok({ value: 3.5 }), ok({ channel: 'temp', value: 25, unit: 'degC' })]);
  assert.strictEqual(a.length, 1);
  assert.strictEqual(a[0].channel, 'v33');
  assert.deepStrictEqual(a[0].limit, [3.234, 3.366]);
});

console.log(`${passed} tests passed`);
