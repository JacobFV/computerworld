// Language features
'use strict';
const log = console.log;

// closures & loops
const fns = [];
for (let i = 0; i < 3; i++) fns.push(() => i);
log(fns.map(f => f()));
var vfns = [];
for (var j = 0; j < 3; j++) vfns.push(() => j);
log(vfns.map(f => f()));

// destructuring
const [x, , y = 10, ...others] = [1, 2, undefined, 4, 5];
log(x, y, others);
const { p: { q = 'dq' } = {}, r: renamed = 7 } = { p: {}, r: undefined };
log(q, renamed);
function params({ a = 1, b } = {}, [c, d] = [3, 4], ...rest) { return [a, b, c, d, rest]; }
log(params(), params({ b: 2 }, [5], 6, 7));
let m1 = 1, m2 = 2;
[m1, m2] = [m2, m1];
log(m1, m2);

// spread
const o1 = { a: 1, b: 2 };
const o2 = { ...o1, b: 3, c: 4 };
log(o2, [...'hey'], Math.max(...[3, 9, 2]));

// optional chaining & nullish
const deep = { a: { b: { c: 42 } }, f() { return 'called'; } };
log(deep?.a?.b?.c, deep.x?.y, deep.f?.(), deep.g?.(), null ?? 'default', 0 ?? 'zero', 0 || 'or');
let la = null; la ??= 5; let lb = 1; lb &&= 2; let lc = 0; lc ||= 3;
log(la, lb, lc);

// labels, switch
outer: for (let i = 0; i < 3; i++) {
  for (let k = 0; k < 3; k++) {
    if (k === 1) continue outer;
    if (i === 2) break outer;
    log('ik', i, k);
  }
}
function sw(v) {
  switch (v) {
    case 1: return 'one';
    case 2:
    case 3: return 'two-three';
    default: return 'other';
  }
}
log(sw(1), sw(3), sw(9));

// getters/setters, computed keys, symbols
const key = 'dyn';
const obj = {
  _v: 1,
  get v() { return this._v * 10; },
  set v(n) { this._v = n; },
  [key + 'amic']: true,
  [Symbol.for('s')]: 'sym',
  method() { return 'm'; },
};
obj.v = 5;
log(obj.v, obj.dynamic, obj[Symbol.for('s')], obj);

