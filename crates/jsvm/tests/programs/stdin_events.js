// process, stdin, readline, EventEmitter
const EventEmitter = require('events');
const readline = require('readline');
class Store extends EventEmitter {
  constructor() { super(); this.items = []; }
  add(item) { this.items.push(item); this.emit('added', item, this.items.length); return this; }
}
const store = new Store();
store.on('added', (item, n) => console.log('added', item, n));
store.once('added', () => console.log('first add only'));
store.add('a').add('b');
console.log(store.listenerCount('added'), store.eventNames(), store instanceof EventEmitter);
const e = new EventEmitter();
const h = () => console.log('should not run');
e.on('x', h); e.off('x', h); e.emit('x');
e.on('error', (err) => console.log('error event:', err.message));
e.emit('error', new Error('boom'));
e.prependListener('multi', () => console.log('prepended'));
e.on('multi', (a, b) => console.log('multi', a + b));
console.log(e.emit('multi', 1, 2), e.emit('nothing'));
console.log(new EventEmitter());
EventEmitter.once(e, 'later').then(([v]) => console.log('once promise', v));
e.emit('later', 42);

console.log(process.argv.slice(2), process.argv[0].includes('node'), typeof process.pid, process.cwd() === require('path').resolve('.'));
console.log(typeof process.hrtime.bigint(), process.hrtime().length, typeof process.uptime(), typeof process.memoryUsage().heapUsed);
console.log(process.env.HOME === undefined || typeof process.env.HOME === 'string', process.version.startsWith('v'), process.versions.node.split('.').length);

const rl = readline.createInterface({ input: process.stdin });
const lines = [];
rl.on('line', (line) => lines.push(line.trim()));
rl.on('close', () => {
  const [n, ...rest] = lines;
  const nums = rest.slice(0, Number(n)).map(Number);
  console.log('lines', lines.length, 'sum', nums.reduce((a, b) => a + b, 0), 'max', Math.max(...nums));
  console.log('words', lines.slice(Number(n) + 1).join(' ').split(/\s+/).filter(Boolean).length);
  process.stdout.write('done without newline');
  process.stdout.write('\n');
  process.exitCode = 3;
});
process.on('exit', (code) => console.log('exiting with', code));
