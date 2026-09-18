// Event loop ordering: sync, nextTick, microtasks, timers, immediates
const order = [];
const log = (s) => { order.push(s); console.log(s); };
setTimeout(() => log('timeout 0'), 0);
setTimeout(() => log('timeout 10'), 10);
setTimeout(() => log('timeout 5'), 5);
setImmediate(() => log('immediate'));
process.nextTick(() => log('nextTick 1'));
Promise.resolve().then(() => log('promise 1')).then(() => log('promise 2'));
queueMicrotask(() => log('microtask'));
process.nextTick(() => log('nextTick 2'));
(async () => {
  log('async start');
  await null;
  log('after await 1');
  await new Promise((r) => setTimeout(r, 20));
  log('after sleep 20');
  const results = await Promise.all([1, Promise.resolve(2), new Promise((r) => setTimeout(() => r(3), 1))]);
  log('all ' + JSON.stringify(results));
  const settled = await Promise.allSettled([Promise.reject(new Error('no')), Promise.resolve('yes')]);
  log('settled ' + settled.map((s) => s.status + ':' + (s.value || s.reason.message)).join(','));
  const raced = await Promise.race([new Promise((r) => setTimeout(() => r('slow'), 50)), new Promise((r) => setTimeout(() => r('fast'), 5))]);
  log('race ' + raced);
  try {
    await Promise.any([Promise.reject(new Error('a')), Promise.reject(new Error('b'))]);
  } catch (e) {
    log(e.constructor.name + ' ' + e.message + ' ' + e.errors.length);
  }
  const t0 = Date.now();
  await new Promise((r) => setTimeout(r, 1000));
  log('elapsed >= 1000: ' + (Date.now() - t0 >= 1000));
})();
log('sync end');

const iv = setInterval(() => {
  log('interval');
  if (order.filter((x) => x === 'interval').length === 3) clearInterval(iv);
}, 300);

async function* agen() {
  yield 1;
  await new Promise((r) => setTimeout(r, 2));
  yield 2;
}
(async () => {
  for await (const v of agen()) log('agen ' + v);
  for await (const v of [Promise.resolve('a'), 'b']) log('for await ' + v);
})();

const p = new Promise((resolve, reject) => { reject(new Error('handled now')); });
p.catch((e) => log('caught ' + e.message));
process.on('exit', (code) => console.log('exit event', code, order.length));
