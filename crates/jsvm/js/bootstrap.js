'use strict';
// Completes the Node environment on top of the native core: process as an
// EventEmitter, process.stdin, Buffer, URL, AbortController, Intl, ...

const EventEmitter = require('events');

// ---- process
Object.setPrototypeOf(process, EventEmitter.prototype);
EventEmitter.init.call(process);

const readStdin = process['%readStdin'];
delete process['%readStdin'];

const stdin = new EventEmitter();
let stdinEncoding = null;
let stdinScheduled = false;
let stdinDone = false;
stdin.fd = 0;
stdin.readable = true;
stdin.setEncoding = function setEncoding(enc) { stdinEncoding = enc || 'utf8'; return this; };
function deliverStdin() {
  if (stdinDone) return;
  stdinDone = true;
  const data = readStdin();
  if (data.length) stdin.emit('data', stdinEncoding ? data : Buffer.from(data));
  stdin.readable = false;
  stdin.emit('end');
  stdin.emit('close');
}
function scheduleStdin() {
  if (stdinScheduled) return;
  stdinScheduled = true;
  setImmediate(deliverStdin);
}
stdin.__onListenerAdded = function (type) {
  if (type === 'data' || type === 'end' || type === 'readable' || type === 'close') scheduleStdin();
};
stdin.resume = function resume() { scheduleStdin(); return this; };
stdin.pause = function pause() { return this; };
stdin.unref = function unref() { return this; };
stdin.ref = function ref() { return this; };
stdin.setRawMode = function setRawMode() { return this; };
let stdinReadBuffer = null;
stdin.read = function read() {
  if (stdinReadBuffer === null) {
    const d = readStdin();
    stdinReadBuffer = d;
    if (!d.length) return null;
    return stdinEncoding ? d : Buffer.from(d);
  }
  return null;
};
stdin.pipe = function pipe(dest) {
  stdin.on('data', (d) => dest.write(d));
  return dest;
};
stdin[Symbol.asyncIterator] = async function* () {
  stdinDone = true;
  const d = readStdin();
  if (d.length) yield stdinEncoding ? d : Buffer.from(d);
};
Object.defineProperty(process, 'stdin', { value: stdin, enumerable: true, configurable: true, writable: true });

process.abort = function abort() { process.exit(134); };
process.kill = function kill() { return true; };
process.binding = function () { return {}; };
process.features = { inspector: false, ipv6: true, tls: false, typescript: 'strip' };
process.config = { variables: {} };
process.allowedNodeEnvironmentFlags = new Set();
process.report = {};

