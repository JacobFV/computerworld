'use strict';
// net over the simulated network. A connection to another machine is accepted
// or refused by the world (DNS, routes, a listening service); every simulated
// service speaks HTTP, so the bytes written are parsed as HTTP/1.x requests,
// each is carried through the world's network, and the reply arrives as
// HTTP/1.1 bytes at the simulated time it takes. Servers created here accept
// connections from this same program (loopback).

const EventEmitter = require('events');
const wire = require('internal/httpwire');

let nextPort = 40000;
const ephemeral = () => ++nextPort;

function isIP(s) { return wire.isIPv4(s) ? 4 : wire.isIPv6(s) ? 6 : 0; }

class Socket extends EventEmitter {
  constructor(options = {}) {
    super();
    this.connecting = false;
    this.pending = true;
    this.destroyed = false;
    this.readable = false;
    this.writable = false;
    this.remoteAddress = undefined;
    this.remotePort = undefined;
    this.remoteFamily = undefined;
    this.localAddress = undefined;
    this.localPort = undefined;
    this.bytesRead = 0;
    this.bytesWritten = 0;
    this.timeout = undefined;
    this.allowHalfOpen = !!options.allowHalfOpen;
    this._encoding = null;
    this._flowing = null;
    this._queue = [];
    this._ended = false;
    this._endEmitted = false;
    this._peer = null;
    this._remote = null;
    this._out = Buffer.alloc(0);
    this._remoteClosed = false;
  }
  get readyState() {
    if (this.connecting) return 'opening';
    if (this.readable && this.writable) return 'open';
    if (this.readable) return 'readOnly';
    if (this.writable) return 'writeOnly';
    return 'closed';
  }
  connect(...args) {
    let opts = {};
    let cb;
    if (typeof args[args.length - 1] === 'function') cb = args.pop();
    if (typeof args[0] === 'object' && args[0] !== null) opts = args[0];
    else { opts.port = args[0]; opts.host = args[1]; }
    const port = Number(opts.port);
    const host = opts.host || 'localhost';
    if (cb) this.once('connect', cb);
    this.connecting = true;
    process.nextTick(() => this._doConnect(host, port));
    return this;
  }
  _doConnect(host, port) {
    const r = wire.resolveAddress(host);
    if (r.error) return this.destroy(r.error);
    const address = r.address;
    this.emit('lookup', null, address, 4, host);
    const server = wire.servers.get(port);
    if (wire.isLocal(address) && server && server.listening) {
      const peer = new Socket();
      this.localAddress = '127.0.0.1';
      this.localPort = ephemeral();
      peer.localAddress = '127.0.0.1';
      peer.localPort = port;
      peer.remoteAddress = this.localAddress;
      peer.remotePort = this.localPort;
      peer.remoteFamily = 'IPv4';
      this.remoteAddress = address === 'localhost' ? '127.0.0.1' : address;
      this.remotePort = port;
      this.remoteFamily = 'IPv4';
      this._peer = peer;
      peer._peer = this;
      peer._open();
      this._open();
      setImmediate(() => {
        server._connections++;
        server.emit('connection', peer);
        this.emit('connect');
        this.emit('ready');
      });
      return;
    }
    if (wire.isLocal(address)) {
      const e = wire.connectError({ code: 'ECONNREFUSED', errno: -111 }, address === 'localhost' ? '127.0.0.1' : address, port);
      return this.destroy(e);
    }
    const c = binding.tcpConnect(host, port);
    if (c.error) return this.destroy(wire.connectError(c.error, address, port));
    this.remoteAddress = c.remoteAddress;
    this.remotePort = c.remotePort;
    this.remoteFamily = 'IPv4';
    this.localAddress = c.localAddress;
    this.localPort = c.localPort;
    this._remote = { host, port };
    binding.scheduleIo(() => {
      this._open();
      this.emit('connect');
      this.emit('ready');
    }, c.elapsedMs);
  }
  _open() {
    this.connecting = false;
    this.pending = false;
    this.readable = true;
    this.writable = true;
  }
  setEncoding(enc) { this._encoding = enc || 'utf8'; return this; }
  setTimeout(ms, cb) {
    this.timeout = ms;
    if (cb) this.once('timeout', cb);
    return this;
  }
  setNoDelay() { return this; }
  setKeepAlive() { return this; }
  ref() { return this; }
  unref() { return this; }
  address() { return { address: this.localAddress, family: 'IPv4', port: this.localPort }; }
  pause() { this._flowing = false; return this; }
  resume() { this._flowing = true; setImmediate(() => this._drain()); return this; }
  isPaused() { return this._flowing === false; }
  on(ev, fn) {
    super.on(ev, fn);
    if (ev === 'data' && this._flowing !== false) { this._flowing = true; setImmediate(() => this._drain()); }
    return this;
  }
  _push(chunk) {
    if (chunk === null) this._ended = true;
    else { this.bytesRead += chunk.length; this._queue.push(chunk); }
    setImmediate(() => this._drain());
  }
  _drain() {
    if (!this._flowing) return;
    while (this._queue.length) {
      const c = this._queue.shift();
      this.emit('data', this._encoding ? c.toString(this._encoding) : c);
    }
    if (this._ended && !this._endEmitted) {
      this._endEmitted = true;
      this.readable = false;
      this.emit('end');
      if (!this.allowHalfOpen) this.end();
      if (!this.writable) this._close();
    }
  }
  read() { return this._queue.length ? this._queue.shift() : null; }
  write(data, encoding, cb) {
    if (typeof encoding === 'function') { cb = encoding; encoding = 'utf8'; }
    if (this.destroyed || this._writeEnded) {
      const e = new Error('This socket has been ended by the other party');
      e.code = 'EPIPE';
      process.nextTick(() => { this.emit('error', e); if (cb) cb(e); });
      return false;
    }
    const buf = typeof data === 'string' ? Buffer.from(data, encoding || 'utf8') : Buffer.from(data);
    this.bytesWritten += buf.length;
    if (this.connecting) {
      this.once('connect', () => this._send(buf));
    } else {
      this._send(buf);
    }
    if (cb) process.nextTick(cb);
    return true;
  }
  _send(buf) {
    if (this._peer) { this._peer._push(buf); return; }
    if (!this._remote) return;
    this._out = Buffer.concat([this._out, buf]);
    for (;;) {
      if (this._remoteClosed) return;
      const p = wire.parseMessage(this._out, true);
      if (!p) return;
      if (p.bad) {
        this._out = Buffer.alloc(0);
        this._remoteClosed = true;
        this._push(Buffer.from('HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n'));
        this._push(null);
        return;
      }
      this._out = p.rest;
      const m = p.msg;
      let host = m.headers.host || this._remote.host;
      if (!host.includes(':') && this._remote.port !== 80) host += `:${this._remote.port}`;
      const url = /^https?:\/\//.test(m.url) ? m.url : `http://${host}${m.url}`;
      const r = binding.httpRequest(m.method, url, m.raw, m.body, this.timeout || 0);
      const close = m.version === 'HTTP/1.0' || /close/i.test(m.headers.connection || '');
      if (r.error) {
        this._remoteClosed = true;
        binding.scheduleIo(() => {
          const e = new Error('read ECONNRESET');
          e.errno = -104; e.code = 'ECONNRESET'; e.syscall = 'read';
          this.destroy(e);
        }, r.elapsedMs);
        return;
      }
      const reason = require('http').STATUS_CODES[r.status] || 'Unknown';
      const body = m.method === 'HEAD' ? Buffer.alloc(0) : r.body;
      const bytes = wire.serializeResponse(r.status, reason, r.headers, body, close);
      if (close) this._remoteClosed = true;
      binding.scheduleIo(() => {
        this._push(bytes);
        if (close) this._push(null);
      }, r.elapsedMs);
      if (close) return;
    }
  }
  end(data, encoding, cb) {
    if (typeof data === 'function') { cb = data; data = undefined; }
    if (typeof encoding === 'function') { cb = encoding; encoding = undefined; }
    if (data !== undefined && data !== null) this.write(data, encoding);
    if (this._writeEnded) { if (cb) process.nextTick(cb); return this; }
    this._writeEnded = true;
    const finish = () => {
      this.writable = false;
      this.emit('finish');
      if (cb) cb();
      if (this._peer) this._peer._push(null);
      if (!this.readable || this._endEmitted) this._close();
    };
    if (this.connecting) this.once('connect', () => process.nextTick(finish));
    else process.nextTick(finish);
    return this;
  }
  _close(hadError = false) {
    if (this._closed) return;
    this._closed = true;
    this.destroyed = true;
    setImmediate(() => this.emit('close', hadError));
  }
  destroy(err) {
    if (this.destroyed) return this;
    this.destroyed = true;
    this.readable = false;
    this.writable = false;
    this.connecting = false;
    process.nextTick(() => {
      if (err) this.emit('error', err);
      this._closed = true;
      this.emit('close', !!err);
    });
    if (this._peer && !this._peer._ended) this._peer._push(null);
    return this;
  }
  destroySoon() { this.end(); }
  resetAndDestroy() { return this.destroy(); }
  pipe(dest) {
    this.on('data', (c) => dest.write(c));
    this.on('end', () => { if (dest !== process.stdout && dest !== process.stderr && typeof dest.end === 'function') dest.end(); });
    return dest;
  }
  async *[Symbol.asyncIterator]() {
    const chunks = [];
    let done = false;
    let wake = null;
    this.on('data', (c) => { chunks.push(c); if (wake) { wake(); wake = null; } });
    this.on('end', () => { done = true; if (wake) { wake(); wake = null; } });
    this.on('close', () => { done = true; if (wake) { wake(); wake = null; } });
    for (;;) {
      if (chunks.length) { yield chunks.shift(); continue; }
      if (done) return;
      await new Promise((r) => { wake = r; });
    }
  }
}

