'use strict';
// A small, synchronous-ish subset of node:stream.

const EventEmitter = require('events');

class Stream extends EventEmitter {
  pipe(dest) {
    this.on('data', (chunk) => dest.write(chunk));
    this.on('end', () => { if (typeof dest.end === 'function' && dest !== process.stdout && dest !== process.stderr) dest.end(); });
    return dest;
  }
}

class Readable extends Stream {
  constructor(opts = {}) {
    super();
    this._buffer = [];
    this._ended = false;
    this._flowing = false;
    this._objectMode = !!opts.objectMode;
    this._encoding = opts.encoding || null;
    if (typeof opts.read === 'function') this._read = opts.read;
    this.readable = true;
  }
  static from(iterable, opts) {
    const r = new Readable({ objectMode: true, ...opts });
    (async () => {
      for await (const x of iterable) r.push(x);
      r.push(null);
    })();
    return r;
  }
  _read() {}
  push(chunk) {
    if (chunk === null) {
      this._ended = true;
      setImmediate(() => this._flush());
      return false;
    }
    this._buffer.push(chunk);
    setImmediate(() => this._flush());
    return true;
  }
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
    }
  }
  on(ev, fn) {
    super.on(ev, fn);
    if (ev === 'data') {
      this._flowing = true;
      this._read();
      setImmediate(() => this._flush());
    }
    return this;
  }
  setEncoding(e) { this._encoding = e; return this; }
  resume() { this._flowing = true; setImmediate(() => this._flush()); return this; }
  pause() { this._flowing = false; return this; }
  read() { return this._buffer.length ? this._buffer.shift() : null; }
  destroy(err) { if (err) this.emit('error', err); this.emit('close'); return this; }
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
  }
}

class Writable extends Stream {
  constructor(opts = {}) {
    super();
    if (typeof opts.write === 'function') this._write = opts.write;
    if (typeof opts.final === 'function') this._final = opts.final;
    this.writable = true;
  }
  _write(chunk, enc, cb) { cb(); }
  write(chunk, enc, cb) {
    if (typeof enc === 'function') { cb = enc; enc = 'utf8'; }
    this._write(chunk, enc || 'utf8', (err) => {
      if (err) this.emit('error', err);
      if (cb) cb(err);
    });
    return true;
  }
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
  }
  destroy() { this.emit('close'); return this; }
}

class Duplex extends Readable {
  constructor(opts = {}) {
    super(opts);
    this._w = new Writable(opts);
    this.writable = true;
  }
  write(chunk, enc, cb) { return this._w.write.call(this, chunk, enc, cb); }
  _write(chunk, enc, cb) { cb(); }
  end(chunk, enc, cb) {
    if (chunk !== undefined && chunk !== null && typeof chunk !== 'function') this.write(chunk, enc);
    if (typeof this._flushTransform === 'function') this._flushTransform();
    else this.push(null);
    if (typeof cb === 'function') cb();
    return this;
  }
}

class Transform extends Duplex {
  constructor(opts = {}) {
    super(opts);
    if (typeof opts.transform === 'function') this._transform = opts.transform;
    if (typeof opts.flush === 'function') this._flushFn = opts.flush;
  }
  _transform(chunk, enc, cb) { cb(null, chunk); }
  _write(chunk, enc, cb) {
    this._transform(chunk, enc, (err, out) => {
      if (out !== undefined && out !== null) this.push(out);
      cb(err);
    });
  }
  _flushTransform() {
    if (this._flushFn) this._flushFn((err, out) => { if (out !== undefined && out !== null) this.push(out); this.push(null); });
    else this.push(null);
  }
}

class PassThrough extends Transform {}

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
