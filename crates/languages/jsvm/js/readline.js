'use strict';

const EventEmitter = require('events');

class Interface extends EventEmitter {
  constructor(input, output, completer, terminal) {
    super();
    const opts = input && typeof input === 'object' && typeof input.on !== 'function' ? input : { input, output, completer, terminal };
    this.input = opts.input;
    this.output = opts.output;
    this.terminal = !!opts.terminal;
    this._prompt = opts.prompt === undefined ? '> ' : opts.prompt;
    this.closed = false;
    this.line = '';
    this._buffer = '';
    this._lines = [];
    this._questions = [];
    this._paused = false;
    this._ended = false;
    this._started = false;
    this._lineWaiters = [];
    const self = this;
    if (this.input && typeof this.input.on === 'function') {
      this._onData = (chunk) => self._write(typeof chunk === 'string' ? chunk : chunk.toString());
      this._onEnd = () => self._end();
      this.input.on('data', this._onData);
      this.input.on('end', this._onEnd);
      if (typeof this.input.resume === 'function') this.input.resume();
    }
  }

  get [Symbol.toStringTag]() { return 'Interface'; }

  _write(s) {
    this._buffer += s;
    let i;
    while ((i = this._buffer.search(/\r?\n/)) !== -1) {
      const m = this._buffer.match(/\r?\n/);
      const line = this._buffer.slice(0, i);
      this._buffer = this._buffer.slice(i + m[0].length);
      this._emitLine(line);
      if (this.closed) return;
    }
  }

  _emitLine(line) {
    if (this._questions.length) {
      const cb = this._questions.shift();
      cb(line);
      return;
    }
    if (this._lineWaiters.length) {
      this._lineWaiters.shift()({ value: line, done: false });
      return;
    }
    if (this.listenerCount('line') === 0 && this._iterating) {
      this._lines.push(line);
      return;
    }
    this.emit('line', line);
  }

  _end() {
    if (this._buffer.length) {
      const b = this._buffer;
      this._buffer = '';
      this._emitLine(b);
    }
    this._ended = true;
    this.close();
  }

  setPrompt(p) { this._prompt = p; }
  getPrompt() { return this._prompt; }

  prompt() {
    if (this.output && typeof this.output.write === 'function') this.output.write(this._prompt);
  }

  question(query, opts, cb) {
    if (typeof opts === 'function') cb = opts;
    if (this.closed) {
      const e = new Error('readline was closed');
      e.code = 'ERR_USE_AFTER_CLOSE';
      throw e;
    }
    if (this.output && typeof this.output.write === 'function') this.output.write(query);
    this._questions.push(cb);
  }

  write(d) {
    if (typeof d === 'string') this._write(d);
  }

  pause() { this._paused = true; this.emit('pause'); return this; }
  resume() { this._paused = false; this.emit('resume'); return this; }

  close() {
    if (this.closed) return;
    this.closed = true;
    if (this.input && typeof this.input.removeListener === 'function') {
      this.input.removeListener('data', this._onData);
      this.input.removeListener('end', this._onEnd);
    }
    while (this._lineWaiters.length) this._lineWaiters.shift()({ value: undefined, done: true });
    this.emit('close');
  }

  [Symbol.asyncIterator]() {
    this._iterating = true;
    const self = this;
    return {
      next() {
        if (self._lines.length) return Promise.resolve({ value: self._lines.shift(), done: false });
        if (self.closed) return Promise.resolve({ value: undefined, done: true });
        return new Promise((resolve) => self._lineWaiters.push(resolve));
      },
      return() {
        self.close();
        return Promise.resolve({ value: undefined, done: true });
      },
      [Symbol.asyncIterator]() { return this; },
    };
  }
}

function createInterface(input, output, completer, terminal) {
  return new Interface(input, output, completer, terminal);
}

class PromisesInterface extends Interface {
  question(query, opts) {
    return new Promise((resolve) => super.question(query, opts, resolve));
  }
}

function clearLine(stream, dir, cb) { if (typeof cb === 'function') process.nextTick(cb); return true; }
function clearScreenDown(stream, cb) { if (typeof cb === 'function') process.nextTick(cb); return true; }
function cursorTo(stream, x, y, cb) { if (typeof y === 'function') y(); else if (typeof cb === 'function') process.nextTick(cb); return true; }
function moveCursor(stream, dx, dy, cb) { if (typeof cb === 'function') process.nextTick(cb); return true; }
function emitKeypressEvents() {}

const promises = {
  Interface: PromisesInterface,
  createInterface: (input, output, completer, terminal) => new PromisesInterface(input, output, completer, terminal),
};

module.exports = {
  Interface,
  createInterface,
  clearLine,
  clearScreenDown,
  cursorTo,
  moveCursor,
  emitKeypressEvents,
  promises,
};
if (module.id === 'readline/promises') module.exports = promises;
