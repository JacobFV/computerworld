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
