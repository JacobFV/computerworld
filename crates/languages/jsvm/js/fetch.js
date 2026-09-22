'use strict';
// fetch, Headers, Request, Response (WHATWG, as Node 24's undici exposes them),
// carried over the http/https modules and so over the world's network.

const kInspect = Symbol.for('nodejs.util.inspect.custom');

function toByteString(v) { return String(v); }
function normName(name) {
  name = String(name);
  if (!/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name)) throw new TypeError(`Headers.append: "${name}" is an invalid header name.`);
  return name.toLowerCase();
}
function normValue(v) { return String(v).replace(/^[\t\n\r ]+|[\t\n\r ]+$/g, ''); }

class Headers {
  #list = new Map();
  constructor(init) {
    if (init === undefined || init === null) return;
    if (init instanceof Headers) {
      for (const [k, v] of init.#list) this.#list.set(k, v.slice());
    } else if (typeof init[Symbol.iterator] === 'function') {
      for (const pair of init) {
        const p = Array.from(pair);
        if (p.length !== 2) throw new TypeError('Headers constructor: expected name/value pair to be length 2, found ' + p.length + '.');
        this.append(p[0], p[1]);
      }
    } else if (typeof init === 'object') {
      for (const k of Object.keys(init)) this.append(k, init[k]);
    } else {
      throw new TypeError("Headers constructor: The provided value is not of type '(record<ByteString, ByteString> or sequence<sequence<ByteString>>)'.");
    }
  }
  append(name, value) {
    const k = normName(name);
    const list = this.#list.get(k);
    if (list) list.push(normValue(value)); else this.#list.set(k, [normValue(value)]);
  }
  delete(name) { this.#list.delete(normName(name)); }
  get(name) {
    const l = this.#list.get(normName(name));
    return l ? l.join(', ') : null;
  }
  getSetCookie() { return (this.#list.get('set-cookie') || []).slice(); }
  has(name) { return this.#list.has(normName(name)); }
  set(name, value) { this.#list.set(normName(name), [normValue(value)]); }
  forEach(cb, thisArg) { for (const [k, v] of this) cb.call(thisArg, v, k, this); }
  *entries() {
    const keys = [...this.#list.keys()].sort();
    for (const k of keys) {
      if (k === 'set-cookie') for (const v of this.#list.get(k)) yield [k, v];
      else yield [k, this.#list.get(k).join(', ')];
    }
  }
  *keys() { for (const [k] of this.entries()) yield k; }
  *values() { for (const [, v] of this.entries()) yield v; }
  [Symbol.iterator]() { return this.entries(); }
  get [Symbol.toStringTag]() { return 'Headers'; }
  [kInspect](depth, options, inspect) {
    const o = {};
    for (const [k, v] of this) o[k] = v;
    return `Headers ${inspect(o, options)}`;
  }
}

class Blob {
  #bytes;
  #type;
  constructor(parts = [], options = {}) {
    const bufs = [];
    for (const p of parts) {
      if (p instanceof Blob) bufs.push(p._buffer());
      else if (typeof p === 'string') bufs.push(Buffer.from(p, 'utf8'));
      else if (p instanceof ArrayBuffer) bufs.push(Buffer.from(new Uint8Array(p)));
      else if (ArrayBuffer.isView(p)) bufs.push(Buffer.from(p.buffer, p.byteOffset, p.byteLength));
      else bufs.push(Buffer.from(String(p)));
    }
    this.#bytes = Buffer.concat(bufs);
    this.#type = options.type ? String(options.type).toLowerCase() : '';
  }
  _buffer() { return this.#bytes; }
  get size() { return this.#bytes.length; }
  get type() { return this.#type; }
  // Blob reads go through a stream of the parts: the deepest of the body reads.
  text() { return after(8, Promise.resolve(this.#bytes.toString('utf8'))); }
  arrayBuffer() { const u = new Uint8Array(this.#bytes); return after(8, Promise.resolve(u.buffer)); }
  bytes() { return after(8, Promise.resolve(new Uint8Array(this.#bytes))); }
  slice(start, end, type) { return new Blob([this.#bytes.subarray(start, end)], { type }); }
  stream() { return new ReadableStream({ bytes: this.#bytes }); }
  get [Symbol.toStringTag]() { return 'Blob'; }
  [kInspect]() { return `Blob { size: ${this.size}, type: '${this.type}' }`; }
}

class File extends Blob {
  constructor(parts, name, options = {}) {
    super(parts, options);
    this.name = String(name);
    this.lastModified = options.lastModified || Date.now();
  }
}

class FormData {
  #entries = [];
  append(name, value, filename) { this.#entries.push([String(name), value instanceof Blob ? value : String(value), filename]); }
  delete(name) { this.#entries = this.#entries.filter((e) => e[0] !== String(name)); }
  get(name) { const e = this.#entries.find((x) => x[0] === String(name)); return e ? e[1] : null; }
  getAll(name) { return this.#entries.filter((x) => x[0] === String(name)).map((x) => x[1]); }
  has(name) { return this.#entries.some((x) => x[0] === String(name)); }
  set(name, value, filename) { this.delete(name); this.append(name, value, filename); }
  *entries() { for (const [k, v] of this.#entries) yield [k, v]; }
  *keys() { for (const [k] of this.#entries) yield k; }
  *values() { for (const [, v] of this.#entries) yield v; }
  [Symbol.iterator]() { return this.entries(); }
  forEach(cb, thisArg) { for (const [k, v] of this) cb.call(thisArg, v, k, this); }
  get [Symbol.toStringTag]() { return 'FormData'; }
}

// A minimal byte ReadableStream: one chunk, then done.
class ReadableStream {
  #chunks;
  #locked = false;
  constructor(source = {}) {
    this.#chunks = source.bytes && source.bytes.length ? [new Uint8Array(source.bytes)] : [];
  }
  get locked() { return this.#locked; }
  getReader() {
    if (this.#locked) throw new TypeError('ReadableStream is locked');
    this.#locked = true;
    const chunks = this.#chunks;
    return {
      read: () => Promise.resolve(chunks.length ? { value: chunks.shift(), done: false } : { value: undefined, done: true }),
      releaseLock: () => { this.#locked = false; },
      cancel: () => { chunks.length = 0; return Promise.resolve(); },
      closed: Promise.resolve(),
    };
  }
  cancel() { this.#chunks.length = 0; return Promise.resolve(); }
  async *[Symbol.asyncIterator]() {
    const r = this.getReader();
    for (;;) {
      const { value, done } = await r.read();
      if (done) return;
      yield value;
    }
  }
  get [Symbol.toStringTag]() { return 'ReadableStream'; }
  [kInspect]() { return `ReadableStream { locked: ${this.#locked}, state: 'readable', supportsBYOB: true }`; }
}

function extractBody(body, headers) {
  if (body === undefined || body === null) return null;
  let bytes;
  let type = null;
  if (typeof body === 'string') { bytes = Buffer.from(body, 'utf8'); type = 'text/plain;charset=UTF-8'; }
  else if (body instanceof URLSearchParams) { bytes = Buffer.from(body.toString(), 'utf8'); type = 'application/x-www-form-urlencoded;charset=UTF-8'; }
  else if (body instanceof Blob) { bytes = body._buffer(); type = body.type || null; }
  else if (body instanceof ArrayBuffer) bytes = Buffer.from(new Uint8Array(body));
  else if (ArrayBuffer.isView(body)) bytes = Buffer.from(body.buffer, body.byteOffset, body.byteLength);
  else if (body instanceof FormData) {
    const boundary = '----formdata-undici-0' + String(Math.floor(Math.random() * 1e11)).padStart(11, '0');
    const parts = [];
    for (const [k, v] of body) {
      if (v instanceof Blob) {
        parts.push(Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="${k}"; filename="${v.name || 'blob'}"\r\nContent-Type: ${v.type || 'application/octet-stream'}\r\n\r\n`), v._buffer(), Buffer.from('\r\n'));
      } else {
        parts.push(Buffer.from(`--${boundary}\r\nContent-Disposition: form-data; name="${k}"\r\n\r\n${v}\r\n`));
      }
    }
    parts.push(Buffer.from(`--${boundary}--\r\n`));
    bytes = Buffer.concat(parts);
    type = `multipart/form-data; boundary=${boundary}`;
  } else bytes = Buffer.from(String(body), 'utf8');
  if (type && headers && !headers.has('content-type')) headers.set('content-type', type);
  return bytes;
}

// Settles after `n` promise turns. undici reads a body through a stream reader in
// an async loop, so a read settles a characteristic number of microtask turns
// later (more for a tee'd branch after clone()), and programs observe that
// ordering between bodies; the depths here are undici's.
function after(n, p) {
  for (let i = 1; i < n; i++) p = p.then((x) => x);
  return p;
}
const PLAIN_READ = 3;
const TEED_READ = 5;

class Body {
  _initBody(bytes) {
    this._bytes = bytes;
    this._used = false;
    this._teed = false;
    this._stream = bytes === null ? null : new ReadableStream({ bytes });
  }
  get body() { return this._stream; }
  get bodyUsed() { return this._used; }
  _consume() {
    if (this._used || (this._stream && this._stream.locked)) {
      return after(PLAIN_READ, Promise.reject(new TypeError('Body is unusable: Body has already been read')));
    }
    this._used = true;
    return after(this._teed ? TEED_READ : PLAIN_READ, Promise.resolve(this._bytes || Buffer.alloc(0)));
  }
  text() { return this._consume().then((b) => b.toString('utf8')); }
  json() { return this.text().then((t) => JSON.parse(t)); }
  arrayBuffer() { return this._consume().then((b) => new Uint8Array(b).buffer); }
  bytes() { return this._consume().then((b) => new Uint8Array(b)); }
  blob() { return this._consume().then((b) => new Blob([b], { type: this.headers.get('content-type') || '' })); }
  formData() {
    return this.text().then((t) => {
      const fd = new FormData();
      for (const [k, v] of new URLSearchParams(t)) fd.append(k, v);
      return fd;
    });
  }
}

class Request extends Body {
  constructor(input, init = {}) {
    super();
    let base = null;
    if (input instanceof Request) base = input;
    let url;
    if (base) url = base.url;
    else {
      try { url = new URL(String(input)).href; } catch (e) {
        const err = new TypeError(`Failed to parse URL from ${input}`);
        err.cause = e;
        throw err;
      }
    }
    this.url = url;
    this.method = String(init.method || (base ? base.method : 'GET')).toUpperCase();
    this.headers = new Headers(init.headers || (base ? base.headers : undefined));
    this.redirect = init.redirect || (base ? base.redirect : 'follow');
    this.signal = init.signal || (base ? base.signal : new AbortController().signal);
    this.credentials = init.credentials || 'same-origin';
    this.mode = init.mode || 'cors';
    this.cache = init.cache || 'default';
    this.referrer = 'about:client';
    this.referrerPolicy = '';
    this.integrity = '';
    this.keepalive = !!init.keepalive;
    this.destination = '';
    this.duplex = 'half';
    const body = init.body !== undefined ? init.body : (base ? base._bytes : null);
    if (body !== null && body !== undefined && (this.method === 'GET' || this.method === 'HEAD')) {
      throw new TypeError('Request with GET/HEAD method cannot have body.');
    }
    this._initBody(extractBody(body, this.headers));
  }
  clone() { return new Request(this); }
  get [Symbol.toStringTag]() { return 'Request'; }
}

const STATUS = () => require('http').STATUS_CODES;

class Response extends Body {
  constructor(body = null, init = {}) {
    super();
    const status = init.status === undefined ? 200 : init.status;
    if (status < 200 || status > 599) {
      throw new RangeError(`init["status"] must be in the range of 200 to 599, inclusive.`);
    }
    this.status = status;
    this.statusText = init.statusText === undefined ? '' : String(init.statusText);
    this.headers = new Headers(init.headers);
    this.type = init._type || 'default';
    this.url = init._url || '';
    this.redirected = !!init._redirected;
    this._initBody(extractBody(body, this.headers));
  }
  get ok() { return this.status >= 200 && this.status <= 299; }
  clone() {
    if (this.bodyUsed) throw new TypeError('Response.clone: Body has already been consumed.');
    const r = new Response(this._bytes, { status: this.status, statusText: this.statusText, headers: this.headers, _type: this.type, _url: this.url, _redirected: this.redirected });
    // Both branches now read through a tee.
    this._teed = true;
    r._teed = true;
    return r;
  }
  static json(data, init = {}) {
    const headers = new Headers(init.headers);
    if (!headers.has('content-type')) headers.set('content-type', 'application/json');
    return new Response(JSON.stringify(data), { ...init, headers });
  }
  static error() { const r = new Response(null, { status: 200 }); r.status = 0; r.type = 'error'; return r; }
  static redirect(url, status = 302) {
    const r = new Response(null, { status, headers: { location: new URL(url).href } });
    return r;
  }
  get [Symbol.toStringTag]() { return 'Response'; }
  [kInspect](depth, options, inspect) {
    const o = {
      status: this.status, statusText: this.statusText, headers: this.headers, body: this.body,
      bodyUsed: this.bodyUsed, ok: this.ok, redirected: this.redirected, type: this.type, url: this.url,
    };
    return `Response ${inspect(o, options)}`;
  }
}

function abortError(signal) {
  if (signal && signal.reason !== undefined) return signal.reason;
  return new DOMException('This operation was aborted', 'AbortError');
}

function fetch(input, init = {}) {
  let request;
  try {
    request = new Request(input, init);
  } catch (e) {
    return Promise.reject(e);
  }
  return new Promise((resolve, reject) => {
    if (request.signal && request.signal.aborted) return reject(abortError(request.signal));
    let redirects = 0;
    const attempt = (url, method, bodyBytes) => {
      const u = new URL(url);
      if (u.protocol !== 'http:' && u.protocol !== 'https:') {
        if (u.protocol === 'data:') {
          const [meta, data] = url.slice(5).split(',');
          const bytes = meta.endsWith(';base64') ? Buffer.from(data, 'base64') : Buffer.from(decodeURIComponent(data));
          return resolve(new Response(bytes, { status: 200, statusText: 'OK', headers: { 'content-type': meta.replace(/;base64$/, '') || 'text/plain;charset=US-ASCII' }, _type: 'basic', _url: url }));
        }
        const err = new TypeError('fetch failed');
        err.cause = new Error('unknown scheme');
        return reject(err);
      }
      const mod = require(u.protocol === 'https:' ? 'https' : 'http');
      const headers = {};
      for (const [k, v] of request.headers) headers[k] = v;
      const defaults = { accept: '*/*', 'accept-language': '*', 'sec-fetch-mode': 'cors', 'user-agent': 'node', 'accept-encoding': 'gzip, deflate' };
      for (const k of Object.keys(defaults)) if (headers[k] === undefined) headers[k] = defaults[k];
      if (bodyBytes && headers['content-length'] === undefined) headers['content-length'] = String(bodyBytes.length);
      const req = mod.request(u, { method, headers });
      const onAbort = () => { req.destroy(); reject(abortError(request.signal)); };
      if (request.signal) request.signal.addEventListener('abort', onAbort, { once: true });
      req.on('error', (e) => {
        if (request.signal && request.signal.aborted) return;
        const err = new TypeError('fetch failed');
        err.cause = e;
        reject(err);
      });
      req.on('response', (res) => {
        const chunks = [];
        res.on('data', (c) => chunks.push(c));
        res.on('end', () => {
          if (request.signal) request.signal.removeEventListener('abort', onAbort);
          const status = res.statusCode;
          const location = res.headers.location;
          if ([301, 302, 303, 307, 308].includes(status) && location && request.redirect !== 'manual') {
            if (request.redirect === 'error') {
              const err = new TypeError('fetch failed');
              err.cause = new Error('unexpected redirect');
              return reject(err);
            }
            if (++redirects > 20) {
              const err = new TypeError('fetch failed');
              err.cause = new Error('redirect count exceeded');
              return reject(err);
            }
            const next = new URL(location, url).href;
            const keep = status === 307 || status === 308;
            const nextMethod = keep ? method : (method === 'HEAD' ? 'HEAD' : (status === 303 || method === 'POST' ? 'GET' : method));
            return attempt(next, nextMethod, keep ? bodyBytes : null);
          }
          const h = new Headers();
          for (let i = 0; i + 1 < res.rawHeaders.length; i += 2) h.append(res.rawHeaders[i], res.rawHeaders[i + 1]);
          const noBody = method === 'HEAD' || status === 204 || status === 304;
          const r = new Response(noBody ? null : Buffer.concat(chunks), { status: status < 200 ? 200 : status, statusText: res.statusMessage, headers: h, _type: 'basic', _url: url, _redirected: redirects > 0 });
          resolve(r);
        });
      });
      req.end(bodyBytes || undefined);
    };
    attempt(request.url, request.method, request._bytes);
  });
}

module.exports = { fetch, Headers, Request, Response, FormData, Blob, File, ReadableStream };