// ---- Buffer
class Buffer extends Uint8Array {
  static from(value, encodingOrOffset, length) {
    if (typeof value === 'string') return binding.bufferFromString(value, encodingOrOffset || 'utf8');
    if (value instanceof ArrayBuffer) {
      const u = new Uint8Array(value, encodingOrOffset || 0, length);
      const b = Buffer.alloc(u.length);
      b.set(u);
      return b;
    }
    if (value && typeof value === 'object') {
      if (value.type === 'Buffer' && Array.isArray(value.data)) value = value.data;
      const arr = Array.from(value);
      const b = Buffer.alloc(arr.length);
      for (let i = 0; i < arr.length; i++) b[i] = arr[i] & 255;
      return b;
    }
    const e = new TypeError('The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object. Received ' + (value === undefined ? 'undefined' : value === null ? 'null' : `type ${typeof value} (${require('util').inspect(value)})`));
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  static alloc(size, fill, encoding) {
    const b = new Buffer(size);
    if (fill !== undefined && fill !== 0) b.fill(fill, 0, b.length, encoding);
    return b;
  }
  static allocUnsafe(size) { return new Buffer(size); }
  static allocUnsafeSlow(size) { return new Buffer(size); }
  static isBuffer(b) { return b instanceof Buffer; }
  static isEncoding(e) { return ['utf8', 'utf-8', 'hex', 'base64', 'base64url', 'ascii', 'latin1', 'binary', 'ucs2', 'ucs-2', 'utf16le', 'utf-16le'].includes(String(e).toLowerCase()); }
  static byteLength(s, enc) { return binding.byteLength(s, enc); }
  static concat(list, total) {
    if (total === undefined) total = list.reduce((n, b) => n + b.length, 0);
    const out = Buffer.alloc(total);
    let off = 0;
    for (const b of list) {
      for (let i = 0; i < b.length && off < total; i++) out[off++] = b[i];
    }
    return out;
  }
  static compare(a, b) { return a.compare(b); }
  toString(encoding, start, end) { return binding.bufferToString(this, encoding || 'utf8', start, end); }
  toJSON() { return { type: 'Buffer', data: Array.from(this) }; }
  equals(o) {
    if (this.length !== o.length) return false;
    for (let i = 0; i < this.length; i++) if (this[i] !== o[i]) return false;
    return true;
  }
  compare(o) {
    const n = Math.min(this.length, o.length);
    for (let i = 0; i < n; i++) {
      if (this[i] !== o[i]) return this[i] < o[i] ? -1 : 1;
    }
    return this.length === o.length ? 0 : this.length < o.length ? -1 : 1;
  }
  write(string, offset = 0, length, encoding = 'utf8') {
    if (typeof offset === 'string') { encoding = offset; offset = 0; }
    const b = binding.bufferFromString(string, encoding);
    const n = Math.min(b.length, this.length - offset, length === undefined ? Infinity : length);
    for (let i = 0; i < n; i++) this[offset + i] = b[i];
    return n;
  }
  fill(value, offset = 0, end = this.length, encoding) {
    if (typeof value === 'string') {
      const b = binding.bufferFromString(value, encoding || 'utf8');
      if (b.length === 0) return this;
      for (let i = offset, j = 0; i < end; i++, j++) this[i] = b[j % b.length];
      return this;
    }
    return super.fill(value, offset, end);
  }
  slice(start, end) { return this.subarray(start, end); }
  indexOf(v, from = 0) {
    if (typeof v === 'string') v = Buffer.from(v);
    if (typeof v === 'number') return super.indexOf(v, from);
    outer: for (let i = from; i <= this.length - v.length; i++) {
      for (let j = 0; j < v.length; j++) if (this[i + j] !== v[j]) continue outer;
      return i;
    }
    return -1;
  }
  includes(v, from) { return this.indexOf(v, from) !== -1; }
  copy(target, targetStart = 0, sourceStart = 0, sourceEnd = this.length) {
    let n = 0;
    for (let i = sourceStart; i < sourceEnd && targetStart + n < target.length; i++, n++) target[targetStart + n] = this[i];
    return n;
  }
  readUInt8(o = 0) { return this[o]; }
  readInt8(o = 0) { const v = this[o]; return v > 127 ? v - 256 : v; }
  readUInt16LE(o = 0) { return this[o] | (this[o + 1] << 8); }
  readUInt16BE(o = 0) { return (this[o] << 8) | this[o + 1]; }
  readUInt32LE(o = 0) { return (this[o] | (this[o + 1] << 8) | (this[o + 2] << 16)) + this[o + 3] * 0x1000000; }
  readUInt32BE(o = 0) { return this[o] * 0x1000000 + ((this[o + 1] << 16) | (this[o + 2] << 8) | this[o + 3]); }
  readInt32LE(o = 0) { return this[o] | (this[o + 1] << 8) | (this[o + 2] << 16) | (this[o + 3] << 24); }
  readInt32BE(o = 0) { return (this[o] << 24) | (this[o + 1] << 16) | (this[o + 2] << 8) | this[o + 3]; }
  writeUInt8(v, o = 0) { this[o] = v; return o + 1; }
  writeUInt16LE(v, o = 0) { this[o] = v; this[o + 1] = v >>> 8; return o + 2; }
  writeUInt16BE(v, o = 0) { this[o] = v >>> 8; this[o + 1] = v; return o + 2; }
  writeUInt32LE(v, o = 0) { this[o] = v; this[o + 1] = v >>> 8; this[o + 2] = v >>> 16; this[o + 3] = v >>> 24; return o + 4; }
  writeUInt32BE(v, o = 0) { this[o] = v >>> 24; this[o + 1] = v >>> 16; this[o + 2] = v >>> 8; this[o + 3] = v; return o + 4; }
  writeInt32LE(v, o = 0) { return this.writeUInt32LE(v >>> 0, o); }
  writeInt32BE(v, o = 0) { return this.writeUInt32BE(v >>> 0, o); }
}
Buffer.prototype.readUint8 = Buffer.prototype.readUInt8;
Buffer.prototype.readUint16LE = Buffer.prototype.readUInt16LE;
Buffer.prototype.readUint32LE = Buffer.prototype.readUInt32LE;
Buffer.poolSize = 8192;
binding.setBufferProto(Buffer.prototype);
Object.defineProperty(globalThis, 'Buffer', { value: Buffer, writable: true, configurable: true, enumerable: false });

function atob(s) {
  s = String(s).replace(/[\t\n\f\r ]/g, '');
  if (s.length % 4 === 1 || /[^A-Za-z0-9+/=]/.test(s)) {
    const e = new Error('The string to be decoded is not correctly encoded.');
    e.name = 'InvalidCharacterError';
    throw e;
  }
  return binding.bufferToString(binding.bufferFromString(s, 'base64'), 'latin1');
}
function btoa(s) {
  s = String(s);
  for (let i = 0; i < s.length; i++) {
    if (s.charCodeAt(i) > 255) {
      const e = new Error('Invalid character');
      e.name = 'InvalidCharacterError';
      throw e;
    }
  }
  return binding.bufferToString(binding.bufferFromString(s, 'latin1'), 'base64');
}

// ---- URL
const url = require('url');

// ---- DOMException
const domCodes = {
  IndexSizeError: 1, HierarchyRequestError: 3, WrongDocumentError: 4, InvalidCharacterError: 5, NoModificationAllowedError: 7,
  NotFoundError: 8, NotSupportedError: 9, InvalidStateError: 11, SyntaxError: 12, InvalidModificationError: 13, NamespaceError: 14,
  InvalidAccessError: 15, TypeMismatchError: 17, SecurityError: 18, NetworkError: 19, AbortError: 20, URLMismatchError: 21,
  QuotaExceededError: 22, TimeoutError: 23, InvalidNodeTypeError: 24, DataCloneError: 25,
};
class DOMException extends Error {
  #name;
  constructor(message = '', options = 'Error') {
    super(message);
    this.#name = options !== null && typeof options === 'object' ? String(options.name === undefined ? 'Error' : options.name) : String(options);
  }
  get name() { return this.#name; }
  get code() { return domCodes[this.#name] || 0; }
}
Object.defineProperty(DOMException.prototype, Symbol.toStringTag, { value: 'DOMException', configurable: true });

// ---- AbortController
class AbortSignal extends EventEmitter.EventTarget {
  constructor() {
    super();
    this.aborted = false;
    this.reason = undefined;
    this.onabort = null;
  }
  throwIfAborted() { if (this.aborted) throw this.reason; }
  static abort(reason) {
    const c = new AbortController();
    c.abort(reason);
    return c.signal;
  }
  static timeout(ms) {
    const c = new AbortController();
    setTimeout(() => {
      const e = new Error('The operation was aborted due to timeout');
      e.name = 'TimeoutError';
      e.code = 23;
      c.abort(e);
    }, ms).unref();
    return c.signal;
  }
}
class AbortController {
  constructor() { this.signal = new AbortSignal(); }
  abort(reason) {
    const s = this.signal;
    if (s.aborted) return;
    s.aborted = true;
    if (reason === undefined) {
      reason = new Error('This operation was aborted');
      reason.name = 'AbortError';
      reason.code = 20;
    }
    s.reason = reason;
    const ev = new EventEmitter.Event('abort');
    if (typeof s.onabort === 'function') s.onabort(ev);
    s.dispatchEvent(ev);
  }
}

// ---- Intl (en-US)
const Intl = {
  NumberFormat: class NumberFormat {
    constructor(locale, opts) { this._opts = opts || {}; }
    format(n) { return typeof n === 'bigint' ? n.toLocaleString() : Number(n).toLocaleString('en-US', this._opts); }
    formatToParts(n) { return [{ type: 'literal', value: this.format(n) }]; }
    resolvedOptions() { return { locale: 'en-US', numberingSystem: 'latn', style: this._opts.style || 'decimal', ...this._opts }; }
  },
  DateTimeFormat: class DateTimeFormat {
    constructor(locale, opts) { this._opts = opts || {}; }
    format(d) {
      d = d === undefined ? new Date() : new Date(d);
      const o = this._opts;
      const hasDate = o.year || o.month || o.day || o.weekday || o.dateStyle;
      const hasTime = o.hour || o.minute || o.second || o.timeStyle;
      if (!hasDate && !hasTime) return d.toLocaleDateString('en-US', o);
      return d.toLocaleString('en-US', o);
    }
    resolvedOptions() { return { locale: 'en-US', calendar: 'gregory', numberingSystem: 'latn', timeZone: 'UTC', ...this._opts }; }
  },
  Collator: class Collator {
    constructor(locale, opts) { this._opts = opts || {}; }
    compare(a, b) {
      if (this._opts.numeric) {
        const re = /(\d+)|(\D+)/g;
        const pa = String(a).match(re) || [];
        const pb = String(b).match(re) || [];
        for (let i = 0; i < Math.min(pa.length, pb.length); i++) {
          const x = pa[i];
          const y = pb[i];
          if (/^\d/.test(x) && /^\d/.test(y)) {
            if (Number(x) !== Number(y)) return Number(x) < Number(y) ? -1 : 1;
          } else {
            const c = x.localeCompare(y);
            if (c) return c;
          }
        }
        return pa.length === pb.length ? 0 : pa.length < pb.length ? -1 : 1;
      }
      if (this._opts.sensitivity === 'base' || this._opts.sensitivity === 'accent') {
        return String(a).toLowerCase().localeCompare(String(b).toLowerCase());
      }
      return String(a).localeCompare(String(b));
    }
    resolvedOptions() { return { locale: 'en-US', usage: 'sort', sensitivity: 'variant', ...this._opts }; }
  },
  PluralRules: class PluralRules {
    constructor(locale, opts) { this._opts = opts || {}; }
    select(n) {
      if (this._opts.type === 'ordinal') {
        const t = n % 10;
        const h = n % 100;
        if (t === 1 && h !== 11) return 'one';
        if (t === 2 && h !== 12) return 'two';
        if (t === 3 && h !== 13) return 'few';
        return 'other';
      }
      return n === 1 ? 'one' : 'other';
    }
  },
  RelativeTimeFormat: class RelativeTimeFormat {
    constructor(locale, opts) { this._opts = opts || {}; }
    format(v, unit) {
      unit = String(unit).replace(/s$/, '');
      const abs = Math.abs(v);
      const u = abs === 1 ? unit : unit + 's';
      if (this._opts.numeric === 'auto') {
        if (v === 0 && unit === 'day') return 'today';
        if (v === 1 && unit === 'day') return 'tomorrow';
        if (v === -1 && unit === 'day') return 'yesterday';
      }
      return v < 0 ? `${abs} ${u} ago` : `in ${abs} ${u}`;
    }
  },
  ListFormat: class ListFormat {
    constructor(locale, opts) { this._opts = opts || {}; }
    format(list) {
      const a = Array.from(list);
      const word = this._opts.type === 'disjunction' ? 'or' : 'and';
      if (a.length <= 1) return a.join('');
      if (a.length === 2) return `${a[0]} ${word} ${a[1]}`;
      return `${a.slice(0, -1).join(', ')}, ${word} ${a[a.length - 1]}`;
    }
  },
  getCanonicalLocales: (l) => (l === undefined ? [] : Array.isArray(l) ? l : [l]),
  supportedValuesOf: () => [],
};
Object.defineProperty(Intl, Symbol.toStringTag, { value: 'Intl', configurable: true });

const cryptoGlobal = {
  randomUUID: () => binding.randomUUID(),
  getRandomValues: (a) => binding.getRandomValues(a),
  subtle: {},
};

const defs = {
  URL: url.URL,
  URLSearchParams: url.URLSearchParams,
  AbortController,
  AbortSignal,
  DOMException,
  Event: EventEmitter.Event,
  EventTarget: EventEmitter.EventTarget,
  Intl,
  atob,
  btoa,
  crypto: cryptoGlobal,
  navigator: { hardwareConcurrency: 4, language: 'en-US', platform: 'linux', userAgent: 'Node.js/24' },
};
for (const k of Object.keys(defs)) {
  Object.defineProperty(globalThis, k, { value: defs[k], writable: true, configurable: true, enumerable: false });
}
