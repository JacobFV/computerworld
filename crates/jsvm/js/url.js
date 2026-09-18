'use strict';

const kInspect = Symbol.for('nodejs.util.inspect.custom');
const special = { 'http:': '80', 'https:': '443', 'ws:': '80', 'wss:': '443', 'ftp:': '21', 'file:': '' };

function invalidUrl(input) {
  const e = new TypeError('Invalid URL');
  e.code = 'ERR_INVALID_URL';
  e.input = input;
  return e;
}

function encodeComponent(s, keep) {
  let out = '';
  for (const ch of String(s)) {
    if (/[A-Za-z0-9\-._~]/.test(ch) || keep.includes(ch)) out += ch;
    else out += encodeURIComponent(ch);
  }
  return out;
}

function normalizePath(path) {
  const segs = path.split('/');
  const out = [];
  for (let i = 0; i < segs.length; i++) {
    const s = segs[i];
    if (s === '..') {
      if (out.length > 1) out.pop();
      if (i === segs.length - 1) out.push('');
    } else if (s === '.') {
      if (i === segs.length - 1) out.push('');
    } else out.push(s);
  }
  let p = out.join('/');
  if (!p.startsWith('/')) p = '/' + p;
  return p;
}

class URLSearchParams {
  #list = [];
  #url = null;
  constructor(init = '') {
    if (typeof init === 'string') {
      this.#parse(init);
    } else if (init && typeof init === 'object') {
      if (typeof init[Symbol.iterator] === 'function') {
        for (const pair of init) {
          const p = Array.from(pair);
          if (p.length !== 2) {
            const e = new TypeError('Each query pair must be an iterable [name, value] tuple');
            e.code = 'ERR_INVALID_TUPLE';
            throw e;
          }
          this.#list.push([String(p[0]), String(p[1])]);
        }
      } else {
        for (const k of Object.keys(init)) this.#list.push([k, String(init[k])]);
      }
    }
  }
  #parse(s) {
    this.#list = [];
    if (s.startsWith('?')) s = s.slice(1);
    for (const part of s.split('&')) {
      if (!part) continue;
      const i = part.indexOf('=');
      const k = i >= 0 ? part.slice(0, i) : part;
      const v = i >= 0 ? part.slice(i + 1) : '';
      const dec = (x) => {
        try { return decodeURIComponent(x.replace(/\+/g, ' ')); } catch { return x; }
      };
      this.#list.push([dec(k), dec(v)]);
    }
  }
  _attach(u, s) { this.#url = u; this.#parse(s); }
  #update() { if (this.#url) this.#url._setSearch(this.toString()); }
  append(k, v) { this.#list.push([String(k), String(v)]); this.#update(); }
  delete(k, v) { this.#list = this.#list.filter(([a, b]) => !(a === String(k) && (v === undefined || b === String(v)))); this.#update(); }
  get(k) { const e = this.#list.find(([a]) => a === String(k)); return e ? e[1] : null; }
  getAll(k) { return this.#list.filter(([a]) => a === String(k)).map(([, b]) => b); }
  has(k, v) { return this.#list.some(([a, b]) => a === String(k) && (v === undefined || b === String(v))); }
  set(k, v) {
    k = String(k);
    const i = this.#list.findIndex(([a]) => a === k);
    if (i < 0) this.#list.push([k, String(v)]);
    else {
      this.#list[i][1] = String(v);
      this.#list = this.#list.filter(([a], j) => a !== k || j === i);
    }
    this.#update();
  }
  sort() { this.#list.sort((a, b) => (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0)); this.#update(); }
  get size() { return this.#list.length; }
  forEach(fn, thisArg) { for (const [k, v] of this.#list) fn.call(thisArg, v, k, this); }
  keys() { return this.#list.map(([k]) => k)[Symbol.iterator](); }
  values() { return this.#list.map(([, v]) => v)[Symbol.iterator](); }
  entries() { return this.#list.map(([k, v]) => [k, v])[Symbol.iterator](); }
  [Symbol.iterator]() { return this.entries(); }
  toString() {
    const enc = (s) => encodeComponent(s, '*').replace(/%20/g, '+');
    return this.#list.map(([k, v]) => `${enc(k)}=${enc(v)}`).join('&');
  }
  get [Symbol.toStringTag]() { return 'URLSearchParams'; }
  [kInspect](depth, opts) {
    const inspect = require('util').inspect;
    if (this.#list.length === 0) return 'URLSearchParams {}';
    const parts = this.#list.map(([k, v]) => `${inspect(k)} => ${inspect(v)}`);
    const one = `URLSearchParams { ${parts.join(', ')} }`;
    if (one.length <= 80) return one;
    return `URLSearchParams {\n  ${parts.join(',\n  ')} }`;
  }
}

class URL {
  #protocol = '';
  #username = '';
  #password = '';
  #hostname = '';
  #port = '';
  #pathname = '';
  #search = '';
  #hash = '';
  #params;
  constructor(input, base) {
    input = String(input).trim();
    const m = /^([a-zA-Z][a-zA-Z0-9+.-]*):/.exec(input);
    if (!m) {
      if (base === undefined) throw invalidUrl(input);
      const b = base instanceof URL ? base : new URL(base);
      this.#resolveRelative(input, b);
    } else {
      this.#parseAbsolute(input);
    }
    this.#params = new URLSearchParams();
    this.#params._attach(this, this.#search);
  }
  #parseAbsolute(input) {
    const m = /^([a-zA-Z][a-zA-Z0-9+.-]*):(.*)$/s.exec(input);
    this.#protocol = m[1].toLowerCase() + ':';
    let rest = m[2];
    const isSpecial = this.#protocol in special;
    const hi = rest.indexOf('#');
    if (hi >= 0) { this.#hash = rest.slice(hi) === '#' ? '' : encodeComponent(rest.slice(hi), '#!$&\'()*+,;=:@/?%'); rest = rest.slice(0, hi); }
    const qi = rest.indexOf('?');
    if (qi >= 0) { this.#search = rest.slice(qi) === '?' ? '' : encodeComponent(rest.slice(qi), '?!$&\'()*+,;=:@/%'); rest = rest.slice(0, qi); }
    if (rest.startsWith('//') || (isSpecial && this.#protocol !== 'file:')) {
      rest = rest.replace(/^\/*/, '');
      const si = rest.search(/[/\\]/);
      let auth = si >= 0 ? rest.slice(0, si) : rest;
      let path = si >= 0 ? rest.slice(si).replace(/\\/g, '/') : '';
      const at = auth.lastIndexOf('@');
      if (at >= 0) {
        const cred = auth.slice(0, at);
        auth = auth.slice(at + 1);
        const ci = cred.indexOf(':');
        this.#username = ci >= 0 ? cred.slice(0, ci) : cred;
        this.#password = ci >= 0 ? cred.slice(ci + 1) : '';
      }
      const pm = /^(\[[^\]]*\]|[^:]*)(?::(\d*))?$/.exec(auth);
      if (!pm) throw invalidUrl(this.#protocol + m[2]);
      this.#hostname = pm[1].toLowerCase();
      if (/[\s<>^|%]/.test(this.#hostname) || (isSpecial && this.#protocol !== 'file:' && this.#hostname === '')) throw invalidUrl(input);
      const port = pm[2] || '';
      if (port && Number(port) > 65535) throw invalidUrl(input);
      this.#port = port === special[this.#protocol] ? '' : port ? String(Number(port)) : '';
      this.#pathname = isSpecial ? normalizePath(encodeComponent(path || '/', '/!$&\'()*+,;=:@%')) : path;
    } else {
      this.#pathname = rest;
    }
  }
  #resolveRelative(input, b) {
    this.#protocol = b.protocol;
    this.#username = b.username;
    this.#password = b.password;
    this.#hostname = b.hostname;
    this.#port = b.port;
    let rest = input;
    const hi = rest.indexOf('#');
    if (hi >= 0) { this.#hash = rest.slice(hi); rest = rest.slice(0, hi); }
    const qi = rest.indexOf('?');
    let hadQuery = false;
    if (qi >= 0) { this.#search = rest.slice(qi) === '?' ? '' : rest.slice(qi); rest = rest.slice(0, qi); hadQuery = true; }
    if (rest.startsWith('//')) {
      this.#parseAbsolute(b.protocol + input);
      return;
    }
    if (rest === '') {
      this.#pathname = b.pathname;
      if (!hadQuery) this.#search = b.search;
    } else if (rest.startsWith('/')) {
      this.#pathname = normalizePath(rest);
    } else {
      const dir = b.pathname.slice(0, b.pathname.lastIndexOf('/') + 1);
      this.#pathname = normalizePath(dir + rest);
    }
  }
  _setSearch(s) { this.#search = s ? '?' + s : ''; }
  get protocol() { return this.#protocol; }
  set protocol(v) { this.#protocol = String(v).replace(/:?$/, ':'); }
  get username() { return this.#username; }
  set username(v) { this.#username = String(v); }
  get password() { return this.#password; }
  set password(v) { this.#password = String(v); }
  get hostname() { return this.#hostname; }
  set hostname(v) { this.#hostname = String(v); }
  get port() { return this.#port; }
  set port(v) { v = String(v); this.#port = v === special[this.#protocol] ? '' : v; }
  get host() { return this.#hostname + (this.#port ? ':' + this.#port : ''); }
  set host(v) { const [h, p] = String(v).split(':'); this.#hostname = h; this.#port = p || ''; }
  get origin() {
    if (this.#protocol in special && this.#protocol !== 'file:') return `${this.#protocol}//${this.host}`;
    return 'null';
  }
  get pathname() { return this.#pathname; }
  set pathname(v) { this.#pathname = normalizePath(String(v)); }
  get search() { return this.#search; }
  set search(v) { v = String(v); this.#search = v === '' || v === '?' ? '' : v.startsWith('?') ? v : '?' + v; this.#params._attach(this, this.#search); }
  get searchParams() { return this.#params; }
  get hash() { return this.#hash; }
  set hash(v) { v = String(v); this.#hash = v === '' || v === '#' ? '' : v.startsWith('#') ? v : '#' + v; }
  get href() {
    const auth = this.#username || this.#password ? `${this.#username}${this.#password ? ':' + this.#password : ''}@` : '';
    const hasHost = this.#protocol in special || this.#hostname !== '';
    return `${this.#protocol}${hasHost ? '//' : ''}${auth}${this.host}${this.#pathname}${this.#search}${this.#hash}`;
  }
  set href(v) { const u = new URL(v); this.#protocol = u.protocol; this.#hostname = u.hostname; this.#port = u.port; this.#pathname = u.pathname; this.#search = u.search; this.#hash = u.hash; }
  toString() { return this.href; }
  toJSON() { return this.href; }
  get [Symbol.toStringTag]() { return 'URL'; }
  static canParse(u, b) { try { new URL(u, b); return true; } catch { return false; } }
  static parse(u, b) { try { return new URL(u, b); } catch { return null; } }
  [kInspect](depth, opts) {
    const inspect = require('util').inspect;
    const obj = {
      href: this.href,
      origin: this.origin,
      protocol: this.protocol,
      username: this.username,
      password: this.password,
      host: this.host,
      hostname: this.hostname,
      port: this.port,
      pathname: this.pathname,
      search: this.search,
      searchParams: this.searchParams,
      hash: this.hash,
    };
    if (depth < 0) return '[URL]';
    return 'URL ' + inspect(obj, { ...opts, depth });
  }
}

function fileURLToPath(u) {
  const s = typeof u === 'string' ? u : u.href;
  if (!s.startsWith('file://')) {
    const e = new TypeError('The URL must be of scheme file');
    e.code = 'ERR_INVALID_URL_SCHEME';
    throw e;
  }
  return decodeURIComponent(new URL(s).pathname);
}

function pathToFileURL(p) {
  const path = require('path');
  return new URL('file://' + encodeComponent(path.resolve(p), '/'));
}

function parse(str, parseQuery) {
  const out = {
    protocol: null, slashes: null, auth: null, host: null, port: null, hostname: null,
    hash: null, search: null, query: null, pathname: null, path: null, href: str,
  };
  try {
    const u = new URL(str);
    out.protocol = u.protocol;
    out.slashes = true;
    out.auth = u.username ? u.username + (u.password ? ':' + u.password : '') : null;
    out.host = u.host;
    out.port = u.port || null;
    out.hostname = u.hostname;
    out.hash = u.hash || null;
    out.search = u.search || null;
    out.query = parseQuery ? require('querystring').parse(u.search.slice(1)) : (u.search ? u.search.slice(1) : null);
    out.pathname = u.pathname;
    out.path = u.pathname + u.search;
    out.href = u.href;
  } catch {
    const qi = str.indexOf('?');
    out.pathname = qi >= 0 ? str.slice(0, qi) : str;
    out.search = qi >= 0 ? str.slice(qi) : null;
    out.query = parseQuery ? require('querystring').parse(qi >= 0 ? str.slice(qi + 1) : '') : (qi >= 0 ? str.slice(qi + 1) : null);
    out.path = str;
  }
  return out;
}

function format(u) {
  if (u instanceof URL) return u.href;
  if (typeof u === 'string') return u;
  let s = '';
  if (u.protocol) s += u.protocol + (u.slashes || /^https?:|^ftp:|^file:/.test(u.protocol) ? '//' : '');
  if (u.auth) s += u.auth + '@';
  s += u.host || ((u.hostname || '') + (u.port ? ':' + u.port : ''));
  s += u.pathname || '';
  if (u.search) s += u.search;
  else if (u.query && typeof u.query === 'object') {
    const q = require('querystring').stringify(u.query);
    if (q) s += '?' + q;
  }
  if (u.hash) s += u.hash;
  return s;
}

function resolve(from, to) {
  return new URL(to, new URL(from, 'resolve://')).href.replace(/^resolve:\/\/\/?/, '/').replace(/^\/\//, '/');
}

module.exports = { URL, URLSearchParams, fileURLToPath, pathToFileURL, parse, format, resolve, domainToASCII: (d) => d, domainToUnicode: (d) => d };
