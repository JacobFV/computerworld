'use strict';
// http (and, through https.js, https) over the simulated network. Requests go
// through the world's network like any client of this machine: DNS, routes,
// service availability, failures, and the simulated latency (the response
// arrives as an I/O completion at that time). Servers created here serve
// requests from this same program (loopback).

const EventEmitter = require('events');
const { Readable } = require('stream');
const wire = require('internal/httpwire');

const STATUS_CODES = {
  100: 'Continue', 101: 'Switching Protocols', 102: 'Processing', 103: 'Early Hints',
  200: 'OK', 201: 'Created', 202: 'Accepted', 203: 'Non-Authoritative Information', 204: 'No Content',
  205: 'Reset Content', 206: 'Partial Content', 207: 'Multi-Status', 208: 'Already Reported', 226: 'IM Used',
  300: 'Multiple Choices', 301: 'Moved Permanently', 302: 'Found', 303: 'See Other', 304: 'Not Modified',
  305: 'Use Proxy', 307: 'Temporary Redirect', 308: 'Permanent Redirect',
  400: 'Bad Request', 401: 'Unauthorized', 402: 'Payment Required', 403: 'Forbidden', 404: 'Not Found',
  405: 'Method Not Allowed', 406: 'Not Acceptable', 407: 'Proxy Authentication Required', 408: 'Request Timeout',
  409: 'Conflict', 410: 'Gone', 411: 'Length Required', 412: 'Precondition Failed', 413: 'Payload Too Large',
  414: 'URI Too Long', 415: 'Unsupported Media Type', 416: 'Range Not Satisfiable', 417: 'Expectation Failed',
  418: "I'm a Teapot", 421: 'Misdirected Request', 422: 'Unprocessable Entity', 423: 'Locked',
  424: 'Failed Dependency', 425: 'Too Early', 426: 'Upgrade Required', 428: 'Precondition Required',
  429: 'Too Many Requests', 431: 'Request Header Fields Too Large', 451: 'Unavailable For Legal Reasons',
  500: 'Internal Server Error', 501: 'Not Implemented', 502: 'Bad Gateway', 503: 'Service Unavailable',
  504: 'Gateway Timeout', 505: 'HTTP Version Not Supported', 506: 'Variant Also Negotiates',
  507: 'Insufficient Storage', 508: 'Loop Detected', 509: 'Bandwidth Limit Exceeded', 510: 'Not Extended',
  511: 'Network Authentication Required',
};

const METHODS = ['ACL', 'BIND', 'CHECKOUT', 'CONNECT', 'COPY', 'DELETE', 'GET', 'HEAD', 'LINK', 'LOCK',
  'M-SEARCH', 'MERGE', 'MKACTIVITY', 'MKCALENDAR', 'MKCOL', 'MOVE', 'NOTIFY', 'OPTIONS', 'PATCH', 'POST',
  'PROPFIND', 'PROPPATCH', 'PURGE', 'PUT', 'QUERY', 'REBIND', 'REPORT', 'SEARCH', 'SOURCE', 'SUBSCRIBE',
  'TRACE', 'UNBIND', 'UNLINK', 'UNLOCK', 'UNSUBSCRIBE'];

const noBodyByDefault = new Set(['GET', 'HEAD', 'DELETE', 'OPTIONS', 'TRACE', 'CONNECT']);

