'use strict';

const kCapture = Symbol('kCapture');
const kShapeMode = Symbol('shapeMode');
const errorMonitor = Symbol('events.errorMonitor');
let defaultMaxListeners = 10;

function EventEmitter(opts) {
  EventEmitter.init.call(this, opts);
}

EventEmitter.prototype._events = undefined;
EventEmitter.prototype._eventsCount = 0;
EventEmitter.prototype._maxListeners = undefined;

Object.defineProperty(EventEmitter, 'defaultMaxListeners', {
  enumerable: true,
  get() { return defaultMaxListeners; },
  set(n) { defaultMaxListeners = n; },
});

EventEmitter.init = function init(opts) {
  if (this._events === undefined || this._events === Object.getPrototypeOf(this)._events) {
    this._events = Object.create(null);
    this._eventsCount = 0;
    this[kShapeMode] = false;
  } else {
    this[kShapeMode] = true;
  }
  this._maxListeners = this._maxListeners || undefined;
  this[kCapture] = !!(opts && opts.captureRejections);
};

function ensure(target) {
  if (target._events === undefined || target._events === null) {
    target._events = Object.create(null);
    target._eventsCount = 0;
  }
  return target._events;
}

function checkListener(listener) {
  if (typeof listener !== 'function') {
    const err = new TypeError('The "listener" argument must be of type function. Received ' +
      (listener === null ? 'null' : listener === undefined ? 'undefined' : `type ${typeof listener} (${require('util').inspect(listener)})`));
    err.code = 'ERR_INVALID_ARG_TYPE';
    throw err;
  }
}

EventEmitter.prototype.setMaxListeners = function setMaxListeners(n) {
  this._maxListeners = n;
  return this;
};

function getMaxListeners(that) {
  if (that._maxListeners === undefined) return defaultMaxListeners;
  return that._maxListeners;
}

EventEmitter.prototype.getMaxListeners = function getMaxListeners_() {
  return getMaxListeners(this);
};

EventEmitter.prototype.emit = function emit(type, ...args) {
  const events = this._events;
  if (type === 'error') {
    if (events !== undefined && events[errorMonitor] !== undefined) this.emit(errorMonitor, ...args);
    if (events === undefined || events.error === undefined) {
      const er = args[0];
      if (er instanceof Error) {
        throw er;
      }
      const util = require('util');
      const err = new Error('Unhandled error. (' + util.inspect(er) + ')');
      err.code = 'ERR_UNHANDLED_ERROR';
      err.context = er;
      throw err;
    }
  }
  if (events === undefined) return false;
  const handler = events[type];
  if (handler === undefined) return false;
  if (typeof handler === 'function') {
    Reflect.apply(handler, this, args);
  } else {
    const listeners = handler.slice();
    for (let i = 0; i < listeners.length; ++i) {
      Reflect.apply(listeners[i], this, args);
    }
  }
  return true;
};

function addListener(target, type, listener, prepend) {
  checkListener(listener);
  const events = ensure(target);
  if (events.newListener !== undefined) {
    target.emit('newListener', type, listener.listener ? listener.listener : listener);
  }
  const existing = target._events[type];
  if (existing === undefined) {
    target._events[type] = listener;
    ++target._eventsCount;
  } else {
    let list;
    if (typeof existing === 'function') {
      list = target._events[type] = prepend ? [listener, existing] : [existing, listener];
    } else {
      list = existing;
      if (prepend) list.unshift(listener); else list.push(listener);
    }
    const m = getMaxListeners(target);
    if (m > 0 && list.length > m && !list.warned) {
      list.warned = true;
      const name = target.constructor && target.constructor.name ? target.constructor.name : 'EventEmitter';
      process.emitWarning(`Possible EventEmitter memory leak detected. ${list.length} ${String(type)} listeners added to [${name}]. MaxListeners is ${m}. Use emitter.setMaxListeners() to increase limit`, 'MaxListenersExceededWarning');
    }
  }
  if (target.__onListenerAdded) target.__onListenerAdded(type);
  return target;
}

EventEmitter.prototype.addListener = function addListener_(type, listener) {
  return addListener(this, type, listener, false);
};
EventEmitter.prototype.on = EventEmitter.prototype.addListener;
EventEmitter.prototype.prependListener = function prependListener(type, listener) {
  return addListener(this, type, listener, true);
};

function onceWrapper(...args) {
  if (!this.fired) {
    this.target.removeListener(this.type, this.wrapFn);
    this.fired = true;
    return Reflect.apply(this.listener, this.target, args);
  }
}

function onceWrap(target, type, listener) {
  const state = { fired: false, wrapFn: undefined, target, type, listener };
  const wrapped = onceWrapper.bind(state);
  wrapped.listener = listener;
  state.wrapFn = wrapped;
  return wrapped;
}

EventEmitter.prototype.once = function once(type, listener) {
  checkListener(listener);
  this.on(type, onceWrap(this, type, listener));
  return this;
};

EventEmitter.prototype.prependOnceListener = function prependOnceListener(type, listener) {
  checkListener(listener);
  this.prependListener(type, onceWrap(this, type, listener));
  return this;
};

