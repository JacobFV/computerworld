'use strict';
// Shared plumbing for http, https, net, dns and fetch: Node-shaped network
// errors, the world HTTP exchange, the in-process loopback registry (servers
// this program listens on) and an HTTP/1.x parser for raw sockets.

const localAddress = binding.localAddress();

function isIPv4(s) {
  if (typeof s !== 'string') return false;
  const p = s.split('.');
  return p.length === 4 && p.every((x) => /^\d{1,3}$/.test(x) && Number(x) <= 255 && (x === '0' || x[0] !== '0'));
}
function isIPv6(s) {
  return typeof s === 'string' && s.includes(':') && /^[0-9a-fA-F:.]+$/.test(s);
}
function isLocal(addr) {
  return addr === '127.0.0.1' || addr === 'localhost' || addr === '::1' || addr === '0.0.0.0' ||
    addr === '' || addr === localAddress || (typeof addr === 'string' && addr.startsWith('127.'));
}

// `getaddrinfo ENOTFOUND host`
function lookupError(hostname, err, syscall = 'getaddrinfo') {
  const code = err.code === 'ENOTFOUND' ? 'ENOTFOUND' : (err.code === 'ENETUNREACH' ? 'EAI_AGAIN' : err.code);
  const e = new Error(`${syscall} ${code} ${hostname}`);
  e.errno = code === 'EAI_AGAIN' ? -3001 : -3008;
  e.code = code;
  e.syscall = syscall;
  e.hostname = hostname;
  return e;
}

// `connect ECONNREFUSED 10.0.0.5:443`
function connectError(err, address, port) {
  const e = new Error(`connect ${err.code} ${address}:${port}`);
  e.errno = err.errno;
  e.code = err.code;
  e.syscall = 'connect';
  e.address = address;
  e.port = port;
  return e;
}

function resolveAddress(host) {
  if (isIPv4(host)) return { address: host };
  if (host === 'localhost') return { address: '127.0.0.1' };
  const r = binding.dnsLookup(host);
  if (r.error) return { error: lookupError(host, r.error) };
  return { address: r.addresses[0] };
}

// Turns a failed world exchange into the error Node raises.
function exchangeError(err, host, port) {
  if (err.code === 'ENOTFOUND' || err.code === 'ENETUNREACH' && !isIPv4(host)) return lookupError(host, err);
  if (err.code === 'ETIMEDOUT') {
    const e = new Error('read ETIMEDOUT');
    e.errno = -110;
    e.code = 'ETIMEDOUT';
    e.syscall = 'read';
    return e;
  }
  if (err.code === 'ECONNRESET') {
    const e = new Error('socket hang up');
    e.code = 'ECONNRESET';
    return e;
  }
  const r = resolveAddress(host);
  return connectError(err, r.address || host, port);
}

// Loopback: servers this program is listening on, by port.
const servers = new Map();

function flatHeaders(headers) {
  const out = [];
  for (const [k, v] of headers) {
    if (Array.isArray(v)) for (const x of v) out.push(String(k), String(x));
    else out.push(String(k), String(v));
  }
  return out;
}

// Node's incoming header object: lower-case names, duplicates joined with ', '
// (set-cookie as an array; a few singletons keep the first value).
const singletons = new Set(['age', 'authorization', 'content-length', 'content-type', 'etag', 'expires',
  'from', 'host', 'if-modified-since', 'if-unmodified-since', 'last-modified', 'location',
  'max-forwards', 'proxy-authorization', 'referer', 'retry-after', 'server', 'user-agent']);
function headersObject(raw) {
  const h = {};
  for (let i = 0; i + 1 < raw.length; i += 2) {
    const k = raw[i].toLowerCase();
    const v = raw[i + 1];
    if (k === 'set-cookie') {
      (h[k] || (h[k] = [])).push(v);
    } else if (h[k] === undefined) {
      h[k] = v;
    } else if (!singletons.has(k)) {
      h[k] += (k === 'cookie' ? '; ' : ', ') + v;
    }
  }
  return h;
}

// Parses one HTTP/1.x message (request when `isRequest`) from a Buffer.
// Returns { msg, rest } or null when incomplete, or { bad: true }.
function parseMessage(buf, isRequest) {
  const text = buf.toString('latin1');
  let end = text.indexOf('\r\n\r\n');
  let sep = 4;
  if (end < 0) { end = text.indexOf('\n\n'); sep = 2; }
  const nl = text.indexOf('\n');
  if (nl >= 0) {
    const first = text.slice(0, nl).replace(/\r$/, '');
    const ok = isRequest ? /^[A-Z]+ \S+ HTTP\/1\.\d$/.test(first) : /^HTTP\/1\.\d \d{3}( .*)?$/.test(first);
    if (!ok) return { bad: true };
  } else if (buf.length > 16384) {
    return { bad: true };
  }
  if (end < 0) return null;
  const lines = text.slice(0, end).split(/\r?\n/);
  const first = lines[0].split(' ');
  const raw = [];
  for (const l of lines.slice(1)) {
    const i = l.indexOf(':');
    if (i > 0) raw.push(l.slice(0, i).trim(), l.slice(i + 1).trim());
  }
  const h = headersObject(raw);
  let body = buf.subarray(end + sep);
  let rest;
  if (/chunked/i.test(h['transfer-encoding'] || '')) {
    const parts = [];
    let pos = 0;
    for (;;) {
      const t = body.toString('latin1');
      const lf = t.indexOf('\r\n', pos);
      if (lf < 0) return null;
      const size = parseInt(t.slice(pos, lf).split(';')[0], 16) || 0;
      if (body.length < lf + 2 + size + 2) return null;
      parts.push(body.subarray(lf + 2, lf + 2 + size));
      pos = lf + 2 + size + 2;
      if (size === 0) break;
    }
    rest = body.subarray(pos);
    body = Buffer.concat(parts);
  } else {
    const len = Number(h['content-length'] || 0);
    if (!isRequest && h['content-length'] === undefined) {
      rest = Buffer.alloc(0);
    } else {
      if (body.length < len) return null;
      rest = body.subarray(len);
      body = body.subarray(0, len);
    }
  }
  const msg = isRequest
    ? { method: first[0], url: first[1], version: first[2], raw, headers: h, body }
    : { status: Number(first[1]), reason: first.slice(2).join(' '), version: first[0], raw, headers: h, body };
  return { msg, rest };
}

function serializeResponse(status, reason, raw, body, close) {
  const lines = [`HTTP/1.1 ${status} ${reason}`];
  let hasLen = false;
  for (let i = 0; i + 1 < raw.length; i += 2) {
    const k = raw[i];
    if (/^(connection|transfer-encoding)$/i.test(k)) continue;
    if (/^content-length$/i.test(k)) { hasLen = true; lines.push(`${k}: ${body.length}`); continue; }
    lines.push(`${k}: ${raw[i + 1]}`);
  }
  if (!hasLen) lines.push(`Content-Length: ${body.length}`);
  lines.push(close ? 'Connection: close' : 'Connection: keep-alive');
  return Buffer.concat([Buffer.from(lines.join('\r\n') + '\r\n\r\n', 'latin1'), body]);
}

module.exports = {
  localAddress, isIPv4, isIPv6, isLocal, lookupError, connectError, resolveAddress,
  exchangeError, servers, flatHeaders, headersObject, parseMessage, serializeResponse,
};
