// The `cw` global a web application talks to its machine through. Installed by the
// host before the application's own script; see `docs/custom-application.md` and
// `crates/applications/web/types/cw.d.ts`, which types every member for app authors.
//
// The host and this file speak over a reserved key space of `localStorage`, the one
// synchronous channel a document already has into its host: the boot facts and the
// world clock are read from it, and requests, state and window facts are written to
// it. Nothing an application stores under its own keys can collide with it.
(() => {
  'use strict';
  const KEY = '\u0001cw:';
  const store = globalThis.localStorage;
  const boot = JSON.parse(store.getItem(KEY + 'boot'));
  const send = (message) => store.setItem(KEY + 'out', JSON.stringify(message));
  let env = boot.env;
  let state = boot.state;
  let next = 1;
  const pending = new Map();
  const listeners = new Set();

  function applyEnv() {
    const root = document.documentElement;
    root.setAttribute('data-platform', env.platform);
    if (env.mobile) root.setAttribute('data-mobile', '');
    else root.removeAttribute('data-mobile');
    const sheet = document.getElementById('cw-theme');
    if (sheet) sheet.textContent = env.css;
  }

  function request(kind, fields) {
    const id = next++;
    return new Promise((resolve, reject) => {
      pending.set(id, { resolve, reject });
      send(Object.assign({ op: 'request', id, kind }, fields));
    });
  }

  Object.defineProperty(globalThis, '__cw_deliver', {
    value(replies) {
      for (const reply of replies) {
        const waiting = pending.get(reply.id);
        if (!waiting) continue;
        pending.delete(reply.id);
        if (reply.error !== undefined) waiting.reject(new Error(reply.error));
        else waiting.resolve(reply.value);
      }
    },
  });
  Object.defineProperty(globalThis, '__cw_env', {
    value(nextEnv) {
      env = nextEnv;
      applyEnv();
      for (const listener of Array.from(listeners)) listener(env);
    },
  });

  const response = (r) =>
    Object.freeze({
      status: r.status,
      ok: r.status >= 200 && r.status < 300,
      body: r.body,
      text: () => Promise.resolve(r.body),
      json: () => Promise.resolve(JSON.parse(r.body)),
    });

  const cw = {
    kind: boot.kind,
    argument: boot.argument,
    get env() {
      return env;
    },
    onEnv(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    now() {
      return Number(store.getItem(KEY + 'now'));
    },
    state: Object.freeze({
      get: () => state,
      set(value) {
        state = value;
        send({ op: 'state', value });
      },
    }),
    fs: Object.freeze({
      readFile: (path) => request('read', { path: String(path) }),
      writeFile: (path, content) =>
        request('write', { path: String(path), content: String(content) }),
      list: (path) => request('list', { path: String(path) }),
      mkdir: (path) => request('mkdir', { path: String(path) }),
    }),
    fetch(url, init) {
      const method = ((init && init.method) || 'GET').toUpperCase();
      const body = init && init.body != null ? String(init.body) : '';
      return request('http', { url: String(url), method, body }).then(response);
    },
    launch: (kind, argument) =>
      request('launch', { app: String(kind), argument: argument == null ? '' : String(argument) }),
    emit: (name, data) =>
      request('emit', { name: String(name), data: data === undefined ? null : data }),
    refuse(message) {
      send({ op: 'refuse', message: String(message) });
    },
    window: Object.freeze({
      set(facts) {
        send({ op: 'chrome', chrome: facts });
      },
    }),
  };
  Object.defineProperty(globalThis, 'cw', { value: Object.freeze(cw) });
  applyEnv();
})();
