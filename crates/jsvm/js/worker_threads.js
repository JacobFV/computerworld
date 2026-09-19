'use strict';
// node:worker_threads — several JavaScript contexts in one interpreter.
//
// The interpreter runs one context at a time and swaps globals, frames, timers
// and module registries when it changes over (see src/workers.rs). This module
// is loaded again in every context that requires it, so the three hooks the
// interpreter calls keep all of their state on `globalThis`: a worker's global
// is a copy of its parent's, and a function copied along with it reads the
// globals of whatever context is running.
const EventEmitter = require('events');

function def(o, k, v) {
  Object.defineProperty(o, k, { value: v, writable: true, configurable: true });
}

function portTable() {
  let t = globalThis['%portTable'];
  if (!t) {
    t = new Map();
    globalThis['%portTable'] = t;
  }
  return t;
}

function workerTable() {
  let t = globalThis['%workerTable'];
  if (!t) {
    t = new Map();
    globalThis['%workerTable'] = t;
  }
  return t;
}

// Called by the interpreter in a new context before its entry runs.
globalThis['%workerSetup'] = function (id, port, peer) {
  globalThis['%workerThreadId'] = id;
  globalThis['%workerPort'] = port;
  globalThis['%workerPeer'] = peer;
  // Whatever the parent's global held belongs to the parent, not here.
  globalThis['%portTable'] = undefined;
  globalThis['%workerTable'] = undefined;
  globalThis['%workerQueue'] = undefined;
};

// Called by the interpreter with a message that has arrived for this context.
globalThis['%workerDeliver'] = function (port, value, quiet) {
  const table = globalThis['%portTable'];
  const p = table ? table.get(port) : undefined;
  if (p) {
    if (quiet) p.$queue.push(value);
    else p.$recv(value);
    return;
  }
  // The port object does not exist yet: keep the message until it does.
  let q = globalThis['%workerQueue'];
  if (!q) {
    q = [];
    globalThis['%workerQueue'] = q;
  }
  q.push([port, value]);
};

// Called by the interpreter when the event loop comes round: a message that
// arrived while the program was busy reaches its listeners here.
globalThis['%workerFlush'] = function () {
  const table = globalThis['%portTable'];
  if (!table) return false;
  let any = false;
  for (const p of table.values()) {
    if (p.$started && p.$queue.length) {
      any = true;
      p.$flush();
    }
  }
  return any;
};

// Called by the interpreter when a worker of this context starts, throws or ends.
globalThis['%workerEvent'] = function (id, kind, payload) {
  const table = globalThis['%workerTable'];
  const w = table ? table.get(id) : undefined;
  if (!w) return;
  if (kind === 'online') w.emit('online');
  else if (kind === 'error') w.$error(payload);
  else w.$exit(payload);
};

// ------------------------------------------------------------------ transfer

// A port travels as a marker; the context it arrives in makes its own object
// for it, and the interpreter moves the port's ownership across.
function encode(value, list, dest) {
  const seen = new Map();
  function walk(v) {
    if (v === null || typeof v !== 'object') return v;
    if (v instanceof MessagePort) {
      if (list.indexOf(v) < 0) {
        const e = new Error(`${v} could not be cloned.`);
        e.name = 'DataCloneError';
        throw e;
      }
      binding.workerMovePort(v.$id, dest);
      portTable().delete(v.$id);
      def(v, '$moved', true);
      return { '%mport': v.$id, '%mpeer': v.$peer };
    }
    if (seen.has(v)) return seen.get(v);
    if (Array.isArray(v)) {
      const out = [];
      seen.set(v, out);
      for (const x of v) out.push(walk(x));
      return out;
    }
    const proto = Object.getPrototypeOf(v);
    if (proto === Object.prototype || proto === null) {
      const out = {};
      seen.set(v, out);
      for (const k of Object.keys(v)) out[k] = walk(v[k]);
      return out;
    }
    // Everything else (dates, buffers, maps, errors) the clone itself copies.
    return v;
  }
  return walk(value);
}

function decode(value) {
  const seen = new Map();
  function walk(v) {
    if (v === null || typeof v !== 'object') return v;
    if (seen.has(v)) return seen.get(v);
    if (Array.isArray(v)) {
      seen.set(v, v);
      for (let i = 0; i < v.length; i++) v[i] = walk(v[i]);
      return v;
    }
    const proto = Object.getPrototypeOf(v);
    if (proto === Object.prototype || proto === null) {
      if (typeof v['%mport'] === 'number') {
        return new MessagePort(v['%mport'], v['%mpeer']);
      }
      seen.set(v, v);
      for (const k of Object.keys(v)) v[k] = walk(v[k]);
      return v;
    }
    return v;
  }
  return walk(value);
}

// --------------------------------------------------------------- MessagePort

class MessagePort extends EventEmitter {
  constructor(id, peer) {
    super();
    def(this, '$id', id);
    def(this, '$peer', peer);
    def(this, '$queue', []);
    def(this, '$started', false);
    def(this, '$closed', false);
    def(this, '$moved', false);
    portTable().set(id, this);
    const q = globalThis['%workerQueue'];
    if (q && q.length) {
      const rest = [];
      for (const m of q) {
        if (m[0] === id) this.$recv(m[1]);
        else rest.push(m);
      }
      globalThis['%workerQueue'] = rest;
    }
  }

