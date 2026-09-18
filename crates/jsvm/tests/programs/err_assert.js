// A failing assertion reports its diff.
const assert = require('assert');
assert.strictEqual(1 + 1, 2);
assert.deepStrictEqual({ a: [1, 2] }, { a: [1, 2] });
try {
  assert.strictEqual('hello', 'world');
} catch (e) {
  console.log(e.name, e.code, e.message.split('\n')[0]);
}
console.log('checking totals');
assert.deepStrictEqual({ total: 10, items: ['a', 'b', 'c'] }, { total: 12, items: ['a', 'b', 'c'] });
