'use strict';
// A small, synchronous-ish subset of node:stream.

const EventEmitter = require('events');

// The constructors are plain functions, as Node's are: packages written against
// Node's streams subclass them the ES5 way (`Stream.call(this)` and
// `util.inherits`, as `send` under express.static does), and ES6 `class ...
// extends Readable` works on them all the same.
function inherit(ctor, base) {
  Object.setPrototypeOf(ctor.prototype, base.prototype);
  Object.setPrototypeOf(ctor, base);
}

function Stream(opts) {
  EventEmitter.call(this, opts);
}
inherit(Stream, EventEmitter);
Stream.prototype.pipe = function pipe(dest) {
  this.on('data', (chunk) => dest.write(chunk));
  this.on('end', () => { if (typeof dest.end === 'function' && dest !== process.stdout && dest !== process.stderr) dest.end(); });
  return dest;
};

function Readable(opts = {}) {
  if (!(this instanceof Readable)) return new Readable(opts);
  Stream.call(this, opts);
  opts = opts || {};
  this._buffer = [];
  this._ended = false;
  this._flowing = false;
  this._objectMode = !!opts.objectMode;
  this._encoding = opts.encoding || null;
  if (typeof opts.read === 'function') this._read = opts.read;
  this.readable = true;
}
inherit(Readable, Stream);
Readable.from = function from(iterable, opts) {
  const r = new Readable({ objectMode: true, ...opts });
  (async () => {
    for await (const x of iterable) r.push(x);
    r.push(null);
  })();
  return r;
};
Object.assign(Readable.prototype, {
  _read() {},
  push(chunk) {
    this._readPending = false;
    if (chunk === null) {
      this._ended = true;
      setImmediate(() => this._flush());
      return false;
    }
    this._buffer.push(chunk);
    setImmediate(() => this._flush());
    return true;
  },
  _flush() {
    if (!this._flowing && this.listenerCount('data') === 0) return;
    while (this._buffer.length) {
      let c = this._buffer.shift();
      if (this._encoding && typeof c !== 'string') c = c.toString(this._encoding);
      this.emit('data', c);
    }
    if (this._ended && !this._endEmitted) {
      this._endEmitted = true;
      this.readable = false;
      this.emit('end');
      this.emit('close');
    } else if (!this._ended && this._flowing) {
      this._pull();
    }
  },
  // Ask for more, as Node does whenever a flowing stream's buffer runs dry:
  // `_read` is called again once what it last pushed has been delivered.
  _pull() {
    if (this._readPending) return;
    this._readPending = true;
    this._read();
  },
  on(ev, fn) {
    EventEmitter.prototype.on.call(this, ev, fn);
    if (ev === 'data') {
      this._flowing = true;
      this._pull();
      setImmediate(() => this._flush());
    }
    return this;
  },
  setEncoding(e) { this._encoding = e; return this; },
  resume() { this._flowing = true; setImmediate(() => this._flush()); return this; },
  pause() { this._flowing = false; return this; },
  read() { return this._buffer.length ? this._buffer.shift() : null; },
  destroy(err) { if (err) this.emit('error', err); this.emit('close'); return this; },
  async *[Symbol.asyncIterator]() {
    const chunks = [];
    let done = false;
    let wake = null;
    this.on('data', (c) => { chunks.push(c); if (wake) { wake(); wake = null; } });
    this.on('end', () => { done = true; if (wake) { wake(); wake = null; } });
    while (true) {
      if (chunks.length) { yield chunks.shift(); continue; }
      if (done) return;
      await new Promise((r) => { wake = r; });
    }
  },
});
Readable.prototype.addListener = Readable.prototype.on;

function Writable(opts = {}) {
  if (!(this instanceof Writable)) return new Writable(opts);
  Stream.call(this, opts);
  opts = opts || {};
  if (typeof opts.write === 'function') this._write = opts.write;
  if (typeof opts.final === 'function') this._final = opts.final;
  this.writable = true;
}
inherit(Writable, Stream);
Object.assign(Writable.prototype, {
  _write(chunk, enc, cb) { cb(); },
  write(chunk, enc, cb) {
    if (typeof enc === 'function') { cb = enc; enc = 'utf8'; }
    this._write(chunk, enc || 'utf8', (err) => {
      if (err) this.emit('error', err);
      if (cb) cb(err);
    });
    return true;
  },
  end(chunk, enc, cb) {
    if (typeof chunk === 'function') { cb = chunk; chunk = undefined; }
    if (chunk !== undefined && chunk !== null) this.write(chunk, enc);
    const done = () => {
      this.writable = false;
      this.emit('finish');
      this.emit('close');
      if (cb) cb();
    };
    if (this._final) this._final(done); else setImmediate(done);
    return this;
  },
  destroy() { this.emit('close'); return this; },
});

function Duplex(opts = {}) {
  if (!(this instanceof Duplex)) return new Duplex(opts);
  Readable.call(this, opts);
  this._w = new Writable(opts);
  this.writable = true;
}
inherit(Duplex, Readable);
Object.assign(Duplex.prototype, {
  write(chunk, enc, cb) { return this._w.write.call(this, chunk, enc, cb); },
  _write(chunk, enc, cb) { cb(); },
  end(chunk, enc, cb) {
    if (chunk !== undefined && chunk !== null && typeof chunk !== 'function') this.write(chunk, enc);
    if (typeof this._flushTransform === 'function') this._flushTransform();
    else this.push(null);
    if (typeof cb === 'function') cb();
    return this;
  },
});

function Transform(opts = {}) {
  if (!(this instanceof Transform)) return new Transform(opts);
  Duplex.call(this, opts);
  opts = opts || {};
  if (typeof opts.transform === 'function') this._transform = opts.transform;
  if (typeof opts.flush === 'function') this._flushFn = opts.flush;
}
inherit(Transform, Duplex);
Object.assign(Transform.prototype, {
  _transform(chunk, enc, cb) { cb(null, chunk); },
  _write(chunk, enc, cb) {
    this._transform(chunk, enc, (err, out) => {
      if (out !== undefined && out !== null) this.push(out);
      cb(err);
    });
  },
  _flushTransform() {
    if (this._flushFn) this._flushFn((err, out) => { if (out !== undefined && out !== null) this.push(out); this.push(null); });
    else this.push(null);
  },
});

function PassThrough(opts) {
  if (!(this instanceof PassThrough)) return new PassThrough(opts);
  Transform.call(this, opts);
}
inherit(PassThrough, Transform);

function pipeline(...streams) {
  const cb = typeof streams[streams.length - 1] === 'function' ? streams.pop() : null;
  for (let i = 0; i < streams.length - 1; i++) streams[i].pipe(streams[i + 1]);
  const last = streams[streams.length - 1];
  if (cb) {
    last.on('finish', () => cb(null));
    last.on('end', () => cb(null));
    last.on('error', cb);
  }
  return last;
}

function finished(stream, cb) {
  stream.on('end', () => cb(null));
  stream.on('finish', () => cb(null));
  stream.on('error', cb);
}

Stream.Readable = Readable;
Stream.Writable = Writable;
Stream.Duplex = Duplex;
Stream.Transform = Transform;
Stream.PassThrough = PassThrough;
Stream.Stream = Stream;
Stream.pipeline = pipeline;
Stream.finished = finished;
Stream.promises = {
  pipeline: (...s) => new Promise((res, rej) => pipeline(...s, (e) => (e ? rej(e) : res()))),
  finished: (s) => new Promise((res, rej) => finished(s, (e) => (e ? rej(e) : res()))),
};

module.exports = Stream;
