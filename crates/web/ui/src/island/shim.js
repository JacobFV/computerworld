// The React an island's code sees: the API of React 18, implemented over cw-ui's
// renderer through the natives on `globalThis.__cw` (see island.rs). Elements are
// React's own shape ({ $$typeof, type, key, ref, props }), so packages that build,
// inspect or clone them work unchanged; hooks are cw-ui's hooks of the component
// cw-ui is rendering.
(function () {
  'use strict';
  const cw = globalThis.__cw;
  const ELEMENT = Symbol.for('react.element');
  const FRAGMENT = Symbol.for('react.fragment');
  const STRICT = Symbol.for('react.strict_mode');
  const SUSPENSE = Symbol.for('react.suspense');
  const PROFILER = Symbol.for('react.profiler');
  const PROVIDER = Symbol.for('react.provider');
  const CONTEXT = Symbol.for('react.context');
  const FORWARD_REF = Symbol.for('react.forward_ref');
  const MEMO = Symbol.for('react.memo');
  const ID = Symbol('cw.value');

  // ------------------------------------------------------------ cw-ui values
  // A value of the compiled side is a live view: a Proxy whose traps read and
  // write it where it lives. Its target is an array for an array, so
  // Array.isArray and the array methods see one.
  const handler = {
    get(t, k) {
      if (k === ID) return t[ID];
      if (typeof k === 'symbol') {
        if (k === Symbol.iterator && Array.isArray(t)) return Array.prototype[Symbol.iterator];
        return undefined;
      }
      // What the value inherits is the VM's own: an array's methods are
      // Array.prototype's, working through these traps, as on any array-like.
      const proto = Array.isArray(t) ? Array.prototype : Object.prototype;
      if (k !== 'length' && k in proto && !cw.phas(t[ID], k)) return proto[k];
      return cw.pget(t[ID], k);
    },
    set(t, k, v) {
      if (typeof k === 'symbol') return false;
      return cw.pset(t[ID], k, v);
    },
    has(t, k) {
      if (k === ID) return true;
      if (typeof k === 'symbol') return false;
      return cw.phas(t[ID], k);
    },
    ownKeys(t) {
      const keys = cw.pkeys(t[ID]);
      if (Array.isArray(t)) keys.push('length');
      return keys;
    },
    getOwnPropertyDescriptor(t, k) {
      if (typeof k === 'symbol') return undefined;
      if (Array.isArray(t) && k === 'length') {
        return { value: cw.pget(t[ID], 'length'), writable: true, enumerable: false, configurable: false };
      }
      if (!cw.phas(t[ID], k)) return undefined;
      return { value: cw.pget(t[ID], k), writable: true, enumerable: true, configurable: true };
    },
    deleteProperty(t, k) {
      if (typeof k === 'symbol') return false;
      return cw.pdel(t[ID], k);
    },
    defineProperty(t, k, d) {
      if (typeof k === 'symbol') return false;
      return cw.pset(t[ID], k, d.value);
    },
    getPrototypeOf(t) {
      return Array.isArray(t) ? Array.prototype : Object.prototype;
    },
  };
  cw.proxy = function (id, isArray) {
    const t = isArray ? [] : {};
    Object.defineProperty(t, ID, { value: id, configurable: true });
    return new Proxy(t, handler);
  };
  cw.idOf = function (v) {
    if (v !== null && typeof v === 'object') {
      const id = v[ID];
      return typeof id === 'number' ? id : -1;
    }
    return -1;
  };
  cw.keys = (o) => Object.keys(o);
  cw.isArray = (o) => Array.isArray(o);
  cw.toArray = (o) => Array.from(o);
  cw.str = (v) => String(v);
  cw.num = (v) => Number(v);
  cw.json = (v, gap) => JSON.stringify(v, null, gap);
  cw.has = (o, k) => k in o;
  cw.instance = (v, name) => typeof globalThis[name] === 'function' && v instanceof globalThis[name];
  cw.method = (o, name, args) => o[name](...args);
  cw.op = (op, a, b) => {
    switch (op) {
      case '+': return a + b;
      case '-': return a - b;
      case '*': return a * b;
      case '/': return a / b;
      case '%': return a % b;
      case '**': return a ** b;
      case '==': return a == b;
      case '!=': return a != b;
      case '<': return a < b;
      case '<=': return a <= b;
      case '>': return a > b;
      case '>=': return a >= b;
      case 'in': return a in b;
    }
    return undefined;
  };
  cw.construct = (f, args) => new f(...args);
  cw.then = (p, ok, bad) => p.then(ok, bad);
  cw.isThenable = (v) => v !== null && (typeof v === 'object' || typeof v === 'function') && typeof v.then === 'function';

  // ------------------------------------------------------------ elements
  const RESERVED = { key: true, ref: true, __self: true, __source: true };
  function createElement(type, config, ...children) {
    let key = null;
    let ref = null;
    const props = {};
    if (config != null) {
      if (config.ref !== undefined) ref = config.ref;
      if (config.key !== undefined) key = '' + config.key;
      for (const p in config) {
        if (Object.prototype.hasOwnProperty.call(config, p) && !RESERVED[p]) props[p] = config[p];
      }
    }
    if (children.length === 1) props.children = children[0];
    else if (children.length > 1) props.children = children;
    if (type && type.defaultProps) {
      for (const p in type.defaultProps) if (props[p] === undefined) props[p] = type.defaultProps[p];
    }
    return { $$typeof: ELEMENT, type, key, ref, props, _owner: null };
  }
  function jsx(type, config, maybeKey) {
    let key = null;
    let ref = null;
    const props = {};
    if (maybeKey !== undefined) key = '' + maybeKey;
    if (config != null) {
      if (config.key !== undefined) key = '' + config.key;
      if (config.ref !== undefined) ref = config.ref;
      for (const p in config) {
        if (Object.prototype.hasOwnProperty.call(config, p) && !RESERVED[p]) props[p] = config[p];
      }
    }
    if (type && type.defaultProps) {
      for (const p in type.defaultProps) if (props[p] === undefined) props[p] = type.defaultProps[p];
    }
    return { $$typeof: ELEMENT, type, key, ref, props, _owner: null };
  }
  function isValidElement(o) {
    return typeof o === 'object' && o !== null && o.$$typeof === ELEMENT;
  }
  function cloneElement(el, config, ...children) {
    const props = Object.assign({}, el.props);
    let key = el.key;
    let ref = el.ref;
    if (config != null) {
      if (config.ref !== undefined) ref = config.ref;
      if (config.key !== undefined) key = '' + config.key;
      for (const p in config) {
        if (Object.prototype.hasOwnProperty.call(config, p) && !RESERVED[p]) {
          props[p] = config[p] === undefined && el.type && el.type.defaultProps ? el.type.defaultProps[p] : config[p];
        }
      }
    }
    if (children.length === 1) props.children = children[0];
    else if (children.length > 1) props.children = children;
    return { $$typeof: ELEMENT, type: el.type, key, ref, props, _owner: null };
  }

  function flatten(children, out) {
    if (children === null || children === undefined || typeof children === 'boolean') return out;
    if (Array.isArray(children)) {
      for (const c of children) flatten(c, out);
    } else {
      out.push(children);
    }
    return out;
  }
  const Children = {
    map(children, fn, ctx) {
      if (children === null || children === undefined) return children;
      return flatten(children, []).map((c, i) => fn.call(ctx, c, i));
    },
    forEach(children, fn, ctx) {
      flatten(children, []).forEach((c, i) => fn.call(ctx, c, i));
    },
    count(children) {
      return flatten(children, []).length;
    },
    toArray(children) {
      return flatten(children, []);
    },
    only(children) {
      if (!isValidElement(children)) throw new Error('React.Children.only expected to receive a single React element child.');
      return children;
    },
  };

  // ------------------------------------------------------------ hooks
  const hook = cw.hook;
  function useState(init) { return hook('state', init); }
  function useReducer(reducer, init, initFn) {
    return arguments.length > 2 ? hook('reducer', reducer, init, initFn) : hook('reducer', reducer, init);
  }
  function useMemo(f, deps) { return hook('memo', f, deps); }
  function useCallback(f, deps) { return hook('callback', f, deps); }
  function useRef(init) { return hook('ref', init); }
  function useEffect(f, deps) { return arguments.length > 1 ? hook('effect', f, deps) : hook('effect', f); }
  function useLayoutEffect(f, deps) { return arguments.length > 1 ? hook('layout', f, deps) : hook('layout', f); }
  function useContext(ctx) { return hook('context', ctx); }
  function useId() { return hook('id'); }
  function useSyncExternalStore(subscribe, get) { return hook('store', subscribe, get); }
  function useImperativeHandle(ref, create, deps) {
    return arguments.length > 2 ? hook('imperative', ref, create, deps) : hook('imperative', ref, create);
  }
  function useTransition() { return [false, (f) => f()]; }
  function useDeferredValue(v) { return v; }
  function useDebugValue() {}
  function startTransition(f) { f(); }

  function createContext(defaultValue) {
    const ctx = { $$typeof: CONTEXT, _cw: cw.newContext(defaultValue), _currentValue: defaultValue };
    ctx.Provider = { $$typeof: PROVIDER, _context: ctx };
    ctx.Consumer = ctx;
    return ctx;
  }
  cw.contextOf = function (id) {
    const ctx = { $$typeof: CONTEXT, _cw: id };
    ctx.Provider = { $$typeof: PROVIDER, _context: ctx };
    ctx.Consumer = ctx;
    return ctx;
  };
  function forwardRef(render) {
    const f = function (props, ref) { return render(props, ref); };
    f.__cwForwardRef = true;
    f.render = render;
    f.$$typeof = FORWARD_REF;
    f.displayName = render.displayName || render.name;
    return f;
  }
  // `memo(C)` renders as C (renders are pure; only render counts differ).
  function memo(type) { return type; }
  function createRef() { return { current: null }; }
  class Component {
    constructor(props, context) { this.props = props; this.context = context; this.state = null; }
    setState() { throw new Error('class components are not supported in an island'); }
    forceUpdate() {}
  }
  Component.prototype.isReactComponent = {};
  class PureComponent extends Component {}
  function lazy() { throw new Error('React.lazy is not supported in an island'); }

  const React = {
    createElement, cloneElement, isValidElement, Children, Fragment: FRAGMENT, StrictMode: STRICT,
    Suspense: SUSPENSE, Profiler: PROFILER, createContext, forwardRef, memo, createRef, Component,
    PureComponent, lazy, useState, useReducer, useMemo, useCallback, useRef, useEffect,
    useLayoutEffect, useInsertionEffect: useLayoutEffect, useContext, useId, useSyncExternalStore,
    useImperativeHandle, useTransition, useDeferredValue, useDebugValue, startTransition,
    version: '18.3.1',
  };
  React.default = React;
  const jsxRuntime = { jsx, jsxs: jsx, jsxDEV: jsx, Fragment: FRAGMENT };
  // The app's root, when its entry runs here: what it renders, which cw-ui
  // mounts in the container (`island_root` in cw-tsx names it).
  const rootOf = () => ({
    render(el) { cw.rendered = el; },
    unmount() { cw.rendered = null; },
  });
  const ReactDOM = {
    flushSync(f) { return f(); },
    createPortal() { throw new Error('createPortal is not supported in an island'); },
    createRoot: () => rootOf(),
    hydrateRoot: (container, el) => { cw.rendered = el; return rootOf(); },
    render(el) { cw.rendered = el; },
    version: '18.3.1',
  };
  ReactDOM.default = ReactDOM;
  globalThis.React = React;
  globalThis.ReactDOM = ReactDOM;
  globalThis.__cw_jsx = jsxRuntime;
  cw.symbols = { ELEMENT, FRAGMENT, STRICT, SUSPENSE, PROFILER, PROVIDER, CONTEXT };

  // ------------------------------------------------------------ the host
  globalThis.window = globalThis;
  globalThis.self = globalThis;
  // `x.toLocaleString(…)` for compiled code: the VM's own (jsvm's Intl).
  globalThis.__cw_locale = (x, m, ...a) => x[m](...a);
  // The page, through cw-ui's own builtins (the same the compiled code uses).
  const B = (name, ...a) => cw.builtin(name, a);
  globalThis.document = {
    getElementById: (id) => B('GetElementById', id),
    querySelector: (s) => B('QuerySelector', s),
    querySelectorAll: (s) => B('QuerySelectorAll', s),
    get body() { return B('DocumentBody'); },
    get documentElement() { return B('DocumentElement'); },
    get activeElement() { return B('ActiveElement'); },
    get title() { return B('DocumentTitle'); },
    addEventListener: (t, f, o) => B('DocumentAddListener', t, f, o),
    removeEventListener: (t, f, o) => B('DocumentRemoveListener', t, f, o),
  };
  globalThis.addEventListener = (t, f, o) => B('WindowAddListener', t, f, o);
  globalThis.removeEventListener = (t, f, o) => B('WindowRemoveListener', t, f, o);
  globalThis.setTimeout = (f, ms, ...args) => cw.timer(0, f, ms, args);
  globalThis.setInterval = (f, ms, ...args) => cw.timer(1, f, ms, args);
  globalThis.clearTimeout = (id) => cw.clearTimer(id);
  globalThis.clearInterval = (id) => cw.clearTimer(id);
  globalThis.queueMicrotask = (f) => Promise.resolve().then(f);
  const fmt = (args) => args.map((a) => (typeof a === 'string' ? a : cw.inspect(a))).join(' ');
  globalThis.console = {
    log: (...a) => cw.log(0, fmt(a)),
    info: (...a) => cw.log(0, fmt(a)),
    debug: (...a) => cw.log(0, fmt(a)),
    warn: (...a) => cw.log(1, fmt(a)),
    error: (...a) => cw.log(2, fmt(a)),
  };
})();
