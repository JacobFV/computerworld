'use strict';
// dns over the world's DNS. Answers arrive as I/O completions.

const wire = require('internal/httpwire');

function queryError(syscall, hostname, code) {
  const e = new Error(`${syscall} ${code} ${hostname}`);
  e.errno = undefined;
  e.code = code;
  e.syscall = syscall;
  e.hostname = hostname;
  return e;
}

function validateHostname(hostname) {
  if (typeof hostname !== 'string') {
    const e = new TypeError(`The "hostname" argument must be of type string. Received ${hostname === undefined ? 'undefined' : typeof hostname}`);
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
}

function lookup(hostname, options, callback) {
  if (typeof options === 'function') { callback = options; options = {}; }
  if (typeof options === 'number') options = { family: options };
  options = options || {};
  validateHostname(hostname);
  if (typeof callback !== 'function') {
    const e = new TypeError('The "callback" argument must be of type function. Received undefined');
    e.code = 'ERR_INVALID_ARG_TYPE';
    throw e;
  }
  const done = (err, addrs) => {
    if (err) return callback(err);
    if (options.all) return callback(null, addrs.map((a) => ({ address: a, family: wire.isIPv6(a) ? 6 : 4 })));
    const a = addrs[0];
    return callback(null, a, wire.isIPv6(a) ? 6 : 4);
  };
  if (!hostname) {
    process.nextTick(done, null, [options.family === 6 ? '::1' : '127.0.0.1']);
    return {};
  }
  if (wire.isIPv4(hostname) || wire.isIPv6(hostname)) {
    process.nextTick(done, null, [hostname]);
    return {};
  }
  if (hostname === 'localhost') {
    binding.scheduleIo(done, 0, null, [options.family === 6 ? '::1' : '127.0.0.1']);
    return {};
  }
  if (options.family === 6) {
    binding.scheduleIo(() => callback(wire.lookupError(hostname, { code: 'ENOTFOUND' })), 0);
    return {};
  }
  const r = binding.dnsLookup(hostname);
  if (r.error) binding.scheduleIo(() => callback(wire.lookupError(hostname, r.error)), 0);
  else binding.scheduleIo(done, 0, null, r.addresses);
  return {};
}

function resolveWith(syscall, family) {
  return function resolveN(hostname, options, callback) {
    if (typeof options === 'function') { callback = options; options = {}; }
    validateHostname(hostname);
    if (family === 6) {
      const r = binding.dnsLookup(hostname);
      const code = r.error ? 'ENOTFOUND' : 'ENODATA';
      binding.scheduleIo(() => callback(queryError(syscall, hostname, code)), 0);
      return {};
    }
    if (wire.isIPv4(hostname)) {
      binding.scheduleIo(() => callback(null, [hostname]), 0);
      return {};
    }
    const r = binding.dnsLookup(hostname);
    if (r.error) binding.scheduleIo(() => callback(queryError(syscall, hostname, 'ENOTFOUND')), 0);
    else if (options && options.ttl) binding.scheduleIo(() => callback(null, r.addresses.map((address) => ({ address, ttl: 60 }))), 0);
    else binding.scheduleIo(() => callback(null, r.addresses.slice()), 0);
    return {};
  };
}

const resolve4 = resolveWith('queryA', 4);
const resolve6 = resolveWith('queryAaaa', 6);

function resolve(hostname, rrtype, callback) {
  if (typeof rrtype === 'function') { callback = rrtype; rrtype = 'A'; }
  switch (rrtype) {
    case 'A': return resolve4(hostname, callback);
    case 'AAAA': return resolve6(hostname, callback);
    default: {
      const syscall = `query${rrtype[0]}${rrtype.slice(1).toLowerCase()}`;
      const r = binding.dnsLookup(hostname);
      binding.scheduleIo(() => callback(queryError(syscall, hostname, r.error ? 'ENOTFOUND' : 'ENODATA')), 0);
      return {};
    }
  }
}

function reverse(ip, callback) {
  binding.scheduleIo(() => callback(queryError('getHostByAddr', ip, 'ENOTFOUND')), 0);
  return {};
}

function lookupService(address, port, callback) {
  const names = { 80: 'http', 443: 'https', 22: 'ssh', 21: 'ftp', 25: 'smtp', 53: 'domain' };
  binding.scheduleIo(() => callback(null, address === '127.0.0.1' ? 'localhost' : address, names[port] || String(port)), 0);
}

let servers = ['127.0.0.53'];
function getServers() { return servers.slice(); }
function setServers(s) { servers = Array.from(s); }

function promisified(fn) {
  return (...args) => new Promise((resolve, reject) => fn(...args, (err, ...res) => (err ? reject(err) : resolve(res.length > 1 ? res : res[0]))));
}

const promises = {
  lookup(hostname, options) {
    return new Promise((resolve, reject) => lookup(hostname, options || {}, (err, address, family) => {
      if (err) return reject(err);
      if (options && options.all) return resolve(address);
      resolve({ address, family });
    }));
  },
  resolve: promisified(resolve),
  resolve4: promisified(resolve4),
  resolve6: promisified(resolve6),
  reverse: promisified(reverse),
  lookupService(address, port) {
    return new Promise((resolve, reject) => lookupService(address, port, (err, hostname, service) => (err ? reject(err) : resolve({ hostname, service }))));
  },
  getServers,
  setServers,
};

class Resolver {
  constructor() {}
  cancel() {}
  getServers() { return getServers(); }
  setServers(s) { setServers(s); }
}
for (const [k, f] of Object.entries({ resolve, resolve4, resolve6, reverse })) Resolver.prototype[k] = f;
class PromisesResolver extends Resolver {}
for (const k of ['resolve', 'resolve4', 'resolve6', 'reverse']) PromisesResolver.prototype[k] = promises[k];
promises.Resolver = PromisesResolver;

module.exports = {
  lookup, lookupService, resolve, resolve4, resolve6, reverse, getServers, setServers,
  Resolver, promises,
  ADDRCONFIG: 1024, V4MAPPED: 2048, ALL: 256,
  NODATA: 'ENODATA', FORMERR: 'EFORMERR', SERVFAIL: 'ESERVFAIL', NOTFOUND: 'ENOTFOUND',
  NOTIMP: 'ENOTIMP', REFUSED: 'EREFUSED', BADQUERY: 'EBADQUERY', BADNAME: 'EBADNAME',
  CONNREFUSED: 'ECONNREFUSED', TIMEOUT: 'ETIMEOUT', CANCELLED: 'ECANCELLED',
};
