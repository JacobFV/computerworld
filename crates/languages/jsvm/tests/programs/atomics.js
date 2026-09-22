// SharedArrayBuffer and Atomics: the bytes two threads share, and the
// operations that read and write them a step at a time.
const { Worker } = require('worker_threads');

const sab = new SharedArrayBuffer(16);
console.log(
  'buffer',
  sab.byteLength,
  Object.prototype.toString.call(sab),
  sab instanceof ArrayBuffer,
  sab.slice(4, 12).byteLength
);
console.log('lock free', Atomics.isLockFree(1), Atomics.isLockFree(4), Atomics.isLockFree(3));

const i32 = new Int32Array(sab);
console.log('store', Atomics.store(i32, 1, 7), 'load', Atomics.load(i32, 1));
console.log('add', Atomics.add(i32, 1, 5), Atomics.load(i32, 1));
console.log('sub', Atomics.sub(i32, 1, 2), Atomics.load(i32, 1));
console.log('and', Atomics.and(i32, 1, 6), Atomics.load(i32, 1));
console.log('or', Atomics.or(i32, 1, 9), Atomics.load(i32, 1));
console.log('xor', Atomics.xor(i32, 1, 3), Atomics.load(i32, 1));
console.log('exchange', Atomics.exchange(i32, 1, 100), Atomics.load(i32, 1));
console.log('cas miss', Atomics.compareExchange(i32, 1, 5, 42), Atomics.load(i32, 1));
console.log('cas hit', Atomics.compareExchange(i32, 1, 100, 42), Atomics.load(i32, 1));
console.log('float index', Atomics.store(i32, 1, 2.9), Atomics.load(i32, 1));

const u8 = new Uint8Array(new SharedArrayBuffer(4));
console.log('u8 wrap', Atomics.store(u8, 0, 300), Atomics.load(u8, 0));
console.log('u8 add', Atomics.add(u8, 0, 200), Atomics.load(u8, 0));
const i8 = new Int8Array(new SharedArrayBuffer(4));
console.log('i8 wrap', Atomics.store(i8, 0, 200), Atomics.load(i8, 0));
console.log('i8 sub', Atomics.sub(i8, 0, 100), Atomics.load(i8, 0));

for (const [what, f] of [
  ['out of range', () => Atomics.load(i32, 4)],
  ['negative', () => Atomics.load(i32, -1)],
  ['float array', () => Atomics.add(new Float64Array(4), 0, 1)],
  ['plain array', () => Atomics.load([1, 2], 0)],
  ['unshared wait', () => Atomics.wait(new Int32Array(4), 0, 0)],
  ['byte wait', () => Atomics.wait(u8, 0, 0)],
]) {
  try {
    f();
    console.log(what, 'no error');
  } catch (e) {
    console.log(what, e.name, e.message);
  }
}

// A value that is not the one waited for comes back at once, and a wait with
// no time left times out without anything else having to happen.
console.log('not equal', Atomics.wait(i32, 1, 0));
console.log('timed out', Atomics.wait(i32, 1, 42, 0));
console.log('async not equal', JSON.stringify(Atomics.waitAsync(i32, 1, 0)));
console.log('async timed out', JSON.stringify(Atomics.waitAsync(i32, 1, 42, 0)));

// The worker flips a cell, the main thread is waiting for it to.
const flag = new Int32Array(sab, 8, 2);
const w = new Worker(
  `const { workerData } = require('worker_threads');
   const flag = new Int32Array(workerData, 8, 2);
   Atomics.store(flag, 0, 1);
   Atomics.notify(flag, 0);
   Atomics.add(flag, 1, 5);
   const pending = new Int32Array(workerData, 0, 1);
   setTimeout(() => { Atomics.add(pending, 0, 11); Atomics.notify(pending, 0); }, 5);`,
  { eval: true, workerData: sab }
);

// A wait that does not block: the promise settles once the cell moves.
const pending = new Int32Array(sab, 0, 1);
const async_wait = Atomics.waitAsync(pending, 0, Atomics.load(pending, 0), 1000);
console.log('async pending', async_wait.async);
async_wait.value.then((v) => console.log('async settled', v, Atomics.load(pending, 0)));

console.log('before', Atomics.load(flag, 0));
console.log('waited', Atomics.wait(flag, 0, 0), Atomics.load(flag, 0));
console.log('counter', Atomics.load(flag, 1));
console.log('nobody waiting', Atomics.notify(flag, 0));
w.on('exit', (code) => console.log('worker exit', code));
