// What the libraries under real web-app backends (Express, body-parser,
// jsonwebtoken, depd, debug) need from Node's core.

// encodeurl / punycode: surrogate escapes in a non-unicode pattern.
const UNMATCHED = /(^|[^\uD800-\uDBFF])[\uDC00-\uDFFF]|[\uD800-\uDBFF]([^\uDC00-\uDFFF]|$)/g;
console.log('unmatched', 'plain ascii'.replace(UNMATCHED, '$1�$2'));
console.log('pair', /😀/.test('smile \u{1F600}'), /^[\uD800-\uDBFF][\uDC00-\uDFFF]$/.test('x'));

// depd: Error.prepareStackTrace receives CallSite objects.
function where() {
  const prep = Error.prepareStackTrace;
  Error.prepareStackTrace = (err, sites) => sites;
  const obj = {};
  Error.captureStackTrace(obj, where);
  const sites = obj.stack;
  Error.prepareStackTrace = prep;
  return sites;
}
function caller() { return where(); }
const sites = caller();
console.log('sites', Array.isArray(sites), sites.length > 0);
console.log('site0', sites[0].getFunctionName(), sites[0].getFileName().endsWith('main.js'),
  typeof sites[0].getLineNumber(), typeof sites[0].getColumnNumber(), sites[0].isNative(), sites[0].isEval());
Error.prepareStackTrace = (err, s) => `${err.message} @ ${s[0].getFunctionName()}`;
function thrower() { return new Error('lazy').stack; }
console.log('prepared', thrower());
Error.prepareStackTrace = undefined;
console.log('plain', new Error('text').stack.split('\n')[0]);

// debug: tty.isatty on a pipe.
const tty = require('tty');
console.log('isatty', typeof tty.isatty, tty.isatty(1), tty.isatty(99));

// jwa: SlowBuffer and its prototype.
const buffer = require('buffer');
console.log('slowbuffer', typeof buffer.SlowBuffer, buffer.SlowBuffer.prototype === buffer.Buffer.prototype, buffer.kMaxLength > 0);

// iconv-lite: StringDecoder subclassed the ES5 way.
const { StringDecoder } = require('string_decoder');
function Decoder(enc) { StringDecoder.call(this, enc); }
Decoder.prototype = Object.create(StringDecoder.prototype);
const d = new Decoder('utf8');
console.log('decoder', d.write(Buffer.from([0xe2, 0x82])) + d.end(Buffer.from([0xac])));

// jsonwebtoken: secret KeyObjects.
const crypto = require('crypto');
const key = crypto.createSecretKey(Buffer.from('superSecret'));
console.log('key', key instanceof crypto.KeyObject, key.type, key.symmetricKeySize, key.asymmetricKeyType);
console.log('hmac', crypto.createHmac('sha256', key).update('a.b').digest('base64url') ===
  crypto.createHmac('sha256', 'superSecret').update('a.b').digest('base64url'));
console.log('export', key.export().toString());
try { crypto.createPrivateKey('superSecret'); console.log('private', 'accepted'); } catch (e) { console.log('private', 'refused'); }

// send (express.static): streams subclassed the ES5 way.
const Stream = require('stream');
const util = require('util');
function SendStream() { Stream.call(this); this.kind = 'send'; }
util.inherits(SendStream, Stream);
const ss = new SendStream();
console.log('es5 stream', ss instanceof Stream, typeof ss.pipe, typeof ss.on, ss.kind);
function Upper(opts) { Stream.Transform.call(this, opts); }
util.inherits(Upper, Stream.Transform);
Upper.prototype._transform = function (chunk, enc, cb) { cb(null, String(chunk).toUpperCase()); };
const up = new Upper();
let upOut = '';
up.on('data', (c) => { upOut += c; });
up.on('end', () => console.log('es5 transform', upOut));
up.end('shout');
class Counter extends Stream.Readable {
  constructor() { super(); this.n = 0; }
  _read() { this.push(this.n < 3 ? String(this.n++) : null); }
}
let counted = '';
new Counter().on('data', (c) => { counted += c; }).on('end', () => { console.log('class readable', counted); serve(); });
console.log('callable', Stream.Readable({}) instanceof Stream.Readable);

// on-headers / compression: writeHead is called when the head goes out.
function serve() {
const http = require('http');
const server = http.createServer((req, res) => {
  const writeHead = res.writeHead;
  res.writeHead = function (...args) { res.setHeader('x-on-headers', 'fired'); return writeHead.apply(this, args); };
  console.log('server sees', typeof res._implicitHeader, req.connection === req.socket);
  res.end('ok');
});
server.listen(0, () => {
  http.get({ port: server.address().port, path: '/' }, (res) => {
    console.log('client got', res.statusCode, res.headers['x-on-headers']);
    res.resume();
    res.on('end', () => server.close());
  });
});
}

// Unicode property escapes beyond letters and digits (punctuation, symbols, emoji).
console.log('props', /^\p{P}+$/u.test('!?,.'), /\p{S}/u.test('a+b'), /\p{Emoji}/u.test('ok \u{1F600}'),
  /^\p{Emoji}$/u.test('a'), '$5 €3'.match(/\p{Sc}/gu).join(''), /[\p{L}\p{P}]+/u.exec('hi, you')[0],
  /\P{P}+/u.exec('!!abc!!')[0], /\p{General_Category=Math_Symbol}/u.test('='), /\p{Extended_Pictographic}/u.test('❤'));