EventEmitter.prototype.removeListener = function removeListener(type, listener) {
  checkListener(listener);
  const events = this._events;
  if (events === undefined) return this;
  const list = events[type];
  if (list === undefined) return this;
  if (list === listener || list.listener === listener) {
    if (--this._eventsCount === 0) this._events = Object.create(null);
    else delete events[type];
    if (events.removeListener) this.emit('removeListener', type, list.listener || listener);
  } else if (typeof list !== 'function') {
    let position = -1;
    for (let i = list.length - 1; i >= 0; i--) {
      if (list[i] === listener || list[i].listener === listener) {
        position = i;
        break;
      }
    }
    if (position < 0) return this;
    list.splice(position, 1);
    if (list.length === 1) events[type] = list[0];
    if (events.removeListener !== undefined) this.emit('removeListener', type, listener);
  }
  return this;
};
EventEmitter.prototype.off = EventEmitter.prototype.removeListener;

EventEmitter.prototype.removeAllListeners = function removeAllListeners(type) {
  const events = this._events;
  if (events === undefined) return this;
  if (arguments.length === 0) {
    this._events = Object.create(null);
    this._eventsCount = 0;
  } else if (events[type] !== undefined) {
    if (--this._eventsCount === 0) this._events = Object.create(null);
    else delete events[type];
  }
  return this;
};

function listeners(target, type, unwrap) {
  const events = target._events;
  if (events === undefined) return [];
  const l = events[type];
  if (l === undefined) return [];
  if (typeof l === 'function') return unwrap ? [l.listener || l] : [l];
  return unwrap ? l.map((x) => x.listener || x) : l.slice();
}

EventEmitter.prototype.listeners = function listeners_(type) {
  return listeners(this, type, true);
};
EventEmitter.prototype.rawListeners = function rawListeners(type) {
  return listeners(this, type, false);
};
EventEmitter.prototype.listenerCount = function listenerCount(type, listener) {
  const events = this._events;
  if (events !== undefined) {
    const l = events[type];
    if (typeof l === 'function') return listener === undefined || l === listener || l.listener === listener ? 1 : 0;
    if (l !== undefined) {
      if (listener === undefined) return l.length;
      return l.filter((x) => x === listener || x.listener === listener).length;
    }
  }
  return 0;
};
EventEmitter.listenerCount = (emitter, type) => emitter.listenerCount(type);
EventEmitter.prototype.eventNames = function eventNames() {
  return this._eventsCount > 0 ? Reflect.ownKeys(this._events) : [];
};

EventEmitter.once = function once(emitter, name) {
  return new Promise((resolve, reject) => {
    const errorListener = (err) => {
      emitter.removeListener(name, resolver);
      reject(err);
    };
    const resolver = (...args) => {
      if (name !== 'error') emitter.removeListener('error', errorListener);
      resolve(args);
    };
    emitter.once(name, resolver);
    if (name !== 'error') emitter.once('error', errorListener);
  });
};

EventEmitter.on = function on(emitter, event) {
  const queue = [];
  const waiting = [];
  let done = false;
  emitter.on(event, (...args) => {
    if (waiting.length) waiting.shift()({ value: args, done: false });
    else queue.push(args);
  });
  emitter.on('close', () => {
    done = true;
    while (waiting.length) waiting.shift()({ value: undefined, done: true });
  });
  return {
    next() {
      if (queue.length) return Promise.resolve({ value: queue.shift(), done: false });
      if (done) return Promise.resolve({ value: undefined, done: true });
      return new Promise((r) => waiting.push(r));
    },
    return() { done = true; return Promise.resolve({ value: undefined, done: true }); },
    [Symbol.asyncIterator]() { return this; },
  };
};

EventEmitter.EventEmitter = EventEmitter;
EventEmitter.usingDomains = false;
EventEmitter.captureRejectionSymbol = Symbol.for('nodejs.rejection');
EventEmitter.errorMonitor = errorMonitor;
EventEmitter.getEventListeners = (emitter, name) => emitter.listeners(name);
EventEmitter.setMaxListeners = (n) => { defaultMaxListeners = n; };

class EventTarget {
  #listeners = new Map();
  addEventListener(type, fn) {
    if (!this.#listeners.has(type)) this.#listeners.set(type, []);
    this.#listeners.get(type).push(fn);
  }
  removeEventListener(type, fn) {
    const l = this.#listeners.get(type);
    if (l) this.#listeners.set(type, l.filter((x) => x !== fn));
  }
  dispatchEvent(ev) {
    const l = this.#listeners.get(ev.type) || [];
    for (const fn of l.slice()) {
      if (typeof fn === 'function') fn.call(this, ev); else fn.handleEvent(ev);
    }
    return true;
  }
}

class Event {
  constructor(type, init = {}) {
    this.type = type;
    this.bubbles = !!init.bubbles;
    this.cancelable = !!init.cancelable;
    this.defaultPrevented = false;
    this.timeStamp = performance.now();
  }
  preventDefault() { this.defaultPrevented = true; }
  stopPropagation() {}
}

EventEmitter.EventTarget = EventTarget;
EventEmitter.Event = Event;

module.exports = EventEmitter;
