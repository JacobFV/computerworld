'use strict';
// Completes the Node environment on top of the native core: process as an
// EventEmitter, process.stdin, Buffer, URL, AbortController, Intl, ...

const EventEmitter = require('events');

// ---- process
Object.setPrototypeOf(process, EventEmitter.prototype);
EventEmitter.init.call(process);

const readStdin = process['%readStdin'];
delete process['%readStdin'];
const readLine = process['%readLine'];
delete process['%readLine'];
const isInteractive = process['%isInteractive'];
delete process['%isInteractive'];
// Whether input is a terminal is known once the run starts, not while this
// file is being set up.
const terminalInput = isInteractive;

const stdin = new EventEmitter();
let stdinEncoding = null;
let stdinScheduled = false;
let stdinDone = false;
stdin.fd = 0;
stdin.readable = true;
stdin.setEncoding = function setEncoding(enc) { stdinEncoding = enc || 'utf8'; return this; };
function deliverStdin() {
  if (stdinDone) return;
  if (terminalInput()) {
    // At a terminal input arrives a line at a time, and only an end-of-file
    // ends the stream; a line nobody has typed yet suspends the run.
    if (stdin.listenerCount('data') === 0 && stdin.listenerCount('readable') === 0) {
      stdinScheduled = false;
      return;
    }
    const line = readLine();
    if (line === null) {
      stdinDone = true;
      stdin.readable = false;
      stdin.emit('end');
      stdin.emit('close');
      return;
    }
    stdin.emit('data', stdinEncoding ? line : Buffer.from(line));
    setImmediate(deliverStdin);
    return;
  }
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
  if (terminalInput()) {
    const line = readLine();
    if (line === null) return null;
    return stdinEncoding ? line : Buffer.from(line);
  }
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
  if (terminalInput()) {
    for (;;) {
      const line = readLine();
      if (line === null) break;
      yield stdinEncoding ? line : Buffer.from(line);
    }
    stdinDone = true;
    return;
  }
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

// ---- DataView
const dvScratch = new ArrayBuffer(8);
const dvBytes = new Uint8Array(dvScratch);
const dvViews = {
  Int8: new Int8Array(dvScratch), Uint8: new Uint8Array(dvScratch),
  Int16: new Int16Array(dvScratch), Uint16: new Uint16Array(dvScratch),
  Int32: new Int32Array(dvScratch), Uint32: new Uint32Array(dvScratch),
  Float32: new Float32Array(dvScratch), Float64: new Float64Array(dvScratch),
  BigInt64: new BigInt64Array(dvScratch), BigUint64: new BigUint64Array(dvScratch),
};
const dvSizes = { Int8: 1, Uint8: 1, Int16: 2, Uint16: 2, Int32: 4, Uint32: 4, Float32: 4, Float64: 8, BigInt64: 8, BigUint64: 8 };
const dvToIndex = (v) => {
  const n = Math.trunc(Number(v)) || 0;
  if (n < 0 || n > Number.MAX_SAFE_INTEGER) throw new RangeError('Offset is outside the bounds of the DataView');
  return n;
};
class DataView {
  #bytes;
  #buffer;
  constructor(buffer, byteOffset, byteLength) {
    if (!(buffer instanceof ArrayBuffer)) throw new TypeError('First argument to DataView constructor must be an ArrayBuffer');
    const off = Math.trunc(Number(byteOffset)) || 0;
    if (off < 0 || off > buffer.byteLength) throw new RangeError(`Start offset ${byteOffset} is outside the bounds of the buffer`);
    const len = byteLength === undefined ? buffer.byteLength - off : Math.trunc(Number(byteLength)) || 0;
    if (len < 0 || off + len > buffer.byteLength) throw new RangeError(`Invalid DataView length ${byteLength}`);
    this.#buffer = buffer;
    this.#bytes = new Uint8Array(buffer, off, len);
  }
  get buffer() { return this.#buffer; }
  get byteLength() { return this.#bytes.length; }
  get byteOffset() { return this.#bytes.byteOffset; }
  get [Symbol.toStringTag]() { return 'DataView'; }
  static #get(dv, type, offset, little) {
    const n = dvSizes[type];
    const at = dvToIndex(offset);
    const bytes = dv.#bytes;
    if (at + n > bytes.length) throw new RangeError('Offset is outside the bounds of the DataView');
    for (let i = 0; i < n; i++) dvBytes[i] = bytes[at + (little ? i : n - 1 - i)];
    return dvViews[type][0];
  }
  static #set(dv, type, offset, value, little) {
    const n = dvSizes[type];
    const at = dvToIndex(offset);
    const view = dvViews[type];
    view[0] = type.startsWith('Big') ? BigInt(value) : Number(value);
    const bytes = dv.#bytes;
    if (at + n > bytes.length) throw new RangeError('Offset is outside the bounds of the DataView');
    for (let i = 0; i < n; i++) bytes[at + (little ? i : n - 1 - i)] = dvBytes[i];
  }
  getInt8(o) { return DataView.#get(this, 'Int8', o, true); }
  getUint8(o) { return DataView.#get(this, 'Uint8', o, true); }
  getInt16(o, le) { return DataView.#get(this, 'Int16', o, !!le); }
  getUint16(o, le) { return DataView.#get(this, 'Uint16', o, !!le); }
  getInt32(o, le) { return DataView.#get(this, 'Int32', o, !!le); }
  getUint32(o, le) { return DataView.#get(this, 'Uint32', o, !!le); }
  getFloat32(o, le) { return DataView.#get(this, 'Float32', o, !!le); }
  getFloat64(o, le) { return DataView.#get(this, 'Float64', o, !!le); }
  getBigInt64(o, le) { return DataView.#get(this, 'BigInt64', o, !!le); }
  getBigUint64(o, le) { return DataView.#get(this, 'BigUint64', o, !!le); }
  setInt8(o, v) { DataView.#set(this, 'Int8', o, v, true); }
  setUint8(o, v) { DataView.#set(this, 'Uint8', o, v, true); }
  setInt16(o, v, le) { DataView.#set(this, 'Int16', o, v, !!le); }
  setUint16(o, v, le) { DataView.#set(this, 'Uint16', o, v, !!le); }
  setInt32(o, v, le) { DataView.#set(this, 'Int32', o, v, !!le); }
  setUint32(o, v, le) { DataView.#set(this, 'Uint32', o, v, !!le); }
  setFloat32(o, v, le) { DataView.#set(this, 'Float32', o, v, !!le); }
  setFloat64(o, v, le) { DataView.#set(this, 'Float64', o, v, !!le); }
  setBigInt64(o, v, le) { DataView.#set(this, 'BigInt64', o, v, !!le); }
  setBigUint64(o, v, le) { DataView.#set(this, 'BigUint64', o, v, !!le); }
}
Object.defineProperty(globalThis, 'DataView', { value: DataView, writable: true, configurable: true, enumerable: false });
{
  const typedIsView = ArrayBuffer.isView;
  Object.defineProperty(ArrayBuffer, 'isView', {
    value: { isView(x) { return typedIsView(x) || x instanceof DataView; } }.isView,
    writable: true,
    configurable: true,
    enumerable: false,
  });
}


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
}
Buffer.poolSize = 8192;
// Node's numeric accessors on Buffer.
const bufSep = (s) => {
  let out = '';
  let i = s.length;
  const start = s[0] === '-' ? 1 : 0;
  for (; i >= start + 4; i -= 3) out = `_${s.slice(i - 3, i)}${out}`;
  return `${s.slice(0, i)}${out}`;
};
function bufRangeError(name, range, received) {
  let r;
  if (Number.isInteger(received) && Math.abs(received) > 2 ** 32) r = bufSep(String(received));
  else if (typeof received === 'bigint') {
    r = String(received);
    if (received > 2n ** 32n || received < -(2n ** 32n)) r = bufSep(r);
    r += 'n';
  } else r = require('util').inspect(received);
  const e = new RangeError(`The value of "${name}" is out of range. It must be ${range}. Received ${r}`);
  e.code = 'ERR_OUT_OF_RANGE';
  Object.defineProperty(e, 'name', { value: 'RangeError [ERR_OUT_OF_RANGE]', enumerable: false, writable: true, configurable: true });
  e.stack; // eslint-disable-line no-unused-expressions
  delete e.name;
  return e;
}
function bufCheckOffset(buf, offset, n) {
  if (offset === undefined) offset = 0;
  if (typeof offset !== 'number') {
    const e = new TypeError(`The "offset" argument must be of type number. Received ${offset === null ? 'null' : `type ${typeof offset} (${require('util').inspect(offset)})`}`);
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  const max = buf.length - n;
  if (!Number.isInteger(offset)) throw bufRangeError('offset', 'an integer', offset);
  if (max < 0) {
    const e = new RangeError('Attempt to access memory outside buffer bounds');
    e.code = 'ERR_BUFFER_OUT_OF_BOUNDS';
    throw e;
  }
  if (offset < 0 || offset > max) throw bufRangeError('offset', `>= 0 and <= ${max}`, offset);
  return offset;
}
function bufView(buf) { return new DataView(buf.buffer, buf.byteOffset, buf.length); }
const bufTypes = [
  // [Node name, DataView type, bytes, min, max]
  ['Int8', 'Int8', 1, -128, 127], ['UInt8', 'Uint8', 1, 0, 255],
  ['Int16', 'Int16', 2, -32768, 32767], ['UInt16', 'Uint16', 2, 0, 65535],
  ['Int32', 'Int32', 4, -2147483648, 2147483647], ['UInt32', 'Uint32', 4, 0, 4294967295],
  ['Float', 'Float32', 4], ['Double', 'Float64', 8],
  ['BigInt64', 'BigInt64', 8, -(2n ** 63n), 2n ** 63n - 1n], ['BigUInt64', 'BigUint64', 8, 0n, 2n ** 64n - 1n],
];
function bufRange(min, max, n) {
  const big = typeof min === 'bigint' ? 'n' : '';
  if (n > 4) return min === 0n ? `>= 0n and < 2n ** ${n * 8}n` : `>= -(2n ** ${n * 8 - 1}n) and < 2n ** ${n * 8 - 1}n`;
  return `>= ${min}${big} and <= ${max}${big}`;
}
function bufDefine(name, fn) {
  Object.defineProperty(fn, 'name', { value: name });
  Object.defineProperty(Buffer.prototype, name, { value: fn, writable: true, configurable: true, enumerable: true });
  if (name.includes('UInt')) Object.defineProperty(Buffer.prototype, name.replace('UInt', 'Uint'), { value: fn, writable: true, configurable: true, enumerable: true });
}
for (const [nodeName, dvType, n, min, max] of bufTypes) {
  const ends = n === 1 ? [''] : ['LE', 'BE'];
  for (const end of ends) {
    const little = end !== 'BE';
    bufDefine(`read${nodeName}${end}`, function (offset = 0) {
      offset = bufCheckOffset(this, offset, n);
      return bufView(this)[`get${dvType}`](offset, little);
    });
    bufDefine(`write${nodeName}${end}`, function (value, offset = 0) {
      const big = typeof min === 'bigint';
      if (!big) value = +value;
      if (min !== undefined && (value < min || value > max)) throw bufRangeError('value', bufRange(min, max, n), value);
      offset = bufCheckOffset(this, offset, n);
      if (big && typeof value !== 'bigint') throw new TypeError('Cannot mix BigInt and other types, use explicit conversions');
      bufView(this)[`set${dvType}`](offset, value, little);
      return offset + n;
    });
  }
}
for (const end of ['LE', 'BE']) {
  const little = end === 'LE';
  const readU = (buf, offset, n) => {
    let v = 0;
    for (let i = 0; i < n; i++) v += buf[offset + (little ? i : n - 1 - i)] * 2 ** (8 * i);
    return v;
  };
  const checkLen = (n) => {
    if (!Number.isInteger(n) || n < 1 || n > 6) throw bufRangeError('byteLength', '>= 1 and <= 6', n);
  };
  bufDefine(`readUInt${end}`, function (offset, byteLength) {
    checkLen(byteLength);
    offset = bufCheckOffset(this, offset, byteLength);
    return readU(this, offset, byteLength);
  });
  bufDefine(`readInt${end}`, function (offset, byteLength) {
    checkLen(byteLength);
    offset = bufCheckOffset(this, offset, byteLength);
    const v = readU(this, offset, byteLength);
    const lim = 2 ** (8 * byteLength - 1);
    return v >= lim ? v - 2 * lim : v;
  });
  const write = (buf, value, offset, n, min, max) => {
    checkLen(n);
    value = +value;
    if (value < min || value > max) {
      const range = n > 4
        ? (min === 0 ? `>= 0 and < 2 ** ${n * 8}` : `>= -(2 ** ${n * 8 - 1}) and < 2 ** ${n * 8 - 1}`)
        : `>= ${min} and <= ${max}`;
      throw bufRangeError('value', range, value);
    }
    offset = bufCheckOffset(buf, offset, n);
    let v = value < 0 ? value + 2 ** (8 * n) : value;
    for (let i = 0; i < n; i++) {
      buf[offset + (little ? i : n - 1 - i)] = v % 256;
      v = Math.floor(v / 256);
    }
    return offset + n;
  };
  bufDefine(`writeUInt${end}`, function (value, offset, byteLength) {
    return write(this, value, offset, byteLength, 0, 2 ** (8 * byteLength) - 1);
  });
  bufDefine(`writeInt${end}`, function (value, offset, byteLength) {
    return write(this, value, offset, byteLength, -(2 ** (8 * byteLength - 1)), 2 ** (8 * byteLength - 1) - 1);
  });
}
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
  atob,
  btoa,
  crypto: cryptoGlobal,
  navigator: { hardwareConcurrency: 4, language: 'en-US', platform: 'linux', userAgent: 'Node.js/24' },
};
for (const k of Object.keys(defs)) {
  Object.defineProperty(globalThis, k, { value: defs[k], writable: true, configurable: true, enumerable: false });
}

// Intl loads on first use: it carries the locale data with it.
{
  const settle = (v) => {
    Object.defineProperty(globalThis, 'Intl', { value: v, writable: true, configurable: true, enumerable: false });
    return v;
  };
  Object.defineProperty(globalThis, 'Intl', {
    configurable: true,
    enumerable: false,
    get() { return settle(require('internal/intl').Intl); },
    set(v) { settle(v); },
  });
}

// The locale-aware methods of the built-in prototypes go through Intl, which
// loads with the first of them that is called.
{
  const intl = () => require('internal/intl').Intl;
  Number.prototype.toLocaleString = function toLocaleString(locales, options) {
    return new (intl().NumberFormat)(locales, options).format(this);
  };
  BigInt.prototype.toLocaleString = function toLocaleString(locales, options) {
    return new (intl().NumberFormat)(locales, options).format(this);
  };
  String.prototype.localeCompare = function localeCompare(that, locales, options) {
    return new (intl().Collator)(locales, options).compare(String(this), String(that));
  };
  const dateDefaults = (options, date, time) => {
    const o = options === undefined ? {} : Object(options);
    const has = ['weekday', 'year', 'month', 'day', 'dateStyle'].some((k) => o[k] !== undefined);
    const hasTime = ['hour', 'minute', 'second', 'timeStyle', 'dayPeriod',
      'fractionalSecondDigits'].some((k) => o[k] !== undefined);
    const out = { ...o };
    if (date && !has && !hasTime) {
      Object.assign(out, { year: 'numeric', month: 'numeric', day: 'numeric' });
    }
    if (time && !hasTime && !has) {
      Object.assign(out, { hour: 'numeric', minute: 'numeric', second: 'numeric' });
    }
    return out;
  };
  Date.prototype.toLocaleString = function toLocaleString(locales, options) {
    if (Number.isNaN(this.getTime())) return 'Invalid Date';
    return new (intl().DateTimeFormat)(locales, dateDefaults(options, true, true)).format(this);
  };
  Date.prototype.toLocaleDateString = function toLocaleDateString(locales, options) {
    if (Number.isNaN(this.getTime())) return 'Invalid Date';
    return new (intl().DateTimeFormat)(locales, dateDefaults(options, true, false)).format(this);
  };
  Date.prototype.toLocaleTimeString = function toLocaleTimeString(locales, options) {
    if (Number.isNaN(this.getTime())) return 'Invalid Date';
    return new (intl().DateTimeFormat)(locales, dateDefaults(options, false, true)).format(this);
  };
}

// fetch and its classes load on first use (they carry the http stack with them).
for (const name of ['fetch', 'Headers', 'Request', 'Response', 'FormData', 'Blob', 'File', 'ReadableStream']) {
  const settle = (v) => {
    Object.defineProperty(globalThis, name, { value: v, writable: true, configurable: true, enumerable: false });
    return v;
  };
  Object.defineProperty(globalThis, name, {
    configurable: true,
    enumerable: false,
    get() { return settle(require('internal/fetch')[name]); },
    set(v) { settle(v); },
  });
}
