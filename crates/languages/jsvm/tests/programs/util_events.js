// util, events, Buffer, crypto hashes, URL and friends.
const util = require('util');
const EventEmitter = require('events');
const crypto = require('crypto');

console.log(util.format('%s is %d years and %i days; %j; %o; %%', 'Bob', 42.5, 3.9, { a: 1 }, [1]));
console.log(util.format('extra', 'args', 1, { b: [2] }));
console.log(util.inspect({ deep: { deeper: { deepest: { gone: 1 } } } }, { depth: 1 }));
console.log(util.inspect('str'), util.inspect([1, 2, 3], { colors: false }), util.isDeepStrictEqual({ a: [1] }, { a: [1] }));
console.log(util.types.isPromise(Promise.resolve()), util.types.isRegExp(/x/), util.types.isDate(new Date(0)));
const wait = util.promisify((ms, cb) => setTimeout(() => cb(null, ms * 2), ms));
wait(5).then((v) => console.log('promisified', v));

class Job extends EventEmitter {}
const job = new Job();
job.on('progress', (p) => console.log('progress', p));
job.once('done', function (result) {
  console.log('done', result, this === job);
});
job.prependListener('progress', () => console.log('(first listener)'));
job.emit('progress', 50);
job.emit('done', 'ok');
job.emit('done', 'ignored');
console.log(job.listenerCount('progress'), job.eventNames(), job.emit('nothing'));
try {
  job.emit('error', new Error('unhandled job error'));
} catch (e) {
  console.log('emit error threw:', e.message);
}

const buf = Buffer.from('héllo wörld', 'utf8');
console.log(buf, buf.length, buf.toString('hex'), buf.toString('base64'));
console.log(Buffer.from('aGVsbG8=', 'base64').toString(), Buffer.concat([Buffer.from([1, 2]), Buffer.alloc(2, 9)]));
const b2 = Buffer.alloc(8);
b2.writeUInt32BE(0xdeadbeef, 0);
b2.writeInt16LE(-2, 4);
console.log(b2, b2.readUInt32BE(0).toString(16), b2.readInt16LE(4), b2.slice(0, 2), Buffer.isBuffer(b2));
console.log(buf.equals(Buffer.from('héllo wörld')), Buffer.compare(Buffer.from('a'), Buffer.from('b')), buf.indexOf('w'));

console.log(crypto.createHash('sha256').update('abc').digest('hex'));
console.log(crypto.createHash('md5').update('hello').digest('hex'), crypto.createHash('sha1').update('').digest('base64'));
console.log(crypto.createHmac('sha256', 'key').update('message').digest('hex'));

const u = new URL('https://user:pw@example.com:8080/a/b/../c?x=1&y=two#frag');
console.log(u.href, u.host, u.pathname, u.search, u.hash, u.searchParams.get('y'), u.origin);
u.searchParams.append('z', 'a b');
console.log(u.toString(), [...u.searchParams.keys()]);
const te = new TextEncoder();
console.log(te.encode('€'), new TextDecoder().decode(te.encode('round trip ✓')));
console.log(structuredClone({ d: new Date(0), m: new Map([[1, { x: 1 }]]) }));