function validateHeaderName(name) {
  if (typeof name !== 'string' || !/^[\^_`a-zA-Z\-0-9!#$%&'*+.|~]+$/.test(name)) {
    const e = new TypeError(`Header name must be a valid HTTP token ["${name}"]`);
    e.code = 'ERR_INVALID_HTTP_TOKEN';
    throw e;
  }
}
function validateHeaderValue(name, value) {
  if (value === undefined) {
    const e = new TypeError(`Invalid value "${value}" for header "${name}"`);
    e.code = 'ERR_HTTP_INVALID_HEADER_VALUE';
    throw e;
  }
  if (/[^\t\x20-\x7e\x80-\xff]/.test(String(value))) {
    const e = new TypeError(`Invalid character in header content ["${name}"]`);
    e.code = 'ERR_INVALID_CHAR';
    throw e;
  }
}

class Agent extends EventEmitter {
  constructor(options = {}) {
    super();
    this.options = { path: null, ...options };
    this.keepAlive = options.keepAlive !== undefined ? !!options.keepAlive : false;
    this.keepAliveMsecs = options.keepAliveMsecs || 1000;
    this.maxSockets = options.maxSockets || Infinity;
    this.maxFreeSockets = options.maxFreeSockets || 256;
    this.maxTotalSockets = options.maxTotalSockets || Infinity;
    this.defaultPort = options.defaultPort || 80;
    this.protocol = options.protocol || 'http:';
    this.requests = {};
    this.sockets = {};
    this.freeSockets = {};
  }
  destroy() {}
  getName(o = {}) { return `${o.host || 'localhost'}:${o.port || ''}:${o.localAddress || ''}`; }
}
const globalAgent = new Agent({ keepAlive: true, scheduling: 'lifo', timeout: 5000 });

class OutgoingMessage extends EventEmitter {
  constructor() {
    super();
    this._headers = new Map();
    this.headersSent = false;
    this.finished = false;
    this.writableEnded = false;
    this.writableFinished = false;
    this.destroyed = false;
    this._chunks = [];
    this._wrote = false;
    this.sendDate = false;
  }
  setHeader(name, value) {
    if (this.headersSent) {
      const e = new Error('Cannot set headers after they are sent to the client');
      e.code = 'ERR_HTTP_HEADERS_SENT';
      throw e;
    }
    validateHeaderName(name);
    validateHeaderValue(name, value);
    this._headers.set(name.toLowerCase(), [name, value]);
    return this;
  }
  appendHeader(name, value) {
    const k = name.toLowerCase();
    const cur = this._headers.get(k);
    if (!cur) return this.setHeader(name, value);
    const vals = Array.isArray(cur[1]) ? cur[1] : [cur[1]];
    this._headers.set(k, [cur[0], vals.concat(value)]);
    return this;
  }
  setHeaders(headers) {
    for (const [k, v] of (headers instanceof Map ? headers : Object.entries(headers))) this.setHeader(k, v);
    return this;
  }
  getHeader(name) { const h = this._headers.get(String(name).toLowerCase()); return h ? h[1] : undefined; }
  getHeaders() {
    const o = Object.create(null);
    for (const [k, [, v]] of this._headers) o[k] = v;
    return o;
  }
  getHeaderNames() { return [...this._headers.keys()]; }
  getRawHeaderNames() { return [...this._headers.values()].map((x) => x[0]); }
  hasHeader(name) { return this._headers.has(String(name).toLowerCase()); }
  removeHeader(name) {
    if (this.headersSent) {
      const e = new Error('Cannot remove headers after they are sent to the client');
      e.code = 'ERR_HTTP_HEADERS_SENT';
      throw e;
    }
    this._headers.delete(String(name).toLowerCase());
  }
  _rawHeaders() {
    const out = [];
    for (const [, [k, v]] of this._headers) {
      if (Array.isArray(v)) for (const x of v) out.push(k, String(x));
      else out.push(k, String(v));
    }
    return out;
  }
  write(chunk, encoding, cb) {
    if (typeof encoding === 'function') { cb = encoding; encoding = undefined; }
    if (this.writableEnded) {
      const e = new Error('write after end');
      e.code = 'ERR_STREAM_WRITE_AFTER_END';
      process.nextTick(() => this.emit('error', e));
      return false;
    }
    if (chunk === undefined || chunk === null) {
      const e = new TypeError('The "chunk" argument must be of type string or an instance of Buffer, TypedArray, or DataView. Received ' + chunk);
      e.code = 'ERR_INVALID_ARG_TYPE';
      throw e;
    }
    this._wrote = true;
    this._chunks.push(typeof chunk === 'string' ? Buffer.from(chunk, encoding || 'utf8') : Buffer.from(chunk));
    if (cb) process.nextTick(cb);
    return true;
  }
  _body() { return Buffer.concat(this._chunks); }
  flushHeaders() {}
  cork() {}
  uncork() {}
  setTimeout(ms, cb) { this._timeoutMs = ms; if (cb) this.once('timeout', cb); return this; }
}

class IncomingMessage extends Readable {
  constructor(socket) {
    super({});
    this.socket = socket || null;
    // The deprecated alias Express still reads (`req.protocol`).
    this.connection = this.socket;
    this.httpVersionMajor = 1;
    this.httpVersionMinor = 1;
    this.httpVersion = '1.1';
    this.complete = false;
    this.rawHeaders = [];
    this.rawTrailers = [];
    this.headers = {};
    this.trailers = {};
    this.aborted = false;
    this.upgrade = false;
    this.url = '';
    this.method = null;
    this.statusCode = null;
    this.statusMessage = null;
  }
  get headersDistinct() {
    const o = Object.create(null);
    for (let i = 0; i + 1 < this.rawHeaders.length; i += 2) {
      const k = this.rawHeaders[i].toLowerCase();
      (o[k] || (o[k] = [])).push(this.rawHeaders[i + 1]);
    }
    return o;
  }
  setTimeout(ms, cb) { if (cb) this.once('timeout', cb); return this; }
  _deliver(body) {
    if (body && body.length) this.push(body);
    this.push(null);
    this.on('end', () => { this.complete = true; });
  }
}

function fakeSocket(localPort, remote) {
  const s = new EventEmitter();
  s.remoteAddress = remote.address;
  s.remotePort = remote.port;
  s.remoteFamily = 'IPv4';
  s.localAddress = remote.local || wire.localAddress;
  s.localPort = localPort;
  s.encrypted = remote.encrypted || undefined;
  s.setTimeout = () => s;
  s.setNoDelay = () => s;
  s.setKeepAlive = () => s;
  s.destroy = () => s;
  s.end = () => s;
  s.ref = () => s;
  s.unref = () => s;
  s.address = () => ({ address: s.localAddress, family: 'IPv4', port: localPort });
  return s;
}

let portCounter = 40000;

function parseRequestArgs(input, options, cb, defaults) {
  if (typeof input === 'string') input = new URL(input);
  let opts = {};
  if (input && typeof input === 'object' && (input instanceof URL || typeof input.href === 'string' && input.searchParams)) {
    opts = {
      protocol: input.protocol, hostname: input.hostname.startsWith('[') ? input.hostname.slice(1, -1) : input.hostname,
      port: input.port ? Number(input.port) : undefined, path: `${input.pathname || '/'}${input.search || ''}`,
    };
    if (input.username || input.password) opts.auth = `${decodeURIComponent(input.username)}:${decodeURIComponent(input.password)}`;
  } else if (typeof input === 'function') {
    cb = input;
    input = {};
  } else {
    cb = options;
    options = input;
    input = null;
  }
  if (typeof options === 'function') { cb = options; options = {}; }
  opts = Object.assign({}, opts, options || {});
  if (opts.protocol && opts.protocol !== defaults.protocol) {
    const e = new TypeError(`Protocol "${opts.protocol}" not supported. Expected "${defaults.protocol}"`);
    e.code = 'ERR_INVALID_PROTOCOL';
    throw e;
  }
  return [opts, cb];
}

class ClientRequest extends OutgoingMessage {
  constructor(input, options, cb, defaults = { protocol: 'http:', port: 80, encrypted: false }) {
    super();
    const [opts, callback] = parseRequestArgs(input, options, cb, defaults);
    this._defaults = defaults;
    this.agent = opts.agent === undefined ? (defaults.agent || globalAgent) : opts.agent;
    this.method = String(opts.method || 'GET').toUpperCase();
    if (!/^[!#$%&'*+\-.^_`|~0-9A-Za-z]+$/.test(this.method)) {
      const e = new TypeError(`Method must be a valid HTTP token ["${opts.method}"]`);
      e.code = 'ERR_INVALID_HTTP_TOKEN';
      throw e;
    }
    this.protocol = defaults.protocol;
    this.host = opts.hostname || (opts.host ? String(opts.host).replace(/:\d+$/, '') : 'localhost');
    this.port = Number(opts.port || defaults.port);
    this.path = opts.path || '/';
    if (/[ - ]/.test(this.path)) {
      const e = new TypeError('Request path contains unescaped characters');
      e.code = 'ERR_UNESCAPED_CHARACTERS';
      throw e;
    }
    this.timeout = opts.timeout;
    this.reusedSocket = false;
    this.maxHeadersCount = null;
    this.aborted = false;
    if (opts.headers) {
      if (Array.isArray(opts.headers)) {
        for (let i = 0; i + 1 < opts.headers.length; i += 2) this.appendHeader(opts.headers[i], opts.headers[i + 1]);
      } else {
        for (const k of Object.keys(opts.headers)) this.setHeader(k, opts.headers[k]);
      }
    }
    if (!this.hasHeader('host') && opts.setHost !== false) {
      let h = this.host.includes(':') ? `[${this.host}]` : this.host;
      if (this.port !== defaults.port) h += `:${this.port}`;
      this.setHeader('Host', h);
    }
    if (opts.auth && !this.hasHeader('authorization')) {
      this.setHeader('Authorization', 'Basic ' + Buffer.from(opts.auth).toString('base64'));
    }
    if (callback) this.once('response', callback);
    if (opts.signal) {
      const onAbort = () => this.destroy(Object.assign(new Error('The operation was aborted'), { name: 'AbortError', code: 'ABORT_ERR' }));
      if (opts.signal.aborted) process.nextTick(onAbort);
      else opts.signal.addEventListener('abort', onAbort, { once: true });
    }
    this.socket = null;
    process.nextTick(() => {
      if (this.destroyed) return;
      this.socket = fakeSocket(++portCounter, { address: this.host, port: this.port, encrypted: defaults.encrypted });
      this.connection = this.socket;
      this.emit('socket', this.socket);
    });
  }
  abort() {
    if (this.aborted) return;
    this.aborted = true;
    process.nextTick(() => this.emit('abort'));
    this.destroy();
  }
  destroy(err) {
    if (this.destroyed) return this;
    this.destroyed = true;
    if (this._res && !this._res.complete) {
      this._res.aborted = true;
      const e = new Error('aborted');
      e.code = 'ECONNRESET';
      process.nextTick(() => { this._res.emit('aborted'); this._res.emit('error', e); this._res.emit('close'); });
    } else if (!this._res) {
      if (!err && this.writableEnded) {
        err = new Error('socket hang up');
        err.code = 'ECONNRESET';
      }
      process.nextTick(() => {
        if (err) this.emit('error', err);
        this.emit('close');
      });
    }
    return this;
  }
  end(chunk, encoding, cb) {
    if (typeof chunk === 'function') { cb = chunk; chunk = undefined; }
    if (typeof encoding === 'function') { cb = encoding; encoding = undefined; }
    if (this.writableEnded) return this;
    let contentLength = null;
    if (chunk !== undefined && chunk !== null) {
      if (!this._wrote) contentLength = typeof chunk === 'string' ? Buffer.byteLength(chunk, encoding || 'utf8') : chunk.length;
      this.write(chunk, encoding);
    } else if (!this._wrote) {
      contentLength = 0;
    }
    this.writableEnded = true;
    this.finished = true;
    const raw = this._rawHeaders();
    const lower = new Set(raw.filter((_, i) => i % 2 === 0).map((k) => k.toLowerCase()));
    if (!lower.has('connection')) raw.push('Connection', this.agent && this.agent.keepAlive === false ? 'close' : 'keep-alive');
    if (!lower.has('content-length') && !lower.has('transfer-encoding')) {
      if (!noBodyByDefault.has(this.method)) {
        if (contentLength !== null) raw.push('Content-Length', String(contentLength));
        else raw.push('Transfer-Encoding', 'chunked');
      } else if (this._wrote && contentLength === null) {
        raw.push('Transfer-Encoding', 'chunked');
      } else if (contentLength) {
        raw.push('Content-Length', String(contentLength));
      }
    }
    this.headersSent = true;
    this._sentRaw = raw;
    const body = this._body();
    process.nextTick(() => {
      this.writableFinished = true;
      this.emit('finish');
      if (cb) cb();
    });
    setImmediate(() => this._dispatch(raw, body));
    return this;
  }
  _dispatch(raw, body) {
    if (this.destroyed) return;
    const r = wire.resolveAddress(this.host);
    const server = wire.servers.get(this.port);
    if (!r.error && wire.isLocal(r.address) && server && server._http && server.listening) {
      server._handle(this, raw, body);
      return;
    }
    if (!r.error && wire.isLocal(r.address)) {
      return this._fail(wire.connectError({ code: 'ECONNREFUSED', errno: -111 }, r.address === '0.0.0.0' ? '127.0.0.1' : r.address, this.port));
    }
    const host = this.host.includes(':') ? `[${this.host}]` : this.host;
    const url = `${this.protocol}//${host}:${this.port}${this.path}`;
    const res = binding.httpRequest(this.method, url, raw, body, 0);
    if (res.error) {
      binding.scheduleIo(() => this._fail(wire.exchangeError(res.error, this.host, this.port)), res.elapsedMs);
      return;
    }
    const heads = res.headers.slice();
    if (!heads.some((k, i) => i % 2 === 0 && /^content-length$/i.test(k))) heads.push('content-length', String(res.body.length));
    const deliver = () => this._respond(res.status, STATUS_CODES[res.status] || 'unknown', heads, this.method === 'HEAD' ? Buffer.alloc(0) : res.body);
    if (this._timeoutMs && res.elapsedMs > this._timeoutMs || this.timeout && res.elapsedMs > this.timeout) {
      const t = Math.min(this._timeoutMs || Infinity, this.timeout || Infinity);
      binding.scheduleIo(() => this.emit('timeout'), t);
    }
    binding.scheduleIo(() => { if (!this.destroyed) deliver(); }, res.elapsedMs);
  }
  _fail(err) {
    if (this.destroyed) return;
    this.destroyed = true;
    this.emit('error', err);
    this.emit('close');
  }
  _respond(status, message, raw, body) {
    const res = new IncomingMessage(this.socket);
    res.statusCode = status;
    res.statusMessage = message;
    res.rawHeaders = raw;
    res.headers = wire.headersObject(raw);
    res.req = this;
    this.res = res;
    this._res = res;
    const listened = this.emit('response', res);
    if (!listened) res.resume();
    res._deliver(body);
    res.on('end', () => {
      process.nextTick(() => { this.destroyed = true; res.emit('close'); this.emit('close'); });
    });
  }
  setNoDelay() {}
  setSocketKeepAlive() {}
}

