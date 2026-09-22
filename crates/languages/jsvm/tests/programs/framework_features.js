// Language and library features the front-end framework bundles (React, Vue, Svelte,
// styled-components, emotion) rely on, each exercised directly.

// React's input value tracker: wrap a prototype accessor on the instance, then
// redefine with a generic descriptor, which must keep the accessor.
class Input { #v = ''; get value() { return this.#v; } set value(v) { this.#v = String(v); } }
const node = new Input();
const desc = Object.getOwnPropertyDescriptor(Input.prototype, 'value');
let tracked = '';
Object.defineProperty(node, 'value', { configurable: true, get() { return desc.get.call(this); }, set(v) { tracked = '' + v; desc.set.call(this, v); } });
Object.defineProperty(node, 'value', { enumerable: desc.enumerable });
node.value = 42;
const after = Object.getOwnPropertyDescriptor(node, 'value');
console.log('tracker', tracked, node.value, typeof after.get, typeof after.set, after.enumerable, after.configurable);
delete node.value;
node.value = 'proto';
console.log('untracked', tracked, node.value);
// A generic descriptor over a data property keeps the value.
const o = { a: 1 };
Object.defineProperty(o, 'a', { enumerable: false });
console.log('data', o.a, JSON.stringify(Object.getOwnPropertyDescriptor(o, 'a')));

// Vue's runtime template compiler: new Function with a parameter list and `with`.
const render = new Function('Vue', 'const { h } = Vue\nreturn function render(_ctx) { with (_ctx) { return h + ":" + msg } }');
console.log('Function', render({ h: 'H' })({ msg: 'hello' }), render.name, render.length);
console.log('Function2', new Function('a', 'b = 2', '...rest', 'return a + b + rest.length')(1, undefined, 9, 9), Function('return typeof this')());
console.log('Function3', new Function('a, b', 'return a * b')(6, 7), (function () {}).constructor === Function);
const AsyncFunction = (async function () {}).constructor;
const GeneratorFunction = (function* () {}).constructor;
console.log('ctor kinds', AsyncFunction.name, GeneratorFunction.name, [...new GeneratorFunction('yield 1; yield 2')()].join());
try { new Function('return ]'); } catch (e) { console.log('Function syntax', e instanceof SyntaxError); }

// A function declaration's name is an ordinary binding, not the immutable
// self-reference a named function *expression* gets: emotion's `_extends` helper
// overwrites itself with `Object.assign` on its first call.
function _extends() { _extends = Object.assign ? Object.assign.bind() : function (t) { return t; }; return _extends.apply(null, arguments); }
console.log('decl rebinds', JSON.stringify(_extends({ a: 1 }, { b: 2 })), typeof _extends, _extends.name);
(function () { function g() { g = 2; return g; } console.log('decl sloppy', g(), typeof g); })();
(function () { 'use strict'; function g() { g = 3; return g; } console.log('decl strict', g(), typeof g); })();
{ function blockFn() { blockFn = 4; return blockFn; } console.log('decl block', blockFn()); }
console.log('expr self', (function self() { try { self = 1; } catch (e) { return 'threw ' + (e instanceof TypeError); } return typeof self; })(),
  (function () { 'use strict'; return (function self() { try { self = 1; return 'no'; } catch (e) { return e instanceof TypeError; } })(); })());

// Vue's reactivity: Proxy traps and Reflect with receivers.
const seen = [];
const target = { a: 1, get b() { return this.a + 1; }, list: [1, 2] };
const p = new Proxy(target, {
  get(t, k, r) { seen.push('get:' + String(k)); return Reflect.get(t, k, r); },
  set(t, k, v, r) { seen.push('set:' + String(k)); return Reflect.set(t, k, v, r); },
  has(t, k) { seen.push('has:' + String(k)); return Reflect.has(t, k); },
  deleteProperty(t, k) { seen.push('del:' + String(k)); return Reflect.deleteProperty(t, k); },
  ownKeys(t) { seen.push('keys'); return Reflect.ownKeys(t); },
});
p.a = 5; void p.b; void ('a' in p); delete p.list; Object.keys(p);
console.log('proxy', seen.join(' '), p.b);
const arr = new Proxy([1, 2, 3], { get(t, k, r) { return Reflect.get(t, k, r); } });
console.log('proxy array', Array.isArray(arr), arr.length, arr.map((x) => x * 2).join(), JSON.stringify(arr), arr.includes(2));
const revocable = Proxy.revocable({}, {});
revocable.revoke();
try { revocable.proxy.x; } catch (e) { console.log('revoked', e instanceof TypeError); }
console.log('reflect', Reflect.ownKeys({ b: 1, a: 2, [Symbol.iterator]: 3, 1: 0 }).map(String).join(), Reflect.getPrototypeOf([]) === Array.prototype,
  Reflect.construct(Date, [0]) instanceof Date, Reflect.apply(Math.max, null, [1, 3, 2]), Reflect.defineProperty({}, 'x', { value: 1 }),
  Reflect.construct(function () { return new.target; }, [], Map) === Map);
const m = new Proxy(new Map([[1, 2]]), { get(t, k, r) { const v = Reflect.get(t, k); return typeof v === 'function' ? v.bind(t) : v; } });
console.log('proxy map', m.get(1), m.size);

// WeakRef, FinalizationRegistry, WeakMap/WeakSet (Vue's target maps, React's caches).
const key = {};
const wr = new WeakRef(key);
const fr = new FinalizationRegistry(() => {});
fr.register(key, 'held'); fr.unregister(key);
const wm = new WeakMap([[key, 1]]); const ws = new WeakSet([key]);
console.log('weak', wr.deref() === key, wm.get(key), ws.has(key), wm.has({}), Object.prototype.toString.call(wr), typeof fr.register);

// Classes: accessors, statics, private methods, static blocks, Symbol.species-free subclassing.
class Base { static #count = 0; static make() { Base.#count++; return new this(); } static get count() { return Base.#count; } #secret() { return 's'; } reveal() { return this.#secret(); } static { Base.tag = 'T'; } }
class Derived extends Base { get kind() { return 'derived'; } set kind(v) { this._k = v; } static get [Symbol.species]() { return Base; } }
const d = Derived.make(); d.kind = 'x';
console.log('class', d.kind, d._k, Base.count, d.reveal(), Base.tag, d instanceof Derived, Object.getOwnPropertyNames(Derived.prototype).join());
class Arr extends Array { sum() { return this.reduce((a, b) => a + b, 0); } }
const ar = Arr.from([1, 2, 3]);
console.log('subclass', ar.sum(), ar instanceof Arr, ar.length, class extends Error { }.name === '', new (class E2 extends Error { constructor(m) { super(m); this.name = 'E2'; } })('m').toString());

// Tagged templates (styled-components, emotion): strings identity, raw, interpolation.
const calls = [];
function css(strings, ...vals) { calls.push(strings); return strings.raw.map((s, i) => s + (i < vals.length ? (typeof vals[i] === 'function' ? vals[i]({ c: 'red' }) : vals[i]) : '')).join(''); }
const make = () => css`color: ${(p) => p.c};\n  width: ${10}px; content: "é\n";`;
console.log('tagged', JSON.stringify(make()), (make(), calls[0] === calls[1]), Object.isFrozen(calls[0]), calls[0].length, String.raw`a\n${1}b`);
console.log('tagged member', ({ t(s) { return this.k + s[0]; }, k: 'K' }).t`x`, ((s) => s[0] === undefined)`\unicode`);

// Generators and iteration protocols.
function* gen() { const x = yield 1; try { yield x * 2; } finally { yield 'cleanup'; } return 'done'; }
const g = gen();
console.log('gen', JSON.stringify([g.next(), g.next(21), g.return('early'), g.next(), g.next()]));
function* deleg() { const r = yield* gen(); yield r; }
console.log('yield*', [...deleg()].join());
const it = { from: 1, to: 3, [Symbol.iterator]() { let c = this.from, t = this.to; return { next: () => (c <= t ? { value: c++, done: false } : { value: undefined, done: true }), return() { console.log('iter closed'); return {}; } }; } };
for (const v of it) { if (v === 2) break; }
const [first, ...others] = it; console.log('destructure', first, others.join(), Math.max(...it), Array.from(it, (x) => x * x).join(), new Set(it).size);
console.log('entries', [...new Map([[1, 'a']]).entries()].join(), [...'a𝒳b'].length, Object.fromEntries(Object.entries({ a: 1 }).map(([k, v]) => [k, v + 1])).a);

// Regex features.
const dm = /(?<year>\d{4})-(?<month>\d{2})/u.exec('on 2024-05-01');
console.log('named groups', dm.groups.year, dm.groups.month, dm.index, '2024-05'.replace(/(?<y>\d+)-(?<m>\d+)/, '$<m>/$<y>'), 'aa'.replace(/(?<c>a)\k<c>/, (...a) => JSON.stringify(a.at(-1))));
console.log('lookbehind', 'price $42 €17'.match(/(?<=\$)\d+/)[0], 'price $42 €17'.match(/(?<!\$)\b\d+/)[0], 'a-b_c'.replace(/(?<=[-_])\w/g, (c) => c.toUpperCase()));
const sticky = /foo/y; sticky.lastIndex = 3;
console.log('sticky', sticky.test('barfoo'), sticky.lastIndex, sticky.test('barfoo'), sticky.lastIndex, /a/y.flags, 'aXa'.split(/x/i).join());
console.log('regex misc', 'aBc'.replace(/b/i, '[$&|$`|$\']'), [...'a1b22'.matchAll(/\d+/g)].map((x) => x[0] + '@' + x.index).join(), /a.c/s.test('a\nc'), /(?:a|b)+?c/.exec('ababc')[0], 'x'.replaceAll('x', '$$'), /[\u{1F600}-\u{1F64F}]/u.test('😀'), /(a)|(b)/d.exec('b').indices[2].join());
// Unicode property escapes (general categories and the binary properties).
console.log('unicode props', /\p{L}+/u.exec('..abcé!')[0], /\P{L}/u.test('1'), /\p{Lu}/u.test('A'), /\p{Lu}/u.test('a'),
  /[\p{N}\p{L}]+/gu.exec('-a1-')[0], /\p{White_Space}/u.test('\t'), /\p{ASCII}/u.test('é'), '1a-é'.replace(/\p{L}/gu, '.'),
  /\p{Number}/u.test('٣'), /\p{Alphabetic}/u.test('中'), new RegExp('\\p{Ll}', 'u').test('q'), /\p{L}/.test('p{L}'));

// Vue's template parser and the hyphenate/camelize helpers.
console.log('vue regex', 'fooBarBaz'.replace(/\B([A-Z])/g, '-$1').toLowerCase(), 'foo-bar-baz'.replace(/-(\w)/g, (_, c) => (c ? c.toUpperCase() : '')), /^(?:v-|:|\.|@|#)/.test('@click'), /([\s\S]*?)\s+(?:in|of)\s+(\S[\s\S]*)/.exec('(item, i) in items').slice(1).join('~'));

// Intl bits.
console.log('intl', new Intl.NumberFormat('en-US').format(1234567.891), new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(12.5), (1234.5).toLocaleString('en-US'), new Intl.DateTimeFormat('en-US', { timeZone: 'UTC', year: 'numeric', month: 'short', day: 'numeric' }).format(new Date(0)), new Intl.PluralRules('en-US').select(1), new Intl.PluralRules('en-US').select(2), ['b', 'a', 'C'].sort(new Intl.Collator('en').compare).join(), typeof Intl.DateTimeFormat().resolvedOptions().timeZone);

// Misc builtins the bundles probe.
console.log('misc', typeof queueMicrotask, typeof structuredClone, typeof globalThis, Object.is(-0, 0), Number.isInteger(5), [1, [2, [3]]].flat(Infinity).join(), 'ab'.padStart(4, '-'), Symbol.for('react.element').toString(), Symbol('x').description, typeof Symbol.asyncIterator, [3, 1, 2].toSorted().join(), Object.hasOwn({ q: 1 }, 'q'), 'x'.at(-1), Array.prototype[Symbol.unscopables].flat, Math.clz32(1), Math.imul(3, 4), Math.fround(1.1).toFixed(4), Math.trunc(-1.5), Math.sign(-3), Number.EPSILON > 0, String.fromCodePoint(0x1F600).codePointAt(0), isFinite('12'), label());
function label() { outer: for (let i = 0; i < 3; i++) { for (let j = 0; j < 3; j++) { if (j === 1) continue outer; if (i === 2) break outer; } } return 'label-ok'; }
const { a: da = 5, ...restObj } = { b: 2, c: 3 }; console.log('spread', da, JSON.stringify(restObj), JSON.stringify({ ...restObj, ...null, d: [...'hi'] }), ((a, b = a + 1, ...r) => a + b + r.length)(1), null ?? 'dflt', ({ x: { y: 1 } }).x?.y, (void 0)?.z, (null)?.[1], 2 ** 10, (() => { let x = null; x ??= 4; x ||= 5; x &&= 6; return x; })());

// The with statement (Vue's runtime-compiled render functions).
const wo = { a: 1, f() { return this === wo; }, [Symbol.unscopables]: { hidden: true }, hidden: 'no' };
var hidden = 'outer', wb = 2;
with (wo) {
  console.log(a, wb, f(), hidden, typeof a, typeof nothere);
  a = 10; wb = 20;
  var hoisted = a + wb;
  let inner = 5;
  const fn = () => a + inner;
  console.log(fn(), [1, 2].map((x) => x + a).join());
  a++;
  a += 5;
}
console.log(wo.a, wb, hoisted);
function wrender(_ctx) { with (_ctx) { const { pre } = helpers; return function (n) { return pre + msg + n; }; } }
var helpers = { pre: '>' };
const wp = new Proxy({ msg: 'hi' }, { has(t, k) { return k[0] !== '_' && k !== 'helpers'; }, get(t, k) { return k === Symbol.unscopables ? undefined : t[k]; } });
console.log(wrender(wp)(1));
with ({ x: 1 }) with ({ y: 2 }) console.log(x + y);
try { with (null) {} } catch (e) { console.log(e instanceof TypeError); }

// Async iteration and ordering of microtasks vs. awaits (schedulers depend on it).
(async () => {
  const order = [];
  async function* agen() { for (let i = 0; i < 3; i++) { await null; yield i; } }
  for await (const v of agen()) order.push('v' + v);
  for await (const v of [Promise.resolve('p1'), 'p2']) order.push(v);
  const readable = { [Symbol.asyncIterator]() { let n = 0; return { next: async () => ({ value: n, done: n++ >= 2 }), return: async () => { order.push('closed'); return {}; } }; } };
  for await (const v of readable) { if (v === 1) break; }
  console.log('async iteration', order.join());
  const seq = [];
  const p1 = Promise.resolve().then(() => seq.push('then1')).then(() => seq.push('then2'));
  queueMicrotask(() => seq.push('qm'));
  (async () => { seq.push('a0'); await undefined; seq.push('a1'); await undefined; seq.push('a2'); })();
  setTimeout(() => seq.push('timeout'), 0);
  await p1; await new Promise((r) => setTimeout(r, 5));
  console.log('ordering', seq.join());
  console.log('combinators', JSON.stringify(await Promise.allSettled([Promise.reject(new Error('x')), 1]).then((r) => r.map((x) => x.status))), await Promise.any([Promise.reject(1), Promise.resolve(2)]), await Promise.race([new Promise(() => {}), 3]));
  try { await Promise.any([Promise.reject(1)]); } catch (e) { console.log('aggregate', e instanceof AggregateError, e.errors.join()); }
})();