  $recv(value) {
    this.$queue.push(value);
    this.$flush();
  }

  $flush() {
    if (!this.$started) return;
    while (this.$queue.length && !this.$closed) {
      const v = this.$queue.shift();
      this.emit('message', decode(v));
    }
  }

  postMessage(value, transferList) {
    if (this.$closed || this.$moved) return;
    const list = Array.isArray(transferList) ? transferList : [];
    binding.workerPost(this.$peer, encode(value, list, this.$peer));
  }

  start() {
    this.$started = true;
    this.$flush();
  }

  close() {
    if (this.$closed) return;
    this.$closed = true;
    portTable().delete(this.$id);
    this.emit('close');
  }

  ref() {}
  unref() {}

  on(name, fn) {
    const r = super.on(name, fn);
    if (name === 'message') this.start();
    return r;
  }

  addListener(name, fn) {
    return this.on(name, fn);
  }

  once(name, fn) {
    const r = super.once(name, fn);
    if (name === 'message') this.start();
    return r;
  }

  get onmessage() {
    return this.$onmessage || null;
  }

  set onmessage(fn) {
    if (this.$onmessage) this.removeListener('message', this.$onmessage);
    def(this, '$onmessage', fn);
    if (fn) this.on('message', (data) => fn({ data }));
  }
}

class MessageChannel {
  constructor() {
    const ends = binding.workerPortPair();
    this.port1 = new MessagePort(ends[0], ends[1]);
    this.port2 = new MessagePort(ends[1], ends[0]);
  }
}

// -------------------------------------------------------------------- Worker

class Worker extends EventEmitter {
  constructor(filename, options) {
    super();
    const o = options || {};
    const source = typeof filename === 'string' ? filename : String(filename);
    const h = binding.workerNew(source, !!o.eval);
    def(this, '$id', h.id);
    def(this, '$exited', false);
    def(this, '$code', 0);
    def(this, '$waiting', []);
    const list = Array.isArray(o.transferList) ? o.transferList : [];
    binding.workerSetData(h.id, encode(o.workerData, list, h.peer));
    def(this, '$port', new MessagePort(h.port, h.peer));
    this.$port.on('message', (v) => this.emit('message', v));
    this.$port.on('messageerror', (v) => this.emit('messageerror', v));
    workerTable().set(h.id, this);
  }

  get threadId() {
    return this.$id;
  }

  postMessage(value, transferList) {
    this.$port.postMessage(value, transferList);
  }

  terminate() {
    binding.workerTerminate(this.$id);
    if (this.$exited) return Promise.resolve(this.$code);
    // The thread is joined before the promise settles, so the `exit` event
    // and whatever it set going come first.
    return new Promise((resolve) => this.$waiting.push(resolve));
  }

  $error(err) {
    if (this.listenerCount('error') === 0) {
      // Node rethrows an unheard worker error on the main thread a tick
      // later, so the `exit` event still comes first.
      process.nextTick(() => {
        throw err;
      });
      return;
    }
    this.emit('error', err);
  }

  $exit(code) {
    if (this.$exited) return;
    this.$exited = true;
    this.$code = code;
    this.$port.close();
    workerTable().delete(this.$id);
    this.emit('exit', code);
    const waiting = this.$waiting;
    def(this, '$waiting', []);
    for (const resolve of waiting) resolve(code);
  }

  ref() {}
  unref() {}

  get stdout() {
    return process.stdout;
  }

  get stderr() {
    return process.stderr;
  }

  get performance() {
    return { eventLoopUtilization: () => ({ idle: 0, active: 0, utilization: 0 }) };
  }
}

function receiveMessageOnPort(port) {
  if (!(port instanceof MessagePort)) {
    throw new TypeError(
      'The "port" argument must be a MessagePort instance'
    );
  }
  // Let the other contexts run, so a message sent meanwhile is here to take.
  binding.workerDrain();
  if (!port.$queue.length) return undefined;
  return { message: decode(port.$queue.shift()) };
}

const threadId = globalThis['%workerThreadId'] === undefined
  ? 0
  : globalThis['%workerThreadId'];
const isMainThread = threadId === 0;
const parentPort = isMainThread
  ? null
  : new MessagePort(globalThis['%workerPort'], globalThis['%workerPeer']);
const workerData = isMainThread
  ? null
  : decode(globalThis['%workerData'] === undefined ? null : globalThis['%workerData']);

module.exports = {
  Worker,
  MessagePort,
  MessageChannel,
  isMainThread,
  parentPort,
  threadId,
  workerData,
  receiveMessageOnPort,
  resourceLimits: {},
  SHARE_ENV: Symbol.for('nodejs.worker_threads.SHARE_ENV'),
  markAsUntransferable() {},
  isMarkedAsUntransferable() {
    return false;
  },
  moveMessagePortToContext() {
    throw new Error('moveMessagePortToContext is not supported');
  },
  setEnvironmentData() {
    throw new Error('setEnvironmentData is not supported');
  },
  getEnvironmentData() {
    return undefined;
  },
  BroadcastChannel: undefined,
};