class ServerResponse extends OutgoingMessage {
  constructor(req) {
    super();
    this.req = req;
    this.statusCode = 200;
    this.statusMessage = undefined;
    this.sendDate = true;
    this.strictContentLength = false;
  }
  writeHead(statusCode, reason, headers) {
    if (typeof reason !== 'string') { headers = reason; reason = undefined; }
    this.statusCode = statusCode;
    if (reason) this.statusMessage = reason;
    if (headers) {
      if (Array.isArray(headers)) for (let i = 0; i + 1 < headers.length; i += 2) this.appendHeader(headers[i], headers[i + 1]);
      else for (const k of Object.keys(headers)) this.setHeader(k, headers[k]);
    }
    // The headers are fixed from here on, as when Node stores them.
    this._header = true;
    this.headersSent = true;
    return this;
  }
  // What `write` and `end` call when no one has written the head yet; middleware
  // (`on-headers`, `compression`) wraps `writeHead` and calls this itself.
  _implicitHeader() { this.writeHead(this.statusCode); }
  write(chunk, encoding, cb) {
    if (!this._header && !this.writableEnded) this._implicitHeader();
    return super.write(chunk, encoding, cb);
  }
  writeContinue() {}
  writeProcessing() {}
  end(chunk, encoding, cb) {
    if (typeof chunk === 'function') { cb = chunk; chunk = undefined; }
    if (typeof encoding === 'function') { cb = encoding; encoding = undefined; }
    if (this.writableEnded) return this;
    let contentLength = null;
    if (chunk !== undefined && chunk !== null) {
      if (!this._wrote) contentLength = typeof chunk === 'string' ? Buffer.byteLength(chunk, encoding || 'utf8') : chunk.length;
      // Node's own write, not one middleware may have put on the response.
      OutgoingMessage.prototype.write.call(this, chunk, encoding);
    } else if (!this._wrote) {
      contentLength = 0;
    }
    if (!this._header) this._implicitHeader();
    this.writableEnded = true;
    this.finished = true;
    this.headersSent = true;
    const raw = this._rawHeaders();
    const lower = new Set(raw.filter((_, i) => i % 2 === 0).map((k) => k.toLowerCase()));
    if (this.sendDate && !lower.has('date')) raw.push('Date', new Date().toUTCString());
    if (!lower.has('connection')) raw.push('Connection', 'keep-alive', 'Keep-Alive', 'timeout=5');
    const noBody = this.statusCode === 204 || this.statusCode === 304 || (this.statusCode >= 100 && this.statusCode < 200);
    if (!noBody && !lower.has('content-length') && !lower.has('transfer-encoding')) {
      if (contentLength !== null) raw.push('Content-Length', String(contentLength));
      else raw.push('Transfer-Encoding', 'chunked');
    }
    const body = this._body();
    process.nextTick(() => {
      this.writableFinished = true;
      this.emit('finish');
      if (cb) cb();
      this.emit('close');
    });
    if (this._onEnd) this._onEnd(this.statusCode, this.statusMessage || STATUS_CODES[this.statusCode] || 'unknown', raw, body);
    return this;
  }
}

