// Generators, iterators, async functions and promise combinators.
function* range(start, end, step = 1) {
  for (let i = start; i < end; i += step) yield i;
}
function* take(it, n) {
  let i = 0;
  for (const v of it) {
    if (i++ >= n) return 'done early';
    yield v;
  }
}
function* naturals() {
  let n = 0;
  while (true) yield n++;
}
console.log([...range(0, 10, 3)], [...take(naturals(), 5)]);
const g = take(naturals(), 2);
console.log(g.next(), g.next(), g.next(), g.next());

function* conversation() {
  const name = yield 'What is your name?';
  const hobby = yield `Hello ${name}! Hobby?`;
  try {
    yield `${name} likes ${hobby}`;
  } finally {
    console.log('cleanup ran');
  }
}
const c = conversation();
console.log(c.next().value, c.next('Ada').value, c.next('math').value, c.return('bye'), c.next());

function* delegating() {
  const r = yield* inner();
  yield 'inner returned ' + r;
}
function* inner() {
  yield 'a';
  yield 'b';
  return 'r';
}
console.log(Array.from(delegating()));

class Tree {
  constructor(value, children = []) {
    this.value = value;
    this.children = children;
  }
  *[Symbol.iterator]() {
    yield this.value;
    for (const ch of this.children) yield* ch;
  }
}
const tree = new Tree(1, [new Tree(2, [new Tree(3)]), new Tree(4)]);
console.log([...tree], Math.max(...tree));

const iterable = {
  from: 1,
  to: 4,
  [Symbol.iterator]() {
    let cur = this.from;
    const to = this.to;
    return {
      next: () => (cur <= to ? { value: cur++, done: false } : { value: undefined, done: true }),
      return() {
        console.log('iterator closed');
        return { done: true };
      },
    };
  },
};
for (const v of iterable) {
  if (v === 2) break;
  console.log('iter', v);
}
const [first, second] = iterable;
console.log(first, second);

const sleep = (ms, v) => new Promise((r) => setTimeout(() => r(v), ms));
async function worker(name, jobs) {
  const out = [];
  for (const j of jobs) {
    out.push(`${name}:${await sleep(j, j)}`);
  }
  return out;
}
async function* ticker(n) {
  for (let i = 0; i < n; i++) {
    await sleep(5);
    yield i;
  }
}
async function main() {
  const results = await Promise.all([worker('a', [10, 30]), worker('b', [20, 5])]);
  console.log(results);
  for await (const t of ticker(3)) console.log('tick', t);
  const errors = await Promise.allSettled([sleep(1, 'ok'), Promise.reject(new TypeError('bad'))]);
  console.log(errors.map((e) => e.status + ' ' + (e.value ?? e.reason.message)));
  try {
    await (async () => {
      await sleep(1);
      throw new Error('inner async failure');
    })();
  } catch (e) {
    console.log('caught:', e.message);
  }
  const first = await Promise.race([sleep(50, 'slow'), sleep(10, 'quick')]);
  const any = await Promise.any([Promise.reject(new Error('x')), sleep(3, 'any-ok')]);
  console.log(first, any);
  const thenable = { then(resolve) { resolve('from thenable'); } };
  console.log(await thenable);
  const it = ticker(2)[Symbol.asyncIterator]();
  console.log(await it.next(), await it.next(), await it.next());
  return 'main finished';
}
main().then(console.log, (e) => console.log('failed', e));
console.log('sync code done');