class Server extends EventEmitter {
  constructor(options, listener) {
    super();
    if (typeof options === 'function') { listener = options; options = {}; }
    if (listener) this.on('connection', listener);
    this.listening = false;
    this._port = null;
    this._host = null;
    this._connections = 0;
    this.maxConnections = undefined;
  }
  listen(...args) {
    let cb;
    if (typeof args[args.length - 1] === 'function') cb = args.pop();
    let port = 0;
    let host = '::';
    if (typeof args[0] === 'object' && args[0] !== null) { port = args[0].port || 0; host = args[0].host || host; }
    else { if (args[0] !== undefined) port = Number(args[0]); if (typeof args[1] === 'string') host = args[1]; }
    if (!port) port = ephemeral();
    if (wire.servers.has(port)) {
      const e = new Error(`listen EADDRINUSE: address already in use ${host === '::' ? ':::' : host + ':'}${port}`);
      e.code = 'EADDRINUSE'; e.errno = -98; e.syscall = 'listen'; e.address = host; e.port = port;
      process.nextTick(() => this.emit('error', e));
      return this;
    }
    this._port = port;
    this._host = host;
    wire.servers.set(port, this);
    this.listening = true;
    if (cb) this.once('listening', cb);
    process.nextTick(() => this.emit('listening'));
    return this;
  }
  address() {
    if (!this.listening) return null;
    return { address: this._host, family: this._host.includes(':') ? 'IPv6' : 'IPv4', port: this._port };
  }
  close(cb) {
    if (this.listening) wire.servers.delete(this._port);
    const was = this.listening;
    this.listening = false;
    process.nextTick(() => {
      if (!was) {
        const e = new Error('Server is not running.');
        e.code = 'ERR_SERVER_NOT_RUNNING';
        if (cb) cb(e);
        return;
      }
      if (cb) cb();
      this.emit('close');
    });
    return this;
  }
  getConnections(cb) { process.nextTick(cb, null, this._connections); return this; }
  ref() { return this; }
  unref() { return this; }
}

function createServer(options, listener) { return new Server(options, listener); }
function connect(...args) {
  const s = new Socket(typeof args[0] === 'object' ? args[0] : {});
  return s.connect(...args);
}

class BlockList {
  constructor() { this._rules = []; }
  addAddress(a) { this._rules.push(a); }
  check(a) { return this._rules.includes(a); }
}

class SocketAddress {
  constructor(o = {}) { this.address = o.address || '127.0.0.1'; this.port = o.port || 0; this.family = o.family || 'ipv4'; this.flowlabel = 0; }
}

module.exports = {
  Socket, Stream: Socket, Server, createServer, connect, createConnection: connect,
  isIP, isIPv4: wire.isIPv4, isIPv6: wire.isIPv6, BlockList, SocketAddress,
  getDefaultAutoSelectFamily: () => true, setDefaultAutoSelectFamily() {},
};