class Server extends EventEmitter {
  constructor(options, requestListener) {
    super();
    if (typeof options === 'function') { requestListener = options; options = {}; }
    if (requestListener) this.on('request', requestListener);
    this._http = true;
    this.listening = false;
    this.timeout = 0;
    this.keepAliveTimeout = 5000;
    this.headersTimeout = 60000;
    this.requestTimeout = 300000;
    this.maxHeadersCount = null;
    this._port = null;
  }
  listen(...args) {
    return require('net').Server.prototype.listen.apply(this, args);
  }
  address() { return require('net').Server.prototype.address.call(this); }
  close(cb) { return require('net').Server.prototype.close.call(this, cb); }
  closeAllConnections() {}
  closeIdleConnections() {}
  setTimeout(ms, cb) { this.timeout = ms; if (cb) this.on('timeout', cb); return this; }
  ref() { return this; }
  unref() { return this; }
  _handle(clientReq, raw, body) {
    const sock = fakeSocket(this._port, { address: '127.0.0.1', port: ++portCounter, local: '127.0.0.1' });
    const req = new IncomingMessage(sock);
    req.method = clientReq.method;
    req.url = clientReq.path;
    req.rawHeaders = raw;
    req.headers = wire.headersObject(raw);
    const res = new ServerResponse(req);
    res.socket = sock;
    res._onEnd = (status, message, rawOut, out) => {
      setImmediate(() => clientReq._respond(status, message, rawOut, clientReq.method === 'HEAD' ? Buffer.alloc(0) : out));
    };
    this.emit('connection', sock);
    this.emit('request', req, res);
    req._deliver(body);
  }
}

function createServer(options, listener) { return new Server(options, listener); }
function request(input, options, cb) { return new ClientRequest(input, options, cb); }
function get(input, options, cb) {
  const req = request(input, options, cb);
  req.end();
  return req;
}

module.exports = {
  METHODS, STATUS_CODES, Agent, ClientRequest, IncomingMessage, OutgoingMessage, Server, ServerResponse,
  createServer, request, get, globalAgent, validateHeaderName, validateHeaderValue,
  maxHeaderSize: 16384, setMaxIdleHTTPParsers() {},
};
