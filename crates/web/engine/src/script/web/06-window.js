// Prelude part 6: window, navigator, location, history, storage, timers, fetch and
// XHR, observers, the browser dispatch hooks, and the globals.
(function () {
'use strict';
const C = globalThis['%core'];
const { W, hooks, define, defineGlobal, isNode, domError, typeError, EventTarget, Event, fire, fireSimple, defineHandlerAttr, globalEventTypes, windowEventTypes, queueTask } = C;

// ---------------------------------------------------------------- AbortController (on our EventTarget)

class AbortSignal extends EventTarget {
  constructor() { super(); this._aborted = false; this._reason = undefined; this.onabort = null; }
  get aborted() { return this._aborted; }
  get reason() { return this._reason; }
  throwIfAborted() { if (this._aborted) throw this._reason; }
  static abort(reason) { const c = new AbortController(); c.abort(reason); return c.signal; }
  static timeout(ms) { const c = new AbortController(); setTimeout(() => c.abort(domError('TimeoutError', 'signal timed out')), ms); return c.signal; }
  static any(signals) { const c = new AbortController(); for (const s of signals) { if (s.aborted) { c.abort(s.reason); break; } s.addEventListener('abort', () => c.abort(s.reason)); } return c.signal; }
}
defineHandlerAttr(AbortSignal.prototype, 'abort');
class AbortController {
  constructor() { this.signal = new AbortSignal(); }
  abort(reason) {
    const s = this.signal;
    if (s._aborted) return;
    s._aborted = true;
    s._reason = reason === undefined ? domError('AbortError', 'signal is aborted without reason') : reason;
    fire(s, new Event('abort'));
  }
}

// ---------------------------------------------------------------- Blob, File, FormData, Headers, Request, Response

function bytesOf(part) {
  if (part instanceof Blob) return part._bytes;
  if (part instanceof ArrayBuffer) return new Uint8Array(part);
  if (ArrayBuffer.isView(part)) return new Uint8Array(part.buffer, part.byteOffset, part.byteLength);
  return W.utf8Encode(String(part));
}
function concatBytes(parts) {
  let n = 0;
  for (const p of parts) n += p.length;
  const out = new Uint8Array(n);
  let o = 0;
  for (const p of parts) { out.set(p, o); o += p.length; }
  return out;
}
class Blob {
  constructor(parts, options) {
    this._bytes = concatBytes((parts || []).map(bytesOf));
    this.type = options && options.type ? String(options.type).toLowerCase() : '';
  }
  get size() { return this._bytes.length; }
  slice(start, end, type) { const b = new Blob([], { type }); b._bytes = this._bytes.slice(start, end); return b; }
  text() { return Promise.resolve(W.utf8Decode(this._bytes)); }
  arrayBuffer() { return Promise.resolve(this._bytes.buffer.slice(this._bytes.byteOffset, this._bytes.byteOffset + this._bytes.byteLength)); }
  bytes() { return Promise.resolve(this._bytes.slice()); }
  stream() { return null; }
  get [Symbol.toStringTag]() { return 'Blob'; }
}
class File extends Blob {
  constructor(parts, name, options) { super(parts, options); this.name = String(name); this.lastModified = options && options.lastModified !== undefined ? options.lastModified : Date.now(); this.webkitRelativePath = ''; }
  get [Symbol.toStringTag]() { return 'File'; }
}
class FormData {
  constructor(form, submitter) {
    this._entries = [];
    if (form) {
      if (!isNode(form) || form.localName !== 'form') throw typeError("Failed to construct 'FormData': parameter 1 is not of type 'HTMLFormElement'.");
      for (const [k, v] of W.formDataSet(form, submitter || null)) this._entries.push([k, v]);
      fire(form, new C.FormDataEvent('formdata', { formData: this, bubbles: true }));
    }
  }
  _val(v, filename) { if (v instanceof Blob) { return v instanceof File && filename === undefined ? v : new File([v], filename === undefined ? 'blob' : filename, { type: v.type }); } return String(v); }
  append(k, v, filename) { this._entries.push([String(k), this._val(v, filename)]); }
  set(k, v, filename) { k = String(k); const i = this._entries.findIndex((e) => e[0] === k); if (i < 0) this._entries.push([k, this._val(v, filename)]); else { this._entries[i] = [k, this._val(v, filename)]; this._entries = this._entries.filter((e, j) => j <= i || e[0] !== k); } }
  get(k) { const e = this._entries.find((x) => x[0] === String(k)); return e ? e[1] : null; }
  getAll(k) { return this._entries.filter((x) => x[0] === String(k)).map((x) => x[1]); }
  has(k) { return this._entries.some((x) => x[0] === String(k)); }
  delete(k) { this._entries = this._entries.filter((x) => x[0] !== String(k)); }
  entries() { return this._entries.map((e) => [e[0], e[1]]).values(); }
  keys() { return this._entries.map((e) => e[0]).values(); }
  values() { return this._entries.map((e) => e[1]).values(); }
  forEach(cb, thisArg) { for (const [k, v] of this._entries) cb.call(thisArg, v, k, this); }
  [Symbol.iterator]() { return this.entries(); }
  get [Symbol.toStringTag]() { return 'FormData'; }
}
function urlencode(pairs) { return pairs.map(([k, v]) => encodeURIComponent(k).replace(/%20/g, '+') + '=' + encodeURIComponent(v instanceof Blob ? (v.name || 'blob') : v).replace(/%20/g, '+')).join('&'); }

class Headers {
  constructor(init) {
    this._list = [];
    if (init instanceof Headers) for (const [k, v] of init._list) this._list.push([k, v]);
    else if (Array.isArray(init) || (init && typeof init[Symbol.iterator] === 'function')) for (const pair of init) { if (pair.length !== 2) throw typeError('Headers constructor: expected name/value pair to be length 2, found ' + pair.length + '.'); this.append(pair[0], pair[1]); }
    else if (init && typeof init === 'object') for (const k of Object.keys(init)) this.append(k, init[k]);
  }
  _check(name) { name = String(name); if (!/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(name)) throw typeError("Failed to execute 'append' on 'Headers': Invalid name"); return name.toLowerCase(); }
  append(k, v) { this._list.push([this._check(k), String(v).trim()]); }
  set(k, v) { k = this._check(k); const i = this._list.findIndex((e) => e[0] === k); if (i < 0) this._list.push([k, String(v).trim()]); else { this._list[i][1] = String(v).trim(); this._list = this._list.filter((e, j) => j <= i || e[0] !== k); } }
  get(k) { k = String(k).toLowerCase(); const vals = this._list.filter((e) => e[0] === k).map((e) => e[1]); return vals.length ? vals.join(', ') : null; }
  has(k) { k = String(k).toLowerCase(); return this._list.some((e) => e[0] === k); }
  delete(k) { k = String(k).toLowerCase(); this._list = this._list.filter((e) => e[0] !== k); }
  getSetCookie() { return this._list.filter((e) => e[0] === 'set-cookie').map((e) => e[1]); }
  _sorted() { const names = Array.from(new Set(this._list.map((e) => e[0]))).sort(); return names.map((n) => [n, this.get(n)]); }
  forEach(cb, thisArg) { for (const [k, v] of this._sorted()) cb.call(thisArg, v, k, this); }
  entries() { return this._sorted().values(); }
  keys() { return this._sorted().map((e) => e[0]).values(); }
  values() { return this._sorted().map((e) => e[1]).values(); }
  [Symbol.iterator]() { return this.entries(); }
  get [Symbol.toStringTag]() { return 'Headers'; }
}

function extractBody(body, headers) {
  if (body === null || body === undefined) return null;
  if (typeof body === 'string') { if (!headers.has('content-type')) headers.set('content-type', 'text/plain;charset=UTF-8'); return W.utf8Encode(body); }
  if (body instanceof URLSearchParams) { if (!headers.has('content-type')) headers.set('content-type', 'application/x-www-form-urlencoded;charset=UTF-8'); return W.utf8Encode(body.toString()); }
  if (body instanceof FormData) { if (!headers.has('content-type')) headers.set('content-type', 'multipart/form-data; boundary=----ComputerworldFormBoundary'); return W.utf8Encode(multipart(body)); }
  if (body instanceof Blob) { if (body.type && !headers.has('content-type')) headers.set('content-type', body.type); return body._bytes; }
  if (body instanceof ArrayBuffer || ArrayBuffer.isView(body)) return bytesOf(body);
  return W.utf8Encode(String(body));
}
function multipart(fd) {
  let s = '';
  for (const [k, v] of fd._entries) {
    s += '------ComputerworldFormBoundary\r\n';
    if (v instanceof Blob) s += 'Content-Disposition: form-data; name="' + k + '"; filename="' + (v.name || 'blob') + '"\r\nContent-Type: ' + (v.type || 'application/octet-stream') + '\r\n\r\n' + W.utf8Decode(v._bytes) + '\r\n';
    else s += 'Content-Disposition: form-data; name="' + k + '"\r\n\r\n' + v + '\r\n';
  }
  return s + '------ComputerworldFormBoundary--\r\n';
}

class Body {
  constructor() { this._body = null; this._used = false; }
  get bodyUsed() { return this._used; }
  get body() { return this._body === null ? null : { getReader: () => { let done = false; return { read: () => { if (done) return Promise.resolve({ value: undefined, done: true }); done = true; return Promise.resolve({ value: this._body, done: false }); }, cancel: () => Promise.resolve(), releaseLock() {} }; }, cancel: () => Promise.resolve(), locked: false }; }
  _consume() { if (this._used) return Promise.reject(typeError('body stream already read')); this._used = true; return Promise.resolve(this._body || new Uint8Array(0)); }
  text() { return this._consume().then((b) => W.utf8Decode(b)); }
  json() { return this.text().then((t) => JSON.parse(t)); }
  arrayBuffer() { return this._consume().then((b) => b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength)); }
  bytes() { return this._consume().then((b) => b.slice()); }
  blob() { return this._consume().then((b) => { const bl = new Blob([], { type: (this.headers.get('content-type') || '').split(';')[0] }); bl._bytes = b; return bl; }); }
  formData() { return this.text().then((t) => { const fd = new FormData(); for (const [k, v] of new URLSearchParams(t)) fd.append(k, v); return fd; }); }
}
class Request extends Body {
  constructor(input, init) {
    super();
    init = init || {};
    if (input instanceof Request) { this.url = input.url; this.method = input.method; this.headers = new Headers(input.headers); this._body = input._body; this.mode = input.mode; this.credentials = input.credentials; this.cache = input.cache; this.redirect = input.redirect; this.referrer = input.referrer; this.signal = input.signal; }
    else { this.url = W.resolveUrl(String(input), document.baseURI); this.method = 'GET'; this.headers = new Headers(); this.mode = 'cors'; this.credentials = 'same-origin'; this.cache = 'default'; this.redirect = 'follow'; this.referrer = 'about:client'; this.signal = new AbortController().signal; }
    if (init.method !== undefined) { const m = String(init.method); if (['get', 'post', 'put', 'delete', 'head', 'options', 'patch'].includes(m.toLowerCase())) this.method = m.toUpperCase() === 'PATCH' ? 'PATCH' : m.toUpperCase(); else this.method = m; }
    if (init.headers !== undefined) this.headers = new Headers(init.headers);
    if (init.body !== undefined && init.body !== null) { if (this.method === 'GET' || this.method === 'HEAD') throw typeError('Request with GET/HEAD method cannot have body.'); this._body = extractBody(init.body, this.headers); }
    for (const k of ['mode', 'credentials', 'cache', 'redirect', 'referrer', 'integrity', 'keepalive', 'priority']) if (init[k] !== undefined) this[k] = init[k];
    if (init.signal) this.signal = init.signal;
    this.destination = ''; this.referrerPolicy = ''; this.duplex = 'half';
  }
  clone() { const r = new Request(this); r._body = this._body; return r; }
  get [Symbol.toStringTag]() { return 'Request'; }
}
class Response extends Body {
  constructor(body, init) {
    super();
    init = init || {};
    this.status = init.status === undefined ? 200 : init.status | 0;
    if (this.status < 200 || this.status > 599) throw new RangeError('Failed to construct \'Response\': The status provided (' + this.status + ') is outside the range [200, 599].');
    this.statusText = init.statusText === undefined ? '' : String(init.statusText);
    this.headers = new Headers(init.headers);
    this.url = '';
    this.type = 'default';
    this.redirected = false;
    this._body = body === undefined || body === null ? null : extractBody(body, this.headers);
  }
  get ok() { return this.status >= 200 && this.status < 300; }
  clone() { const r = new Response(null, { status: this.status, statusText: this.statusText, headers: this.headers }); r._body = this._body; r.url = this.url; r.type = this.type; return r; }
  static error() { const r = new Response(null, { status: 200 }); r.status = 0; r.type = 'error'; return r; }
  static redirect(url, status) { return new Response(null, { status: status || 302, headers: { location: String(url) } }); }
  static json(data, init) { const r = new Response(JSON.stringify(data), init); r.headers.set('content-type', 'application/json'); return r; }
  get [Symbol.toStringTag]() { return 'Response'; }
}

function fetch(input, init) {
  let req;
  try { req = new Request(input, init); } catch (e) { return Promise.reject(e); }
  return new Promise((resolve, reject) => {
    if (req.signal && req.signal.aborted) { reject(req.signal.reason); return; }
    const headers = req.headers._list.slice();
    const raw = W.fetch(req.url, req.method, headers, req._body);
    // The transport answered synchronously; the page sees it in a task.
    setTimeout(() => {
      if (req.signal && req.signal.aborted) { reject(req.signal.reason); return; }
      if (raw === null) { reject(typeError('Failed to fetch')); return; }
      const r = new Response(null, { status: Math.max(200, raw.status), statusText: raw.statusText, headers: raw.headers });
      r.status = raw.status;
      r._body = raw.body;
      r.url = raw.url;
      r.type = 'basic';
      resolve(r);
    }, 0);
  });
}

// ---------------------------------------------------------------- XMLHttpRequest

class XMLHttpRequestEventTarget extends EventTarget {}
for (const t of ['loadstart', 'progress', 'abort', 'error', 'load', 'timeout', 'loadend']) defineHandlerAttr(XMLHttpRequestEventTarget.prototype, t);
class XMLHttpRequestUpload extends XMLHttpRequestEventTarget {}
class XMLHttpRequest extends XMLHttpRequestEventTarget {
  constructor() {
    super();
    this._state = 0; this._method = 'GET'; this._url = ''; this._async = true; this._headers = []; this._response = null; this._responseBytes = null; this._sent = false; this._aborted = false;
    this.responseType = ''; this.timeout = 0; this.withCredentials = false; this.upload = new XMLHttpRequestUpload();
  }
  get readyState() { return this._state; }
  get status() { return this._response ? this._response.status : 0; }
  get statusText() { return this._response ? this._response.statusText : ''; }
  get responseURL() { return this._response ? this._response.url : ''; }
  get responseText() { if (this.responseType !== '' && this.responseType !== 'text') throw domError('InvalidStateError', "Failed to read the 'responseText' property from 'XMLHttpRequest': The value is only accessible if the object's 'responseType' is '' or 'text' (was '" + this.responseType + "')."); return this._state < 3 || !this._responseBytes ? '' : W.utf8Decode(this._responseBytes); }
  get responseXML() { if (this._state !== 4 || !this._responseBytes) return null; const ct = this.getResponseHeader('content-type') || ''; if (!/xml|html/.test(ct) && this.responseType !== 'document') return null; return new C.DOMParser().parseFromString(W.utf8Decode(this._responseBytes), 'text/html'); }
  get response() {
    if (this._state !== 4 || !this._responseBytes) return this.responseType === '' || this.responseType === 'text' ? '' : null;
    const b = this._responseBytes;
    switch (this.responseType) {
      case '': case 'text': return W.utf8Decode(b);
      case 'json': try { return JSON.parse(W.utf8Decode(b)); } catch (e) { return null; }
      case 'arraybuffer': return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
      case 'blob': { const bl = new Blob([], { type: (this.getResponseHeader('content-type') || '').split(';')[0] }); bl._bytes = b; return bl; }
      case 'document': return this.responseXML;
      default: return null;
    }
  }
  _change(state) { this._state = state; fire(this, new Event('readystatechange')); }
  open(method, url, async, user, password) {
    if (arguments.length < 2) throw typeError("Failed to execute 'open' on 'XMLHttpRequest': 2 arguments required, but only " + arguments.length + ' present.');
    const m = String(method).toUpperCase();
    if (['CONNECT', 'TRACE', 'TRACK'].includes(m)) throw domError('SecurityError', "Failed to execute 'open' on 'XMLHttpRequest': '" + method + "' HTTP method is unsupported.");
    this._method = m; this._url = W.resolveUrl(String(url), document.baseURI); this._async = async === undefined ? true : !!async; this._headers = []; this._response = null; this._responseBytes = null; this._sent = false; this._aborted = false;
    this._change(1);
  }
  setRequestHeader(name, value) { if (this._state !== 1 || this._sent) throw domError('InvalidStateError', "Failed to execute 'setRequestHeader' on 'XMLHttpRequest': The object's state must be OPENED."); this._headers.push([String(name).toLowerCase(), String(value).trim()]); }
  getResponseHeader(name) { if (!this._response) return null; name = String(name).toLowerCase(); const vals = this._response.headers.filter((h) => h[0] === name).map((h) => h[1]); return vals.length ? vals.join(', ') : null; }
  getAllResponseHeaders() { if (!this._response) return ''; return this._response.headers.slice().sort((a, b) => (a[0] < b[0] ? -1 : 1)).map((h) => h[0] + ': ' + h[1] + '\r\n').join(''); }
  overrideMimeType() {}
  send(body) {
    if (this._state !== 1 || this._sent) throw domError('InvalidStateError', "Failed to execute 'send' on 'XMLHttpRequest': The object's state must be OPENED.");
    this._sent = true;
    const headers = new Headers(this._headers);
    let bytes = null;
    if (body !== undefined && body !== null && this._method !== 'GET' && this._method !== 'HEAD') {
      if (isNode(body)) { bytes = W.utf8Encode(body.nodeType === 9 ? W.innerHTML(body) : W.outerHTML(body)); if (!headers.has('content-type')) headers.set('content-type', 'text/html;charset=UTF-8'); }
      else bytes = extractBody(body, headers);
    }
    const raw = W.fetch(this._url, this._method, headers._list, bytes);
    const finish = () => {
      if (this._aborted) return;
      if (raw === null) { this._response = null; this._change(4); fire(this, new C.ProgressEvent('error')); fire(this, new C.ProgressEvent('loadend')); return; }
      this._response = raw;
      this._responseBytes = raw.body;
      this._change(2);
      this._change(3);
      fire(this, new C.ProgressEvent('progress', { lengthComputable: true, loaded: raw.body.length, total: raw.body.length }));
      this._change(4);
      fire(this, new C.ProgressEvent('load', { lengthComputable: true, loaded: raw.body.length, total: raw.body.length }));
      fire(this, new C.ProgressEvent('loadend', { lengthComputable: true, loaded: raw.body.length, total: raw.body.length }));
    };
    fire(this, new C.ProgressEvent('loadstart'));
    if (this._async) setTimeout(finish, 0);
    else finish();
  }
  abort() {
    if (this._state === 0 || this._state === 4 && !this._sent) { this._state = 0; return; }
    this._aborted = true;
    if (this._sent && this._state !== 4) { this._change(4); fire(this, new C.ProgressEvent('abort')); fire(this, new C.ProgressEvent('loadend')); }
    this._state = 0;
  }
  get [Symbol.toStringTag]() { return 'XMLHttpRequest'; }
}
for (const [k, v] of Object.entries({ UNSENT: 0, OPENED: 1, HEADERS_RECEIVED: 2, LOADING: 3, DONE: 4 })) { Object.defineProperty(XMLHttpRequest, k, { value: v, enumerable: true }); Object.defineProperty(XMLHttpRequest.prototype, k, { value: v, enumerable: true }); }
defineHandlerAttr(XMLHttpRequest.prototype, 'readystatechange');

class WebSocket extends EventTarget {
  constructor(url) { super(); throw domError('SecurityError', "Failed to construct 'WebSocket': WebSockets are not available in this realm (" + String(url) + ')'); }
}
WebSocket.CONNECTING = 0; WebSocket.OPEN = 1; WebSocket.CLOSING = 2; WebSocket.CLOSED = 3;
class EventSource extends EventTarget { constructor(url) { super(); this.url = String(url); this.readyState = 2; this.onmessage = null; this.onerror = null; this.onopen = null; } close() {} }

// ---------------------------------------------------------------- MessageChannel

// The realm's task queue without a delay (Node's setImmediate, kept before the
// Node globals are removed below).
const postTask = globalThis.setImmediate;
class MessagePort extends EventTarget {
  constructor() { super(); this._other = null; this._started = false; this._queue = []; this._closed = false; }
  postMessage(data) {
    const other = this._other;
    if (!other || other._closed) return;
    const msg = structuredClone(data);
    // One task per message, in posting order across every port. A posted message
    // is a task that runs as soon as the loop gets to it, with no timer delay
    // (which is why React's scheduler posts to itself), so it goes on the
    // immediate queue the event loop drains without advancing the clock.
    postTask(() => { other._queue.push(msg); other._flush(); });
  }
  _flush() {
    if (!this._started) return;
    const q = this._queue;
    this._queue = [];
    for (const m of q) fire(this, new C.MessageEvent('message', { data: m, source: null, ports: [] }));
  }
  start() { this._started = true; this._flush(); }
  close() { this._closed = true; }
  get onmessage() { return C.getHandler(this, 'message'); }
  set onmessage(v) { C.setHandler(this, 'message', v, false); this.start(); }
  get [Symbol.toStringTag]() { return 'MessagePort'; }
}
defineHandlerAttr(MessagePort.prototype, 'messageerror');
class MessageChannel {
  constructor() { this.port1 = new MessagePort(); this.port2 = new MessagePort(); this.port1._other = this.port2; this.port2._other = this.port1; }
}
class BroadcastChannel extends EventTarget { constructor(name) { super(); this.name = String(name); this.onmessage = null; } postMessage() {} close() {} }

// ---------------------------------------------------------------- observers

const mutationObservers = new Set();
let pendingRecords = false;
class MutationRecord {
  constructor(type, target) { this.type = type; this.target = target; this.addedNodes = C.nodeList([]); this.removedNodes = C.nodeList([]); this.previousSibling = null; this.nextSibling = null; this.attributeName = null; this.attributeNamespace = null; this.oldValue = null; }
}
class MutationObserver {
  constructor(callback) { if (typeof callback !== 'function') throw typeError("Failed to construct 'MutationObserver': The callback provided as parameter 1 is not a function."); this._callback = callback; this._records = []; this._targets = new Map(); }
  observe(target, options) {
    if (!isNode(target)) throw typeError("Failed to execute 'observe' on 'MutationObserver': parameter 1 is not of type 'Node'.");
    options = Object.assign({}, options || {});
    if (options.attributeOldValue || options.attributeFilter) options.attributes = options.attributes === undefined ? true : options.attributes;
    if (options.characterDataOldValue) options.characterData = options.characterData === undefined ? true : options.characterData;
    if (!options.childList && !options.attributes && !options.characterData) throw typeError("Failed to execute 'observe' on 'MutationObserver': The options object must set at least one of 'attributes', 'characterData', or 'childList' to true.");
    if (options.attributeOldValue && !options.attributes) throw typeError("Failed to execute 'observe' on 'MutationObserver': The options object may only set 'attributeOldValue' to true when 'attributes' is true or not present.");
    this._targets.set(target, options);
    mutationObservers.add(this);
    C.mutationObserving = true;
    W.setFlag('observing', true);
  }
  disconnect() { this._targets.clear(); this._records = []; mutationObservers.delete(this); if (mutationObservers.size === 0) { C.mutationObserving = false; W.setFlag('observing', false); } }
  takeRecords() { const r = this._records; this._records = []; return r; }
  _queue(record) { this._records.push(record); scheduleDelivery(); }
}
function scheduleDelivery() {
  if (pendingRecords) return;
  pendingRecords = true;
  queueMicrotask(() => {
    pendingRecords = false;
    for (const o of Array.from(mutationObservers)) {
      const r = o._records;
      if (r.length === 0) continue;
      o._records = [];
      try { o._callback.call(o, r, o); } catch (e) { C.reportError(e); }
    }
  });
}
function interested(node, kind, name) {
  const out = [];
  for (const o of mutationObservers) {
    let best = null;
    for (const [target, opts] of o._targets) {
      if (target !== node && !(opts.subtree && target.contains(node))) continue;
      if (!opts[kind]) continue;
      if (kind === 'attributes' && opts.attributeFilter && !opts.attributeFilter.includes(name)) continue;
      if (!best || target === node) best = opts;
    }
    if (best) out.push([o, best]);
  }
  return out;
}
hooks.childList = (parent, added, removed, prev, next) => {
  for (const [o] of interested(parent, 'childList')) {
    const r = new MutationRecord('childList', parent);
    r.addedNodes = C.nodeList(added); r.removedNodes = C.nodeList(removed); r.previousSibling = prev; r.nextSibling = next;
    o._queue(r);
  }
};
hooks.characterData = (node, oldValue) => {
  for (const [o, opts] of interested(node, 'characterData')) {
    const r = new MutationRecord('characterData', node);
    if (opts.characterDataOldValue) r.oldValue = oldValue;
    o._queue(r);
  }
};
C.attributeRecord = (el, name, oldValue) => {
  for (const [o, opts] of interested(el, 'attributes', name)) {
    const r = new MutationRecord('attributes', el);
    r.attributeName = name;
    if (opts.attributeOldValue) r.oldValue = oldValue;
    o._queue(r);
  }
};
C.mutationObserving = false;

const resizeObservers = new Set();
class ResizeObserverEntry { constructor(target, rect) { this.target = target; this.contentRect = rect; const box = [{ inlineSize: rect.width, blockSize: rect.height }]; this.contentBoxSize = box; this.borderBoxSize = box; this.devicePixelContentBoxSize = box; } }
class ResizeObserver {
  constructor(cb) { if (typeof cb !== 'function') throw typeError("Failed to construct 'ResizeObserver': The callback provided as parameter 1 is not a function."); this._cb = cb; this._targets = new Map(); }
  observe(el) { if (!isNode(el)) throw typeError("Failed to execute 'observe' on 'ResizeObserver': parameter 1 is not of type 'Element'."); this._targets.set(el, null); resizeObservers.add(this); W.setFlag('observers', true); scheduleLayoutCheck(); }
  unobserve(el) { this._targets.delete(el); }
  disconnect() { this._targets.clear(); resizeObservers.delete(this); }
  _check() {
    const entries = [];
    for (const [el, last] of this._targets) {
      const r = W.boundingRect(el);
      const m = W.boxMetrics(el);
      const size = m[6] + 'x' + m[7];
      if (last === size) continue;
      this._targets.set(el, size);
      entries.push(new ResizeObserverEntry(el, new C.DOMRectReadOnly(0, 0, m[6], m[7])));
      void r;
    }
    if (entries.length) { try { this._cb.call(this, entries, this); } catch (e) { C.reportError(e); } }
  }
}
const intersectionObservers = new Set();
class IntersectionObserverEntry { constructor(target, rootBounds, rect, inter, ratio, isIntersecting, time) { this.target = target; this.rootBounds = rootBounds; this.boundingClientRect = rect; this.intersectionRect = inter; this.intersectionRatio = ratio; this.isIntersecting = isIntersecting; this.time = time; } }
class IntersectionObserver {
  constructor(cb, options) {
    if (typeof cb !== 'function') throw typeError("Failed to construct 'IntersectionObserver': The callback provided as parameter 1 is not a function.");
    options = options || {};
    this._cb = cb; this.root = options.root || null; this.rootMargin = options.rootMargin || '0px 0px 0px 0px';
    const t = options.threshold === undefined ? [0] : Array.isArray(options.threshold) ? options.threshold : [options.threshold];
    this.thresholds = t.map(Number).sort((a, b) => a - b);
    this._targets = new Map();
    this._queued = [];
  }
  observe(el) { if (!isNode(el)) throw typeError("Failed to execute 'observe' on 'IntersectionObserver': parameter 1 is not of type 'Element'."); if (this._targets.has(el)) return; this._targets.set(el, null); intersectionObservers.add(this); W.setFlag('observers', true); scheduleLayoutCheck(); }
  unobserve(el) { this._targets.delete(el); }
  disconnect() { this._targets.clear(); intersectionObservers.delete(this); }
  takeRecords() { const q = this._queued; this._queued = []; return q; }
  _check() {
    const vp = W.viewport();
    const rootRect = this.root && isNode(this.root) ? C.rectOf(W.boundingRect(this.root)) : new C.DOMRectReadOnly(0, 0, vp[0], vp[1]);
    const entries = [];
    for (const [el, last] of this._targets) {
      const rect = C.rectOf(W.boundingRect(el));
      const x0 = Math.max(rect.left, rootRect.left), y0 = Math.max(rect.top, rootRect.top), x1 = Math.min(rect.right, rootRect.right), y1 = Math.min(rect.bottom, rootRect.bottom);
      const inter = x1 > x0 && y1 > y0 ? new C.DOMRectReadOnly(x0, y0, x1 - x0, y1 - y0) : new C.DOMRectReadOnly(0, 0, 0, 0);
      const area = rect.width * rect.height;
      const ratio = area > 0 ? (inter.width * inter.height) / area : (x1 >= x0 && y1 >= y0 && W.isRendered(el) ? 1 : 0);
      const isIntersecting = W.isRendered(el) && x1 >= x0 && y1 >= y0 && (ratio > 0 || (area === 0 && rect.width >= 0));
      let idx = -1;
      if (isIntersecting) { idx = 0; for (let i = 0; i < this.thresholds.length; i++) if (ratio >= this.thresholds[i]) idx = i + 1; }
      if (idx === last) continue;
      this._targets.set(el, idx);
      entries.push(new IntersectionObserverEntry(el, rootRect, rect, inter, ratio, isIntersecting, performance.now()));
    }
    if (entries.length) { try { this._cb.call(this, entries, this); } catch (e) { C.reportError(e); } }
  }
}
let layoutCheckQueued = false;
function scheduleLayoutCheck() { if (layoutCheckQueued) return; layoutCheckQueued = true; setTimeout(() => { layoutCheckQueued = false; hooks.afterLayout(); }, 0); }
hooks.afterLayout = () => {
  for (const o of Array.from(resizeObservers)) o._check();
  for (const o of Array.from(intersectionObservers)) o._check();
};
class PerformanceObserver { constructor(cb) { this._cb = cb; } observe() {} disconnect() {} takeRecords() { return []; } static get supportedEntryTypes() { return ['mark', 'measure', 'navigation', 'resource', 'paint']; } }
class ReportingObserver { constructor() {} observe() {} disconnect() {} takeRecords() { return []; } }

// ---------------------------------------------------------------- timers, frames

const rafCallbacks = new Map();
let rafId = 0;
function requestAnimationFrame(cb) { if (typeof cb !== 'function') throw typeError("Failed to execute 'requestAnimationFrame' on 'Window': The callback provided as parameter 1 is not a function."); const id = ++rafId; rafCallbacks.set(id, cb); W.setFlag('raf', true); return id; }
function cancelAnimationFrame(id) { rafCallbacks.delete(id); if (rafCallbacks.size === 0) W.setFlag('raf', false); }
hooks.animationFrame = (t) => {
  const cbs = Array.from(rafCallbacks.entries());
  rafCallbacks.clear();
  W.setFlag('raf', false);
  for (const [, cb] of cbs) { try { cb(t); } catch (e) { C.reportError(e); } }
  if (rafCallbacks.size) W.setFlag('raf', true);
};
function requestIdleCallback(cb, options) { const timeout = options && options.timeout; return setTimeout(() => { const start = performance.now(); cb({ didTimeout: false, timeRemaining: () => Math.max(0, 50 - (performance.now() - start)) }); }, timeout ? Math.min(timeout, 1) : 1); }
function cancelIdleCallback(id) { clearTimeout(id); }
// Browser timers return numbers.
const nodeSetTimeout = globalThis.setTimeout, nodeSetInterval = globalThis.setInterval, nodeClearTimeout = globalThis.clearTimeout;
function setTimeout(cb, delay, ...args) {
  if (typeof cb !== 'function') { const src = String(cb); cb = () => (0, eval)(src); }
  const t = nodeSetTimeout(cb, delay === undefined ? 0 : Math.max(0, Number(delay) || 0), ...args);
  return +t;
}
function setInterval(cb, delay, ...args) {
  if (typeof cb !== 'function') { const src = String(cb); cb = () => (0, eval)(src); }
  const t = nodeSetInterval(cb, delay === undefined ? 0 : Math.max(0, Number(delay) || 0), ...args);
  return +t;
}
function clearTimeout(id) { nodeClearTimeout(id); }

// ---------------------------------------------------------------- location, history, navigator, screen

function currentUrl() { return new URL(W.url()); }
const location = {};
for (const p of ['href', 'protocol', 'host', 'hostname', 'port', 'pathname', 'search', 'hash', 'origin']) {
  Object.defineProperty(location, p, {
    get() { const u = currentUrl(); return u[p]; },
    set(v) {
      if (p === 'origin') return;
      const u = currentUrl();
      if (p === 'href') { navigateTo(String(v)); return; }
      u[p] = String(v);
      navigateTo(u.href);
    },
    enumerable: true, configurable: false,
  });
}
function navigateTo(href) {
  const target = W.resolveUrl(href, document.baseURI);
  const cur = W.url();
  if (target.split('#')[0] === cur.split('#')[0] && (target.includes('#') || cur.includes('#'))) {
    if (target === cur) { scrollToFragment(); return; }
    W.history('push', null, target);
    scrollToFragment();
    // A fragment navigation changes the session history entry, so it fires
    // popstate (what React Router's hash history listens for) before hashchange.
    fire(globalThis, new C.PopStateEvent('popstate', { state: null }));
    fire(globalThis, new C.HashChangeEvent('hashchange', { oldURL: cur, newURL: target }));
    return;
  }
  W.navigate(target);
}
function scrollToFragment() {
  const h = currentUrl().hash.slice(1);
  if (!h) return;
  const el = document.getElementById(decodeURIComponent(h)) || document.querySelector('a[name="' + h.replace(/"/g, '\\"') + '"]');
  if (el) W.scrollIntoView(el, true, '');
}
define(location, 'assign', (url) => navigateTo(String(url)));
define(location, 'replace', (url) => { const target = W.resolveUrl(String(url), document.baseURI); const cur = W.url(); if (target.split('#')[0] === cur.split('#')[0] && (target.includes('#') || cur.includes('#'))) { W.history('replace', null, target); scrollToFragment(); fire(globalThis, new C.PopStateEvent('popstate', { state: null })); fire(globalThis, new C.HashChangeEvent('hashchange', { oldURL: cur, newURL: target })); return; } W.navigate(target); });
define(location, 'reload', () => W.navigate(W.url()));
define(location, 'toString', () => W.url());
define(location, 'ancestorOrigins', { length: 0, item: () => null, contains: () => false });
Object.defineProperty(location, Symbol.toStringTag, { value: 'Location' });
hooks.hashchange = (oldUrl, newUrl) => { fire(globalThis, new C.HashChangeEvent('hashchange', { oldURL: oldUrl, newURL: newUrl })); };
hooks.popstate = (state) => { fire(globalThis, new C.PopStateEvent('popstate', { state: state === null ? null : JSON.parse(state) })); };

const history = {
  get length() { return W.history('length'); },
  get state() { const s = W.history('state'); return s === null ? null : JSON.parse(s); },
  scrollRestoration: 'auto',
  pushState(state, title, url) { const s = state === undefined || state === null ? null : JSON.stringify(structuredClone(state)); W.history('push', s, url === undefined || url === null ? '' : String(url)); },
  replaceState(state, title, url) { const s = state === undefined || state === null ? null : JSON.stringify(structuredClone(state)); W.history('replace', s, url === undefined || url === null ? '' : String(url)); },
  go(delta) { delta = delta === undefined ? 0 : delta | 0; if (delta === 0) { W.navigate(W.url()); return; } const before = W.url(); setTimeout(() => { const r = W.history('go', delta); if (!r) return; hooks.popstate(r[1]); if (before.split('#')[0] === r[0].split('#')[0] && before !== r[0]) hooks.hashchange(before, r[0]); }, 0); },
  back() { this.go(-1); },
  forward() { this.go(1); },
};
Object.defineProperty(history, Symbol.toStringTag, { value: 'History' });

const navigator = {
  userAgent: 'Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Computerworld/1.0',
  appName: 'Netscape', appVersion: '5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Computerworld/1.0', appCodeName: 'Mozilla', product: 'Gecko', productSub: '20030107', vendor: 'Computerworld', vendorSub: '',
  language: 'en-US', languages: Object.freeze(['en-US', 'en']), platform: 'Linux x86_64', onLine: true, hardwareConcurrency: 4, maxTouchPoints: 0, cookieEnabled: true, doNotTrack: null, deviceMemory: 8, webdriver: false, pdfViewerEnabled: false,
  userAgentData: { brands: [{ brand: 'Chromium', version: '124' }, { brand: 'Computerworld', version: '1' }], mobile: false, platform: 'Linux', getHighEntropyValues: () => Promise.resolve({ platform: 'Linux', mobile: false, brands: [] }) },
  clipboard: { writeText: () => Promise.resolve(), readText: () => Promise.resolve(''), write: () => Promise.resolve(), read: () => Promise.resolve([]) },
  sendBeacon: () => true,
  javaEnabled: () => false,
  vibrate: () => false,
  getGamepads: () => [],
  registerProtocolHandler() {},
  permissions: { query: () => Promise.resolve({ state: 'prompt', onchange: null, addEventListener() {}, removeEventListener() {} }) },
  connection: { effectiveType: '4g', rtt: 50, downlink: 10, saveData: false, addEventListener() {}, removeEventListener() {} },
  mediaDevices: { enumerateDevices: () => Promise.resolve([]), getUserMedia: () => Promise.reject(domError('NotAllowedError', 'Permission denied')) },
  storage: { estimate: () => Promise.resolve({ quota: 1073741824, usage: 0 }), persist: () => Promise.resolve(false), persisted: () => Promise.resolve(false) },
  locks: { request: (name, opts, cb) => Promise.resolve((typeof opts === 'function' ? opts : cb)({ name })) },
  mimeTypes: { length: 0, item: () => null, namedItem: () => null },
  plugins: { length: 0, item: () => null, namedItem: () => null, refresh() {} },
  serviceWorker: undefined,
  geolocation: { getCurrentPosition: (ok, err) => { if (err) setTimeout(() => err({ code: 1, message: 'User denied Geolocation' }), 0); }, watchPosition: () => 0, clearWatch() {} },
  share: () => Promise.reject(domError('NotAllowedError', 'Share not available.')),
  canShare: () => false,
};
Object.defineProperty(navigator, Symbol.toStringTag, { value: 'Navigator' });
const screen = {
  get width() { return W.viewport()[0]; }, get height() { return W.viewport()[1]; }, get availWidth() { return W.viewport()[0]; }, get availHeight() { return W.viewport()[1]; }, availLeft: 0, availTop: 0, colorDepth: 24, pixelDepth: 24,
  orientation: { type: 'landscape-primary', angle: 0, addEventListener() {}, removeEventListener() {}, lock: () => Promise.resolve(), unlock() {} }, isExtended: false,
};
Object.defineProperty(screen, Symbol.toStringTag, { value: 'Screen' });

// ---------------------------------------------------------------- storage

class Storage {
  constructor() { throw typeError('Illegal constructor'); }
  getItem(k) { return W.storageOp(this._area(), 'get', String(k)); }
  setItem(k, v) { W.storageOp(this._area(), 'set', String(k), String(v)); }
  removeItem(k) { W.storageOp(this._area(), 'remove', String(k)); }
  clear() { W.storageOp(this._area(), 'clear', ''); }
  key(i) { const keys = W.storageOp(this._area(), 'keys', ''); return keys[i] === undefined ? null : keys[i]; }
  get length() { return W.storageOp(this._area(), 'keys', '').length; }
  _area() { return W.nodeId(this); }
  get [Symbol.toStringTag]() { return 'Storage'; }
}

// ---------------------------------------------------------------- crypto, performance

const crypto = globalThis.crypto || {};
define(crypto, 'getRandomValues', (arr) => { if (!ArrayBuffer.isView(arr)) throw typeError("Failed to execute 'getRandomValues' on 'Crypto': parameter 1 is not of type 'ArrayBufferView'."); const bytes = W.randomBytes(arr.byteLength); new Uint8Array(arr.buffer, arr.byteOffset, arr.byteLength).set(bytes); return arr; });
define(crypto, 'randomUUID', () => { const b = W.randomBytes(16); b[6] = (b[6] & 0x0f) | 0x40; b[8] = (b[8] & 0x3f) | 0x80; const h = Array.from(b, (x) => x.toString(16).padStart(2, '0')).join(''); return h.slice(0, 8) + '-' + h.slice(8, 12) + '-' + h.slice(12, 16) + '-' + h.slice(16, 20) + '-' + h.slice(20); });
define(crypto, 'subtle', { digest: () => Promise.reject(domError('NotSupportedError', 'Not supported.')) });
Object.defineProperty(crypto, Symbol.toStringTag, { value: 'Crypto' });
const perf = globalThis.performance;
const perfEntries = [];
define(perf, 'mark', (name, options) => { const e = { name: String(name), entryType: 'mark', startTime: options && options.startTime !== undefined ? options.startTime : perf.now(), duration: 0, detail: options && options.detail !== undefined ? options.detail : null, toJSON() { return { name: this.name, entryType: this.entryType, startTime: this.startTime, duration: this.duration }; } }; perfEntries.push(e); return e; });
define(perf, 'measure', (name, start, end) => {
  let s = 0, d = 0;
  const find = (n) => perfEntries.filter((e) => e.name === n && e.entryType === 'mark').pop();
  if (typeof start === 'object' && start !== null) { s = start.start !== undefined ? (typeof start.start === 'string' ? find(start.start).startTime : start.start) : 0; const e = start.end !== undefined ? (typeof start.end === 'string' ? find(start.end).startTime : start.end) : perf.now(); d = start.duration !== undefined ? start.duration : e - s; }
  else { if (start !== undefined) { const m = find(start); if (!m) throw domError('SyntaxError', "Failed to execute 'measure' on 'Performance': The mark '" + start + "' does not exist."); s = m.startTime; } const e = end !== undefined ? find(end).startTime : perf.now(); d = e - s; }
  const entry = { name: String(name), entryType: 'measure', startTime: s, duration: d, detail: null, toJSON() { return { name: this.name, entryType: this.entryType, startTime: this.startTime, duration: this.duration }; } };
  perfEntries.push(entry);
  return entry;
});
define(perf, 'getEntries', () => perfEntries.slice());
define(perf, 'getEntriesByType', (t) => perfEntries.filter((e) => e.entryType === t));
define(perf, 'getEntriesByName', (n, t) => perfEntries.filter((e) => e.name === n && (t === undefined || e.entryType === t)));
define(perf, 'clearMarks', (n) => { for (let i = perfEntries.length - 1; i >= 0; i--) if (perfEntries[i].entryType === 'mark' && (n === undefined || perfEntries[i].name === n)) perfEntries.splice(i, 1); });
define(perf, 'clearMeasures', (n) => { for (let i = perfEntries.length - 1; i >= 0; i--) if (perfEntries[i].entryType === 'measure' && (n === undefined || perfEntries[i].name === n)) perfEntries.splice(i, 1); });
define(perf, 'clearResourceTimings', () => {});
define(perf, 'setResourceTimingBufferSize', () => {});
define(perf, 'toJSON', () => ({ timeOrigin: perf.timeOrigin }));
// Made on first use, from the realm's clock origin (which the booted prelude does
// not hold, so one heap image of it serves every realm).
Object.defineProperty(perf, 'timing', { configurable: true, enumerable: false, get() { const o = Math.floor(perf.timeOrigin); const t = { navigationStart: o, fetchStart: o, domLoading: o, domContentLoadedEventStart: 0, domContentLoadedEventEnd: 0, loadEventStart: 0, loadEventEnd: 0, responseEnd: o, domInteractive: 0, domComplete: 0 }; define(perf, 'timing', t); return t; } });
define(perf, 'navigation', { type: 0, redirectCount: 0 });
define(perf, 'memory', { usedJSHeapSize: 10000000, totalJSHeapSize: 20000000, jsHeapSizeLimit: 2000000000 });
define(perf, 'eventCounts', new Map());

// ---------------------------------------------------------------- dialogs, misc window methods

function alert(msg) { W.alert('alert', arguments.length ? String(msg) : ''); }
function confirm(msg) { W.alert('confirm', arguments.length ? String(msg) : ''); return true; }
function prompt(msg, def) { W.alert('prompt', arguments.length ? String(msg) : ''); return null; }
function scrollWindow(x, y, by) {
  if (typeof x === 'object' && x !== null) { const o = x; x = o.left; y = o.top; }
  const cur = W.scrollOf(null);
  const nx = x === undefined ? null : (by ? cur[0] + (+x || 0) : +x || 0);
  const ny = y === undefined ? null : (by ? cur[1] + (+y || 0) : +y || 0);
  W.scrollTo(null, nx, ny);
  const after = W.scrollOf(null);
  if (after[0] !== cur[0] || after[1] !== cur[1]) queueTask(() => fireSimple(document, 'scroll', true, false));
}

// ---------------------------------------------------------------- dispatch hooks (from Realm::dispatch)

function mouseInit(x, y, button, mods, detail, related) {
  const s = W.windowScroll();
  return { bubbles: true, cancelable: true, composed: true, clientX: x, clientY: y, pageX: x + s[0], pageY: y + s[1], screenX: x, screenY: y, button: button, buttons: button === 0 ? 1 : button === 1 ? 4 : 2, detail: detail, view: globalThis, relatedTarget: related || null, ctrlKey: mods.ctrlKey, shiftKey: mods.shiftKey, altKey: mods.altKey, metaKey: mods.metaKey };
}
hooks.pointer = (kind, target, x, y, button, mods, detail) => {
  const names = { down: ['pointerdown', 'mousedown'], up: ['pointerup', 'mouseup'], move: ['pointermove', 'mousemove'] }[kind];
  const init = mouseInit(x, y, button, mods, detail);
  const pe = new C.PointerEvent(names[0], init);
  const pPrevented = !fire(target, pe);
  if (pPrevented && kind === 'down') return true;
  const me = new C.MouseEvent(names[1], init);
  if (kind === 'move') { init.buttons = 0; }
  return !fire(target, me);
};
// `fake`: the content moved under a still pointer, so only the boundary events
// fire (no `pointermove`/`mousemove`).
hooks.hover = (oldT, newT, x, y, mods, fake) => {
  const init = mouseInit(x, y, 0, mods, 0);
  init.buttons = 0;
  if (oldT) {
    fire(oldT, new C.PointerEvent('pointerout', Object.assign({}, init, { relatedTarget: newT })));
    fire(oldT, new C.MouseEvent('mouseout', Object.assign({}, init, { relatedTarget: newT })));
    // mouseleave on the old chain up to the common ancestor (does not bubble).
    let n = oldT;
    while (n && n !== document) { if (!(newT && n.contains(newT))) { fire(n, new C.PointerEvent('pointerleave', Object.assign({}, init, { bubbles: false, relatedTarget: newT }))); fire(n, new C.MouseEvent('mouseleave', Object.assign({}, init, { bubbles: false, relatedTarget: newT }))); } n = n.parentNode; }
  }
  if (newT) {
    fire(newT, new C.PointerEvent('pointerover', Object.assign({}, init, { relatedTarget: oldT })));
    fire(newT, new C.MouseEvent('mouseover', Object.assign({}, init, { relatedTarget: oldT })));
    const chain = [];
    let n = newT;
    while (n && n !== document) { if (!(oldT && n.contains(oldT))) chain.push(n); n = n.parentNode; }
    for (let i = chain.length - 1; i >= 0; i--) { fire(chain[i], new C.PointerEvent('pointerenter', Object.assign({}, init, { bubbles: false, relatedTarget: oldT }))); fire(chain[i], new C.MouseEvent('mouseenter', Object.assign({}, init, { bubbles: false, relatedTarget: oldT }))); }
    if (!fake) {
      fire(newT, new C.PointerEvent('pointermove', init));
      fire(newT, new C.MouseEvent('mousemove', init));
    }
  }
};
hooks.focusChange = (oldT, newT) => {
  if (oldT) {
    fire(oldT, new C.FocusEvent('blur', { relatedTarget: newT }));
    fire(oldT, new C.FocusEvent('focusout', { bubbles: true, composed: true, relatedTarget: newT }));
    if (oldT['%changePending']) { define(oldT, '%changePending', false); fireSimple(oldT, 'change', true, false); }
  }
  if (newT) {
    fire(newT, new C.FocusEvent('focus', { relatedTarget: oldT }));
    fire(newT, new C.FocusEvent('focusin', { bubbles: true, composed: true, relatedTarget: oldT }));
  }
};
hooks.click = (target, x, y, mods, detail) => {
  const prevented = !fire(target, new C.PointerEvent('click', mouseInit(x, y, 0, mods, detail)));
  if (detail === 2) fire(target, new C.MouseEvent('dblclick', mouseInit(x, y, 0, mods, 2)));
  return prevented;
};
hooks.contextmenu = (target, x, y, mods) => { fire(target, new C.PointerEvent('contextmenu', mouseInit(x, y, 2, mods, 0))); };
hooks.auxclick = (target, x, y, mods) => { fire(target, new C.PointerEvent('auxclick', mouseInit(x, y, 1, mods, 1))); };
hooks.key = (kind, target, key, code, mods, repeat) => !fire(target, new C.KeyboardEvent(kind, { bubbles: true, cancelable: true, composed: true, key, code, repeat, view: globalThis, ctrlKey: mods.ctrlKey, shiftKey: mods.shiftKey, altKey: mods.altKey, metaKey: mods.metaKey }));
hooks.beforeInput = (target, data, inputType) => !fire(target, new C.InputEvent('beforeinput', { bubbles: true, cancelable: true, composed: true, data, inputType }));
hooks.input = (target, data, inputType) => {
  const isCheck = target.localName === 'input' && (target.type === 'checkbox' || target.type === 'radio');
  fire(target, new C.InputEvent('input', { bubbles: true, composed: true, data: isCheck ? null : data, inputType: isCheck ? '' : inputType }));
  if (!isCheck && target.localName !== 'select') define(target, '%changePending', true);
};
hooks.change = (target) => { define(target, '%changePending', false); fireSimple(target, 'change', true, false); };
hooks.submit = (form, submitter) => !fire(form, new C.SubmitEvent('submit', { bubbles: true, cancelable: true, submitter }));
hooks.reset = (form) => !fireSimple(form, 'reset', true, true);
hooks.toggle = (details) => { queueTask(() => fire(details, new C.ToggleEvent('toggle', { oldState: details.open ? 'closed' : 'open', newState: details.open ? 'open' : 'closed' }))); };
hooks.scroll = (target) => { if (target) fireSimple(target, 'scroll', false, false); else { fireSimple(document, 'scroll', true, false); } };
hooks.wheel = (target, x, y, dx, dy, mods) => !fire(target, new C.WheelEvent('wheel', Object.assign(mouseInit(x, y, 0, mods, 0), { deltaX: dx, deltaY: dy, deltaMode: 0 })));
hooks.resize = () => { C.reevaluateMedia(); fireSimple(globalThis, 'resize', false, false); };
hooks.visibility = () => { fireSimple(document, 'visibilitychange', true, false); };
hooks.pageshow = () => { fire(globalThis, new C.PageTransitionEvent('pageshow', { persisted: false })); };
hooks.unload = () => {
  const ev = new C.BeforeUnloadEvent();
  fire(globalThis, ev);
  const msg = ev.defaultPrevented ? (ev.returnValue || 'Changes you made may not be saved.') : null;
  fire(globalThis, new C.PageTransitionEvent('pagehide', { persisted: false }));
  fireSimple(document, 'visibilitychange', true, false);
  fireSimple(globalThis, 'unload', false, false);
  return msg;
};
// `element.click()`: the click event and the activation behaviour, in script.
hooks.syntheticClick = (el) => {
  let toggled = null;
  if (el.localName === 'input' && (el.type === 'checkbox' || el.type === 'radio')) { toggled = el.checked; el.checked = el.type === 'radio' ? true : !toggled; }
  const notPrevented = C.dispatch(el, new C.PointerEvent('click', { bubbles: true, cancelable: true, composed: true, view: globalThis, detail: 0 }));
  if (!notPrevented) { if (toggled !== null) el.checked = toggled; return; }
  if (toggled !== null) { hooks.input(el, null, ''); hooks.change(el); return; }
  let n = el;
  while (n && n.nodeType === 1) {
    const tag = n.localName;
    if ((tag === 'a' || tag === 'area') && n.hasAttribute('href')) { navigateTo(n.getAttribute('href')); return; }
    if (tag === 'button' || (tag === 'input' && ['submit', 'image', 'reset', 'button'].includes(n.type))) {
      const form = n.form;
      if (!form || W.isDisabled(n)) return;
      if (n.type === 'reset') { form.reset(); return; }
      if (n.type === 'button') return;
      hooks.programmaticSubmit(form, n, true);
      return;
    }
    if (tag === 'label') { const c = n.control; if (c && c !== el && !c.contains(el)) c.click(); return; }
    if (tag === 'summary' && n.parentNode && n.parentNode.localName === 'details') { n.parentNode.open = !n.parentNode.open; return; }
    n = n.parentNode;
  }
};
hooks.programmaticSubmit = (form, submitter, fireEvent) => {
  if (form['%submitting']) return;
  if (fireEvent) {
    if (!(form.noValidate || (submitter && submitter.formNoValidate))) { if (!hooks.validate(form)) return; }
    define(form, '%submitting', true);
    let ok;
    try { ok = !hooks.submit(form, submitter); } finally { define(form, '%submitting', false); }
    if (!ok) return;
  }
  const method = submitter && submitter.getAttribute('formmethod') ? submitter.getAttribute('formmethod').toLowerCase() : form.method;
  const action = submitter && submitter.getAttribute('formaction') ? W.resolveUrl(submitter.getAttribute('formaction'), document.baseURI) : form.action;
  const enctype = submitter && submitter.getAttribute('formenctype') ? submitter.getAttribute('formenctype') : form.enctype;
  if (method === 'dialog') { const d = form.closest('dialog'); if (d) d.close(submitter && submitter.value !== undefined ? submitter.value : ''); return; }
  W.submitForm(form, submitter, action, method, enctype);
};
// CSS transitions and animations. The style layer reports each one a style change
// started; the events land on the world clock through the realm's own timers, so a
// page that waits for `transitionend` (Vue's `<transition>`) or `animationend`
// (`svelte/transition`) proceeds exactly `delay + duration` later. Running one per
// (element, property) matches CSS Transitions §3: a new transition for a property
// replaces the one in flight, which is cancelled.
const runningAnimations = new WeakMap();
const animationKey = (kind, name) => kind + ':' + name;
const cancelAnimation = (el, key, cancelEvent) => {
  const running = runningAnimations.get(el);
  const cur = running && running.get(key);
  if (!cur) return;
  for (const t of cur.timers) clearTimeout(t);
  running.delete(key);
  if (cancelEvent) fire(el, cancelEvent(cur));
};
const startAnimation = (el, key, timers, delay) => {
  let running = runningAnimations.get(el);
  if (!running) runningAnimations.set(el, (running = new Map()));
  running.set(key, { timers, started: performance.now(), delay });
};
hooks.cssTransition = (el, property, delayMs, durationMs) => {
  const key = animationKey('t', property);
  const elapsed = (cur) => Math.max(0, (performance.now() - cur.started - cur.delay) / 1000);
  cancelAnimation(el, key, (cur) => new C.TransitionEvent('transitioncancel', { bubbles: true, composed: true, propertyName: property, elapsedTime: elapsed(cur) }));
  const init = { bubbles: true, composed: true, propertyName: property, elapsedTime: 0 };
  const timers = [
    setTimeout(() => fire(el, new C.TransitionEvent('transitionstart', init)), delayMs),
    setTimeout(() => {
      const running = runningAnimations.get(el);
      if (running) running.delete(key);
      fire(el, new C.TransitionEvent('transitionend', { ...init, elapsedTime: durationMs / 1000 }));
    }, delayMs + durationMs),
  ];
  startAnimation(el, key, timers, delayMs);
  fire(el, new C.TransitionEvent('transitionrun', init));
};
hooks.cssAnimation = (el, name, delayMs, durationMs, iterations, cancelled) => {
  const key = animationKey('a', name);
  cancelAnimation(el, key, (cur) => new C.AnimationEvent('animationcancel', { bubbles: true, composed: true, animationName: name, elapsedTime: Math.max(0, (performance.now() - cur.started - cur.delay) / 1000) }));
  if (cancelled) return;
  const init = { bubbles: true, composed: true, animationName: name, elapsedTime: 0 };
  const timers = [setTimeout(() => fire(el, new C.AnimationEvent('animationstart', init)), delayMs)];
  // `animation-iteration-count: infinite` never ends, and fires no iteration events
  // here; a finite count fires one `animationiteration` per completed iteration but
  // the last, then `animationend`.
  if (iterations !== null) {
    const whole = Math.floor(iterations);
    for (let i = 1; i < whole; i++) {
      timers.push(setTimeout(() => fire(el, new C.AnimationEvent('animationiteration', { ...init, elapsedTime: (durationMs * i) / 1000 })), delayMs + durationMs * i));
    }
    const total = durationMs * iterations;
    timers.push(setTimeout(() => {
      const running = runningAnimations.get(el);
      if (running) running.delete(key);
      fire(el, new C.AnimationEvent('animationend', { ...init, elapsedTime: total / 1000 }));
    }, delayMs + total));
  }
  startAnimation(el, key, timers, delayMs);
};
hooks.scriptError = (el) => { fireSimple(el, 'error', false, false); };
hooks.radioList = (items) => new C.RadioNodeList(items);
hooks.scriptLoad = (el) => { if (el.hasAttribute('src')) fireSimple(el, 'load', false, false); };
hooks.parsed = () => { C.reevaluateMedia(); };
hooks.readyState = () => { fireSimple(document, 'readystatechange', false, false); };
hooks.domContentLoaded = () => { fireSimple(document, 'DOMContentLoaded', true, false); };
hooks.load = () => {
  const loadEvent = new Event('load');
  loadEvent._targetOverride = document;
  fire(globalThis, loadEvent);
  fire(globalThis, new C.PageTransitionEvent('pageshow', { persisted: false }));
};

// ---------------------------------------------------------------- globals

class Window extends EventTarget {
  constructor() { throw typeError('Illegal constructor'); }
  get [Symbol.toStringTag]() { return 'Window'; }
}
for (const t of globalEventTypes.concat(windowEventTypes)) defineHandlerAttr(Window.prototype, t);
Object.setPrototypeOf(globalThis, Window.prototype);
const document = W.document();
defineGlobal('document', document);
defineGlobal('window', globalThis);
defineGlobal('self', globalThis);
defineGlobal('top', globalThis);
defineGlobal('parent', globalThis);
defineGlobal('frames', globalThis);
defineGlobal('opener', null);
defineGlobal('closed', false);
defineGlobal('length', 0);
defineGlobal('name', '');
defineGlobal('status', '');
defineGlobal('isSecureContext', true);
defineGlobal('crossOriginIsolated', false);
defineGlobal('location', location);
Object.defineProperty(globalThis, 'location', { get() { return location; }, set(v) { location.href = String(v); }, configurable: false, enumerable: true });
defineGlobal('history', history);
defineGlobal('navigator', navigator);
defineGlobal('clientInformation', navigator);
defineGlobal('screen', screen);
defineGlobal('localStorage', W.storage(0));
defineGlobal('sessionStorage', W.storage(1));
defineGlobal('crypto', crypto);
defineGlobal('customElements', C.customElements);
defineGlobal('visualViewport', { get width() { return W.viewport()[0]; }, get height() { return W.viewport()[1]; }, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0, scale: 1, addEventListener() {}, removeEventListener() {} });
Object.defineProperty(globalThis, 'origin', { get() { return location.origin; }, configurable: true });
for (const [k, g] of Object.entries({ innerWidth: () => W.viewport()[0], innerHeight: () => W.viewport()[1], outerWidth: () => W.viewport()[0], outerHeight: () => W.viewport()[1] + 85, devicePixelRatio: () => W.viewport()[2], screenX: () => 0, screenY: () => 0, screenLeft: () => 0, screenTop: () => 0, scrollX: () => W.scrollOf(null)[0], scrollY: () => W.scrollOf(null)[1], pageXOffset: () => W.scrollOf(null)[0], pageYOffset: () => W.scrollOf(null)[1] })) Object.defineProperty(globalThis, k, { get: g, set(v) { define(globalThis, k, v); }, configurable: true, enumerable: true });
const globals = {
  alert, confirm, prompt, print() {}, open(url) { if (url !== undefined && url !== null && String(url) !== '' && String(url) !== 'about:blank') W.navigate('cw-new-tab:' + W.resolveUrl(String(url), document.baseURI)); return null; }, close() {}, stop() {}, focus() {}, blur() {}, find() { return false; },
  scrollTo(x, y) { scrollWindow(x, y, false); }, scroll(x, y) { scrollWindow(x, y, false); }, scrollBy(x, y) { scrollWindow(x, y, true); },
  getComputedStyle: C.getComputedStyle, matchMedia: C.matchMedia, getSelection: () => C.selection,
  requestAnimationFrame, cancelAnimationFrame, requestIdleCallback, cancelIdleCallback, setTimeout, setInterval, clearTimeout, clearInterval: clearTimeout,
  fetch, postMessage(data) { setTimeout(() => fire(globalThis, new C.MessageEvent('message', { data: structuredClone(data), origin: location.origin, source: globalThis })), 0); },
  reportError: (e) => C.reportError(e),
  captureEvents() {}, releaseEvents() {},
  moveTo() {}, moveBy() {}, resizeTo() {}, resizeBy() {},
  webkitRequestAnimationFrame: requestAnimationFrame, webkitCancelAnimationFrame: cancelAnimationFrame,
  createImageBitmap: () => Promise.reject(domError('InvalidStateError', 'Not supported.')),
  queueMicrotask: globalThis.queueMicrotask, structuredClone: globalThis.structuredClone,
  getScreenDetails: () => Promise.reject(domError('NotAllowedError', 'Not supported.')),
};
for (const k of Object.keys(globals)) defineGlobal(k, globals[k]);
const ctors = {
  Event, CustomEvent: C.CustomEvent, UIEvent: C.UIEvent, MouseEvent: C.MouseEvent, PointerEvent: C.PointerEvent, WheelEvent: C.WheelEvent, DragEvent: C.DragEvent, KeyboardEvent: C.KeyboardEvent, InputEvent: C.InputEvent, FocusEvent: C.FocusEvent, CompositionEvent: C.CompositionEvent, ProgressEvent: C.ProgressEvent, PopStateEvent: C.PopStateEvent, HashChangeEvent: C.HashChangeEvent, PageTransitionEvent: C.PageTransitionEvent, BeforeUnloadEvent: C.BeforeUnloadEvent, SubmitEvent: C.SubmitEvent, FormDataEvent: C.FormDataEvent, TransitionEvent: C.TransitionEvent, AnimationEvent: C.AnimationEvent, ErrorEvent: C.ErrorEvent, PromiseRejectionEvent: C.PromiseRejectionEvent, MessageEvent: C.MessageEvent, StorageEvent: C.StorageEvent, ClipboardEvent: C.ClipboardEvent, TouchEvent: C.TouchEvent, ToggleEvent: C.ToggleEvent, CloseEvent: C.CloseEvent, SecurityPolicyViolationEvent: C.SecurityPolicyViolationEvent, MediaQueryListEvent: C.MediaQueryListEvent, EventTarget,
  Node: C.Node, CharacterData: C.CharacterData, Text: C.Text, CDATASection: C.CDATASection, Comment: C.Comment, ProcessingInstruction: C.ProcessingInstruction, DocumentType: C.DocumentType, Attr: C.Attr, NamedNodeMap: C.NamedNodeMap, NodeList: C.NodeList, HTMLCollection: C.HTMLCollection, HTMLFormControlsCollection: C.HTMLFormControlsCollection, RadioNodeList: C.RadioNodeList, HTMLOptionsCollection: C.HTMLOptionsCollection, DOMTokenList: C.DOMTokenList, DOMStringMap: C.DOMStringMap, DOMRect: C.DOMRect, DOMRectReadOnly: C.DOMRectReadOnly, DOMRectList: C.DOMRectList, DOMPoint: C.DOMPoint, DOMPointReadOnly: C.DOMPointReadOnly, NodeFilter: C.NodeFilter, TreeWalker: C.TreeWalker, NodeIterator: C.NodeIterator, AbstractRange: C.AbstractRange, StaticRange: C.StaticRange, Range: C.Range, Selection: C.Selection,
  Element: C.Element, HTMLElement: C.HTMLElement, HTMLUnknownElement: C.HTMLUnknownElement, ElementInternals: C.ElementInternals, Animation: C.Animation, SVGElement: C.SVGElement, SVGGraphicsElement: C.SVGGraphicsElement, SVGSVGElement: C.SVGSVGElement, SVGGeometryElement: C.SVGGeometryElement, SVGPathElement: C.SVGPathElement, SVGUseElement: C.SVGUseElement, SVGAnimatedString: C.SVGAnimatedString, SVGAnimatedLength: C.SVGAnimatedLength,
  DocumentFragment: C.DocumentFragment, ShadowRoot: C.ShadowRoot, Document: C.Document, HTMLDocument: C.HTMLDocument, XMLDocument: C.XMLDocument, DOMImplementation: C.DOMImplementation, DOMParser: C.DOMParser, XMLSerializer: C.XMLSerializer, FontFaceSet: C.FontFaceSet, FontFace: C.FontFace, CustomElementRegistry: C.CustomElementRegistry,
  Image: C.Image, Option: C.Option, Audio: C.Audio, CanvasRenderingContext2D: C.CanvasRenderingContext2D, CanvasGradient: C.CanvasGradient, CanvasPattern: C.CanvasPattern, ImageData: C.ImageData, TextMetrics: C.TextMetrics, Path2D: C.Path2D, FileList: C.FileList,
  CSSStyleDeclaration: C.CSSStyleDeclaration, CSSRule: C.CSSRule, CSSStyleRule: C.CSSStyleRule, CSSGroupingRule: C.CSSGroupingRule, CSSConditionRule: C.CSSConditionRule, CSSMediaRule: C.CSSMediaRule, CSSSupportsRule: C.CSSSupportsRule, CSSLayerBlockRule: C.CSSLayerBlockRule, CSSLayerStatementRule: C.CSSLayerStatementRule, CSSImportRule: C.CSSImportRule, CSSFontFaceRule: C.CSSFontFaceRule, CSSPageRule: C.CSSPageRule, CSSNamespaceRule: C.CSSNamespaceRule, CSSKeyframeRule: C.CSSKeyframeRule, CSSKeyframesRule: C.CSSKeyframesRule, CSSRuleList: C.CSSRuleList, MediaList: C.MediaList, StyleSheet: C.StyleSheet, CSSStyleSheet: C.CSSStyleSheet, StyleSheetList: C.StyleSheetList, CSS: C.CSS, MediaQueryList: C.MediaQueryList,
  AbortController, AbortSignal, Blob, File, FormData, Headers, Request, Response, XMLHttpRequest, XMLHttpRequestEventTarget, XMLHttpRequestUpload, WebSocket, EventSource, MessagePort, MessageChannel, BroadcastChannel, MutationObserver, MutationRecord, ResizeObserver, ResizeObserverEntry, IntersectionObserver, IntersectionObserverEntry, PerformanceObserver, ReportingObserver, Storage, Window,
};
for (const k of Object.keys(ctors)) defineGlobal(k, ctors[k]);
for (const k of Object.keys(C.classes)) defineGlobal(k, C.classes[k]);
for (const [k, v] of Object.entries(ctors).concat(Object.entries(C.classes))) { if (typeof v === 'function' && v.prototype && !Object.getOwnPropertyDescriptor(v.prototype, Symbol.toStringTag)) Object.defineProperty(v.prototype, Symbol.toStringTag, { value: k, configurable: true }); }
for (const k of ['setImmediate', 'clearImmediate', 'process', 'require', 'module', 'exports', 'global', 'Buffer']) { try { delete globalThis[k]; } catch (e) { /* ignore */ } }

// Wrapper prototypes for the natives.
const protos = { Node: C.Node.prototype, Element: C.Element.prototype, HTMLElement: C.HTMLElement.prototype, CharacterData: C.CharacterData.prototype, '#text': C.Text.prototype, '#comment': C.Comment.prototype, '#document': C.HTMLDocument.prototype, '#fragment': C.DocumentFragment.prototype, '#doctype': C.DocumentType.prototype, '*element': C.Element.prototype, '*svg': C.SVGElement.prototype, 'svg:svg': C.SVGSVGElement.prototype, 'svg:path': C.SVGPathElement.prototype, 'svg:use': C.SVGUseElement.prototype, '*unknown': C.HTMLUnknownElement.prototype, '*custom': C.HTMLElement.prototype, NodeList: C.NodeList.prototype, HTMLCollection: C.HTMLCollection.prototype, HTMLFormControlsCollection: C.HTMLFormControlsCollection.prototype, HTMLOptionsCollection: C.HTMLOptionsCollection.prototype, RadioNodeList: C.RadioNodeList.prototype, DOMTokenList: C.DOMTokenList.prototype, DOMStringMap: C.DOMStringMap.prototype, NamedNodeMap: C.NamedNodeMap.prototype, CSSStyleDeclaration: C.CSSStyleDeclaration.prototype, Storage: Storage.prototype, CSSRuleList: C.CSSRuleList.prototype, StyleSheetList: C.StyleSheetList.prototype };
for (const t of ['g', 'rect', 'circle', 'ellipse', 'line', 'polyline', 'polygon', 'text', 'tspan', 'image', 'defs', 'symbol', 'clipPath', 'mask', 'foreignObject', 'a', 'switch']) protos['svg:' + t] = t === 'defs' || t === 'symbol' || t === 'clipPath' || t === 'mask' ? C.SVGElement.prototype : C.SVGGraphicsElement.prototype;
for (const tag of Object.keys(C.tags)) protos[tag] = C.tags[tag].prototype;
W.registerProtos(protos);
W.installParentNode(C.Document.prototype);
W.installParentNode(C.DocumentFragment.prototype);
// Storage objects and the document were created before the prototypes registered.
Object.setPrototypeOf(globalThis.localStorage, Storage.prototype);
Object.setPrototypeOf(globalThis.sessionStorage, Storage.prototype);
Object.setPrototypeOf(document, C.HTMLDocument.prototype);
})();