// classes: static, private, getters, instanceof, inheritance chain
class Counter {
  static count = 0;
  #value = 0;
  static create() { Counter.count++; return new Counter(); }
  inc() { this.#value++; return this; }
  get value() { return this.#value; }
  #secret() { return 'hidden'; }
  reveal() { return this.#secret(); }
  static #priv = 'sp';
  static getPriv() { return Counter.#priv; }
  has(o) { return #value in o; }
}
const c = Counter.create().inc().inc();
log(c.value, Counter.count, c.reveal(), Counter.getPriv(), c.has(c), c.has({}), c);
class Shape { area() { return 0; } toString() { return `${this.constructor.name}(${this.area()})`; } }
class Rect extends Shape { constructor(w, h) { super(); this.w = w; this.h = h; } area() { return this.w * this.h; } }
class Square extends Rect { constructor(s) { super(s, s); } }
const sq = new Square(3);
log(String(sq), sq instanceof Rect, sq instanceof Shape, Object.getPrototypeOf(Square) === Rect, `${sq}`);

// generators
function* gen(n) { for (let i = 0; i < n; i++) { const r = yield i; if (r) log('got', r); } return 'done'; }
const g = gen(3);
log(g.next(), g.next('hello'), g.next(), g.next(), g.next());
function* inner() { yield 1; yield 2; return 3; }
function* outerG() { const r = yield* inner(); yield r; }
log([...outerG()]);
const fibs = function* () { let [a, b] = [0, 1]; while (true) { yield a; [a, b] = [b, a + b]; } };
const firstTen = [];
for (const f of fibs()) { if (firstTen.length >= 10) break; firstTen.push(f); }
log(firstTen);

// iterators protocol
const range = { from: 1, to: 4, [Symbol.iterator]() { let c = this.from, t = this.to; return { next: () => c <= t ? { value: c++, done: false } : { value: undefined, done: true } }; } };
log([...range], Array.from(range, v => v * v));

// tagged templates
function tag(strings, ...vals) { return strings.raw.map((s, i) => s + (vals[i] ?? '')).join('|'); }
log(tag`a${1}b${2}c\n`);

// exceptions with finally
function tryit(n) {
  try {
    if (n === 1) throw new Error('one');
    if (n === 2) return 'returned';
    return 'normal';
  } catch (e) {
    return 'caught ' + e.message;
  } finally {
    log('finally', n);
  }
}
log(tryit(0), tryit(1), tryit(2));
for (const v of [1, 2, 3]) {
  try { if (v === 2) continue; log('loop', v); } finally { log('fin', v); }
}

// typeof / instanceof / in / delete
const dobj = { a: 1, b: 2 };
delete dobj.a;
log('a' in dobj, 'b' in dobj, typeof undeclaredVar, [] instanceof Array, Array.isArray([]));

// numbers
log(0.1 * 3, 1 / 3, 2 ** 53, 2 ** 53 + 1, -0, [-0], 1e21, 123e-20, (255).toString(16), (0.5).toString(2), parseInt('08'), parseFloat('3.14xyz'), Number('0x1F'), +'', +' 12 ', Number(null), Number(undefined));
log(5 % 3, -5 % 3, 5.5 % 2, 7 / 0, -7 / 0, 0 / 0, Math.round(2.5), Math.round(-2.5), Math.round(-0.4));
log(1_000_000, 0b1010, 0o17, 0xff, 10n ** 20n, 7n / 2n, -7n % 3n, typeof 1n, BigInt(123) + 1n);
log((1234.5678).toFixed(2), (0.000001234).toPrecision(2), (123456).toExponential(2), (1e21).toLocaleString(), (1234567.891).toLocaleString());

// strings
log('abc'.padStart(6, '-'), 'abc'.at(-1), 'a,b,,c'.split(','), '  trim  '.trim() + '|', 'Hello'.replace('l', 'L'), 'Hello'.replaceAll('l', 'L'));
log('%s and %d', 'str', 42, 'extra', { z: 1 });
log(`multi
line`, 'x'.repeat(3), 'abc'.includes('b'), 'ABC'.toLowerCase(), 'straße'.toUpperCase(), 'é'.length, '😀'.length, [...'😀'].length);

// Array methods
const nums = [3, 1, 4, 1, 5, 9, 2, 6];
log(nums.slice().sort(), nums.indexOf(1), nums.lastIndexOf(1), nums.includes(9), nums.find(n => n > 4), nums.findIndex(n => n > 4), nums.findLast(n => n < 3));
log(nums.reduce((s, n) => s + n, 0), [[1, [2]], [3]].flat(), [[1, [2]], [3]].flat(Infinity), nums.some(n => n > 8), nums.every(n => n > 0));
log(Array.from({ length: 3 }, (_, i) => i * 2), Array(3).fill(0), new Array(5), [1, 2, 3].join('-'), [1, [2, [3]]].toString(), [3, 2, 1].toSorted(), nums.at(-1));
const sparse = [1, , 3];
log(sparse, sparse.length, 1 in sparse, Object.keys(sparse));
log([10, 9, 1, 100].sort(), ['b', 'a', 'C'].sort(), [1, 2, 3].reverse(), [1, 2, 3, 4].splice(1, 2));
log(Object.entries({ a: 1, b: 2 }), Object.fromEntries([['x', 1]]), Object.assign({}, { a: 1 }, { b: 2 }));
log(Object.keys({ 2: 'b', 1: 'a', z: 'z', 10: 'c' }));
